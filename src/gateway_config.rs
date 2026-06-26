use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayConfig {
    pub bind_addr: SocketAddr,
    pub upstream_addr: String,
    pub upstream_host: String,
    pub health_path: String,
    pub connect_timeout: Duration,
    pub upstream_timeout: Duration,
    pub tls_cert_path: Option<String>,
    pub tls_key_path: Option<String>,
    pub metrics_addr: Option<SocketAddr>,
}

impl GatewayConfig {
    pub fn from_env() -> Result<Self, String> {
        Self::from_pairs(std::env::vars())
    }

    pub fn from_pairs<I, K, V>(pairs: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let values = pairs
            .into_iter()
            .map(|(key, value)| (key.as_ref().to_string(), value.as_ref().to_string()))
            .collect::<BTreeMap<_, _>>();

        let bind_raw = value_or(&values, "MINI_RS_GATEWAY_ADDR", "127.0.0.1:18080");
        let bind_addr = bind_raw
            .parse::<SocketAddr>()
            .map_err(|_| format!("invalid MINI_RS_GATEWAY_ADDR: {bind_raw}"))?;
        let upstream_addr = value_or(&values, "MINI_RS_GATEWAY_UPSTREAM", "127.0.0.1:18081");
        let upstream_host = value_or(&values, "MINI_RS_GATEWAY_UPSTREAM_HOST", "127.0.0.1");
        let health_path = value_or(&values, "MINI_RS_GATEWAY_HEALTH_PATH", "/healthz");
        let connect_timeout = Duration::from_millis(parse_millis(
            &values,
            "MINI_RS_GATEWAY_CONNECT_TIMEOUT_MS",
            1000,
        )?);
        let upstream_timeout = Duration::from_millis(parse_millis(
            &values,
            "MINI_RS_GATEWAY_UPSTREAM_TIMEOUT_MS",
            30000,
        )?);
        let metrics_addr =
            parse_optional_addr(&values, "MINI_RS_GATEWAY_METRICS_ADDR", "127.0.0.1:19091")?;

        Ok(Self {
            bind_addr,
            upstream_addr,
            upstream_host,
            health_path,
            connect_timeout,
            upstream_timeout,
            tls_cert_path: optional_value(&values, "MINI_RS_GATEWAY_TLS_CERT_PATH"),
            tls_key_path: optional_value(&values, "MINI_RS_GATEWAY_TLS_KEY_PATH"),
            metrics_addr,
        })
    }
}

fn value_or(values: &BTreeMap<String, String>, key: &str, fallback: &str) -> String {
    values
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn optional_value(values: &BTreeMap<String, String>, key: &str) -> Option<String> {
    values
        .get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_optional_addr(
    values: &BTreeMap<String, String>,
    key: &str,
    fallback: &str,
) -> Result<Option<SocketAddr>, String> {
    let raw = values
        .get(key)
        .map(|value| value.trim())
        .unwrap_or(fallback);
    if raw.is_empty() {
        return Ok(None);
    }
    raw.parse::<SocketAddr>()
        .map(Some)
        .map_err(|_| format!("invalid {key}: {raw}"))
}

fn parse_millis(
    values: &BTreeMap<String, String>,
    key: &str,
    fallback: u64,
) -> Result<u64, String> {
    let raw = value_or(values, key, &fallback.to_string());
    match raw.parse::<u64>() {
        Ok(value) if value > 0 => Ok(value),
        _ => Err(format!("invalid {key}: {raw}")),
    }
}
