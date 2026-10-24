use super::WifiRecoveryReport;
use crate::core::system_doctor::SystemDoctor;

/// Wi-Fi operations run independently of the fast database/resource sampler.
pub(crate) fn start(doctor: SystemDoctor) {
    // Unit tests and the isolated contract harness must never alter host networking.
    #[cfg(any(test, feature = "verification"))]
    doctor.publish_wifi(WifiRecoveryReport {
        status: "disabled".to_string(),
        ..Default::default()
    });
    #[cfg(not(any(test, feature = "verification")))]
    start_runtime(doctor);
}

#[cfg(not(any(test, feature = "verification")))]
fn start_runtime(doctor: SystemDoctor) {
    use super::{WifiAdapter, WifiConfig, WifiRecovery};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    let config = match WifiConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            doctor.publish_wifi(WifiRecoveryReport {
                enabled: true,
                status: "configuration_error".to_string(),
                last_error: Some(error.code().to_string()),
                ..Default::default()
            });
            return;
        }
    };
    if !config.enabled {
        doctor.publish_wifi(WifiRecoveryReport {
            status: "disabled".to_string(),
            ..Default::default()
        });
        return;
    }
    let probe = match super::probe::HttpsInternetProbe::from_env() {
        Ok(probe) => probe,
        Err(error) => {
            doctor.publish_wifi(WifiRecoveryReport {
                enabled: true,
                status: "configuration_error".to_string(),
                last_error: Some(error.code().to_string()),
                ..Default::default()
            });
            return;
        }
    };
    #[cfg(target_os = "linux")]
    let adapter: Arc<dyn WifiAdapter> =
        Arc::new(super::linux::LinuxWifi::new(config.interface.clone()));
    #[cfg(target_os = "macos")]
    let adapter: Arc<dyn WifiAdapter> =
        Arc::new(super::macos::MacWifi::new(config.interface.clone()));
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let adapter: Arc<dyn WifiAdapter> = Arc::new(UnsupportedWifi);

    let mut recovery = WifiRecovery::new(config, adapter, Arc::new(probe));
    doctor.publish_wifi(recovery.report());
    tokio::spawn(async move {
        loop {
            let report = recovery
                .step(
                    Instant::now(),
                    time::OffsetDateTime::now_utc().unix_timestamp(),
                )
                .await;
            doctor.publish_wifi(report);
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

#[cfg(all(
    not(any(test, feature = "verification")),
    not(any(target_os = "linux", target_os = "macos"))
))]
struct UnsupportedWifi;

#[cfg(all(
    not(any(test, feature = "verification")),
    not(any(target_os = "linux", target_os = "macos"))
))]
#[async_trait::async_trait]
impl super::WifiAdapter for UnsupportedWifi {
    async fn current_connection(&self) -> Result<Option<String>, super::WifiError> {
        Err(super::WifiError::Unsupported)
    }
    async fn scan_open(&self) -> Result<Vec<super::OpenNetwork>, super::WifiError> {
        Err(super::WifiError::Unsupported)
    }
    async fn connect_open(&self, _: &super::OpenNetwork) -> Result<(), super::WifiError> {
        Err(super::WifiError::Unsupported)
    }
    async fn disconnect_attempt(&self) -> Result<(), super::WifiError> {
        Err(super::WifiError::Unsupported)
    }
    async fn restore_connection(&self, _: &str) -> Result<(), super::WifiError> {
        Err(super::WifiError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_bootstrap_disables_native_networking_without_a_runtime() {
        let doctor = SystemDoctor::default();
        start(doctor.clone());
        let report = doctor.snapshot();
        assert!(!report.wifi.enabled);
        assert_eq!(report.wifi.status, "disabled");
        assert_eq!(report.wifi.attempts, 0);
    }
}
