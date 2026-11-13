//! Writes the test files for the real-device checks in `docs/verification.md` into a folder:
//! a 1 kHz sine at −20 dBFS for 10 s as FLAC 16/44.1, 24/96 and 24/192 and WAV 24/192, and
//! stereo Dirac impulse responses for the convolution listening check.
//!
//! ```sh
//! cargo run -p sonder-verify --example hardware_fixtures -- <folder outside the repository>
//! ```

use std::path::PathBuf;

use sonder_verify::fixtures::{dirac, sine, write_flac, write_wav};

fn main() -> std::io::Result<()> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: hardware_fixtures <output folder>"),
    );
    std::fs::create_dir_all(&dir)?;
    for (rate, bits) in [(44_100, 16), (96_000, 24), (192_000, 24)] {
        let path = dir.join(format!("sine-1k-20dbfs-{bits}-{rate}.flac"));
        write_flac(&path, rate, bits, &sine(rate, bits, 1000.0, -20.0, 10.0))?;
        println!("{}", path.display());
    }
    let path = dir.join("sine-1k-20dbfs-24-192000.wav");
    write_wav(&path, 192_000, 24, &sine(192_000, 24, 1000.0, -20.0, 10.0))?;
    println!("{}", path.display());
    for rate in [44_100, 48_000, 96_000] {
        let path = dir.join(format!("dirac-{rate}.wav"));
        write_wav(&path, rate, 24, &dirac(24, 1024))?;
        println!("{}", path.display());
    }
    Ok(())
}
