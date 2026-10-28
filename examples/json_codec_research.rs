//! Synthetic codec and actual Router/WS research. No production database/data.
//! See docs/sonic-json-research.md for the reproducible, opt-in comparison.
#[path = "json_codec_research/fixture.rs"]
mod fixture;
#[path = "json_codec_research/micro.rs"]
mod micro;
#[path = "json_codec_research/routes.rs"]
mod routes;

use serde_json::{Value, json};

pub struct Options {
    pub orders: usize,
    pub samples: usize,
    pub batch: usize,
}

pub fn summary(values: &[f64]) -> Value {
    assert!(!values.is_empty());
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    json!({"unit":"us","p50":sorted[sorted.len()/2],
        "p95":sorted[(sorted.len()*95).div_ceil(100)-1],
        "min":sorted[0],"max":sorted[sorted.len()-1],"samples":values.len(),"samples_us":values})
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = std::env::args().collect();
    if !arguments.iter().any(|value| value == "--synthetic-fixture") {
        return Err(
            "requires --synthetic-fixture; run in an empty temporary working directory".into(),
        );
    }
    if std::path::Path::new("Cargo.toml").exists() || std::path::Path::new(".env").exists() {
        return Err(
            "run this fixture in an empty temporary directory, not the project or a deployment"
                .into(),
        );
    }
    let number = |flag: &str, default| -> Result<usize, Box<dyn std::error::Error>> {
        arguments
            .iter()
            .position(|value| value == flag)
            .map(|index| {
                arguments
                    .get(index + 1)
                    .ok_or_else(|| format!("missing {flag} value"))?
                    .parse::<usize>()
                    .map_err(|error| error.to_string())
            })
            .transpose()
            .map(|value| value.unwrap_or(default))
            .map_err(Into::into)
    };
    let options = Options {
        orders: number("--orders", 285)?,
        samples: number("--samples", 31)?,
        batch: number("--batch", 10)?,
    };
    if !(1..=2000).contains(&options.orders)
        || !(5..=1000).contains(&options.samples)
        || !(1..=1000).contains(&options.batch)
    {
        return Err("orders 1..=2000, samples 5..=1000, batch 1..=1000 required".into());
    }
    let codecs = mini_rs_erp::http::json_research::initialize_from_env()?;
    let fixture = fixture::Fixture::new(options.orders).await;
    let datasets = routes::datasets(&fixture).await;
    let mut cases = Vec::new();
    for (name, value) in &datasets {
        cases.push(micro::encode(name, value, &options));
    }
    let snapshot = fixture
        .state
        .production_maps
        .live_snapshot_shared()
        .await
        .unwrap();
    cases.push(micro::encode(
        "typed_domain_snapshot",
        snapshot.as_ref(),
        &options,
    ));
    cases.push(micro::encode("typed_map_list", &snapshot.maps, &options));
    let map = fixture::map(0, 0);
    cases.push(micro::decode(
        "production_map_request",
        &serde_json::to_vec(&map)?,
        &options,
    ));
    cases.push(micro::decode(
        "integer_map_request",
        br#"{"id":"research","product_code":"P-001","title":"Packaging label","roll_count":12}"#,
        &options,
    ));
    cases.push(micro::numeric_probe());
    cases.extend(routes::http(&fixture, &options).await);
    cases.extend(routes::websockets(&fixture, &options).await);
    cases.push(routes::request_decode(&fixture, &options).await);
    println!(
        "{}",
        json!({"schema":1,"synthetic":true,"production_measurement":false,
        "arch":std::env::consts::ARCH,"encoder":codecs.encoder.name(),"decoder":codecs.decoder.name(),
        "target_features":{"neon":cfg!(target_feature="neon"),"avx2":cfg!(target_feature="avx2")},
        "orders":options.orders,"samples":options.samples,"batch":options.batch,"cases":cases})
    );
    Ok(())
}
