mod ai;
mod app;
mod config;
mod core;
pub(crate) mod db;
pub(crate) mod engine;
mod error;
mod fcm;
#[cfg(test)]
mod fcm_tests;
mod google_sheets;
mod http;
mod rps;
mod store;

use crate::app::AppState;
use crate::config::AppConfig;
use axum::Router;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), error::AppError> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = AppConfig::from_env()?;
    let bind_addr = config.bind_addr;
    let state = AppState::new(config);
    let app = http::router::build_router(state);

    tracing::info!(%bind_addr, "starting mini rs erp");
    let listener_count = listener_count();
    if listener_count == 1 {
        let listener = bind_tcp_listener(bind_addr)?;
        serve_listener(listener, app, 0).await?;
    } else {
        tracing::info!(listener_count, "starting reuseport listener workers");
        let mut handles = Vec::with_capacity(listener_count);
        for worker in 0..listener_count {
            let listener = bind_tcp_listener(bind_addr)?;
            let app = app.clone();
            handles.push(tokio::spawn(async move {
                tracing::info!(worker, "listener worker started");
                serve_listener(listener, app, worker).await
            }));
        }
        for handle in handles {
            handle.await.map_err(|error| {
                error::AppError::Storage(format!("server task failed: {error}"))
            })??;
        }
    }

    Ok(())
}

async fn serve_listener(
    listener: TcpListener,
    app: Router,
    worker: usize,
) -> Result<(), error::AppError> {
    loop {
        let (stream, peer_addr) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                tracing::warn!(%error, worker, "failed to accept connection");
                sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        if let Err(error) = stream.set_nodelay(true) {
            tracing::trace!(%error, %peer_addr, worker, "failed to enable TCP_NODELAY");
        }

        let app = app.clone();
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            let service = TowerToHyperService::new(app);
            let mut builder = http1::Builder::new();
            builder.keep_alive(true);

            if let Err(error) = builder.serve_connection(io, service).await {
                tracing::trace!(%error, %peer_addr, worker, "connection failed");
            }
        });
    }
}

fn listener_count() -> usize {
    std::env::var("MOBILE_API_LISTENER_COUNT")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .and_then(NonZeroUsize::new)
        .map(NonZeroUsize::get)
        .unwrap_or_else(default_listener_count)
}

fn default_listener_count() -> usize {
    #[cfg(unix)]
    {
        std::thread::available_parallelism()
            .map(NonZeroUsize::get)
            .unwrap_or(1)
            .clamp(1, 8)
    }

    #[cfg(not(unix))]
    {
        1
    }
}

fn bind_tcp_listener(bind_addr: SocketAddr) -> Result<tokio::net::TcpListener, error::AppError> {
    let domain = if bind_addr.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&bind_addr.into())?;
    socket.listen(4096)?;

    let listener: std::net::TcpListener = socket.into();
    Ok(tokio::net::TcpListener::from_std(listener)?)
}
