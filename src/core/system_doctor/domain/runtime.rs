use super::DomainRecoveryReport;
use crate::core::system_doctor::SystemDoctor;

#[cfg(not(any(test, feature = "verification")))]
use super::{CloudflareDomainAdapter, DomainConfig, DomainRecovery};

pub(crate) fn start(doctor: SystemDoctor) {
    #[cfg(any(test, feature = "verification"))]
    doctor.publish_domain(DomainRecoveryReport {
        status: "disabled".to_string(),
        ..Default::default()
    });
    #[cfg(not(any(test, feature = "verification")))]
    start_runtime(doctor);
}

#[cfg(not(any(test, feature = "verification")))]
fn start_runtime(doctor: SystemDoctor) {
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };

    let config = match DomainConfig::from_env() {
        Ok(Some(config)) => config,
        Ok(None) => {
            doctor.publish_domain(DomainRecoveryReport {
                status: "disabled".to_string(),
                ..Default::default()
            });
            return;
        }
        Err(error) => {
            doctor.publish_domain(DomainRecoveryReport {
                enabled: true,
                status: "configuration_error".to_string(),
                last_error: Some(error.code().to_string()),
                ..Default::default()
            });
            return;
        }
    };
    let adapter = match CloudflareDomainAdapter::new(config.clone()) {
        Ok(adapter) => Arc::new(adapter),
        Err(error) => {
            doctor.publish_domain(DomainRecoveryReport {
                enabled: true,
                host: config.url.host_str().map(str::to_owned),
                status: "configuration_error".to_string(),
                last_error: Some(error.code().to_string()),
                ..Default::default()
            });
            return;
        }
    };
    let mut recovery = DomainRecovery::new(config, adapter);
    doctor.publish_domain(recovery.report());
    tokio::spawn(async move {
        loop {
            let report = recovery
                .step(
                    Instant::now(),
                    time::OffsetDateTime::now_utc().unix_timestamp(),
                )
                .await;
            doctor.publish_domain(report);
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::admin::models::{AdminServerMonitorDatabase, AdminServerMonitorRuntime};

    #[test]
    fn unit_bootstrap_disables_domain_networking_without_a_runtime() {
        let doctor = SystemDoctor::default();
        start(doctor.clone());
        assert_eq!(doctor.snapshot().domain.status, "disabled");
    }

    #[test]
    fn domain_incident_is_in_general_doctor_status_and_survives_resource_samples() {
        let doctor = SystemDoctor::default();
        doctor.publish_domain(DomainRecoveryReport {
            enabled: true,
            host: Some("erp.example.test".to_owned()),
            status: "critical".to_owned(),
            failures: 3,
            critical: true,
            ..Default::default()
        });
        let report = doctor.observe(
            &AdminServerMonitorDatabase {
                configured: true,
                reachable: true,
                ..Default::default()
            },
            &AdminServerMonitorRuntime {
                memory_percent: 40,
                memory_total_mb: 8192,
                disk_available_mb: 5000,
                disk_total_mb: 10000,
                ..Default::default()
            },
            100,
        );
        assert_eq!(report.status, "critical");
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.id == "domain" && check.critical)
        );
        assert_eq!(report.domain.host.as_deref(), Some("erp.example.test"));
    }
}
