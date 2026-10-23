use super::*;
use std::io::{Read, Write};

fn decode(headers: &axum::http::HeaderMap, bytes: &[u8]) -> Vec<u8> {
    if headers
        .get(header::CONTENT_ENCODING)
        .is_some_and(|value| value == "gzip")
    {
        let mut raw = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .read_to_end(&mut raw)
            .unwrap();
        raw
    } else {
        bytes.to_vec()
    }
}

async fn wire(
    f: &Fixture,
    token: &str,
    path: &str,
    accepts_gzip: bool,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let mut request = Request::builder()
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    if accepts_gzip {
        request = request.header(header::ACCEPT_ENCODING, "gzip");
    }
    let response = f
        .router
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), 10 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    (status, headers, body)
}

#[tokio::test]
async fn identity_query_skips_only_scoped_admin_http_compression_and_preserves_payload_identity() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let token = admin(&f, "identity-admin").await;
    for path in [
        format!(
            "/v1/mobile/admin/production-maps/order-page?view=sequence&apparatus={CUT}&projection=sequence_list_v1"
        ),
        format!("{QUEUE}?apparatus={CUT}&order_id=zakaz-000&detail=true"),
    ] {
        let (status, gzip_headers, gzip) = wire(&f, &token, &path, true).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(gzip_headers[header::CONTENT_ENCODING], "gzip");
        assert!(gzip_headers.get_all(header::VARY).iter().any(|value| {
            value
                .to_str()
                .unwrap()
                .to_lowercase()
                .contains("accept-encoding")
        }));
        let (status, identity_headers, identity) =
            wire(&f, &token, &format!("{path}&compression=identity"), true).await;
        assert_eq!(status, StatusCode::OK);
        assert!(identity_headers.get(header::CONTENT_ENCODING).is_none());
        assert_eq!(decode(&gzip_headers, &gzip), identity);
        assert!(gzip.len() < identity.len());
        // An identity URL is always identity; browsers may manage this header
        // themselves, so no forbidden request header customization is needed.
        let (_, headers, body) =
            wire(&f, &token, &format!("{path}&compression=identity"), false).await;
        assert!(headers.get(header::CONTENT_ENCODING).is_none());
        assert_eq!(body, identity);
        let json: Value = serde_json::from_slice(&identity).unwrap();
        if path.contains("order-page") {
            let (status, _, body) = wire(
                &f,
                &token,
                &format!(
                    "{path}&compression=identity&if_version={}",
                    json["version"].as_str().unwrap()
                ),
                true,
            )
            .await;
            assert_eq!(status, StatusCode::NOT_MODIFIED);
            assert!(body.is_empty());
        }
        assert_eq!(
            wire(&f, &token, &format!("{path}&compression=unknown"), true)
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    // Worker full and bounded queue negotiation retain their ordinary gzip
    // contract, even if an unrelated old query happens to carry the hint.
    for suffix in ["", "&compression=identity"] {
        let (status, headers, _) = wire(
            &f,
            &f.token,
            &format!("{QUEUE}?worker_scope=true&queue_limit=5{suffix}"),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_ENCODING], "gzip");
    }
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

async fn simulated_bandwidth(
    response: axum::response::Response,
    mbps: f64,
) -> axum::response::Response {
    let (parts, body) = response.into_parts();
    // This isolated test layer sees the already-negotiated HTTP body. Buffer
    // before pacing so both encodings use exactly the same 1024-byte algorithm,
    // rather than giving gzip's internal frame boundaries a timer advantage.
    let bytes = to_bytes(body, 10 * 1024 * 1024).await.unwrap();
    let started = tokio::time::Instant::now();
    let stream = async_stream::stream! {
        let mut sent = 0usize;
        for chunk in bytes.chunks(1024) {
            sent += chunk.len();
            let deadline = started+Duration::from_secs_f64(sent as f64*8.0/(mbps*1_000_000.0));
            tokio::time::sleep_until(deadline).await;
            yield Ok::<_, std::io::Error>(bytes::Bytes::copy_from_slice(chunk));
        }
    };
    axum::response::Response::from_parts(parts, Body::from_stream(stream))
}

#[tokio::test]
async fn report_simulated_bandwidth_over_actual_tcp_without_touching_host_network() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let token = admin(&f, "paced-identity-timing").await;
    let client = reqwest::Client::builder()
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .build()
        .unwrap();
    for mbps in [5.0, 20.0] {
        let router = f
            .router
            .clone()
            .layer(axum::middleware::map_response(move |response| {
                simulated_bandwidth(response, mbps)
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        for (name, path) in [
            (
                "sequence20",
                format!(
                    "/v1/mobile/admin/production-maps/order-page?view=sequence&apparatus={CUT}&projection=sequence_list_v1"
                ),
            ),
            (
                "detail1",
                format!("{QUEUE}?apparatus={CUT}&order_id=zakaz-000&detail=true"),
            ),
        ] {
            let mut times = [Vec::new(), Vec::new()];
            let mut bytes = [0usize; 2];
            let mut canonical = Vec::new();
            for iteration in 0..24 {
                for mode in if iteration % 2 == 0 { [0, 1] } else { [1, 0] } {
                    let suffix = if mode == 1 {
                        "&compression=identity"
                    } else {
                        ""
                    };
                    let started = std::time::Instant::now();
                    let response = client
                        .get(format!("http://{address}{path}{suffix}"))
                        .bearer_auth(&token)
                        .header(reqwest::header::ACCEPT_ENCODING, "gzip")
                        .send()
                        .await
                        .unwrap();
                    assert_eq!(response.status(), StatusCode::OK);
                    let compressed = response
                        .headers()
                        .get(header::CONTENT_ENCODING)
                        .is_some_and(|value| value == "gzip");
                    assert_eq!(compressed, mode == 0);
                    let body = response.bytes().await.unwrap();
                    let raw = if compressed {
                        let mut raw = Vec::new();
                        flate2::read::GzDecoder::new(body.as_ref())
                            .read_to_end(&mut raw)
                            .unwrap();
                        raw
                    } else {
                        body.to_vec()
                    };
                    std::hint::black_box(serde_json::from_slice::<Value>(&raw).unwrap());
                    assert!(canonical.is_empty() || canonical == raw);
                    canonical = raw;
                    bytes[mode] = body.len();
                    if iteration >= 4 {
                        times[mode].push(started.elapsed().as_secs_f64() * 1000.0);
                    }
                }
            }
            let gzip_ms = median(&mut times[0]);
            let identity_ms = median(&mut times[1]);
            eprintln!(
                "SIMULATED_NETWORK_REAL_TCP case={name} bandwidth_mbps={mbps} chunk_bytes=1024 samples_each=20 gzip_body_bytes={} identity_body_bytes={} gzip_median_ms={gzip_ms:.4} identity_median_ms={identity_ms:.4} identity_minus_gzip_ms={:.4}",
                bytes[0],
                bytes[1],
                identity_ms - gzip_ms
            );
            // Timing is reported, not asserted: scheduler/timer resolution is
            // platform-dependent and must not become a flaky correctness test.
        }
        server.abort();
    }
}

#[tokio::test]
async fn report_actual_tcp_gzip8_vs_identity_same_admin_payload_and_bandwidth_break_even() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let token = admin(&f, "identity-timing").await;
    let (address, server) = start(&f).await;
    // reqwest is built without its decompression features. Measure received
    // wire bytes, then explicitly inflate and parse on this client.
    let client = reqwest::Client::builder()
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .build()
        .unwrap();
    for (name, path) in [
        (
            "sequence20",
            format!(
                "/v1/mobile/admin/production-maps/order-page?view=sequence&apparatus={CUT}&projection=sequence_list_v1"
            ),
        ),
        (
            "detail1",
            format!("{QUEUE}?apparatus={CUT}&order_id=zakaz-000&detail=true"),
        ),
    ] {
        let mut times = [Vec::new(), Vec::new()];
        let mut decode_times = [Vec::new(), Vec::new()];
        let mut bytes = [0usize; 2];
        let mut canonical = Vec::new();
        for iteration in 0..14 {
            // Alternate request order; the same shared canonical revision and
            // HTTP keepalive connection serve both transport representations.
            for mode in if iteration % 2 == 0 { [0, 1] } else { [1, 0] } {
                let suffix = if mode == 1 {
                    "&compression=identity"
                } else {
                    ""
                };
                let started = std::time::Instant::now();
                let response = client
                    .get(format!("http://{address}{path}{suffix}"))
                    .bearer_auth(&token)
                    .header(reqwest::header::ACCEPT_ENCODING, "gzip")
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                let compressed = response
                    .headers()
                    .get(header::CONTENT_ENCODING)
                    .is_some_and(|value| value == "gzip");
                assert_eq!(compressed, mode == 0);
                let body = response.bytes().await.unwrap();
                let decoding = std::time::Instant::now();
                let raw = if compressed {
                    let mut raw = Vec::new();
                    flate2::read::GzDecoder::new(body.as_ref())
                        .read_to_end(&mut raw)
                        .unwrap();
                    raw
                } else {
                    body.to_vec()
                };
                let value: Value = serde_json::from_slice(&raw).unwrap();
                std::hint::black_box(value);
                if canonical.is_empty() {
                    canonical = raw.clone();
                }
                assert_eq!(raw, canonical);
                if iteration >= 4 {
                    times[mode].push(started.elapsed().as_secs_f64() * 1000.0);
                    decode_times[mode].push(decoding.elapsed().as_secs_f64() * 1000.0);
                }
                bytes[mode] = body.len();
            }
        }
        let gzip_ms = median(&mut times[0]);
        let identity_ms = median(&mut times[1]);
        let gzip_decode_ms = median(&mut decode_times[0]);
        let identity_decode_ms = median(&mut decode_times[1]);
        let delta_bits = (bytes[1] - bytes[0]) as f64 * 8.0;
        let saving_ms = gzip_ms - identity_ms;
        let break_even_mbps = if saving_ms > 0.0 {
            delta_bits / (saving_ms * 1000.0)
        } else {
            f64::INFINITY
        };
        eprintln!(
            "http_transport case={name} fixture=60_orders gzip_level={} gzip_bytes={} identity_bytes={} tcp_gzip_median_ms={gzip_ms:.4} tcp_identity_median_ms={identity_ms:.4} client_inflate_parse_gzip_ms={gzip_decode_ms:.4} client_parse_identity_ms={identity_decode_ms:.4} identity_break_even_mbps={break_even_mbps:.2}",
            std::env::var("MINI_ERP_HTTP_GZIP_LEVEL").unwrap_or("8".into()),
            bytes[0],
            bytes[1]
        );
        for mbps in [1.0, 5.0, 20.0, 100.0] {
            let modeled_gzip_ms = gzip_ms + bytes[0] as f64 * 8.0 / (mbps * 1000.0);
            let modeled_identity_ms = identity_ms + bytes[1] as f64 * 8.0 / (mbps * 1000.0);
            eprintln!(
                "http_transport_model case={name} bandwidth_mbps={mbps} gzip_ms={modeled_gzip_ms:.4} identity_ms={modeled_identity_ms:.4} identity_minus_gzip_ms={:.4}",
                modeled_identity_ms - modeled_gzip_ms
            );
        }
    }
    server.abort();
}

/// Run the SAME synthetic response bytes through compression/inflate/parse or
/// parse-only work. Invoke this ignored test binary under /usr/bin/time -p to
/// measure actual process user+system CPU independently from TCP timings.
#[tokio::test]
#[ignore = "explicit transport CPU experiment; 20000 codec iterations"]
async fn transport_codec_cpu_experiment() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let token = admin(&f, "codec-cpu").await;
    let path = format!(
        "/v1/mobile/admin/production-maps/order-page?view=sequence&apparatus={CUT}&projection=sequence_list_v1&compression=identity"
    );
    let (_, _, mut raw) = wire(&f, &token, &path, true).await;
    // Optional scratch capture lets separate CPU runs use byte-identical
    // payloads, including their epoch/cursor nonce. This contains fixture data.
    if let Ok(path) = std::env::var("MINI_TEST_CODEC_PAYLOAD") {
        if std::path::Path::new(&path).exists() {
            raw = std::fs::read(path).unwrap();
        } else {
            std::fs::write(path, &raw).unwrap();
        }
    }
    let mode = std::env::var("MINI_TEST_CODEC_MODE").unwrap_or("gzip".into());
    assert!(matches!(mode.as_str(), "gzip" | "identity"));
    for _ in 0..20_000 {
        let decoded = if mode == "gzip" {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(8));
            encoder.write_all(&raw).unwrap();
            let encoded = encoder.finish().unwrap();
            let mut decoded = Vec::new();
            flate2::read::GzDecoder::new(encoded.as_slice())
                .read_to_end(&mut decoded)
                .unwrap();
            decoded
        } else {
            raw.clone()
        };
        std::hint::black_box(serde_json::from_slice::<Value>(&decoded).unwrap());
    }
    use sha2::Digest;
    eprintln!(
        "transport_codec_cpu mode={mode} iterations=20000 raw_bytes={} sha256={:x}",
        raw.len(),
        sha2::Sha256::digest(&raw)
    );
}
