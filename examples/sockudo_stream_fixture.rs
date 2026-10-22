//! Synthetic, loopback-only transport fixture. No database or application data.
//!
//! Build with `cargo build --release --example sockudo_stream_fixture` and run
//! through `tools/benchmarks/sockudo_stream_benchmark.py --binary ...`.
//! The baseline route deliberately uses the same Axum/tungstenite versions as
//! commit 95d1863. All routes reuse identical serialized application messages.

use axum::{
    Json, Router,
    extract::{Query, State, ws as baseline},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use mini_rs_erp::http::ws::{self, Message, WebSocketUpgrade};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sockudo_ws::{Compression, Config, DeflateConfig};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};

// Include the production implementation, so the comparison cannot silently
// benchmark a separately reimplemented delta algorithm.
#[path = "../src/http/handlers/admin/production_maps/sequence_delta.rs"]
mod sequence_delta;

const APPARATUS: &str = "synthetic-apparatus";
const EPOCH: &str = "synthetic-benchmark-epoch";
const SCOPE: &str = "synthetic-worker-scope";

#[derive(Clone, Deserialize)]
struct Parameters {
    count: usize,
    updates: usize,
    #[serde(default = "default_protocol")]
    protocol: String,
    #[serde(default = "default_scenario")]
    scenario: String,
    #[serde(default = "default_profile")]
    profile: String,
    #[serde(default = "default_level")]
    level: u32,
    #[serde(default = "default_window")]
    window: u8,
    #[serde(default = "default_threshold")]
    threshold: usize,
}

fn default_protocol() -> String {
    "v1".into()
}
fn default_scenario() -> String {
    "reorder".into()
}
fn default_profile() -> String {
    "production".into()
}
fn default_level() -> u32 {
    3
}
fn default_window() -> u8 {
    15
}
fn default_threshold() -> usize {
    128
}

struct Dataset {
    frames: Vec<String>,
    generation_ns: Vec<u64>,
    decoded_bytes: usize,
}

type Cache = Arc<Mutex<HashMap<String, Arc<Dataset>>>>;

fn identifier(value: usize) -> String {
    format!("00000000-0000-4000-8000-{value:012x}")
}

fn valid(parameters: &Parameters) -> bool {
    (1..=10_000).contains(&parameters.count)
        && (1..=512).contains(&parameters.updates)
        && matches!(parameters.protocol.as_str(), "v1" | "v2")
        && matches!(
            parameters.scenario.as_str(),
            "reorder" | "replace" | "status" | "snapshot"
        )
        && matches!(
            parameters.profile.as_str(),
            "production" | "disabled" | "dedicated" | "shared"
        )
        && parameters.level <= 9
        && (9..=15).contains(&parameters.window)
        && parameters.threshold <= 65_536
}

fn snapshot(sequence: &[String], revision: usize, status: &Value) -> Value {
    json!({
        "type": "snapshot", "epoch": EPOCH, "scope": SCOPE, "rev": revision,
        "sequences": {APPARATUS: sequence}, "visible_order_ids": {APPARATUS: sequence},
        "order_statuses": status,
    })
}

fn list_patch(before: &[String], after: &[String], protocol: &str) -> Value {
    if protocol == "v2" {
        if let Some(edit) = sequence_delta::sequence_edit(before, after) {
            return json!({"upsert": {}, "remove": [], "edit": {APPARATUS: edit}});
        }
    }
    json!({"upsert": {APPARATUS: after}, "remove": []})
}

fn dataset(parameters: &Parameters, cache: &Cache) -> Arc<Dataset> {
    let key = format!(
        "{}:{}:{}:{}",
        parameters.count, parameters.updates, parameters.protocol, parameters.scenario
    );
    let mut cache = cache.lock().unwrap();
    if let Some(existing) = cache.get(&key) {
        return existing.clone();
    }
    let mut sequence: Vec<String> = (0..parameters.count).map(identifier).collect();
    let mut statuses = json!({});
    for item in &sequence {
        statuses[item] = json!("queued");
    }
    let mut frames = vec![snapshot(&sequence, 0, &statuses).to_string()];
    let mut generation_ns = Vec::with_capacity(parameters.updates);
    for step in 0..parameters.updates {
        let started = Instant::now();
        let before = sequence.clone();
        let mut patch = serde_json::Map::new();
        match parameters.scenario.as_str() {
            "reorder" | "snapshot" if sequence.len() > 1 => {
                let source = (step * 7_919) % sequence.len();
                let target = (step * 104_729 + 1) % sequence.len();
                let item = sequence.remove(source);
                sequence.insert(target, item);
            }
            "replace" => {
                let index = (step * 7_919) % sequence.len();
                sequence[index] = identifier(parameters.count + step);
            }
            "status" => {
                let item = &sequence[(step * 7_919) % sequence.len()];
                let status = if step % 2 == 0 {
                    "in_progress"
                } else {
                    "queued"
                };
                statuses[item] = json!(status);
                patch.insert(
                    "order_statuses".into(),
                    json!({"upsert": {item: status}, "remove": []}),
                );
            }
            _ => {}
        }
        let frame = if parameters.scenario == "snapshot" {
            snapshot(&sequence, step + 1, &statuses).to_string()
        } else {
            if before != sequence {
                let changes = list_patch(&before, &sequence, &parameters.protocol);
                patch.insert("sequences".into(), changes.clone());
                patch.insert("visible_order_ids".into(), changes);
            }
            json!({"type": "state_delta", "epoch": EPOCH, "scope": SCOPE,
                "base_rev": step, "rev": step + 1, "patch": patch})
            .to_string()
        };
        generation_ns.push(started.elapsed().as_nanos() as u64);
        frames.push(frame);
    }
    let decoded_bytes = frames.iter().map(String::len).sum();
    let value = Arc::new(Dataset {
        frames,
        generation_ns,
        decoded_bytes,
    });
    // Retain only the current dataset. Connected clients own their Arc, so a
    // later case cannot invalidate an in-flight stream or accumulate all runs.
    cache.clear();
    cache.insert(key, value.clone());
    value
}

async fn prepare(State(cache): State<Cache>, Query(parameters): Query<Parameters>) -> Response {
    if !valid(&parameters) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let dataset = dataset(&parameters, &cache);
    Json(json!({"frames": dataset.frames.len(), "decoded_bytes": dataset.decoded_bytes,
        "bootstrap_bytes": dataset.frames[0].len(),
        "update_generation_ns": dataset.generation_ns,
        "update_decoded_bytes": dataset.frames.iter().skip(1).map(String::len).collect::<Vec<_>>(),
    })).into_response()
}

async fn old_route(
    State(cache): State<Cache>,
    Query(parameters): Query<Parameters>,
    upgrade: baseline::WebSocketUpgrade,
) -> Response {
    if !valid(&parameters) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let data = dataset(&parameters, &cache);
    upgrade
        .on_upgrade(move |mut socket| async move {
            if socket
                .send(baseline::Message::Text(data.frames[0].clone().into()))
                .await
                .is_err()
            {
                return;
            }
            let mut index = 1;
            while let Some(Ok(message)) = socket.recv().await {
                match message {
                    baseline::Message::Text(_) => {
                        if index >= data.frames.len() {
                            break;
                        }
                        if socket
                            .send(baseline::Message::Text(data.frames[index].clone().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                        index += 1;
                    }
                    baseline::Message::Close(_) => break,
                    _ => {}
                }
            }
        })
        .into_response()
}

fn configuration(parameters: &Parameters) -> Config {
    let mut config = ws::low_traffic_config();
    match parameters.profile.as_str() {
        "production" => {}
        "disabled" => {
            config.compression = Compression::Disabled;
            config.deflate = None;
        }
        "dedicated" | "shared" => {
            let shared = parameters.profile == "shared";
            config.compression = if shared {
                Compression::Shared
            } else {
                Compression::Dedicated
            };
            config.deflate = Some(DeflateConfig {
                server_max_window_bits: parameters.window,
                client_max_window_bits: parameters.window,
                server_no_context_takeover: shared,
                client_no_context_takeover: shared,
                compression_level: parameters.level,
                compression_threshold: parameters.threshold,
            });
        }
        _ => unreachable!(),
    }
    config
}

async fn new_route(
    State(cache): State<Cache>,
    Query(parameters): Query<Parameters>,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !valid(&parameters) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let config = configuration(&parameters);
    let data = dataset(&parameters, &cache);
    upgrade
        .config(config)
        .on_upgrade(move |mut socket| async move {
            if socket
                .send(Message::Text(data.frames[0].clone().into()))
                .await
                .is_err()
            {
                return;
            }
            let mut index = 1;
            while let Some(Ok(message)) = socket.recv().await {
                match message {
                    Message::Text(_) => {
                        if index >= data.frames.len() {
                            break;
                        }
                        if socket
                            .send(Message::Text(data.frames[index].clone().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                        index += 1;
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        })
        .into_response()
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    let cache: Cache = Arc::new(Mutex::new(HashMap::new()));
    let router = Router::new()
        .route("/prepare", get(prepare))
        .route("/baseline", get(old_route))
        .route("/sockudo", get(new_route))
        .with_state(cache);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut inputs = serde_json::Map::new();
    macro_rules! input {
        ($path:literal) => {
            inputs.insert(
                $path.into(),
                json!(format!(
                    "{:x}",
                    Sha256::digest(include_bytes!(concat!("../", $path)))
                )),
            );
        };
    }
    input!("Cargo.lock");
    input!("examples/sockudo_stream_fixture.rs");
    input!("src/http/ws.rs");
    input!("src/config.rs");
    input!("src/http/router.rs");
    input!("src/http/router/cors.rs");
    input!("src/http/handlers/admin/production_maps/sequence_delta.rs");
    input!("third_party/sockudo-ws/src/lib.rs");
    input!("third_party/sockudo-ws/src/axum_integration.rs");
    input!("third_party/sockudo-ws/src/deflate.rs");
    input!("third_party/sockudo-ws/src/compression.rs");
    input!("third_party/sockudo-ws/src/protocol.rs");
    input!("third_party/sockudo-ws/src/stream/websocket.rs");
    println!(
        "{}",
        json!({"port": listener.local_addr().unwrap().port(),
        "baseline": "axum0.8.9/tungstenite0.29.0", "dataset": "synthetic",
        "build_inputs_sha256": inputs})
    );
    axum::serve(listener, router).await.unwrap();
}
