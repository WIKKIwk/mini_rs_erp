use std::{
    env, io,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use iroh::{
    Endpoint, EndpointAddr,
    endpoint::{Connection, presets},
    protocol::{AcceptError, ProtocolHandler, Router},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const ALPN: &[u8] = b"/mini-rs-erp/http/1";
const DEFAULT_TARGET: &str = "127.0.0.1:18081";
const DEFAULT_RUNS: usize = 30;
const MAX_HTTP_BYTES: usize = 2 * 1024 * 1024;
const TICKET_FILE_ENV: &str = "IROH_TICKET_FILE";

#[derive(Clone, Debug)]
struct HttpBridge {
    target: String,
}

impl ProtocolHandler for HttpBridge {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let target = self.target.clone();

        async move {
            let (mut send, mut recv) = connection.accept_bi().await?;
            let request = recv
                .read_to_end(MAX_HTTP_BYTES)
                .await
                .map_err(io::Error::other)?;
            let mut upstream = tokio::net::TcpStream::connect(&target).await?;

            upstream.write_all(&request).await?;

            let mut response = Vec::new();
            upstream.read_to_end(&mut response).await?;

            send.write_all(&response).await?;
            send.finish()?;
            connection.closed().await;

            Ok::<(), io::Error>(())
        }
        .await
        .map_err(AcceptError::from_err)
    }
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
    let endpoint = Endpoint::bind(presets::N0).await?;
    let router = Router::builder(endpoint)
        .accept(
            ALPN,
            HttpBridge {
                target: target.clone(),
            },
        )
        .spawn();

    tokio::time::timeout(Duration::from_secs(20), router.endpoint().online())
        .await
        .context("endpoint did not become relay-online within 20 seconds")?;

    let ticket = encode_endpoint_addr(&router.endpoint().addr())?;
    write_ticket_file_if_configured(&ticket).await?;

    eprintln!("target={target}");
    println!("IROH_ENDPOINT_TICKET={ticket}");
    eprintln!("ready; press ctrl-c to stop");

    tokio::signal::ctrl_c().await?;
    router.shutdown().await?;
    Ok(())
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

    for run in 1..=runs {
        let start = Instant::now();
        let connection = endpoint.connect(addr.clone(), ALPN).await?;
        let connected = start.elapsed();

        let (mut send, mut recv) = connection.open_bi().await?;
        send.write_all(health_request()).await?;
        send.finish()?;

        let response = recv.read_to_end(MAX_HTTP_BYTES).await?;
        let total = start.elapsed();
        let code = parse_status_code(&response).unwrap_or(0);

        println!(
            "run={run} code={code} connect_ms={} total_ms={} bytes={}",
            connected.as_micros() as f64 / 1000.0,
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
    let text = std::str::from_utf8(response).ok()?;
    let line = text.lines().next()?;
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
    use super::parse_status_code;

    #[test]
    fn parses_http_status_code() {
        let response = b"HTTP/1.1 200 OK\r\ncontent-length: 11\r\n\r\n{\"ok\":true}";

        assert_eq!(parse_status_code(response), Some(200));
    }

    #[test]
    fn rejects_invalid_http_response() {
        assert_eq!(parse_status_code(b"{\"ok\":true}"), None);
        assert_eq!(parse_status_code(b"HTTP/1.1 nope OK\r\n\r\n"), None);
    }
}
