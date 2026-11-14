# Phase 1 — Analyzer crate `sonder-quality`

## Files

- Create `crates/sonder-quality/{Cargo.toml,src/lib.rs,src/spectrum.rs,src/criteria.rs,src/verdict.rs}`
- Modify root `Cargo.toml` (member + workspace dep; move `realfft` to workspace deps and
  point `crates/sonder-dsp/Cargo.toml` at it)

## API

```rust
pub struct Options { pub spectrogram: bool }          // batch passes false
pub fn analyze(path: &Path, opts: &Options, cancel: &AtomicBool,
               progress: impl FnMut(f32)) -> Result<Report, QualityError>;

pub struct Report {
    pub analyzer_version: u32,                       // bump when thresholds change
    pub format: TrackFormat, pub duration_secs: f64,
    pub verdict: Verdict, pub confidence: Confidence, // Low | Medium | High
    pub evidence: Vec<Evidence>,                     // { id: CriterionId, status, value: Option<f64> }
    pub spectrum: Vec<SpectrumPoint>,                // ~512 log-spaced points, dB rel. peak
    pub cutoff_hz: Option<f64>,
    pub cutoff_track: Vec<f32>,                      // per-second cutoff, Hz
    pub spectrogram: Option<Spectrogram>,            // { cols, rows, db: Vec<u8> } 0..=255 ↔ −120..0 dB
}
pub enum Verdict { Genuine, Suspect, Transcoded, FakeHiRes, FakeBitDepth, Inconclusive }
pub enum EvidenceStatus { Pass, Warn, Fail, NotMeasured }
```

Evidence carries ids and numbers only; sentences are written in the UI catalogs.

## Steps

1. **Decode** full track. PCM: `next_i32` (left-justified), keep an OR-accumulator of every
   sample per channel for bit depth, convert to f64 for the STFT. DSD: `DsdDecoder` →
   `DsdToPcm` at 176.4 kHz; bit-depth and MD5 rows are NotMeasured.
2. **STFT**: Hann window, 8192 points at ≤ 96 kHz, 16384 above; hop = half window; mid
   channel (L+R)/2 plus side (L−R)/2 for stereo. Skip frames quieter than −50 dBFS RMS
   (silence makes cutoffs meaningless). Accumulate average power; per frame keep the cutoff.
3. **Cutoff** (per frame and average): highest frequency where the 1/6-octave-smoothed level
   stays above `reference − 60 dB`, reference = median band power 2–8 kHz. Below 8 kHz of
   treble content overall ⇒ Inconclusive ("little treble").
4. **Slope**: level drop across 500 Hz above the cutoff; ≥ 25 dB ⇒ a wall (lossy lid or
   resampler brickwall), gentle ⇒ natural roll-off.
5. **Stability**: interquartile range of per-frame cutoffs over loud frames. Lossy: IQR
   < 250 Hz (flat lid); genuine: content top edge moves with the music.
6. **Holes**: in 15 kHz–cutoff, fraction of loud frames where a ≥ 1 kHz band drops > 40 dB
   below its neighbours (lossy encoders zero scale-factor bands intermittently).
7. **Stereo field**: side/mid power ratio above 14 kHz vs 2–8 kHz. A collapse > 20 dB at HF
   only is the joint-stereo fingerprint; mono files mark it NotMeasured.
8. **Channel symmetry**: per-channel cutoff delta (`_channelDelta`); > 1 kHz ⇒ warn.
9. **Bit depth**: real bits = 32 − trailing_zeros(OR of all samples). Declared 24 and real
   ≤ 16 ⇒ Fail (padded). Also noise floor (10th percentile of quiet-frame level above the
   cutoff band floor): declared 24 but floor consistent with 16-bit dither (> −100 dBFS) ⇒ Warn.
10. **Hi-res**: rate ≥ 88.2 kHz: power above 24 kHz relative to 2–8 kHz. < −90 dB with a
    wall at ~20–24 kHz ⇒ Fail (upsampled from 44.1/48).
11. **DSD cutoff**: on DSD→PCM, a wall near 20–22 kHz below the modulator noise rise ⇒
    Warn (CD → DSD conversion), never Fail (analog transfers look similar).
12. **MD5** (FLAC only): open with verify, `finalize().verify_ok` → Pass / Fail;
    absent ⇒ NotMeasured. Pass never upgrades the verdict.
13. **Verdict rules** (in `verdict.rs`, one table):
    - bit depth Fail ⇒ FakeBitDepth; hi-res Fail ⇒ FakeHiRes (both may be listed; first wins).
    - wall + (stable OR holes OR stereo collapse) with cutoff ≤ 20.5 kHz ⇒ Transcoded, High
      if ≥ 2 signals, else Medium.
    - wall at 20.5–22 kHz at 44.1/48 kHz ⇒ Suspect, Low (48 → 44.1 resample is lossless).
    - content reaching ≥ 0.9 × Nyquist or a gentle slope with a moving edge ⇒ Genuine.
    - too short (< 3 s), too quiet, or too little treble ⇒ Inconclusive.
14. **Progress / cancel**: progress after each decoded block (fraction of total frames);
    check `cancel` every block, return `QualityError::Cancelled`.

## Tests (`crates/sonder-quality/tests/`)

Synthesize WAV/FLAC fixtures with `hound` (FLAC: reuse the encoder pattern of
`crates/sonder-library/tests/fixtures/mod.rs` if it writes FLAC, else WAV for spectral cases
and a checked-in tiny FLAC for MD5): full-band pink noise; the same through a steep FIR
low-pass at 16 / 19.5 / 20 / 21.5 kHz; with intermittent notched bands; HF side removed;
24-bit with zero low byte; 96 kHz with a 22 kHz wall; −80 dBFS noise; 2 s clip; corrupted
FLAC frame. One test per acceptance row in `plan.md`.

## Validation

`cargo test -p sonder-quality`, `cargo clippy -p sonder-quality --all-targets -- -D warnings`.
Measure one 5-minute 24/96 FLAC in release: target < 3 s.

## Risk

Thresholds are tuned on synthetic data; real MP3/AAC transcodes may differ. Mitigation: run
on a few real transcodes the user owns (encode with an external tool, never in tests) and
adjust; bump `analyzer_version` so caches refresh.
