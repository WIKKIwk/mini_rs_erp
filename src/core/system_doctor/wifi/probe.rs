//! Validate Internet reachability, including after association with an open AP.

use super::{InternetProbe, WifiError};
use async_trait::async_trait;
use reqwest::{Client, StatusCode, Url};
use std::time::Duration;

const DEFAULT_TARGETS: &str =
    "https://www.gstatic.com/generate_204,https://cp.cloudflare.com/generate_204";

pub struct HttpsInternetProbe {
    client: Client,
    targets: Vec<Url>,
}

impl HttpsInternetProbe {
    pub fn from_env() -> Result<Self, WifiError> {
        let configured = match std::env::var("MINI_ERP_DOCTOR_WIFI_PROBE_URLS") {
            Ok(value) => value,
            Err(std::env::VarError::NotPresent) => DEFAULT_TARGETS.to_string(),
            Err(_) => return Err(WifiError::InvalidConfiguration),
        };
        Ok(Self {
            client: build_client(true)?,
            targets: parse_targets(&configured)?,
        })
    }
}

fn build_client(https_only: bool) -> Result<Client, WifiError> {
    Client::builder()
        .https_only(https_only)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(5))
        .pool_max_idle_per_host(1)
        .user_agent("Accord-SystemDoctor/1.0")
        .build()
        .map_err(|_| WifiError::InvalidConfiguration)
}

fn parse_targets(configured: &str) -> Result<Vec<Url>, WifiError> {
    let mut targets = Vec::new();
    for raw in configured.split(',') {
        let target = Url::parse(raw.trim()).map_err(|_| WifiError::InvalidConfiguration)?;
        if target.scheme() != "https"
            || target.host_str().is_none()
            || !target.username().is_empty()
            || target.password().is_some()
            || target.fragment().is_some()
            || target.port_or_known_default() == Some(0)
        {
            return Err(WifiError::InvalidConfiguration);
        }
        targets.push(target);
        if targets.len() > 4 {
            return Err(WifiError::InvalidConfiguration);
        }
    }
    if targets.is_empty() {
        return Err(WifiError::InvalidConfiguration);
    }
    Ok(targets)
}

#[async_trait]
impl InternetProbe for HttpsInternetProbe {
    async fn has_internet(&self) -> Result<bool, WifiError> {
        // Either independent target succeeding is sufficient. A failed provider
        // must not cause Wi-Fi switching while the other remains reachable.
        for target in &self.targets {
            if let Ok(response) = self.client.get(target.clone()).send().await
                && response.status() == StatusCode::NO_CONTENT
            {
                return Ok(true);
            }
        }
        // Captive portal HTML/redirects and TLS failures are not Internet access.
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn fixture(response: &'static str) -> Url {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 4096];
            stream.read(&mut request).await.unwrap();
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        Url::parse(&format!("http://{address}/check")).unwrap()
    }

    fn probe(targets: Vec<Url>) -> HttpsInternetProbe {
        // Only the local test fixtures use HTTP. Production targets require TLS.
        HttpsInternetProbe {
            client: build_client(false).unwrap(),
            targets,
        }
    }

    #[test]
    fn configured_probes_require_https_without_embedded_credentials() {
        assert_eq!(parse_targets(DEFAULT_TARGETS).unwrap().len(), 2);
        for bad in [
            "",
            "http://example.com/check",
            "https://name:secret@example.com/",
            "https://example.com/#fragment",
            "https://example.com:0/",
        ] {
            assert!(parse_targets(bad).is_err(), "{bad}");
        }
        assert!(parse_targets(&["https://example.com/"; 5].join(",")).is_err());
    }

    #[tokio::test]
    async fn reachable204_is_online() {
        let target = fixture("HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n").await;
        assert!(probe(vec![target]).has_internet().await.unwrap());
    }

    #[tokio::test]
    async fn captive_portal_html_is_not_online() {
        let target =
            fixture("HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nlogin").await;
        assert!(!probe(vec![target]).has_internet().await.unwrap());
    }

    #[tokio::test]
    async fn redirects_to_a_working_endpoint_are_not_followed() {
        let destination = fixture("HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n").await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 4096];
            stream.read(&mut request).await.unwrap();
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: {destination}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        let source = Url::parse(&format!("http://{address}/")).unwrap();
        assert!(!probe(vec![source]).has_internet().await.unwrap());
    }

    #[tokio::test]
    async fn another_provider_can_confirm_connectivity() {
        let failed =
            fixture("HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
        let healthy = fixture("HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n").await;
        assert!(probe(vec![failed, healthy]).has_internet().await.unwrap());
    }
}
