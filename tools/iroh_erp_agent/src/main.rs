use std::{
    env, io,
    io::Write,
    net::SocketAddr,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use iroh::{
    Endpoint, EndpointAddr, RelayMode, SecretKey,
    endpoint::{Connection, presets},
    protocol::{AcceptError, ProtocolHandler, Router},
};
use mdns_sd::{ServiceDaemon, ServiceInfo};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const ALPN: &[u8] = b"/mini-rs-erp/http/1";
const DEFAULT_TARGET: &str = "127.0.0.1:18081";
const DEFAULT_RUNS: usize = 30;
const MAX_HTTP_BYTES: usize = 2 * 1024 * 1024;
const TICKET_FILE_ENV: &str = "IROH_TICKET_FILE";

#[derive(Clone, Debug)]
struct HttpBridge {
    target: String,
    streams: Arc<tokio::sync::Semaphore>,
}

impl ProtocolHandler for HttpBridge {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let target = self.target.clone();

        async move {
            while let Ok((send, recv)) = connection.accept_bi().await {
                let Ok(permit) = self.streams.clone().try_acquire_owned() else {
                    continue;
                };
                let target = target.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    if let Err(error) = bridge_http_stream(target, send, recv).await {
                        eprintln!("iroh http stream failed: {error}");
                    }
                });
            }
            connection.closed().await;

            Ok::<(), io::Error>(())
        }
        .await
        .map_err(AcceptError::from_err)
    }
}

async fn bridge_http_stream(
    target: String,
    mut send: iroh::endpoint::SendStream,
    mut recv: iroh::endpoint::RecvStream,
) -> io::Result<()> {
    let mut request =
        tokio::time::timeout(Duration::from_secs(5), read_http_request_head(&mut recv)).await??;
    validate_request(&request, false)?;
    let mut upstream = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::net::TcpStream::connect(&target),
    )
    .await??;
    upstream.set_nodelay(true)?;

    if !is_websocket_upgrade(&request) {
        let mut rest = tokio::time::timeout(
            Duration::from_secs(10),
            recv.read_to_end(MAX_HTTP_BYTES.saturating_sub(request.len())),
        )
        .await?
        .map_err(io::Error::other)?;
        request.append(&mut rest);
        validate_request(&request, true)?;
        tokio::time::timeout(Duration::from_secs(5), upstream.write_all(&request)).await??;

        let mut response = Vec::new();
        tokio::time::timeout(
            Duration::from_secs(30),
            upstream
                .take((MAX_HTTP_BYTES + 1) as u64)
                .read_to_end(&mut response),
        )
        .await??;
        if response.len() > MAX_HTTP_BYTES {
            return Err(io::Error::other("HTTP response exceeds size limit"));
        }

        tokio::time::timeout(Duration::from_secs(10), send.write_all(&response)).await??;
        send.finish()?;
        return Ok(());
    }

    // Do not turn a forged Upgrade header on a normal HTTP route into an
    // unrestricted persistent TCP tunnel. Only tunnel after ERP confirms 101.
    validate_request(&request, true)?;
    tokio::time::timeout(Duration::from_secs(5), upstream.write_all(&request)).await??;
    let mut response_head = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut buffer = [0_u8; 1024];
        while find_http_header_end(&response_head).is_none() {
            if response_head.len() >= 32 * 1024 {
                return Err(io::Error::other("upgrade response head too large"));
            }
            let count = upstream.read(&mut buffer).await?;
            if count == 0 {
                return Err(io::Error::other("incomplete upgrade response"));
            }
            response_head.extend_from_slice(&buffer[..count]);
        }
        Ok::<_, io::Error>(())
    })
    .await??;
    tokio::time::timeout(Duration::from_secs(5), send.write_all(&response_head)).await??;
    if parse_status_code(&response_head) != Some(101) {
        send.finish()?;
        return Ok(());
    }
    tunnel_websocket(upstream, send, recv).await
}

// Do not become a general loopback proxy or accept request smuggling. ERP
// still authorizes EVERY operation; the ticket is an address, not a login.
fn validate_request(request: &[u8], complete: bool) -> io::Result<()> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "unsupported HTTP request");
    let end = find_http_header_end(request).ok_or_else(invalid)?;
    if end > 32 * 1024 {
        return Err(invalid());
    }
    let head = std::str::from_utf8(&request[..end]).map_err(|_| invalid())?;
    let mut lines = head.split("\r\n");
    let parts: Vec<_> = lines.next().ok_or_else(invalid)?.split(' ').collect();
    if parts.len() != 3
        || parts[2] != "HTTP/1.1"
        || !matches!(
            parts[0],
            "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS"
        )
    {
        return Err(invalid());
    }
    let path = parts[1].split('?').next().unwrap_or_default();
    if path != "/healthz" && !path.starts_with("/v1/mobile/") {
        return Err(invalid());
    }
    if is_websocket_upgrade(request) && (parts[0] != "GET" || !path.ends_with("/live")) {
        return Err(invalid());
    }
    let mut content_length = None;
    let mut close = false;
    for line in lines.filter(|line| !line.is_empty()) {
        if line.starts_with([' ', '\t']) {
            return Err(invalid());
        }
        let (name, value) = line.split_once(':').ok_or_else(invalid)?;
        match name.to_ascii_lowercase().as_str() {
            "transfer-encoding" => return Err(invalid()),
            "content-length" => {
                if content_length.is_some() {
                    return Err(invalid());
                }
                content_length = Some(value.trim().parse::<usize>().map_err(|_| invalid())?);
            }
            "connection" => close = value.trim().eq_ignore_ascii_case("close"),
            _ => {}
        }
    }
    if !is_websocket_upgrade(request) && !close {
        return Err(invalid());
    }
    if complete && request.len() - end != content_length.unwrap_or(0) {
        return Err(invalid());
    }
    Ok(())
}

async fn read_http_request_head(recv: &mut iroh::endpoint::RecvStream) -> io::Result<Vec<u8>> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        if request.len() >= MAX_HTTP_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HTTP request exceeds size limit",
            ));
        }

        let Some(bytes_read) = recv.read(&mut buffer).await? else {
            break;
        };
        if bytes_read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..bytes_read]);
        if find_http_header_end(&request).is_some() {
            break;
        }
    }
    Ok(request)
}

async fn tunnel_websocket(
    upstream: tokio::net::TcpStream,
    mut send: iroh::endpoint::SendStream,
    mut recv: iroh::endpoint::RecvStream,
) -> io::Result<()> {
    let (mut upstream_read, mut upstream_write) = upstream.into_split();

    tokio::select! {
        result = async {
            tokio::io::copy(&mut recv, &mut upstream_write).await?;
            upstream_write.shutdown().await
        } => result,
        result = async {
            tokio::io::copy(&mut upstream_read, &mut send).await?;
            send.finish()?;
            Ok(())
        } => result,
    }
}

fn is_websocket_upgrade(request: &[u8]) -> bool {
    let Some(header_end) = find_http_header_end(request) else {
        return false;
    };
    let Ok(headers) = std::str::from_utf8(&request[..header_end]) else {
        return false;
    };
    let mut has_connection_upgrade = false;
    let mut has_websocket_upgrade = false;

    for line in headers.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_ascii_lowercase();
        if name == "connection" {
            has_connection_upgrade = value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("upgrade"));
        } else if name == "upgrade" {
            has_websocket_upgrade = value == "websocket";
        }
    }

    has_connection_upgrade && has_websocket_upgrade
}

fn find_http_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("warn")
        .with_target(false)
        .without_time()
        .init();

    let mut args = env::args().skip(1);

    match args.next().as_deref() {
        Some("agent") => {
            let target = args.next().unwrap_or_else(|| DEFAULT_TARGET.to_string());
            run_agent(target).await
        }
        Some("client") => {
            let ticket = args.next().context("missing endpoint ticket")?;
            let runs = args
                .next()
                .as_deref()
                .unwrap_or("30")
                .parse::<usize>()
                .context("invalid runs")?;

            run_client(ticket, runs).await
        }
        _ => {
            print_usage();
            std::process::exit(2);
        }
    }
}

async fn run_agent(target: String) -> Result<()> {
    let target_addr: SocketAddr = target
        .parse()
        .context("target must be a loopback socket address")?;
    if !target_addr.ip().is_loopback() {
        bail!("bridge target must remain loopback");
    }
    let key_path =
        env::var("IROH_SECRET_KEY_FILE").unwrap_or_else(|_| "garbage/iroh/endpoint.key".into());
    let key = load_or_create_key(Path::new(&key_path))?;
    let mut builder = Endpoint::builder(presets::N0).secret_key(key);
    if env::var("IROH_DISABLE_RELAY").as_deref() == Ok("1") {
        builder = builder.relay_mode(RelayMode::Disabled);
    }
    let endpoint = builder.bind().await?;
    let router = Router::builder(endpoint)
        .accept(
            ALPN,
            HttpBridge {
                target: target.clone(),
                streams: Arc::new(tokio::sync::Semaphore::new(128)),
            },
        )
        .spawn();

    // LAN availability must not depend on relay/WAN availability at boot.
    let mut ticket = encode_endpoint_addr(&router.endpoint().addr())?;
    write_ticket_file_if_configured(&ticket).await?;
    let mdns = advertise_lan(router.endpoint())
        .map_err(|error| {
            eprintln!("LAN discovery unavailable (known addresses/relay still work): {error}");
            error
        })
        .ok();

    eprintln!("target={target}");
    println!("IROH_ENDPOINT_TICKET={ticket}");
    eprintln!("ready; press ctrl-c to stop");

    let mut refresh = tokio::time::interval(Duration::from_secs(2));
    loop {
        tokio::select! {
            result = tokio::signal::ctrl_c() => { result?; break; }
            _ = refresh.tick() => {
                let updated = encode_endpoint_addr(&router.endpoint().addr())?;
                if updated != ticket {
                    write_ticket_file_if_configured(&updated).await?;
                    ticket = updated;
                }
            }
        }
    }
    if let Some(mdns) = mdns {
        let _ = mdns.shutdown();
    }
    router.shutdown().await?;
    Ok(())
}

fn load_or_create_key(path: &Path) -> Result<SecretKey> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            let key = SecretKey::generate();
            file.write_all(&key.to_bytes())?;
            file.sync_all()?;
            Ok(key)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let metadata = std::fs::symlink_metadata(path)?;
            if !metadata.is_file() {
                bail!("key must be a regular file");
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o077 != 0 {
                    bail!("key permissions must be 0600");
                }
            }
            let bytes: [u8; 32] = std::fs::read(path)?.try_into().map_err(|_| {
                anyhow::anyhow!("invalid key file; refusing to silently rotate identity")
            })?;
            Ok(SecretKey::from_bytes(&bytes))
        }
        Err(error) => Err(error.into()),
    }
}

fn advertise_lan(endpoint: &Endpoint) -> Result<ServiceDaemon> {
    let id = endpoint.id().to_string();
    let name = format!("accord-erp-{}", &id[..12]);
    let port = endpoint
        .bound_sockets()
        .into_iter()
        .find(|addr| addr.is_ipv4())
        .context("no IPv4 socket for LAN discovery")?
        .port();
    let daemon = ServiceDaemon::new()?;
    let service = ServiceInfo::new(
        "_accord-erp._udp.local.",
        &name,
        &format!("{name}.local."),
        "",
        port,
        [("server_ref", id.as_str()), ("server_name", "Accord ERP")].as_slice(),
    )?
    .enable_addr_auto();
    daemon.register(service)?;
    Ok(daemon)
}

async fn write_ticket_file_if_configured(ticket: &str) -> Result<()> {
    let path = match env::var(TICKET_FILE_ENV) {
        Ok(path) if !path.trim().is_empty() => path,
        _ => return Ok(()),
    };
    let path = Path::new(&path);
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let temp_path = path.with_extension("tmp");
    tokio::fs::write(&temp_path, format!("{ticket}\n"))
        .await
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    tokio::fs::rename(&temp_path, path)
        .await
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

async fn run_client(ticket: String, runs: usize) -> Result<()> {
    let addr = decode_endpoint_addr(&ticket)?;
    let endpoint = Endpoint::bind(presets::N0).await?;
    let connecting = Instant::now();
    let connection = endpoint.connect(addr.clone(), ALPN).await?;
    let connected = connecting.elapsed();

    for run in 1..=runs {
        let start = Instant::now();

        let (mut send, mut recv) = connection.open_bi().await?;
        send.write_all(health_request()).await?;
        send.finish()?;

        let response = recv.read_to_end(MAX_HTTP_BYTES).await?;
        let total = start.elapsed();
        let code = parse_status_code(&response).unwrap_or(0);

        println!(
            "run={run} code={code} initial_connect_ms={} request_ms={} bytes={}",
            if run == 1 {
                connected.as_micros() as f64 / 1000.0
            } else {
                0.0
            },
            total.as_micros() as f64 / 1000.0,
            response.len()
        );

        if code != 200 {
            bail!(
                "unexpected HTTP status {code}: {}",
                String::from_utf8_lossy(&response)
            );
        }
    }

    endpoint.close().await;
    Ok(())
}

fn encode_endpoint_addr(addr: &EndpointAddr) -> Result<String> {
    let json = serde_json::to_vec(addr)?;
    Ok(URL_SAFE_NO_PAD.encode(json))
}

fn decode_endpoint_addr(ticket: &str) -> Result<EndpointAddr> {
    let json = URL_SAFE_NO_PAD.decode(ticket.as_bytes())?;
    Ok(serde_json::from_slice(&json)?)
}

fn health_request() -> &'static [u8] {
    b"GET /healthz HTTP/1.1\r\nHost: mini-rs-erp\r\nConnection: close\r\n\r\n"
}

fn parse_status_code(response: &[u8]) -> Option<u16> {
    let end = response
        .windows(2)
        .position(|part| part == b"\r\n")
        .unwrap_or(response.len());
    let line = std::str::from_utf8(&response[..end]).ok()?;
    line.split_whitespace().nth(1)?.parse().ok()
}

fn print_usage() {
    eprintln!("usage:");
    eprintln!("  iroh_erp_agent agent [target]");
    eprintln!("  iroh_erp_agent client <endpoint-ticket> [runs]");
    eprintln!();
    eprintln!("defaults:");
    eprintln!("  target = {DEFAULT_TARGET}");
    eprintln!("  runs   = {DEFAULT_RUNS}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_survives_restart_and_ticket_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.key");
        let first = load_or_create_key(&path).unwrap();
        let second = load_or_create_key(&path).unwrap();
        assert_eq!(first.public(), second.public());
        let addr = EndpointAddr::new(first.public());
        let ticket = encode_endpoint_addr(&addr).unwrap();
        assert_eq!(decode_endpoint_addr(&ticket).unwrap().id, addr.id);
        let json: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(ticket).unwrap()).unwrap();
        assert_eq!(json["id"].as_str().unwrap().len(), 64);
    }

    #[test]
    fn corrupt_key_is_not_silently_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.key");
        load_or_create_key(&path).unwrap();
        std::fs::write(&path, b"broken").unwrap();
        assert!(load_or_create_key(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"broken");
    }

    #[test]
    fn restricts_loopback_routes_and_rejects_smuggling() {
        assert!(validate_request(health_request(), true).is_ok());
        for request in [
            "GET /admin HTTP/1.1\r\nConnection: close\r\n\r\n",
            "GET http://other/healthz HTTP/1.1\r\nConnection: close\r\n\r\n",
            "GET /healthz HTTP/1.1\r\nConnection: upgrade\r\nUpgrade: websocket\r\n\r\n",
            "POST /v1/mobile/live HTTP/1.1\r\nConnection: upgrade\r\nUpgrade: websocket\r\n\r\n",
            "GET /healthz HTTP/1.1\r\nConnection: close\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n",
            "POST /v1/mobile/test HTTP/1.1\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
            "GET /healthz HTTP/1.1\r\nConnection: close\r\n\r\nGET /healthz HTTP/1.1\r\n\r\n",
            "POST /v1/mobile/test HTTP/1.1\r\nConnection: close\r\nContent-Length: 3\r\n\r\n{}",
        ] {
            assert!(
                validate_request(request.as_bytes(), true).is_err(),
                "accepted {request:?}"
            );
        }
    }

    #[tokio::test]
    async fn offline_quic_reuses_connection_and_preserves_authorization() -> Result<()> {
        tokio::time::timeout(Duration::from_secs(10), async {
            let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let target = tcp.local_addr()?.to_string();
            let upstream = tokio::spawn(async move {
                for expected in [None, Some("Bearer test-only")] {
                    let (mut socket, _) = tcp.accept().await?;
                    let mut request = Vec::new();
                    let mut byte = [0];
                    while find_http_header_end(&request).is_none() {
                        socket.read_exact(&mut byte).await?;
                        request.push(byte[0]);
                    }
                    let head = String::from_utf8(request).unwrap();
                    let status = if let Some(auth) = expected {
                        assert!(head.contains(&format!("Authorization: {auth}")));
                        "200 OK"
                    } else {
                        assert!(!head.contains("Authorization:"));
                        "401 Unauthorized"
                    };
                    socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}").as_bytes()).await?;
                }
                Ok::<_, io::Error>(())
            });
            let endpoint = Endpoint::builder(presets::N0).relay_mode(RelayMode::Disabled)
                .bind_addr("127.0.0.1:0")?.bind().await?;
            let router = Router::builder(endpoint).accept(ALPN, HttpBridge {
                target, streams: Arc::new(tokio::sync::Semaphore::new(4)),
            }).spawn();
            let peer = EndpointAddr::new(router.endpoint().id()).with_ip_addr(
                router.endpoint().bound_sockets().into_iter().find(|addr| addr.is_ipv4()).unwrap());
            let client = Endpoint::builder(presets::N0).relay_mode(RelayMode::Disabled).bind().await?;
            let connection = client.connect(peer, ALPN).await?;
            for (auth, expected) in [("", 401), ("Authorization: Bearer test-only\r\n", 200)] {
                let (mut send, mut recv) = connection.open_bi().await?;
                send.write_all(format!("GET /v1/mobile/test HTTP/1.1\r\nHost: erp\r\nConnection: close\r\n{auth}\r\n").as_bytes()).await?;
                send.finish()?;
                let response = recv.read_to_end(MAX_HTTP_BYTES).await?;
                assert_eq!(parse_status_code(&response), Some(expected));
            }
            upstream.await??;
            client.close().await;
            router.shutdown().await?;
            Ok::<_, anyhow::Error>(())
        }).await??;
        Ok(())
    }

    #[test]
    fn parses_http_status_code() {
        let response = b"HTTP/1.1 200 OK\r\ncontent-length: 11\r\n\r\n{\"ok\":true}";

        assert_eq!(parse_status_code(response), Some(200));
    }

    #[tokio::test]
    async fn websocket_tunnel_requires_real_101_and_passes_frames() -> Result<()> {
        tokio::time::timeout(Duration::from_secs(10), async {
            for status in [200, 101] {
                let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
                let target = tcp.local_addr()?.to_string();
                let upstream = tokio::spawn(async move {
                    let (mut socket, _) = tcp.accept().await?;
                    let mut request = Vec::new();
                    let mut byte = [0];
                    while find_http_header_end(&request).is_none() {
                        socket.read_exact(&mut byte).await?;
                        request.push(byte[0]);
                    }
                    socket.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Length: 0\r\n\r\n").as_bytes()).await?;
                    if status == 101 {
                        let mut ping = [0; 2];
                        socket.read_exact(&mut ping).await?;
                        assert_eq!(ping, [0x89, 0]);
                        socket.write_all(&[0x81, 2, b'o', b'k']).await?;
                    }
                    Ok::<_, io::Error>(())
                });
                let endpoint = Endpoint::builder(presets::N0).relay_mode(RelayMode::Disabled)
                    .bind_addr("127.0.0.1:0")?.bind().await?;
                let router = Router::builder(endpoint).accept(ALPN, HttpBridge {
                    target, streams: Arc::new(tokio::sync::Semaphore::new(4)),
                }).spawn();
                let peer = EndpointAddr::new(router.endpoint().id()).with_ip_addr(
                    router.endpoint().bound_sockets().into_iter().find(|addr| addr.is_ipv4()).unwrap());
                let client = Endpoint::builder(presets::N0).relay_mode(RelayMode::Disabled).bind().await?;
                let connection = client.connect(peer, ALPN).await?;
                let (mut send, mut recv) = connection.open_bi().await?;
                send.write_all(b"GET /v1/mobile/chat/live HTTP/1.1\r\nHost: erp\r\nConnection: upgrade\r\nUpgrade: websocket\r\n\r\n").await?;
                let mut response = Vec::new();
                let mut buf = [0; 1024];
                while find_http_header_end(&response).is_none() {
                    let size = recv.read(&mut buf).await?.context("upgrade response missing")?;
                    response.extend_from_slice(&buf[..size]);
                }
                assert_eq!(parse_status_code(&response), Some(status));
                if status == 101 { send.write_all(&[0x89, 0]).await?; }
                let tail = recv.read_to_end(1024).await?;
                if status == 101 { assert_eq!(tail, [0x81, 2, b'o', b'k']); }
                else { assert!(tail.is_empty()); }
                upstream.await??;
                client.close().await;
                router.shutdown().await?;
            }
            Ok::<_, anyhow::Error>(())
        }).await??;
        Ok(())
    }

    #[test]
    fn rejects_invalid_http_response() {
        assert_eq!(parse_status_code(b"{\"ok\":true}"), None);
        assert_eq!(parse_status_code(b"HTTP/1.1 nope OK\r\n\r\n"), None);
    }

    #[test]
    fn detects_websocket_upgrade_request() {
        let request = b"GET /v1/mobile/admin/system/monitor/live HTTP/1.1\r\nHost: mini-rs-erp\r\nConnection: keep-alive, Upgrade\r\nUpgrade: websocket\r\n\r\n";

        assert!(is_websocket_upgrade(request));
    }

    #[test]
    fn keeps_plain_http_request_in_close_mode() {
        let request = b"GET /healthz HTTP/1.1\r\nHost: mini-rs-erp\r\nConnection: close\r\n\r\n";

        assert!(!is_websocket_upgrade(request));
    }
}
