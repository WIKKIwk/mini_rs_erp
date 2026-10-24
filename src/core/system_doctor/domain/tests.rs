use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use super::*;

#[derive(Default)]
struct FakeState {
    remote: VecDeque<Result<bool, DomainError>>,
    local: VecDeque<Result<bool, DomainError>>,
    reconnects: usize,
}

struct FakeDomainAdapter(Mutex<FakeState>);

impl FakeDomainAdapter {
    fn new(remote: impl IntoIterator<Item = Result<bool, DomainError>>) -> Arc<Self> {
        Arc::new(Self(Mutex::new(FakeState {
            remote: remote.into_iter().collect(),
            ..FakeState::default()
        })))
    }

    fn local(&self, result: Result<bool, DomainError>) {
        self.0.lock().unwrap().local.push_back(result);
    }

    fn reconnects(&self) -> usize {
        self.0.lock().unwrap().reconnects
    }
}

#[async_trait]
impl DomainAdapter for FakeDomainAdapter {
    async fn domain_is_healthy(&self) -> Result<bool, DomainError> {
        self.0
            .lock()
            .unwrap()
            .remote
            .pop_front()
            .unwrap_or(Ok(false))
    }

    async fn local_service_is_healthy(&self) -> Result<bool, DomainError> {
        self.0.lock().unwrap().local.pop_front().unwrap_or(Ok(true))
    }

    async fn reconnect_tunnel(&self) -> Result<(), DomainError> {
        self.0.lock().unwrap().reconnects += 1;
        Ok(())
    }
}

fn config() -> DomainConfig {
    DomainConfig {
        url: Url::parse("https://erp.example.test").unwrap(),
        local_url: Url::parse("http://127.0.0.1:18081").unwrap(),
        state_dir: None,
        tunnel_name: "mini-rs-erp-erp-example-test".to_owned(),
        check_interval: Duration::from_secs(10),
        failure_samples: 3,
        recovery_samples: 2,
        restart_cooldown: Duration::from_secs(60),
    }
}

async fn at(recovery: &mut DomainRecovery, start: Instant, seconds: u64) -> DomainRecoveryReport {
    recovery
        .step(start + Duration::from_secs(seconds), 1000 + seconds as i64)
        .await
}

#[test]
fn configured_domain_must_be_https_and_local_probe_must_be_loopback() {
    assert_eq!(
        parse_domain_url("erp.example.test").unwrap().as_str(),
        "https://erp.example.test/"
    );
    for url in [
        "http://erp.example.test",
        "https://user:pass@erp.example.test",
        "https://erp.example.test/private/path",
        "https://erp.example.test/?token=secret",
    ] {
        assert!(parse_domain_url(url).is_err(), "{url}");
    }
    assert!(parse_local_url("http://10.0.0.2:18081").is_err());
    assert!(parse_local_url("http://127.0.0.1:18081").is_ok());
}

#[tokio::test]
async fn confirmed_domain_failure_reconnects_tunnel_only_when_local_app_is_healthy() {
    let adapter = FakeDomainAdapter::new([Ok(false), Ok(false), Err(DomainError::ProbeFailed)]);
    adapter.local(Ok(true));
    let mut recovery = DomainRecovery::new(config(), adapter.clone());
    let start = Instant::now();

    assert_eq!(at(&mut recovery, start, 0).await.status, "warning");
    assert_eq!(at(&mut recovery, start, 10).await.status, "warning");
    let incident = at(&mut recovery, start, 20).await;
    assert_eq!(incident.status, "reconnecting");
    assert!(incident.critical);
    assert_eq!(incident.recovery_attempts, 1);
    assert_eq!(incident.next_attempt_at_unix, Some(1080));
    assert_eq!(adapter.reconnects(), 1);

    // The scheduled cooldown prevents restart loops while health is rechecked.
    let still_down = at(&mut recovery, start, 30).await;
    assert_eq!(still_down.status, "critical");
    assert_eq!(adapter.reconnects(), 1);
}

#[tokio::test]
async fn local_service_failure_is_reported_without_restarting_tunnel() {
    let adapter = FakeDomainAdapter::new([Ok(false), Ok(false)]);
    adapter.local(Ok(false));
    let mut recovery = DomainRecovery::new(
        DomainConfig {
            failure_samples: 2,
            ..config()
        },
        adapter.clone(),
    );
    let start = Instant::now();
    at(&mut recovery, start, 0).await;
    let report = at(&mut recovery, start, 10).await;
    assert!(report.critical);
    assert_eq!(
        report.last_error.as_deref(),
        Some("local_service_unavailable")
    );
    assert_eq!(report.recovery_attempts, 0);
    assert_eq!(adapter.reconnects(), 0);
}

#[tokio::test]
async fn two_successful_domain_checks_clear_a_confirmed_incident() {
    let adapter = FakeDomainAdapter::new([Ok(false), Ok(false), Ok(false), Ok(true), Ok(true)]);
    adapter.local(Ok(true));
    let mut recovery = DomainRecovery::new(config(), adapter);
    let start = Instant::now();
    for second in [0, 10, 20] {
        at(&mut recovery, start, second).await;
    }
    let recovering = at(&mut recovery, start, 30).await;
    assert_eq!(recovering.status, "recovering");
    assert!(recovering.critical);
    let healthy = at(&mut recovery, start, 40).await;
    assert_eq!(healthy.status, "healthy");
    assert!(!healthy.critical);
    assert_eq!(healthy.failures, 0);
}
