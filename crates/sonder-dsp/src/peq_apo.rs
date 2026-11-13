//! Equalizer APO's parametric EQ text: the `ParametricEQ.txt` AutoEQ publishes per headphone.
//!
//! [`Peq::from_apo_text`] reads it tolerantly: comments, blank lines, a BOM, CRLF and lines meant
//! for other APO stages (`Channel:`, `GraphicEQ:`, …) never change the EQ silently — anything it
//! cannot use becomes a [`ApoImport::warnings`] entry. Every parsed value still passes through
//! [`Peq::sanitized`], so a wild file cannot reach the engine. [`Peq::to_apo_text`] writes the
//! same format back, and `import(export(eq)) == eq` for any sanitized EQ.

use crate::biquad::BiquadKind;
use crate::peq::{Peq, PeqBand};

/// Q used where the file names none: the cookbook's shelf and pass value, and APO's default.
const DEFAULT_Q: f64 = 0.707;

/// A parsed file: the EQ, already passed through [`Peq::sanitized`], and what the parser skipped
/// or clamped, in file order.
#[derive(Debug, Clone, PartialEq)]
pub struct ApoImport {
    pub peq: Peq,
    pub warnings: Vec<String>,
}

/// Why a file cannot be read as an Equalizer APO parametric EQ. The message is what the UI shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApoError {
    message: &'static str,
    /// What the parser saw, when the message alone would hide it.
    detail: Option<String>,
}

impl ApoError {
    const NOT_PARAMETRIC: Self = Self {
        message: "not an Equalizer APO parametric file: no Preamp line and no Filter line",
        detail: None,
    };
    /// Lines of the right kind were there, but none of them could be used.
    const NOTHING_USABLE: Self = Self {
        message: "not an Equalizer APO parametric file: no usable Preamp or Filter line",
        detail: None,
    };
    const COMMA_DECIMAL: Self = Self {
        message: "numbers must use '.' as the decimal separator and no thousands separators, \
                  as Equalizer APO writes them",
        detail: None,
    };

    pub const fn message(&self) -> &'static str {
        self.message
    }

    /// The same error, saying what the parser saw.
    fn with_detail(mut self, detail: String) -> Self {
        self.detail = Some(detail);
        self
    }
}

impl std::fmt::Display for ApoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message)?;
        match &self.detail {
            Some(detail) => write!(f, " ({detail})"),
            None => Ok(()),
        }
    }
}

impl std::error::Error for ApoError {}

impl Peq {
    /// Reads Equalizer APO parametric text: `Preamp:` plus `Filter N:` lines.
    ///
    /// Fails only when the text holds neither a preamp nor a usable filter (and on a `,` decimal,
    /// which would otherwise be misread as two numbers).
    pub fn from_apo_text(text: &str) -> Result<ApoImport, ApoError> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut preamp_db = 0.0;
        let mut saw_preamp = false;
        // A Preamp or Filter line was present, whether or not it could be used.
        let mut saw_stage = false;
        let mut bands = Vec::new();
        let mut warnings = Vec::new();

        for (index, raw) in text.lines().enumerate() {
            let line_no = index + 1;
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((head, rest)) = line.split_once(':') else {
                warnings.push(format!("line {line_no}: skipped '{}'", short(line)));
                continue;
            };
            let head = head.trim();
            if head.eq_ignore_ascii_case("preamp") {
                saw_stage = true;
                match preamp_value(rest)? {
                    Some(value) => {
                        if saw_preamp {
                            warnings.push(format!(
                                "line {line_no}: Preamp given twice; the last one is used"
                            ));
                        }
                        preamp_db = value;
                        saw_preamp = true;
                    }
                    None => warnings.push(format!(
                        "line {line_no}: preamp '{}' is not a number; ignored",
                        short(rest.trim())
                    )),
                }
                continue;
            }
            if is_filter_head(head) {
                saw_stage = true;
                if let Some(band) = filter_band(line_no, rest, &mut warnings)? {
                    bands.push(band);
                }
                continue;
            }
            warnings.push(format!("line {line_no}: skipped '{}'", short(line)));
        }

        if !saw_preamp && bands.is_empty() {
            return Err(if saw_stage {
                ApoError::NOTHING_USABLE.with_detail(skipped_summary(&warnings))
            } else if warnings.is_empty() {
                ApoError::NOT_PARAMETRIC
            } else {
                // Lines were there but not in a shape APO would read; say what was skipped.
                ApoError::NOT_PARAMETRIC.with_detail(skipped_summary(&warnings))
            });
        }

        let raw = Peq { preamp_db, bands };
        let peq = raw.clone().sanitized();
        if raw.bands.len() > peq.bands.len() {
            warnings.push(format!(
                "kept the first {} of {} bands",
                peq.bands.len(),
                raw.bands.len()
            ));
        }
        if raw.preamp_db != peq.preamp_db {
            warnings.push(format!(
                "preamp {} dB clamped to {} dB",
                raw.preamp_db, peq.preamp_db
            ));
        }
        for (index, (before, after)) in raw.bands.iter().zip(&peq.bands).enumerate() {
            let band = index + 1;
            if before.freq_hz != after.freq_hz {
                warnings.push(format!(
                    "band {band}: frequency {} Hz clamped to {} Hz",
                    before.freq_hz, after.freq_hz
                ));
            }
            if before.gain_db != after.gain_db {
                warnings.push(format!(
                    "band {band}: gain {} dB clamped to {} dB",
                    before.gain_db, after.gain_db
                ));
            }
            if before.q != after.q {
                warnings.push(format!(
                    "band {band}: Q {} clamped to {}",
                    before.q, after.q
                ));
            }
        }

        Ok(ApoImport { peq, warnings })
    }

    /// The EQ as Equalizer APO parametric text: `Preamp:` then one `Filter N:` line per band,
    /// LF line endings.
    pub fn to_apo_text(&self) -> String {
        let mut text = format!("Preamp: {} dB\n", self.preamp_db);
        for (index, band) in self.bands.iter().enumerate() {
            text.push_str(&format!(
                "Filter {}: {} {} Fc {} Hz Gain {} dB Q {}\n",
                index + 1,
                if band.enabled { "ON" } else { "OFF" },
                apo_kind(band.kind),
                band.freq_hz,
                band.gain_db,
                band.q,
            ));
        }
        text
    }
}

/// The APO token for a shape; every one of them reads back as the same kind.
const fn apo_kind(kind: BiquadKind) -> &'static str {
    match kind {
        BiquadKind::Peak => "PK",
        BiquadKind::LowShelf => "LSC",
        BiquadKind::HighShelf => "HSC",
        BiquadKind::LowPass => "LPQ",
        BiquadKind::HighPass => "HPQ",
        BiquadKind::BandPass => "BP",
        BiquadKind::Notch => "NO",
    }
}

/// The shape an APO filter type names, if Sonder has an equivalent.
fn apo_band_kind(token: &str) -> Option<BiquadKind> {
    match token.to_ascii_uppercase().as_str() {
        "PK" | "PEQ" => Some(BiquadKind::Peak),
        "LSC" | "LS" => Some(BiquadKind::LowShelf),
        "HSC" | "HS" => Some(BiquadKind::HighShelf),
        "LP" | "LPQ" => Some(BiquadKind::LowPass),
        "HP" | "HPQ" => Some(BiquadKind::HighPass),
        "BP" => Some(BiquadKind::BandPass),
        "NO" => Some(BiquadKind::Notch),
        _ => None,
    }
}

/// `head` is what precedes the colon: `Filter`, `Filter 3` (or any case).
fn is_filter_head(head: &str) -> bool {
    head.get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("filter"))
        && !head.as_bytes().get(6).is_some_and(u8::is_ascii_alphabetic)
}

/// The value of a `Preamp:` line (unit optional), or `None` when it holds no number.
fn preamp_value(rest: &str) -> Result<Option<f64>, ApoError> {
    let Some(token) = rest.split_whitespace().next() else {
        return Ok(None);
    };
    let (digits, _) = split_unit(token);
    if token.contains(',') {
        return Err(ApoError::COMMA_DECIMAL);
    }
    Ok(digits.parse().ok())
}

/// One `Filter N: ON PK Fc 105 Hz Gain 3.4 dB Q 0.70` line, or `None` with a warning when a part
/// of it cannot be used.
fn filter_band(
    line_no: usize,
    spec: &str,
    warnings: &mut Vec<String>,
) -> Result<Option<PeqBand>, ApoError> {
    let mut tokens = spec.split_whitespace();
    let skip = |warnings: &mut Vec<String>, reason: String| {
        warnings.push(format!("line {line_no}: {reason}; band skipped"));
        Ok(None)
    };
    let Some(on) = tokens.next() else {
        return skip(warnings, "no ON/OFF".to_owned());
    };
    let enabled = if on.eq_ignore_ascii_case("on") {
        true
    } else if on.eq_ignore_ascii_case("off") {
        false
    } else {
        return skip(warnings, format!("'{on}' is not ON/OFF"));
    };
    let Some(kind_token) = tokens.next() else {
        return skip(warnings, "no filter type".to_owned());
    };
    let Some(kind) = apo_band_kind(kind_token) else {
        return skip(warnings, format!("unknown filter type '{kind_token}'"));
    };

    let tokens: Vec<&str> = tokens.collect();
    let mut freq_hz: Option<f64> = None;
    let mut gain_db = 0.0;
    // `Q` and `BW Oct` are the same value in Equalizer APO; whatever comes last wins, as it
    // does there. Keeping one slot is what stops a bandwidth from silently overriding a Q.
    let mut q: Option<f64> = None;
    let mut seen_fc = false;
    let mut seen_gain = false;
    let mut index = 0;
    while let Some(key) = tokens.get(index) {
        let key = key.to_ascii_lowercase();
        index += 1;
        if key.starts_with('#') {
            return skip(
                warnings,
                "a comment after the filter values is not supported; put it on its own line"
                    .to_owned(),
            );
        }
        let Some((value, unit, next)) = number_at(&tokens, index)? else {
            return skip(warnings, format!("'{}' has no value", short(&key)));
        };
        index = next;
        // `None` for the keys that set a field directly; their repeats are counted on the spot.
        let damping = match key.as_str() {
            "fc" => {
                if std::mem::replace(&mut seen_fc, true) {
                    warn_repeat(warnings, line_no, &key);
                }
                freq_hz = Some(match unit {
                    Some(Unit::Khz) => value * 1_000.0,
                    _ => value,
                });
                None
            }
            "gain" => {
                if std::mem::replace(&mut seen_gain, true) {
                    warn_repeat(warnings, line_no, &key);
                }
                gain_db = value;
                None
            }
            "q" => Some(value),
            "bw" => match bandwidth_to_q(value) {
                Some(converted) => Some(converted),
                None => {
                    warnings.push(format!(
                        "line {line_no}: bandwidth {value} Oct is not usable; ignored"
                    ));
                    None
                }
            },
            _ => return skip(warnings, format!("unknown '{}'", short(&key))),
        };
        if let Some(damping) = damping {
            if q.is_some() {
                warnings.push(format!(
                    "line {line_no}: '{key}' follows another Q or BW; the last one is used"
                ));
            }
            q = Some(damping);
        }
    }

    let Some(freq_hz) = freq_hz else {
        return skip(warnings, "no frequency".to_owned());
    };
    Ok(Some(PeqBand {
        kind,
        freq_hz,
        gain_db,
        q: q.unwrap_or(DEFAULT_Q),
        enabled,
    }))
}

/// A repeated key would change the band with no trace.
fn warn_repeat(warnings: &mut Vec<String>, line_no: usize, key: &str) {
    warnings.push(format!(
        "line {line_no}: '{key}' given twice; the last value is used"
    ));
}

/// The first few skip reasons, for an error that would otherwise hide them.
fn skipped_summary(warnings: &[String]) -> String {
    const SHOWN: usize = 3;
    let shown = warnings.len().min(SHOWN);
    let mut text = warnings[..shown].join("; ");
    if warnings.len() > shown {
        text.push_str(&format!("; and {} more", warnings.len() - shown));
    }
    text
}

/// A unit APO writes after (or against) a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Hz,
    Khz,
    Db,
    Oct,
}

fn unit_of(token: &str) -> Option<Unit> {
    match token.to_ascii_lowercase().as_str() {
        "hz" => Some(Unit::Hz),
        "khz" => Some(Unit::Khz),
        "db" => Some(Unit::Db),
        "oct" => Some(Unit::Oct),
        _ => None,
    }
}

/// Splits a trailing unit glued to a number: `3.4dB` → `("3.4", Some(Db))`.
fn split_unit(token: &str) -> (&str, Option<Unit>) {
    let digits_end = token
        .trim_end_matches(|c: char| c.is_ascii_alphabetic())
        .len();
    let (digits, suffix) = token.split_at(digits_end);
    (digits, unit_of(suffix))
}

/// The number at `tokens[index]`, with an attached or following unit, and the index after it.
fn number_at(
    tokens: &[&str],
    start: usize,
) -> Result<Option<(f64, Option<Unit>, usize)>, ApoError> {
    // A unit may precede the value, as in `BW Oct 1.0`.
    let mut index = start;
    let mut unit = tokens.get(index).copied().and_then(unit_of);
    if unit.is_some() {
        index += 1;
    }
    let Some(token) = tokens.get(index).copied() else {
        return Ok(None);
    };
    if token.contains(',') {
        return Err(ApoError::COMMA_DECIMAL);
    }
    let (digits, attached) = split_unit(token);
    let Ok(value) = digits.parse::<f64>() else {
        return Ok(None);
    };
    unit = unit.or(attached);
    let mut next = index + 1;
    if unit.is_none()
        && let Some(found) = tokens.get(next).and_then(|token| unit_of(token))
    {
        unit = Some(found);
        next += 1;
    }
    Ok(Some((value, unit, next)))
}

/// APO's bandwidth in octaves as a Q, the inverse of `Q = √(2ⁿ) / (2ⁿ − 1)`, or `None` when
/// the value cannot be a bandwidth (`0`, negative, or so large `2ⁿ` overflows).
fn bandwidth_to_q(octaves: f64) -> Option<f64> {
    let factor = 2f64.powf(octaves);
    (factor.is_finite() && factor > 1.0).then(|| factor.sqrt() / (factor - 1.0))
}

/// A quoted fragment in a warning, cut to a readable length.
fn short(text: &str) -> String {
    const MAX: usize = 60;
    if text.chars().count() <= MAX {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(MAX).collect();
    cut.push('…');
    cut
}

/// Keeps the constants and `MAX_BANDS` in view of the module's own tests.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::peq::MAX_BANDS;

    fn band(kind: BiquadKind, freq_hz: f64, gain_db: f64, q: f64, enabled: bool) -> PeqBand {
        PeqBand {
            kind,
            freq_hz,
            gain_db,
            q,
            enabled,
        }
    }

    /// The shape AutoEQ publishes for a headphone: a preamp, bell bands, and shelf ends.
    const AUTOEQ: &str = "\
Preamp: -6.2 dB
Filter 1: ON PK Fc 105 Hz Gain 3.4 dB Q 0.70
Filter 2: ON PK Fc 200 Hz Gain -1.1 dB Q 1.20
Filter 3: ON PK Fc 450 Hz Gain 1.5 dB Q 2.00
Filter 4: ON PK Fc 1000 Hz Gain 2.2 dB Q 1.41
Filter 5: ON PK Fc 2000 Hz Gain -2.8 dB Q 0.90
Filter 6: ON PK Fc 3000 Hz Gain 4.0 dB Q 1.50
Filter 7: ON PK Fc 4500 Hz Gain -1.0 dB Q 3.00
Filter 8: ON PK Fc 6000 Hz Gain 3.5 dB Q 2.50
Filter 9: ON PK Fc 12000 Hz Gain -1.5 dB Q 1.10
Filter 10: ON PK Fc 16000 Hz Gain 2.0 dB Q 0.80
Filter 11: ON LSC Fc 105 Hz Gain -3.0 dB Q 0.70
Filter 12: ON HSC Fc 10000 Hz Gain -2.0 dB Q 0.70
";

    /// Every type, both unit spellings, a missing Q, a bandwidth and a disabled band.
    const EVERY_TYPE: &str = "\u{feff}# hand-written\r\n\
Preamp: -4.5 dB\r\n\
Filter 1: ON PK Fc 1000 Hz Gain 2 dB Q 1.0\r\n\
Filter 2: ON PEQ Fc 200 Hz Gain -3 dB Q 0.8\r\n\
Filter 3: ON LSC Fc 60 Hz Gain 4 dB Q 0.7\r\n\
Filter 4: ON LS Fc 60 Hz Gain 4 dB\r\n\
Filter 5: ON HSC Fc 8000 Hz Gain -2 dB Q 0.6\r\n\
Filter 6: ON HS Fc 8000 Hz Gain -2 dB\r\n\
Filter 7: ON LP Fc 18000 Hz\r\n\
Filter 8: ON LPQ Fc 18000 Hz Q 0.7\r\n\
Filter 9: ON HP Fc 30 Hz\r\n\
Filter 10: OFF HPQ Fc 30 Hz Q 0.7\r\n\
Filter 11: ON BP Fc 1000 Hz Q 5\r\n\
Filter 12: ON NO Fc 1000 Hz Q 5\r\n\
Filter 13: ON PK Fc 1.5 kHz Gain 1 dB BW Oct 1\r\n";

    #[test]
    fn reads_the_autoeq_shape() {
        let import = Peq::from_apo_text(AUTOEQ).unwrap();
        assert!(import.warnings.is_empty(), "{:?}", import.warnings);
        assert_eq!(import.peq.preamp_db, -6.2);
        assert_eq!(import.peq.bands.len(), 12);
        assert_eq!(
            import.peq.bands.first(),
            Some(&band(BiquadKind::Peak, 105.0, 3.4, 0.70, true))
        );
        assert_eq!(import.peq.bands[10].kind, BiquadKind::LowShelf);
        assert_eq!(import.peq.bands[10].gain_db, -3.0);
        assert_eq!(import.peq.bands[11].kind, BiquadKind::HighShelf);
    }

    #[test]
    fn maps_every_type_and_both_unit_spellings() {
        let import = Peq::from_apo_text(EVERY_TYPE).unwrap();
        let kinds: Vec<BiquadKind> = import.peq.bands.iter().map(|b| b.kind).collect();
        assert_eq!(
            kinds,
            [
                BiquadKind::Peak,
                BiquadKind::Peak,
                BiquadKind::LowShelf,
                BiquadKind::LowShelf,
                BiquadKind::HighShelf,
                BiquadKind::HighShelf,
                BiquadKind::LowPass,
                BiquadKind::LowPass,
                BiquadKind::HighPass,
                BiquadKind::HighPass,
                BiquadKind::BandPass,
                BiquadKind::Notch,
                BiquadKind::Peak,
            ]
        );
        assert_eq!(import.peq.preamp_db, -4.5);
        // No Q on the shelf and pass lines: the cookbook value.
        assert_eq!(import.peq.bands[3].q, DEFAULT_Q);
        assert_eq!(import.peq.bands[6].q, DEFAULT_Q);
        assert_eq!(import.peq.bands[8].q, DEFAULT_Q);
        // OFF keeps the band, disabled.
        assert!(!import.peq.bands[9].enabled);
        assert_eq!(import.peq.bands[9].freq_hz, 30.0);
        // `Fc 1.5 kHz`; `BW Oct 1` is Q √2 / (2 − 1).
        let last = import.peq.bands[12];
        assert_eq!(last.freq_hz, 1_500.0);
        assert!(
            (last.q - std::f64::consts::SQRT_2).abs() < 1e-12,
            "{}",
            last.q
        );
        assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    }

    #[test]
    fn skips_other_stages_and_names_each_skip() {
        let text = format!(
            "{AUTOEQ}Channel: all\nGraphicEQ: 20 -3.0; 21 -2.9\n\
             Filter 13: ON XX Fc 100 Hz Gain 1 dB Q 1\n\
             Filter 14: ON PK Gain 1 dB Q 1\n"
        );
        let import = Peq::from_apo_text(&text).unwrap();
        assert_eq!(import.peq.bands.len(), 12);
        assert_eq!(import.warnings.len(), 4, "{:?}", import.warnings);
        assert!(import.warnings[0].contains("Channel: all"));
        assert!(import.warnings[1].contains("GraphicEQ"));
        assert!(import.warnings[2].contains("unknown filter type 'XX'"));
        assert!(import.warnings[3].contains("no frequency"));
    }

    #[test]
    fn clamps_out_of_range_values_with_a_warning_each() {
        let text = "\
Preamp: 40 dB
Filter 1: ON PK Fc 40000 Hz Gain 40 dB Q 0.01
";
        let import = Peq::from_apo_text(text).unwrap();
        assert_eq!(import.peq.preamp_db, 12.0);
        let band = import.peq.bands[0];
        assert_eq!((band.freq_hz, band.gain_db, band.q), (24_000.0, 24.0, 0.1));
        assert_eq!(import.warnings.len(), 4, "{:?}", import.warnings);
    }

    #[test]
    fn truncates_past_the_band_limit_with_a_warning() {
        let text: String = std::iter::once("Preamp: 0 dB\n".to_owned())
            .chain((1..=40).map(|i| format!("Filter {i}: ON PK Fc 100 Hz Gain 1 dB Q 1\n")))
            .collect();
        let import = Peq::from_apo_text(&text).unwrap();
        assert_eq!(import.peq.bands.len(), MAX_BANDS);
        assert_eq!(import.warnings.len(), 1, "{:?}", import.warnings);
        assert!(import.warnings[0].contains("kept the first 32 of 40 bands"));
    }

    #[test]
    fn rejects_text_that_is_not_a_parametric_file() {
        let error = Peq::from_apo_text("GraphicEQ: 20 -3.0; 21 -2.9\nChannel: all\n").unwrap_err();
        assert_eq!(error.message(), ApoError::NOT_PARAMETRIC.message());
        // The lines were there, in shapes APO would not read; the error names them.
        assert!(error.to_string().contains("GraphicEQ"), "{error}");
        assert!(Peq::from_apo_text("").is_err());
        // A bare preamp is enough.
        assert_eq!(
            Peq::from_apo_text("Preamp: -3 dB\n").unwrap().peq.preamp_db,
            -3.0
        );
    }

    #[test]
    fn rejects_a_comma_decimal_instead_of_misreading_it() {
        let text = "Filter 1: ON PK Fc 100,5 Hz Gain 1 dB Q 1\n";
        let error = Peq::from_apo_text(text).unwrap_err();
        assert_eq!(error, ApoError::COMMA_DECIMAL);
        assert!(error.message().contains("'.'"));
        let preamp = "Preamp: -6,2 dB\n".to_owned() + AUTOEQ;
        assert_eq!(
            Peq::from_apo_text(&preamp).unwrap_err(),
            ApoError::COMMA_DECIMAL
        );
        // A thousands separator gets the same wording, which now covers it.
        let thousands = Peq::from_apo_text("Filter 1: ON PK Fc 1,000 Hz Gain 1 dB Q 1\n")
            .unwrap_err()
            .to_string();
        assert!(thousands.contains("thousands separators"), "{thousands}");
    }

    #[test]
    fn an_unusable_file_says_what_was_wrong_with_its_lines() {
        // A Filter line is there, but without ON/OFF, so it cannot be used: the error has to
        // say that rather than claim the file has no Filter line.
        let error = Peq::from_apo_text("Filter 1: PK Fc 100 Hz Gain 1 dB Q 1\n").unwrap_err();
        assert_eq!(error.message(), ApoError::NOTHING_USABLE.message());
        assert!(error.to_string().contains("is not ON/OFF"), "{error}");
        // A preamp line that does not parse is reported the same way.
        let error = Peq::from_apo_text("Preamp: quiet dB\n").unwrap_err();
        assert!(error.to_string().contains("is not a number"), "{error}");
    }

    #[test]
    fn a_repeated_key_is_reported_and_the_last_value_wins() {
        let text = "\
Preamp: -1 dB
Preamp: -2.5 dB
Filter 1: ON PK Fc 100 Hz Fc 200 Hz Gain 1 dB Q 1
";
        let import = Peq::from_apo_text(text).unwrap();
        assert_eq!(import.peq.preamp_db, -2.5);
        assert_eq!(import.peq.bands[0].freq_hz, 200.0);
        assert_eq!(import.warnings.len(), 2, "{:?}", import.warnings);
        assert!(import.warnings[0].contains("Preamp given twice"));
        assert!(import.warnings[1].contains("'fc' given twice"));
    }

    #[test]
    fn q_and_bandwidth_take_the_later_value_and_say_so() {
        let text = "\
Filter 1: ON PK Fc 105 Hz Gain 3.4 dB BW Oct 1 Q 0.70
Filter 2: ON PK Fc 105 Hz Gain 3.4 dB Q 0.70 BW Oct 1
";
        let import = Peq::from_apo_text(text).unwrap();
        assert_eq!(import.peq.bands[0].q, 0.70);
        assert!((import.peq.bands[1].q - std::f64::consts::SQRT_2).abs() < 1e-12);
        assert_eq!(import.warnings.len(), 2, "{:?}", import.warnings);
        assert!(
            import.warnings[0].contains("follows another Q or BW"),
            "{:?}",
            import.warnings
        );
    }

    #[test]
    fn an_unusable_bandwidth_warns_instead_of_becoming_the_default_silently() {
        let import =
            Peq::from_apo_text("Filter 1: ON PK Fc 105 Hz Gain 3.4 dB Q 2 BW Oct 0\n").unwrap();
        assert_eq!(import.peq.bands[0].q, 2.0);
        assert!(
            import.warnings.iter().any(|w| w.contains("not usable")),
            "{:?}",
            import.warnings
        );

        let import =
            Peq::from_apo_text("Filter 1: ON PK Fc 105 Hz Gain 3.4 dB BW Oct -1\n").unwrap();
        assert_eq!(import.peq.bands[0].q, DEFAULT_Q);
        assert!(
            import.warnings.iter().any(|w| w.contains("not usable")),
            "{:?}",
            import.warnings
        );
    }

    #[test]
    fn a_unit_glued_to_its_value_reads_the_same() {
        let import =
            Peq::from_apo_text("Preamp: -1.5dB\nFilter 1: ON PK Fc 1.2kHz Gain 3.4dB Q 0.7\n")
                .unwrap();
        assert_eq!(import.peq.preamp_db, -1.5);
        let band = import.peq.bands[0];
        assert_eq!((band.freq_hz, band.gain_db, band.q), (1_200.0, 3.4, 0.7));
        assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    }

    #[test]
    fn a_comment_after_the_values_says_where_it_belongs() {
        let import =
            Peq::from_apo_text("Preamp: -2 dB\nFilter 1: ON PK Fc 100 Hz Gain 1 dB Q 1 # bell\n")
                .unwrap();
        assert_eq!(import.peq.bands.len(), 0);
        assert!(
            import.warnings[0].contains("own line"),
            "{:?}",
            import.warnings
        );
    }

    #[test]
    fn a_long_skipped_line_is_not_echoed_whole() {
        let graphic = format!("GraphicEQ: {}\n", "20 -3.0; ".repeat(80));
        let error = Peq::from_apo_text(&graphic).unwrap_err().to_string();
        assert!(error.contains('…'), "{error}");
        assert!(error.len() < 300, "{} chars: {error}", error.len());
    }

    #[test]
    fn exports_what_it_reads() {
        let peq = Peq {
            preamp_db: -6.25,
            bands: vec![
                band(BiquadKind::Peak, 105.0, 3.4, 0.7, true),
                band(BiquadKind::LowShelf, 60.0, 4.0, 0.707, false),
                band(BiquadKind::HighShelf, 8_000.0, -2.0, 0.6, true),
                band(BiquadKind::LowPass, 18_000.0, 0.0, 0.707, true),
                band(BiquadKind::HighPass, 30.0, 0.0, 0.707, true),
                band(BiquadKind::BandPass, 1_000.0, 0.0, 5.0, true),
                band(BiquadKind::Notch, 1_000.0, 0.0, 5.0, true),
            ],
        }
        .sanitized();
        let text = peq.to_apo_text();
        assert!(
            text.starts_with("Preamp: -6.25 dB\nFilter 1: ON PK Fc 105 Hz Gain 3.4 dB Q 0.7\n")
        );
        assert!(text.contains("Filter 2: OFF LSC Fc 60 Hz Gain 4 dB Q 0.707\n"));
        assert!(text.ends_with("Filter 7: ON NO Fc 1000 Hz Gain 0 dB Q 5\n"));
        assert!(!text.contains('\r'));
        let import = Peq::from_apo_text(&text).unwrap();
        assert!(import.warnings.is_empty(), "{:?}", import.warnings);
        assert_eq!(import.peq, peq);
    }
}
