//! Usage: cargo run --release --example jpeg_bench -- photo1.jpg photo2.jpg ...
//! Compares every mode against the legacy `sips` encoder at the old fixed qualities.
use std::{fs, path::Path, process::Command, time::Instant};

use squeezer_lib::engine::{images::CompressionMode, jpeg::compress_jpeg};
use tokio_util::sync::CancellationToken;

fn main() {
    let out = std::env::temp_dir().join("squeezer-bench");
    fs::create_dir_all(&out).unwrap();
    for arg in std::env::args().skip(1) {
        let input = Path::new(&arg);
        let original = fs::metadata(input).unwrap().len();
        println!("\n{}  original {} KB", input.display(), original / 1024);
        for (name, mode, sips_q) in [
            ("quality ", CompressionMode::Quality, "90"),
            ("balanced", CompressionMode::Balanced, "82"),
            ("small   ", CompressionMode::Small, "68"),
        ] {
            let sips_out = out.join(format!("sips-{sips_q}.jpg"));
            let _ = fs::remove_file(&sips_out);
            Command::new("/usr/bin/sips")
                .args(["-s", "format", "jpeg", "-s", "formatOptions", sips_q, "-o"])
                .arg(&sips_out)
                .arg(input)
                .output()
                .unwrap();
            let sips_bytes = fs::metadata(&sips_out).map(|m| m.len()).unwrap_or(0);
            let moz_out = out.join(format!("moz-{name}.jpg"));
            let started = Instant::now();
            let outcome = compress_jpeg(input, &moz_out, mode, true, &CancellationToken::new()).unwrap();
            let moz_bytes = fs::metadata(&moz_out).unwrap().len();
            println!(
                "  {name}  old sips q{sips_q}: {:>6} KB ({:>3}%)   new q{:<2} score {:.4}: {:>6} KB ({:>3}%)  {:.1}s  -> {}",
                sips_bytes / 1024,
                100 * sips_bytes / original,
                outcome.quality,
                outcome.score,
                moz_bytes / 1024,
                100 * moz_bytes / original,
                started.elapsed().as_secs_f32(),
                moz_out.display()
            );
        }
    }
}
