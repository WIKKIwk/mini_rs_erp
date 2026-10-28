use std::{hint::black_box, time::Instant};

use mini_rs_erp::{core::production_map::ProductionMapDefinition, http::json_research::Engine};
use serde::Serialize;
use serde_json::{Value, json};

use super::{Options, summary};

/// Alternate engine order inside every sample; fixtures and validation are
/// outside the timer. Every operation includes output allocation and drop.
pub fn encode<T: Serialize + ?Sized>(name: &str, input: &T, options: &Options) -> Value {
    let baseline = Engine::SerdeJson.to_vec(input).unwrap();
    let candidate = Engine::SonicRs.to_vec(input).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&baseline).unwrap(),
        serde_json::from_slice::<Value>(&candidate).unwrap(),
        "{name}"
    );
    let mut timings = [Vec::new(), Vec::new()];
    for sample in 0..options.samples + 5 {
        for index in if sample % 2 == 0 { [0, 1] } else { [1, 0] } {
            let engine = [Engine::SerdeJson, Engine::SonicRs][index];
            let started = Instant::now();
            for _ in 0..options.batch {
                black_box(engine.to_vec(black_box(input)).unwrap());
            }
            if sample >= 5 {
                timings[index].push(started.elapsed().as_secs_f64() * 1e6 / options.batch as f64);
            }
        }
    }
    json!({"case":name,"kind":"codec_encode","serde_bytes":baseline.len(),
        "sonic_bytes":candidate.len(),"byte_identical":baseline==candidate,
        "serde_json":summary(&timings[0]),"sonic_rs":summary(&timings[1]),
        "samples":options.samples,"batch":options.batch})
}

pub fn decode(name: &str, input: &[u8], options: &Options) -> Value {
    let expected: ProductionMapDefinition = Engine::SerdeJson.from_slice(input).unwrap();
    let candidate: ProductionMapDefinition = Engine::SonicRs.from_slice(input).unwrap();
    assert_eq!(expected, candidate);
    let mut timings = [Vec::new(), Vec::new()];
    for sample in 0..options.samples + 5 {
        for index in if sample % 2 == 0 { [0, 1] } else { [1, 0] } {
            let engine = [Engine::SerdeJson, Engine::SonicRs][index];
            let started = Instant::now();
            for _ in 0..options.batch {
                black_box(
                    engine
                        .from_slice::<ProductionMapDefinition>(black_box(input))
                        .unwrap(),
                );
            }
            if sample >= 5 {
                timings[index].push(started.elapsed().as_secs_f64() * 1e6 / options.batch as f64);
            }
        }
    }
    json!({"case":name,"kind":"codec_decode","input_bytes":input.len(),
        "serde_json":summary(&timings[0]),"sonic_rs":summary(&timings[1]),
        "samples":options.samples,"batch":options.batch})
}

/// A deterministic numerical compatibility probe, not production traffic or a
/// count of real calculation errors. Raw Sonic is reported separately from
/// the guarded request decoder actually installed in the Router.
pub fn numeric_probe() -> Value {
    let mut seed = 123456789u64;
    let (mut raw_mismatches, mut integrated_mismatches) = (0, 0);
    let mut examples = Vec::new();
    for index in 0..10_000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let raw = format!(
            "{}.{}e{}",
            seed % 10000,
            seed % 1000000000000000000,
            index % 200 - 100
        );
        let baseline: f64 = serde_json::from_str(&raw).unwrap();
        let sonic: f64 = sonic_rs::from_str(&raw).unwrap();
        let integrated: f64 = Engine::SonicRs.from_slice(raw.as_bytes()).unwrap();
        if baseline.to_bits() != sonic.to_bits() {
            raw_mismatches += 1;
            if examples.len() < 5 {
                examples.push(json!({"input":raw,"serde_bits":baseline.to_bits(),"sonic_bits":sonic.to_bits()}));
            }
        }
        if baseline.to_bits() != integrated.to_bits() {
            integrated_mismatches += 1;
        }
    }
    assert_eq!(integrated_mismatches, 0);
    json!({"case":"numeric_compatibility_probe","kind":"compatibility","inputs":10000,
        "raw_sonic_float_bit_mismatches":raw_mismatches,
        "integrated_decoder_float_bit_mismatches":integrated_mismatches,"examples":examples})
}
