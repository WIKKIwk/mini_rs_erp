//! Opt-in, synthetic loopback benchmark. Never reads credentials or writes a
//! production database. Run with `cargo test sequence_position_stream_benchmark
//! --lib -- --ignored --nocapture --test-threads=1`.
//! For an identical pre-change source checkout only, set
//! ACCORD_BENCH_EXPECT_POSITIONS=0 to compare the old wire/performance contract.
use super::*;
use serde_json::{Value, json};
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const APPARATUS: &str = "apparatus:default:bosma_8";
const ORDER_COUNT: usize = 1000;
const SEQUENTIAL_WRITES: usize = 100;
const BURST_WRITES: usize = 513;
const RECOVERY_WRITES: usize = 20;

struct Server {
    address: std::net::SocketAddr,
    task: tokio::task::JoinHandle<()>,
    client: reqwest::Client,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Server {
    async fn start(state: AppState) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, build_router(state)).await.unwrap();
        });
        Self {
            address,
            task,
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap(),
        }
    }

    async fn page(&self, token: &str, search: &str) -> Value {
        let response = self
            .client
            .get(format!(
                "http://{}/v1/mobile/admin/production-maps/order-page",
                self.address
            ))
            .query(&[
                ("view", "sequence"),
                ("apparatus", APPARATUS),
                ("search", search),
            ])
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        response.json().await.unwrap()
    }

    async fn socket(&self, token: &str, pages: &[Value]) -> reqwest::Upgraded {
        let windows = json!([
            {"id":"all", "view":"sequence", "apparatus":APPARATUS,
             "length":20, "version":pages[0]["version"]},
            {"id":"filtered", "view":"sequence", "apparatus":APPARATUS,
             "search":"0003", "length":20, "version":pages[1]["version"]}
        ]);
        let response = self
            .client
            .get(format!(
                "http://{}/v1/mobile/admin/production-maps/live",
                self.address
            ))
            .query(&[
                ("protocol", "page_delta_v1"),
                ("pages", &windows.to_string()),
            ])
            .bearer_auth(token)
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
        response.upgrade().await.unwrap()
    }
}

async fn frame(socket: &mut reqwest::Upgraded) -> (Value, usize) {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let first = socket
                .read_u8()
                .await
                .expect("unexpected stream disconnect");
            let opcode = first & 15;
            let second = socket.read_u8().await.unwrap();
            assert_eq!(
                first & 0x80,
                0x80,
                "benchmark expects bounded complete frames"
            );
            assert_eq!(second & 0x80, 0, "server frame must not be masked");
            let (length, overhead) = match second & 127 {
                126 => (socket.read_u16().await.unwrap() as usize, 4),
                127 => (socket.read_u64().await.unwrap() as usize, 10),
                n => (n as usize, 2),
            };
            assert!(length < 256 * 1024, "stream payload must remain bounded");
            let mut payload = vec![0; length];
            socket.read_exact(&mut payload).await.unwrap();
            if opcode == 1 {
                return (serde_json::from_slice(&payload).unwrap(), length + overhead);
            }
            assert_ne!(opcode, 8, "unexpected server stream close");
            if opcode == 9 {
                assert!(length <= 125);
                let mut pong = vec![0x8a, 0x80 | length as u8, 0, 0, 0, 0];
                pong.extend(payload);
                socket.write_all(&pong).await.unwrap();
            }
        }
    })
    .await
    .expect("stream convergence timeout")
}

fn patch(target: &mut Value, path: &[Value], operation: &Value) {
    if path.is_empty() {
        assert_eq!(operation["op"], "set");
        *target = operation["value"].clone();
        return;
    }
    if path.len() == 1 && operation["op"] == "remove" {
        match &path[0] {
            Value::String(key) => {
                target.as_object_mut().unwrap().remove(key);
            }
            Value::Number(index) => {
                target
                    .as_array_mut()
                    .unwrap()
                    .remove(index.as_u64().unwrap() as usize);
            }
            _ => panic!("invalid patch component"),
        }
        return;
    }
    let nested = match &path[0] {
        Value::String(key) => &mut target[key],
        Value::Number(index) => &mut target[index.as_u64().unwrap() as usize],
        _ => panic!("invalid patch component"),
    };
    patch(nested, &path[1..], operation);
}

#[derive(Default)]
struct Metrics {
    phase: &'static str,
    deltas: usize,
    checkpoints: usize,
    ready: usize,
    bytes: usize,
    filtered_bytes: Vec<f64>,
    all_bytes: Vec<f64>,
    coalesced_revisions: u64,
}

impl Metrics {
    fn apply(&mut self, pages: &mut [Value], message: Value, bytes: usize, positions: bool) {
        self.bytes += bytes;
        let index = match message["window"].as_str().unwrap_or_else(|| {
            panic!(
                "unexpected stream control frame in {}: {message}",
                self.phase
            )
        }) {
            "all" => 0,
            "filtered" => 1,
            _ => panic!("unexpected window"),
        };
        let page = &mut pages[index];
        if message["type"] == "page_window_ready" {
            assert_eq!(
                message["resync"], false,
                "retained live baseline must resume"
            );
            assert_eq!(message["version"], page["version"]);
            assert_eq!(message["rev"], page["rev"]);
            self.ready += 1;
            return;
        }
        assert_eq!(
            message["base_version"], page["version"],
            "illegal delta chain gap"
        );
        assert_eq!(message["epoch"], page["epoch"]);
        let previous = page["rev"].as_u64().unwrap();
        let revision = message["rev"].as_u64().unwrap();
        assert!(revision > previous, "stream revisions must advance");
        self.coalesced_revisions += revision - previous - 1;
        if message["type"] == "page_window_checkpoint" {
            self.checkpoints += 1;
            if positions {
                // A moved filtered row must send its new absolute position.
                // Checkpoints are still legal if an even burst restores rank.
                assert!(message.get("changes").is_none());
            }
            for field in ["version", "epoch", "rev"] {
                page[field] = message[field].clone();
            }
            page["next_cursor"] = message["next_cursors"][0].clone();
        } else {
            assert_eq!(
                message["type"], "page_window_delta",
                "unexpected resync/error frame"
            );
            assert_eq!(message["page_count"], 1);
            for change in message["changes"].as_array().unwrap() {
                assert_eq!(change["index"], 0);
                for operation in change["ops"].as_array().unwrap() {
                    if positions && index == 1 {
                        assert_ne!(
                            operation["path"][0], "maps",
                            "rank-only delta must not resend order maps"
                        );
                    }
                    patch(page, operation["path"].as_array().unwrap(), operation);
                }
            }
            assert_eq!(page["version"], message["version"]);
            assert_eq!(page["rev"], message["rev"]);
            self.deltas += 1;
        }
        if index == 0 {
            self.all_bytes.push(bytes as f64);
        } else {
            self.filtered_bytes.push(bytes as f64);
        }
    }
}

async fn converge(
    socket: &mut reqwest::Upgraded,
    pages: &mut [Value],
    revision: u64,
    metrics: &mut Metrics,
    positions: bool,
) {
    while pages
        .iter()
        .any(|page| page["rev"].as_u64().unwrap() < revision)
    {
        let (message, bytes) = frame(socket).await;
        metrics.apply(pages, message, bytes, positions);
    }
    assert!(pages.iter().all(|page| page["rev"] == revision));
}

fn ranks(pages: &[Value], sequence: &[String], positions: bool) {
    assert_eq!(pages[0]["total"], ORDER_COUNT);
    assert_eq!(pages[0]["maps"].as_array().unwrap().len(), 20);
    assert_eq!(pages[1]["total"], 1);
    assert_eq!(pages[1]["maps"].as_array().unwrap().len(), 1);
    assert_eq!(pages[1]["order_ids"], json!(["zakaz-rank-0003"]));
    if !positions {
        return;
    }
    for page in pages {
        let order_ids = page["order_ids"].as_array().unwrap();
        let metadata = page["sequence_positions"].as_object().unwrap();
        assert_eq!(
            metadata.len(),
            order_ids.len(),
            "metadata must be restricted to loaded rows"
        );
        for id in order_ids {
            let id = id.as_str().unwrap();
            let expected = sequence
                .iter()
                .position(|candidate| candidate == id)
                .unwrap()
                + 1;
            assert_eq!(
                metadata[id], expected,
                "filtered/paginated absolute rank mismatch"
            );
        }
    }
}

async fn rest_matches(server: &Server, token: &str, pages: &[Value]) {
    assert_eq!(
        pages[0],
        server.page(token, "").await,
        "unfiltered reducer differs from REST"
    );
    assert_eq!(
        pages[1],
        server.page(token, "0003").await,
        "filtered reducer differs from REST"
    );
}

fn summary(values: &[f64]) -> Value {
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    json!({"n":values.len(), "p50":values[values.len()/2],
        "p95":values[(values.len()*95).div_ceil(100)-1], "max":values[values.len()-1]})
}

#[tokio::test]
#[ignore = "opt-in synthetic loopback streaming benchmark; prints JSON metrics"]
async fn sequence_position_stream_benchmark() {
    let positions = std::env::var("ACCORD_BENCH_EXPECT_POSITIONS")
        .ok()
        .as_deref()
        != Some("0");
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let mut sequence = Vec::new();
    for index in 0..ORDER_COUNT {
        let id = format!("zakaz-rank-{index:04}");
        state
            .production_maps
            .upsert_map(
                serde_json::from_str(&pechat_order_map_json(
                    &id,
                    "Synthetic rank benchmark",
                    &format!("{index:04}"),
                    APPARATUS,
                ))
                .unwrap(),
            )
            .await
            .unwrap();
        sequence.push(id);
    }
    state
        .production_maps
        .set_apparatus_sequence(APPARATUS, sequence.clone())
        .await
        .unwrap();
    let server = Server::start(state.clone()).await;
    let mut pages = vec![
        server.page(&token, "").await,
        server.page(&token, "0003").await,
    ];
    ranks(&pages, &sequence, positions);
    let bootstrap_bytes = pages
        .iter()
        .map(|page| serde_json::to_vec(page).unwrap().len())
        .collect::<Vec<_>>();
    let mut socket = server.socket(&token, &pages).await;
    let mut metrics = Metrics::default();
    metrics.phase = "bootstrap";
    for _ in 0..2 {
        let (message, bytes) = frame(&mut socket).await;
        metrics.apply(&mut pages, message, bytes, positions);
    }
    let started = Instant::now();
    metrics.phase = "sequential";
    let mut latencies = Vec::new();
    for _ in 0..SEQUENTIAL_WRITES {
        sequence.swap(3, 4);
        let write_started = Instant::now();
        state
            .production_maps
            .set_apparatus_sequence(APPARATUS, sequence.clone())
            .await
            .unwrap();
        converge(
            &mut socket,
            &mut pages,
            state.production_maps.snapshot_revision(),
            &mut metrics,
            positions,
        )
        .await;
        latencies.push(write_started.elapsed().as_secs_f64() * 1000.0);
        ranks(&pages, &sequence, positions);
        rest_matches(&server, &token, &pages).await;
    }
    let sequential_s = started.elapsed().as_secs_f64();
    let sequential_frames = metrics.deltas + metrics.checkpoints;
    assert_eq!(sequential_frames, SEQUENTIAL_WRITES * 2);
    // Emit the common steady workload before the deliberately harsher burst,
    // so an older server's burst failure cannot hide its timing comparison.
    println!(
        "SEQUENCE_STREAM_SEQUENTIAL_BENCHMARK {}",
        json!({
            "positions_enabled":positions, "fixture_orders":ORDER_COUNT,
            "sequential_writes":SEQUENTIAL_WRITES, "sequential_frames":sequential_frames,
            "sequential_duration_s":sequential_s, "write_to_both_windows_ms":summary(&latencies),
            "first_20_write_ms":summary(&latencies[..20]), "last_20_write_ms":summary(&latencies[SEQUENTIAL_WRITES-20..]),
            "all_window_frame_bytes":summary(&metrics.all_bytes),
            "filtered_window_frame_bytes":summary(&metrics.filtered_bytes)
        })
    );

    // A deliberately unread witness proves this burst exceeds the source ring
    // capacity. Actual socket consumption is delayed until the writes finish;
    // legal coalescing may skip revisions, never baseline continuity/final state.
    let mut witness = state.production_maps.subscribe_live();
    metrics.phase = "burst";
    let burst_started = Instant::now();
    for _ in 0..BURST_WRITES {
        sequence.swap(3, 4);
        state
            .production_maps
            .set_apparatus_sequence(APPARATUS, sequence.clone())
            .await
            .unwrap();
    }
    let burst_write_ms = burst_started.elapsed().as_secs_f64() * 1000.0;
    assert!(matches!(
        witness.recv().await,
        Err(tokio::sync::broadcast::error::RecvError::Lagged(_))
    ));
    let catchup_started = Instant::now();
    converge(
        &mut socket,
        &mut pages,
        state.production_maps.snapshot_revision(),
        &mut metrics,
        positions,
    )
    .await;
    let burst_catchup_ms = catchup_started.elapsed().as_secs_f64() * 1000.0;
    ranks(&pages, &sequence, positions);
    rest_matches(&server, &token, &pages).await;
    let burst_frames = metrics.deltas + metrics.checkpoints - sequential_frames;

    // Reconnect from retained versions after a deliberate clean disconnect and
    // an offline write. A delta must be followed by ready for both windows.
    socket.write_all(&[0x88, 0x80, 0, 0, 0, 0]).await.unwrap();
    let mut tail = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), socket.read_to_end(&mut tail))
        .await
        .unwrap()
        .unwrap();
    drop(socket);
    sequence.swap(3, 4);
    state
        .production_maps
        .set_apparatus_sequence(APPARATUS, sequence.clone())
        .await
        .unwrap();
    let reconnect_started = Instant::now();
    metrics.phase = "reconnect";
    let mut socket = server.socket(&token, &pages).await;
    let old_ready = metrics.ready;
    while metrics.ready < old_ready + 2 {
        let (message, bytes) = frame(&mut socket).await;
        metrics.apply(&mut pages, message, bytes, positions);
    }
    let reconnect_ms = reconnect_started.elapsed().as_secs_f64() * 1000.0;
    ranks(&pages, &sequence, positions);
    rest_matches(&server, &token, &pages).await;

    let mut recovery_latencies = Vec::new();
    metrics.phase = "recovery";
    for _ in 0..RECOVERY_WRITES {
        sequence.swap(3, 4);
        let write_started = Instant::now();
        state
            .production_maps
            .set_apparatus_sequence(APPARATUS, sequence.clone())
            .await
            .unwrap();
        converge(
            &mut socket,
            &mut pages,
            state.production_maps.snapshot_revision(),
            &mut metrics,
            positions,
        )
        .await;
        recovery_latencies.push(write_started.elapsed().as_secs_f64() * 1000.0);
        ranks(&pages, &sequence, positions);
        rest_matches(&server, &token, &pages).await;
    }
    if positions {
        assert!(
            metrics.filtered_bytes.iter().all(|bytes| *bytes < 2048.0),
            "rank-only updates must stay sparse"
        );
    }
    println!(
        "SEQUENCE_STREAM_BENCHMARK {}",
        json!({
            "positions_enabled":positions, "fixture_orders":ORDER_COUNT, "loaded_rows":[20,1],
            "sequential_writes":SEQUENTIAL_WRITES, "sequential_frames":sequential_frames,
            "sequential_duration_s":sequential_s, "write_to_both_windows_ms":summary(&latencies),
            "first_20_write_ms":summary(&latencies[..20]), "last_20_write_ms":summary(&latencies[SEQUENTIAL_WRITES-20..]),
            "burst_writes":BURST_WRITES, "burst_frames":burst_frames,
            "burst_write_ms":burst_write_ms, "burst_catchup_ms":burst_catchup_ms,
            "source_ring_lag_witness":true, "coalesced_revision_steps_across_windows":metrics.coalesced_revisions,
            "deltas":metrics.deltas, "checkpoints":metrics.checkpoints,
            "bootstrap_json_bytes":bootstrap_bytes, "stream_wire_bytes":metrics.bytes,
            "all_window_frame_bytes":summary(&metrics.all_bytes), "filtered_window_frame_bytes":summary(&metrics.filtered_bytes),
            "forced_disconnects":1, "reconnect_ms":reconnect_ms,
        "recovery_writes":RECOVERY_WRITES, "recovery_write_to_both_windows_ms":summary(&recovery_latencies),
            "unexpected_disconnects":0, "illegal_baseline_gaps":0, "final_state_mismatches":0,
            "absolute_rank_assertions":positions,
            "scope":"synthetic in-memory store, real loopback HTTP/WebSocket; not a production PostgreSQL/mobile-network soak"
        })
    );
}
