use std::net::SocketAddr;
use std::time::Duration;

use crate::gateway_config::GatewayConfig;

#[test]
fn gateway_config_uses_defaults() {
    let config = GatewayConfig::from_pairs(std::iter::empty::<(&str, &str)>()).unwrap();

    assert_eq!(
        config.bind_addr,
        "127.0.0.1:18080".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(config.upstream_addr, "127.0.0.1:18081");
    assert_eq!(config.upstream_host, "127.0.0.1");
    assert_eq!(config.health_path, "/healthz");
    assert_eq!(config.connect_timeout, Duration::from_millis(1000));
    assert_eq!(config.upstream_timeout, Duration::from_millis(30000));
    assert!(config.tls_cert_path.is_none());
    assert!(config.tls_key_path.is_none());
}

#[test]
fn gateway_config_reads_env_pairs() {
    let config = GatewayConfig::from_pairs([
        ("MINI_RS_GATEWAY_ADDR", "0.0.0.0:8443"),
        ("MINI_RS_GATEWAY_UPSTREAM", "127.0.0.1:18081"),
        (
            "MINI_RS_GATEWAY_UPSTREAM_HOST",
            "mini-rs-erp-test.wspace.sbs",
        ),
        ("MINI_RS_GATEWAY_HEALTH_PATH", "/ready"),
        ("MINI_RS_GATEWAY_CONNECT_TIMEOUT_MS", "1500"),
        ("MINI_RS_GATEWAY_UPSTREAM_TIMEOUT_MS", "45000"),
        ("MINI_RS_GATEWAY_TLS_CERT_PATH", "/etc/ssl/cert.pem"),
        ("MINI_RS_GATEWAY_TLS_KEY_PATH", "/etc/ssl/key.pem"),
    ])
    .unwrap();

    assert_eq!(
        config.bind_addr,
        "0.0.0.0:8443".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(config.upstream_addr, "127.0.0.1:18081");
    assert_eq!(config.upstream_host, "mini-rs-erp-test.wspace.sbs");
    assert_eq!(config.health_path, "/ready");
    assert_eq!(config.connect_timeout, Duration::from_millis(1500));
    assert_eq!(config.upstream_timeout, Duration::from_millis(45000));
    assert_eq!(config.tls_cert_path.as_deref(), Some("/etc/ssl/cert.pem"));
    assert_eq!(config.tls_key_path.as_deref(), Some("/etc/ssl/key.pem"));
}

#[test]
fn gateway_config_rejects_invalid_bind_addr() {
    let error = GatewayConfig::from_pairs([("MINI_RS_GATEWAY_ADDR", "bad addr")]).unwrap_err();
    assert!(error.contains("MINI_RS_GATEWAY_ADDR"));
}
