use async_trait::async_trait;
use bytes::Bytes;
use mini_rs_erp::gateway_config::GatewayConfig;
use pingora_core::Result;
use pingora_core::listeners::tls::TlsSettings;
use pingora_core::server::Server;
use pingora_core::server::configuration::Opt;
use pingora_core::services::listening::Service;
use pingora_core::upstreams::peer::HttpPeer;
use pingora_http::ResponseHeader;
use pingora_proxy::{ProxyHttp, Session};

#[derive(Clone)]
struct MiniRsGateway {
    config: GatewayConfig,
}

#[async_trait]
impl ProxyHttp for MiniRsGateway {
    type CTX = ();

    fn new_ctx(&self) -> Self::CTX {}

    async fn request_filter(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<bool> {
        if session.req_header().uri.path() == self.config.health_path {
            let body = Bytes::from_static(br#"{"ok":true,"service":"mini_rs_gateway"}"#);
            session.respond_error_with_body(200, body).await?;
            return Ok(true);
        }
        Ok(false)
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        Ok(Box::new(HttpPeer::new(
            self.config.upstream_addr.as_str(),
            false,
            self.config.upstream_host.clone(),
        )))
    }

    async fn upstream_request_filter(
        &self,
        session: &mut Session,
        upstream_request: &mut pingora_http::RequestHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<()> {
        upstream_request.insert_header("Host", &self.config.upstream_host)?;
        if let Some(peer) = session.as_downstream().client_addr() {
            upstream_request.insert_header("X-Forwarded-For", peer.to_string())?;
        }
        upstream_request.insert_header("X-Forwarded-Proto", "https")?;
        Ok(())
    }

    async fn response_filter(
        &self,
        _session: &mut Session,
        upstream_response: &mut ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<()>
    where
        Self::CTX: Send + Sync,
    {
        upstream_response.insert_header("Server", "mini-rs-gateway")?;
        upstream_response.remove_header("alt-svc");
        Ok(())
    }
}

fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = GatewayConfig::from_env().expect("valid gateway config");
    let opt = Opt::parse_args();
    let mut server = Server::new(Some(opt)).expect("pingora server");
    server.bootstrap();

    let mut proxy = pingora_proxy::http_proxy_service(
        &server.configuration,
        MiniRsGateway {
            config: config.clone(),
        },
    );

    let bind_addr = config.bind_addr.to_string();
    match (
        config.tls_cert_path.as_deref(),
        config.tls_key_path.as_deref(),
    ) {
        (Some(cert), Some(key)) => {
            let mut tls = TlsSettings::intermediate(cert, key).expect("valid tls settings");
            tls.enable_h2();
            proxy.add_tls_with_settings(&bind_addr, None, tls);
        }
        _ => proxy.add_tcp(&bind_addr),
    }

    let mut metrics = Service::prometheus_http_service();
    metrics.add_tcp("127.0.0.1:19091");

    server.add_service(proxy);
    server.add_service(metrics);
    server.run_forever();
}
