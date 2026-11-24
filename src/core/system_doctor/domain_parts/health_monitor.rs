impl DomainRecovery {
    pub fn new(config: DomainConfig, adapter: std::sync::Arc<dyn DomainAdapter>) -> Self {
        let host = config.url.host_str().map(str::to_owned);
        Self {
            report: DomainRecoveryReport {
                enabled: true,
                host,
                status: "starting".to_owned(),
                ..DomainRecoveryReport::default()
            },
            config,
            adapter,
            critical: false,
            next_check: None,
            next_restart: None,
        }
    }

    pub fn report(&self) -> DomainRecoveryReport {
        self.report.clone()
    }

    pub async fn step(&mut self, now: Instant, now_unix: i64) -> DomainRecoveryReport {
        if self.next_check.is_some_and(|deadline| now < deadline) {
            return self.report();
        }
        self.next_check = Some(now + self.config.check_interval);
        self.report.checked_at_unix = now_unix;
        match self.adapter.domain_is_healthy().await {
            Ok(true) => self.observe_success(now_unix),
            Ok(false) => self.observe_failure(None, now, now_unix).await,
            Err(error) => self.observe_failure(Some(error), now, now_unix).await,
        }
        self.report()
    }

    fn observe_success(&mut self, now_unix: i64) {
        self.report.failures = 0;
        self.report.last_error = None;
        if self.critical {
            self.report.successes = self.report.successes.saturating_add(1);
            if self.report.successes >= self.config.recovery_samples {
                self.critical = false;
                self.report.status = "healthy".to_owned();
                self.report.successes = 0;
                self.report.next_attempt_at_unix = None;
            } else {
                self.report.status = "recovering".to_owned();
            }
        } else {
            self.report.successes = 0;
            self.report.status = "healthy".to_owned();
        }
        self.report.critical = self.critical;
        self.transition(now_unix);
    }

    async fn observe_failure(&mut self, error: Option<DomainError>, now: Instant, now_unix: i64) {
        self.report.successes = 0;
        self.report.failures = self.report.failures.saturating_add(1);
        self.report.last_error = Some(
            error
                .map(DomainError::code)
                .unwrap_or("endpoint_unhealthy")
                .to_owned(),
        );
        if self.report.failures >= self.config.failure_samples {
            self.critical = true;
            self.report.status = "critical".to_owned();
            self.maybe_reconnect(now, now_unix).await;
        } else {
            self.report.status = "warning".to_owned();
        }
        self.report.critical = self.critical;
        self.transition(now_unix);
    }

    async fn maybe_reconnect(&mut self, now: Instant, now_unix: i64) {
        if self.next_restart.is_some_and(|deadline| now < deadline) {
            return;
        }
        self.next_restart = Some(now + self.config.restart_cooldown);
        self.report.next_attempt_at_unix = Some(
            now_unix
                .saturating_add(self.config.restart_cooldown.as_secs().min(i64::MAX as u64) as i64),
        );
        match self.adapter.local_service_is_healthy().await {
            Ok(true) => {
                self.report.recovery_attempts = self.report.recovery_attempts.saturating_add(1);
                match self.adapter.reconnect_tunnel().await {
                    Ok(()) => {
                        self.report.status = "reconnecting".to_owned();
                        self.report.last_error = None;
                    }
                    Err(error) => self.report.last_error = Some(error.code().to_owned()),
                }
            }
            Ok(false) => self.report.last_error = Some("local_service_unavailable".to_owned()),
            Err(error) => self.report.last_error = Some(error.code().to_owned()),
        }
        self.transition(now_unix);
    }

    fn transition(&mut self, now_unix: i64) {
        let status = self.report.status.as_str();
        if self
            .report
            .events
            .last()
            .is_some_and(|event| event.status == status && event.code == self.report.last_error)
        {
            return;
        }
        if self.report.events.len() == MAX_EVENTS {
            self.report.events.remove(0);
        }
        self.report.events.push(DomainRecoveryEvent {
            status: self.report.status.clone(),
            code: self.report.last_error.clone(),
            at_unix: now_unix,
        });
    }
}
