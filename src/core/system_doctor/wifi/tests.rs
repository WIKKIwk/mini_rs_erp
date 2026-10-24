use std::sync::Mutex;

use super::*;

#[derive(Default)]
struct AdapterState {
    calls: Vec<String>,
    active: Option<String>,
    visible: Vec<(OpenNetwork, bool)>,
    snapshot_error: Option<WifiError>,
    scan_error: Option<WifiError>,
    connections: VecDeque<Result<(), WifiError>>,
    cleanups: VecDeque<Result<(), WifiError>>,
    restorations: VecDeque<Result<(), WifiError>>,
}

#[derive(Default)]
struct FakeAdapter(Mutex<AdapterState>);

impl FakeAdapter {
    fn new(networks: Vec<OpenNetwork>) -> Arc<Self> {
        Arc::new(Self(Mutex::new(AdapterState {
            active: Some("original-profile-token".to_owned()),
            visible: networks
                .into_iter()
                .map(|network| (network, true))
                .collect(),
            ..AdapterState::default()
        })))
    }

    fn calls(&self) -> Vec<String> {
        self.0.lock().unwrap().calls.clone()
    }

    fn connection_calls(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter(|call| call.starts_with("connect:"))
            .collect()
    }
}

#[async_trait]
impl WifiAdapter for FakeAdapter {
    async fn current_connection(&self) -> Result<Option<String>, WifiError> {
        let mut state = self.0.lock().unwrap();
        state.calls.push("snapshot".to_owned());
        if let Some(error) = state.snapshot_error {
            return Err(error);
        }
        Ok(state.active.clone())
    }

    async fn scan_open(&self) -> Result<Vec<OpenNetwork>, WifiError> {
        let mut state = self.0.lock().unwrap();
        state.calls.push("scan".to_owned());
        if let Some(error) = state.scan_error {
            return Err(error);
        }
        // The adapter contract excludes secured and ambiguous access points.
        Ok(state
            .visible
            .iter()
            .filter(|(_, open)| *open)
            .map(|(network, _)| network.clone())
            .collect())
    }

    async fn connect_open(&self, network: &OpenNetwork) -> Result<(), WifiError> {
        let mut state = self.0.lock().unwrap();
        state.calls.push(format!("connect:{}", network.ssid));
        state.active = Some(format!("ephemeral:{}", network.bssid));
        state.connections.pop_front().unwrap_or(Ok(()))
    }

    async fn disconnect_attempt(&self) -> Result<(), WifiError> {
        let mut state = self.0.lock().unwrap();
        state.calls.push("cleanup".to_owned());
        let result = state.cleanups.pop_front().unwrap_or(Ok(()));
        if result.is_ok() {
            state.active = None;
        }
        result
    }

    async fn restore_connection(&self, token: &str) -> Result<(), WifiError> {
        let mut state = self.0.lock().unwrap();
        state.calls.push(format!("restore:{token}"));
        let result = state.restorations.pop_front().unwrap_or(Ok(()));
        if result.is_ok() {
            state.active = Some(token.to_owned());
        }
        result
    }
}

struct ProbeState {
    fallback: Result<bool, WifiError>,
    samples: VecDeque<Result<bool, WifiError>>,
    calls: usize,
}

struct FakeProbe(Mutex<ProbeState>);

impl FakeProbe {
    fn new(fallback: Result<bool, WifiError>) -> Arc<Self> {
        Arc::new(Self(Mutex::new(ProbeState {
            fallback,
            samples: VecDeque::new(),
            calls: 0,
        })))
    }

    fn next(&self, sample: Result<bool, WifiError>) {
        self.0.lock().unwrap().samples.push_back(sample);
    }

    fn calls(&self) -> usize {
        self.0.lock().unwrap().calls
    }
}

#[async_trait]
impl InternetProbe for FakeProbe {
    async fn has_internet(&self) -> Result<bool, WifiError> {
        let mut state = self.0.lock().unwrap();
        state.calls += 1;
        state.samples.pop_front().unwrap_or(state.fallback)
    }
}

fn network(ssid: &str, index: u8, signal: i32) -> OpenNetwork {
    OpenNetwork {
        ssid: ssid.to_owned(),
        bssid: format!("02:00:00:00:00:{index:02x}"),
        signal,
    }
}

fn config() -> WifiConfig {
    WifiConfig {
        failure_samples: 1,
        check_interval: Duration::from_secs(10),
        ..WifiConfig::default()
    }
}

async fn at(recovery: &mut WifiRecovery, start: Instant, seconds: u64) -> WifiRecoveryReport {
    recovery
        .step(start + Duration::from_secs(seconds), 1000 + seconds as i64)
        .await
}

#[tokio::test]
async fn healthy_internet_never_scans_or_switches() {
    let adapter = FakeAdapter::new(vec![network("Free", 1, 80)]);
    let probe = FakeProbe::new(Ok(true));
    let mut recovery = WifiRecovery::new(config(), adapter.clone(), probe.clone());
    let start = Instant::now();
    assert_eq!(at(&mut recovery, start, 0).await.status, "online");
    at(&mut recovery, start, 1).await;
    at(&mut recovery, start, 11).await;
    assert!(adapter.calls().is_empty());
    assert_eq!(probe.calls(), 2);
}

#[tokio::test]
async fn single_failure_is_debounced_and_recovery_clears_it() {
    let adapter = FakeAdapter::new(vec![network("Free", 1, 80)]);
    let probe = FakeProbe::new(Ok(true));
    probe.next(Ok(false));
    let mut recovery = WifiRecovery::new(WifiConfig::default(), adapter.clone(), probe);
    let start = Instant::now();
    let suspected = at(&mut recovery, start, 0).await;
    assert_eq!(suspected.status, "suspected_offline");
    assert_eq!(suspected.failures, 1);
    assert!(adapter.calls().is_empty());
    let recovered = at(&mut recovery, start, 16).await;
    assert_eq!(recovered.status, "online");
    assert_eq!(recovered.failures, 0);
    assert!(adapter.calls().is_empty());
}

#[tokio::test]
async fn three_failed_samples_are_required_before_scanning() {
    let adapter = FakeAdapter::new(vec![network("Free", 1, 80)]);
    let probe = FakeProbe::new(Ok(false));
    let mut recovery = WifiRecovery::new(WifiConfig::default(), adapter.clone(), probe);
    let start = Instant::now();
    at(&mut recovery, start, 0).await;
    at(&mut recovery, start, 16).await;
    assert!(adapter.calls().is_empty());
    assert_eq!(at(&mut recovery, start, 32).await.status, "scanning");
    assert_eq!(adapter.calls(), ["snapshot", "scan"]);
}

#[tokio::test]
async fn failed_a_then_b_gets_internet_after_settle_and_retry_delays() {
    let adapter = FakeAdapter::new(vec![network("B", 2, 20), network("A", 1, 90)]);
    let probe = FakeProbe::new(Ok(false));
    let mut recovery = WifiRecovery::new(config(), adapter.clone(), probe.clone());
    let start = Instant::now();
    at(&mut recovery, start, 0).await;
    assert_eq!(at(&mut recovery, start, 1).await.status, "verifying");
    at(&mut recovery, start, 5).await;
    assert_eq!(adapter.connection_calls(), ["connect:A"]);
    assert!(!adapter.calls().contains(&"cleanup".to_owned()));
    let failed_a = at(&mut recovery, start, 7).await;
    assert_eq!(failed_a.status, "waiting");
    assert_eq!(failed_a.last_error.as_deref(), Some("no_internet"));
    at(&mut recovery, start, 11).await;
    assert_eq!(adapter.connection_calls(), ["connect:A"]);
    at(&mut recovery, start, 13).await;
    assert_eq!(adapter.connection_calls(), ["connect:A", "connect:B"]);
    probe.next(Ok(true));
    let online = at(&mut recovery, start, 19).await;
    assert_eq!(online.status, "online");
    assert_eq!(online.current_ssid.as_deref(), Some("B"));
    assert_eq!(online.failures, 0);
    assert!(online.last_error.is_none());
    assert!(
        !adapter
            .calls()
            .iter()
            .any(|call| call.starts_with("restore:"))
    );
}

#[tokio::test]
async fn association_without_internet_is_not_success_and_restores_baseline() {
    let adapter = FakeAdapter::new(vec![network("Captive portal", 1, 90)]);
    let probe = FakeProbe::new(Ok(false));
    let mut recovery = WifiRecovery::new(config(), adapter.clone(), probe);
    let start = Instant::now();
    at(&mut recovery, start, 0).await;
    let associated = at(&mut recovery, start, 1).await;
    assert_eq!(associated.status, "verifying");
    let failed = at(&mut recovery, start, 7).await;
    assert_eq!(failed.status, "restoring");
    assert_eq!(failed.last_error.as_deref(), Some("no_internet"));
    at(&mut recovery, start, 11).await;
    assert!(
        !adapter
            .calls()
            .iter()
            .any(|call| call.starts_with("restore:"))
    );
    assert_eq!(at(&mut recovery, start, 13).await.status, "cooldown");
    assert_eq!(
        adapter.0.lock().unwrap().active.as_deref(),
        Some("original-profile-token")
    );
}

#[tokio::test]
async fn all_failed_access_points_restore_previous_connection_once() {
    let adapter = FakeAdapter::new(vec![network("A", 1, 90), network("B", 2, 70)]);
    let mut recovery = WifiRecovery::new(config(), adapter.clone(), FakeProbe::new(Ok(false)));
    let start = Instant::now();
    for second in [0, 1, 7, 13, 19, 25] {
        at(&mut recovery, start, second).await;
    }
    assert_eq!(recovery.report().status, "cooldown");
    assert_eq!(
        adapter.calls(),
        [
            "snapshot",
            "scan",
            "connect:A",
            "cleanup",
            "connect:B",
            "cleanup",
            "restore:original-profile-token"
        ]
    );
    let before = adapter.calls();
    at(&mut recovery, start, 30).await;
    assert_eq!(adapter.calls(), before);
}

#[tokio::test]
async fn adapter_excludes_closed_networks_and_policy_rejects_invalid_identifiers() {
    let adapter = FakeAdapter::new(vec![network("Open", 1, 30)]);
    {
        let mut state = adapter.0.lock().unwrap();
        state.visible.extend([
            (network("Secured", 2, 100), false),
            (network("", 3, 100), true),
            (network(&"x".repeat(33), 4, 100), true),
            (
                OpenNetwork {
                    bssid: "invalid".to_owned(),
                    ..network("Bad BSSID", 5, 100)
                },
                true,
            ),
        ]);
    }
    let mut recovery = WifiRecovery::new(config(), adapter.clone(), FakeProbe::new(Ok(false)));
    let start = Instant::now();
    at(&mut recovery, start, 0).await;
    at(&mut recovery, start, 1).await;
    assert_eq!(adapter.connection_calls(), ["connect:Open"]);
}

#[tokio::test]
async fn deduplication_round_limit_and_failed_network_cooldown_survive_rescans() {
    let mut duplicate = network("A duplicate", 10, 80);
    duplicate.bssid.make_ascii_uppercase();
    let adapter = FakeAdapter::new(vec![network("A", 10, 90), duplicate, network("B", 2, 70)]);
    let mut recovery = WifiRecovery::new(
        WifiConfig {
            max_networks_per_round: 1,
            ..config()
        },
        adapter.clone(),
        FakeProbe::new(Ok(false)),
    );
    let start = Instant::now();
    for second in [0, 1, 7, 13, 74, 75] {
        at(&mut recovery, start, second).await;
    }
    assert_eq!(adapter.connection_calls(), ["connect:A", "connect:B"]);
    assert_eq!(
        adapter
            .calls()
            .iter()
            .filter(|call| *call == "scan")
            .count(),
        2
    );
}

#[tokio::test]
async fn route_recovery_before_an_attempt_prevents_switching() {
    let adapter = FakeAdapter::new(vec![network("Free", 1, 80)]);
    let probe = FakeProbe::new(Ok(false));
    let mut recovery = WifiRecovery::new(config(), adapter.clone(), probe.clone());
    let start = Instant::now();
    at(&mut recovery, start, 0).await;
    probe.next(Ok(true));
    assert_eq!(at(&mut recovery, start, 1).await.status, "online");
    assert!(adapter.connection_calls().is_empty());
}

#[tokio::test]
async fn command_failure_is_cleaned_before_trying_next_network() {
    let adapter = FakeAdapter::new(vec![network("A", 1, 90), network("B", 2, 80)]);
    adapter
        .0
        .lock()
        .unwrap()
        .connections
        .push_back(Err(WifiError::CommandFailed));
    let mut recovery = WifiRecovery::new(config(), adapter.clone(), FakeProbe::new(Ok(false)));
    let start = Instant::now();
    at(&mut recovery, start, 0).await;
    let error = at(&mut recovery, start, 1).await;
    assert_eq!(error.last_error.as_deref(), Some("command_failed"));
    assert_eq!(
        adapter.calls(),
        ["snapshot", "scan", "connect:A", "cleanup"]
    );
    at(&mut recovery, start, 5).await;
    assert_eq!(adapter.connection_calls().len(), 1);
    at(&mut recovery, start, 7).await;
    assert_eq!(adapter.connection_calls(), ["connect:A", "connect:B"]);
}

#[tokio::test]
async fn cleanup_failure_blocks_further_switching_until_cleanup_succeeds() {
    let adapter = FakeAdapter::new(vec![network("A", 1, 90), network("B", 2, 80)]);
    adapter
        .0
        .lock()
        .unwrap()
        .cleanups
        .push_back(Err(WifiError::CommandFailed));
    let mut recovery = WifiRecovery::new(config(), adapter.clone(), FakeProbe::new(Ok(false)));
    let start = Instant::now();
    for second in [0, 1] {
        at(&mut recovery, start, second).await;
    }
    assert_eq!(at(&mut recovery, start, 7).await.status, "cleanup_failed");
    at(&mut recovery, start, 11).await;
    assert_eq!(adapter.connection_calls(), ["connect:A"]);
    assert_eq!(at(&mut recovery, start, 13).await.status, "waiting");
    assert_eq!(adapter.connection_calls(), ["connect:A"]);
    at(&mut recovery, start, 19).await;
    assert_eq!(adapter.connection_calls(), ["connect:A", "connect:B"]);
}

#[tokio::test]
async fn unavailable_original_ap_does_not_prevent_later_recovery_rounds() {
    let adapter = FakeAdapter::new(vec![network("A", 1, 90)]);
    adapter
        .0
        .lock()
        .unwrap()
        .restorations
        .push_back(Err(WifiError::CommandFailed));
    let mut recovery = WifiRecovery::new(config(), adapter.clone(), FakeProbe::new(Ok(false)));
    let start = Instant::now();
    for second in [0, 1, 7] {
        at(&mut recovery, start, second).await;
    }
    let failed = at(&mut recovery, start, 13).await;
    assert_eq!(failed.status, "restore_failed");
    assert_eq!(failed.last_error.as_deref(), Some("command_failed"));
    at(&mut recovery, start, 30).await;
    assert_eq!(
        adapter
            .calls()
            .iter()
            .filter(|call| *call == "scan")
            .count(),
        1
    );
    adapter
        .0
        .lock()
        .unwrap()
        .visible
        .push((network("New B", 2, 80), true));
    assert_eq!(at(&mut recovery, start, 74).await.status, "scanning");
    at(&mut recovery, start, 75).await;
    assert_eq!(adapter.connection_calls(), ["connect:A", "connect:New B"]);
    at(&mut recovery, start, 81).await;
    at(&mut recovery, start, 87).await;
    assert_eq!(
        adapter.0.lock().unwrap().active.as_deref(),
        Some("original-profile-token")
    );
    assert_eq!(
        adapter
            .calls()
            .iter()
            .filter(|call| *call == "snapshot")
            .count(),
        1
    );
}

#[tokio::test]
async fn failed_ap_cooldown_survives_success_on_another_ap() {
    let adapter = FakeAdapter::new(vec![network("A", 1, 90), network("B", 2, 80)]);
    let probe = FakeProbe::new(Ok(false));
    let mut recovery = WifiRecovery::new(config(), adapter.clone(), probe.clone());
    let start = Instant::now();
    for second in [0, 1, 7, 13] {
        at(&mut recovery, start, second).await;
    }
    probe.next(Ok(true));
    assert_eq!(at(&mut recovery, start, 19).await.status, "online");
    assert_eq!(recovery.cooldowns.len(), 1);
    assert_eq!(at(&mut recovery, start, 30).await.status, "scanning");
    at(&mut recovery, start, 31).await;
    assert_eq!(
        adapter.connection_calls(),
        ["connect:A", "connect:B", "connect:B"]
    );
}

#[tokio::test]
async fn failed_ap_can_retry_after_each_observed_disappearance_and_reappearance() {
    let access_point = network("A", 1, 90);
    let adapter = FakeAdapter::new(vec![access_point.clone()]);
    let mut recovery = WifiRecovery::new(
        WifiConfig {
            round_cooldown: Duration::from_secs(1),
            ..config()
        },
        adapter.clone(),
        FakeProbe::new(Ok(false)),
    );
    let start = Instant::now();
    for second in [0, 1, 7, 13] {
        at(&mut recovery, start, second).await;
    }
    assert_eq!(adapter.connection_calls(), ["connect:A"]);

    adapter.0.lock().unwrap().visible.clear();
    let calls_before_deadline = adapter.calls();
    at(&mut recovery, start, 14).await;
    assert_eq!(adapter.calls(), calls_before_deadline);
    at(&mut recovery, start, 15).await;

    adapter.0.lock().unwrap().visible = vec![(access_point.clone(), true)];
    let calls_before_deadline = adapter.calls();
    at(&mut recovery, start, 16).await;
    assert_eq!(adapter.calls(), calls_before_deadline);
    assert_eq!(at(&mut recovery, start, 18).await.status, "scanning");
    assert_eq!(at(&mut recovery, start, 19).await.status, "verifying");
    assert_eq!(adapter.connection_calls(), ["connect:A", "connect:A"]);

    // A second failed attempt gets a fresh cooldown, and another observed
    // disappearance/reappearance permits another bounded retry before expiry.
    for second in [25, 31] {
        at(&mut recovery, start, second).await;
    }
    adapter.0.lock().unwrap().visible.clear();
    at(&mut recovery, start, 36).await;
    adapter.0.lock().unwrap().visible = vec![(access_point, true)];
    assert_eq!(at(&mut recovery, start, 45).await.status, "scanning");
    assert_eq!(at(&mut recovery, start, 46).await.status, "verifying");
    assert_eq!(
        adapter.connection_calls(),
        ["connect:A", "connect:A", "connect:A"]
    );
}

#[tokio::test]
async fn continuously_visible_failed_ap_keeps_its_cooldown() {
    let adapter = FakeAdapter::new(vec![network("A", 1, 90)]);
    let mut recovery = WifiRecovery::new(
        WifiConfig {
            round_cooldown: Duration::from_secs(1),
            ..config()
        },
        adapter.clone(),
        FakeProbe::new(Ok(false)),
    );
    let start = Instant::now();
    for second in [0, 1, 7, 13, 15, 18, 23] {
        at(&mut recovery, start, second).await;
    }
    assert_eq!(adapter.connection_calls(), ["connect:A"]);
    assert_eq!(recovery.cooldowns.len(), 1);
    assert_eq!(at(&mut recovery, start, 308).await.status, "scanning");
    at(&mut recovery, start, 309).await;
    assert_eq!(adapter.connection_calls(), ["connect:A", "connect:A"]);
}

#[tokio::test]
async fn failed_scan_does_not_count_as_a_failed_ap_disappearing() {
    let access_point = network("A", 1, 90);
    let adapter = FakeAdapter::new(vec![access_point.clone()]);
    let mut recovery = WifiRecovery::new(
        WifiConfig {
            round_cooldown: Duration::from_secs(1),
            ..config()
        },
        adapter.clone(),
        FakeProbe::new(Ok(false)),
    );
    let start = Instant::now();
    for second in [0, 1, 7, 13] {
        at(&mut recovery, start, second).await;
    }
    {
        let mut state = adapter.0.lock().unwrap();
        state.visible.clear();
        state.scan_error = Some(WifiError::CommandFailed);
    }
    assert_eq!(
        at(&mut recovery, start, 15).await.last_error.as_deref(),
        Some("command_failed")
    );
    {
        let mut state = adapter.0.lock().unwrap();
        state.visible = vec![(access_point, true)];
        state.scan_error = None;
    }
    assert_eq!(at(&mut recovery, start, 18).await.status, "cooldown");
    at(&mut recovery, start, 19).await;
    assert_eq!(adapter.connection_calls(), ["connect:A"]);
    assert_eq!(recovery.cooldowns.len(), 1);
}

#[tokio::test]
async fn another_bssid_with_the_same_ssid_does_not_reset_a_failed_ap_cooldown() {
    let original = network("Shared name", 1, 90);
    let other = network("Shared name", 2, 80);
    let adapter = FakeAdapter::new(vec![original.clone()]);
    let mut recovery = WifiRecovery::new(
        WifiConfig {
            round_cooldown: Duration::from_secs(1),
            ..config()
        },
        adapter.clone(),
        FakeProbe::new(Ok(false)),
    );
    let start = Instant::now();
    for second in [0, 1, 7, 13] {
        at(&mut recovery, start, second).await;
    }
    // The original remains present while a different AP with its SSID appears.
    adapter.0.lock().unwrap().visible = vec![(original.clone(), true), (other.clone(), true)];
    assert_eq!(at(&mut recovery, start, 15).await.status, "scanning");
    assert_eq!(recovery.candidates.front().unwrap().bssid, other.bssid);
    at(&mut recovery, start, 16).await;
    assert_eq!(
        adapter.0.lock().unwrap().active,
        Some(format!("ephemeral:{}", other.bssid))
    );
    assert!(recovery.cooldowns.contains_key(&original.bssid));
    assert_eq!(recovery.cooldowns.len(), 1);
    assert!(recovery.candidates.is_empty());
}

#[tokio::test]
async fn internet_recovery_before_retrying_a_reappeared_ap_prevents_switching() {
    let access_point = network("A", 1, 90);
    let adapter = FakeAdapter::new(vec![access_point.clone()]);
    let probe = FakeProbe::new(Ok(false));
    let mut recovery = WifiRecovery::new(
        WifiConfig {
            round_cooldown: Duration::from_secs(1),
            ..config()
        },
        adapter.clone(),
        probe.clone(),
    );
    let start = Instant::now();
    for second in [0, 1, 7, 13] {
        at(&mut recovery, start, second).await;
    }
    adapter.0.lock().unwrap().visible.clear();
    at(&mut recovery, start, 15).await;
    adapter.0.lock().unwrap().visible = vec![(access_point, true)];
    assert_eq!(at(&mut recovery, start, 18).await.status, "scanning");
    probe.next(Ok(true));
    assert_eq!(at(&mut recovery, start, 19).await.status, "online");
    assert_eq!(adapter.connection_calls(), ["connect:A"]);
}

#[tokio::test]
async fn permission_error_suspends_without_repeated_commands() {
    let adapter = FakeAdapter::new(vec![network("Free", 1, 90)]);
    adapter.0.lock().unwrap().scan_error = Some(WifiError::PermissionDenied);
    let mut recovery = WifiRecovery::new(config(), adapter.clone(), FakeProbe::new(Ok(false)));
    let start = Instant::now();
    let report = at(&mut recovery, start, 0).await;
    assert_eq!(report.status, "suspended");
    assert_eq!(report.last_error.as_deref(), Some("permission_denied"));
    let calls = adapter.calls();
    at(&mut recovery, start, 1000).await;
    assert_eq!(adapter.calls(), calls);
    assert!(report.next_attempt_at_unix.is_none());
}

#[tokio::test]
async fn permission_error_during_connection_restores_before_suspending() {
    let adapter = FakeAdapter::new(vec![network("A", 1, 90), network("B", 2, 80)]);
    adapter
        .0
        .lock()
        .unwrap()
        .connections
        .push_back(Err(WifiError::PermissionDenied));
    let mut recovery = WifiRecovery::new(config(), adapter.clone(), FakeProbe::new(Ok(false)));
    let start = Instant::now();
    for second in [0, 1, 2] {
        at(&mut recovery, start, second).await;
    }
    assert_eq!(recovery.report().status, "suspended");
    assert_eq!(adapter.connection_calls(), ["connect:A"]);
    assert!(
        adapter
            .calls()
            .contains(&"restore:original-profile-token".to_owned())
    );
}

#[tokio::test]
async fn unknown_probe_result_is_not_an_offline_sample() {
    let adapter = FakeAdapter::new(vec![network("Free", 1, 90)]);
    let probe = FakeProbe::new(Err(WifiError::TimedOut));
    probe.next(Ok(false));
    let mut recovery = WifiRecovery::new(WifiConfig::default(), adapter.clone(), probe);
    let start = Instant::now();
    assert_eq!(at(&mut recovery, start, 0).await.failures, 1);
    let report = at(&mut recovery, start, 16).await;
    assert_eq!(report.failures, 0);
    assert_eq!(report.status, "unknown");
    assert!(adapter.calls().is_empty());
}

#[tokio::test]
async fn verification_configuration_error_stops_round_and_restores() {
    let adapter = FakeAdapter::new(vec![network("A", 1, 90), network("B", 2, 80)]);
    let probe = FakeProbe::new(Ok(false));
    let mut recovery = WifiRecovery::new(config(), adapter.clone(), probe.clone());
    let start = Instant::now();
    at(&mut recovery, start, 0).await;
    at(&mut recovery, start, 1).await;
    probe.next(Err(WifiError::InvalidConfiguration));
    at(&mut recovery, start, 7).await;
    assert_eq!(at(&mut recovery, start, 8).await.status, "suspended");
    assert_eq!(adapter.connection_calls(), ["connect:A"]);
    assert!(recovery.cooldowns.is_empty());
}

#[tokio::test]
async fn successful_fallback_keeps_original_baseline_across_later_outage() {
    let adapter = FakeAdapter::new(vec![network("A", 1, 90)]);
    let probe = FakeProbe::new(Ok(false));
    let mut recovery = WifiRecovery::new(config(), adapter.clone(), probe.clone());
    let start = Instant::now();
    at(&mut recovery, start, 0).await;
    at(&mut recovery, start, 1).await;
    probe.next(Ok(true));
    assert_eq!(at(&mut recovery, start, 7).await.status, "online");
    at(&mut recovery, start, 18).await;
    at(&mut recovery, start, 19).await;
    at(&mut recovery, start, 25).await;
    at(&mut recovery, start, 31).await;
    assert_eq!(recovery.report().status, "cooldown");
    assert_eq!(
        adapter
            .calls()
            .iter()
            .filter(|call| *call == "snapshot")
            .count(),
        1
    );
    assert_eq!(
        adapter.0.lock().unwrap().active.as_deref(),
        Some("original-profile-token")
    );
}

#[tokio::test]
async fn disabled_config_makes_no_probes_or_adapter_calls() {
    let adapter = FakeAdapter::new(vec![network("A", 1, 90)]);
    let probe = FakeProbe::new(Ok(false));
    let mut recovery = WifiRecovery::new(
        WifiConfig {
            enabled: false,
            ..config()
        },
        adapter.clone(),
        probe.clone(),
    );
    assert_eq!(
        at(&mut recovery, Instant::now(), 0).await.status,
        "disabled"
    );
    assert!(adapter.calls().is_empty());
    assert_eq!(probe.calls(), 0);
}

#[test]
fn config_rejects_bad_values_and_does_not_silently_enable_on_bad_boolean() {
    assert!(WifiConfig::default().validate().is_ok());
    assert_eq!(WifiConfig::from_lookup(|_| None).unwrap().enabled, true);
    for (name, value) in [
        ("MINI_ERP_DOCTOR_WIFI_ENABLED", "perhaps"),
        ("MINI_ERP_DOCTOR_WIFI_SETTLE_DELAY_SECONDS", "0"),
        ("MINI_ERP_DOCTOR_WIFI_FAILURE_SAMPLES", "0"),
        ("MINI_ERP_DOCTOR_WIFI_CHECK_INTERVAL_SECONDS", "-1"),
        (
            "MINI_ERP_DOCTOR_WIFI_NETWORK_COOLDOWN_SECONDS",
            "999999999999",
        ),
        ("MINI_ERP_DOCTOR_WIFI_INTERFACE", "--help"),
        ("MINI_ERP_DOCTOR_WIFI_MAX_NETWORKS_PER_ROUND", "100"),
    ] {
        assert_eq!(
            WifiConfig::from_lookup(|key| (key == name).then(|| value.to_owned())).unwrap_err(),
            WifiError::InvalidConfiguration
        );
    }
    let config = WifiConfig::from_lookup(|key| match key {
        "MINI_ERP_DOCTOR_WIFI_ENABLED" => Some("false".to_owned()),
        "MINI_ERP_DOCTOR_WIFI_INTERFACE" => Some("wlan0".to_owned()),
        _ => None,
    })
    .unwrap();
    assert!(!config.enabled);
    assert_eq!(config.interface.as_deref(), Some("wlan0"));
}

#[tokio::test]
async fn invalid_programmatic_config_cannot_trigger_network_changes() {
    let adapter = FakeAdapter::new(vec![network("A", 1, 90)]);
    let probe = FakeProbe::new(Ok(false));
    let mut recovery = WifiRecovery::new(
        WifiConfig {
            check_interval: Duration::ZERO,
            ..config()
        },
        adapter.clone(),
        probe.clone(),
    );
    let report = at(&mut recovery, Instant::now(), 0).await;
    assert_eq!(report.status, "configuration_error");
    assert_eq!(report.last_error.as_deref(), Some("invalid_configuration"));
    assert_eq!(probe.calls(), 0);
    assert!(adapter.calls().is_empty());
}

#[test]
fn cooldown_memory_and_event_history_are_bounded() {
    let mut recovery = WifiRecovery::new(
        config(),
        FakeAdapter::new(vec![]),
        FakeProbe::new(Ok(false)),
    );
    let now = Instant::now();
    for index in 1..=300 {
        let mut candidate = network("A", 1, 50);
        candidate.bssid = format!("02:00:00:00:{:02x}:{:02x}", index / 256, index % 256);
        recovery.mark_failed(&candidate, now);
        recovery.transition(
            if index % 2 == 0 {
                "waiting"
            } else {
                "verifying"
            },
            index,
        );
    }
    assert_eq!(recovery.cooldowns.len(), MAX_COOLDOWNS);
    assert_eq!(recovery.report.events.len(), MAX_EVENTS);
}
