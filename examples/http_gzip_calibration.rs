//! Measure the active flate2 backend on a private HTTP body supplied on stdin.
//! Emits byte counts and timings only; never persists or prints the input.
use std::io::{Read, Write};
use std::time::Instant;

use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rounds = std::env::args()
        .nth(1)
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or(21);
    if !(1..=1_000).contains(&rounds) {
        return Err("rounds must be in 1..=1000".into());
    }
    let mut input = Vec::new();
    std::io::stdin()
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut input)?;
    if input.is_empty() || input.len() > 64 * 1024 * 1024 {
        return Err("expected a nonempty HTTP body up to 64 MiB on stdin".into());
    }
    let mut results = Vec::new();
    for level in 3..=9 {
        let mut durations = Vec::with_capacity(rounds);
        let mut size = None;
        for sample in 0..=rounds {
            let started = Instant::now();
            let mut encoder = GzEncoder::new(Vec::new(), Compression::new(level));
            encoder.write_all(&input)?;
            let output = encoder.finish()?;
            let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
            if let Some(size) = size {
                if output.len() != size {
                    return Err("nondeterministic gzip output size".into());
                }
            } else {
                let mut decoded = Vec::new();
                GzDecoder::new(output.as_slice()).read_to_end(&mut decoded)?;
                if decoded != input {
                    return Err("gzip round-trip mismatch".into());
                }
                size = Some(output.len());
            }
            if sample > 0 {
                durations.push(elapsed);
            }
        }
        durations.sort_by(f64::total_cmp);
        results.push(json!({
            "level":level,"gzip_bytes":size.unwrap(),
            "compression_p50_ms":durations[durations.len()/2],
            "compression_p95_ms":durations[(durations.len()*95).div_ceil(100)-1],
        }));
    }
    println!(
        "{}",
        json!({"input_bytes":input.len(),"rounds":rounds,"results":results})
    );
    Ok(())
}
