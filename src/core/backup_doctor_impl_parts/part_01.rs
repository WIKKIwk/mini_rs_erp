impl BackupDoctor {
    pub fn from_env() -> Self {
        Self::new(BackupDoctorConfig::from_env())
    }

    pub fn new(config: BackupDoctorConfig) -> Self {
        let doctor = Self {
            inner: Arc::new(BackupDoctorInner {
                config,
                active_job: Mutex::new(None),
                scheduler_started: AtomicBool::new(false),
            }),
        };
        doctor.reconcile_interrupted_jobs();
        doctor
    }

    #[cfg(test)]
    pub fn for_test(
        backup_root: impl Into<PathBuf>,
        script_path: impl Into<PathBuf>,
        database_url: impl Into<String>,
    ) -> Self {
        Self::new(BackupDoctorConfig {
            backup_root: backup_root.into(),
            script_path: script_path.into(),
            restore_script_path: PathBuf::from("missing-restore.sh"),
            database_url: Some(database_url.into()),
            migration_database_url: None,
            admin_database_url: None,
            auto_migrate_after_restore: false,
            auto_enabled: false,
            schedule_hour: 2,
            schedule_minute: 0,
            utc_offset_minutes: 300,
            health_max_age_hours: DEFAULT_HEALTH_MAX_AGE_HOURS,
            max_runtime: StdDuration::from_secs(30),
            min_available_mb: 0,
            retention_enabled: false,
        })
    }

    #[cfg(test)]
    pub fn for_test_with_restore(
        backup_root: impl Into<PathBuf>,
        script_path: impl Into<PathBuf>,
        restore_script_path: impl Into<PathBuf>,
        database_url: impl Into<String>,
    ) -> Self {
        Self::new(BackupDoctorConfig {
            backup_root: backup_root.into(),
            script_path: script_path.into(),
            restore_script_path: restore_script_path.into(),
            database_url: Some(database_url.into()),
            migration_database_url: None,
            admin_database_url: None,
            auto_migrate_after_restore: false,
            auto_enabled: false,
            schedule_hour: 2,
            schedule_minute: 0,
            utc_offset_minutes: 300,
            health_max_age_hours: DEFAULT_HEALTH_MAX_AGE_HOURS,
            max_runtime: StdDuration::from_secs(30),
            min_available_mb: 0,
            retention_enabled: false,
        })
    }

    #[cfg(test)]
    pub fn for_test_with_restore_and_migration(
        backup_root: impl Into<PathBuf>,
        script_path: impl Into<PathBuf>,
        restore_script_path: impl Into<PathBuf>,
        database_url: impl Into<String>,
    ) -> Self {
        let mut doctor = Self::for_test_with_restore(
            backup_root,
            script_path,
            restore_script_path,
            database_url,
        );
        Arc::get_mut(&mut doctor.inner)
            .expect("test backup doctor must be uniquely owned")
            .config
            .auto_migrate_after_restore = true;
        doctor
    }

    pub fn start_scheduler(&self) {
        if cfg!(test)
            || !self.inner.config.auto_enabled
            || self.inner.config.database_url.is_none()
            || self
                .inner
                .scheduler_started
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return;
        }
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            self.inner.scheduler_started.store(false, Ordering::Release);
            tracing::warn!("backup doctor scheduler could not find a Tokio runtime");
            return;
        };
        let doctor = self.clone();
        handle.spawn(async move {
            loop {
                doctor.maybe_start_scheduled_backup();
                tokio::time::sleep(StdDuration::from_secs(60)).await;
            }
        });
    }

    pub fn start_manual_backup(
        &self,
        requested_by: impl Into<String>,
    ) -> Result<AdminServerMonitorBackupSnapshot, BackupDoctorError> {
        self.start_backup("manual", requested_by.into())
    }

    pub fn prepare_import(
        &self,
        requested_by: impl Into<String>,
        filename: &str,
    ) -> Result<BackupImportUpload, BackupDoctorError> {
        let database_url = self
            .inner
            .config
            .database_url
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .ok_or(BackupDoctorError::NotConfigured)?;
        if database_url.trim().is_empty() {
            return Err(BackupDoctorError::NotConfigured);
        }
        if !self.inner.config.restore_script_path.is_file()
            || !self.inner.config.script_path.is_file()
        {
            return Err(BackupDoctorError::EngineUnavailable);
        }
        tokio::runtime::Handle::try_current().map_err(|_| BackupDoctorError::RuntimeUnavailable)?;
        let artifact_name = safe_import_name(filename).ok_or(BackupDoctorError::InvalidImport)?;
        let mut active = self
            .inner
            .active_job
            .lock()
            .map_err(|_| BackupDoctorError::Storage)?;
        if active
            .as_ref()
            .is_some_and(|job| !terminal_status(&job.status))
        {
            return Err(BackupDoctorError::AlreadyRunning);
        }

        let now = OffsetDateTime::now_utc().unix_timestamp();
        let id = format!("import-{now}-{:08x}", rand::random::<u32>());
        let job_dir = self.inner.config.backup_root.join(&id);
        fs::create_dir_all(&job_dir).map_err(|_| BackupDoctorError::Storage)?;
        let job = AdminServerMonitorBackupSnapshot {
            id,
            status: "queued".to_string(),
            source: "imported".to_string(),
            requested_by: requested_by.into().trim().to_string(),
            created_at_unix: now,
            artifact_name,
            ..Default::default()
        };
        write_manifest(&job_dir, &job).map_err(|_| BackupDoctorError::Storage)?;
        *active = Some(job.clone());
        drop(active);

        Ok(BackupImportUpload {
            job,
            path: job_dir.join("uploaded.dump.part"),
        })
    }

    pub fn complete_import(
        &self,
        upload: BackupImportUpload,
    ) -> Result<AdminServerMonitorBackupSnapshot, BackupDoctorError> {
        if !upload.path.is_file() {
            return Err(BackupDoctorError::Storage);
        }
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| BackupDoctorError::RuntimeUnavailable)?;
        let doctor = self.clone();
        let job = upload.job.clone();
        let staged_path = upload.path;
        handle.spawn(async move {
            doctor.run_import(job, staged_path).await;
        });
        Ok(upload.job)
    }

    pub fn abort_import(&self, id: &str, error: impl Into<String>) {
        let job = self
            .inner
            .active_job
            .lock()
            .ok()
            .and_then(|active| active.clone())
            .filter(|job| job.id == id);
        if let Some(job) = job {
            self.finish_failed(job, error.into());
        }
    }

    pub fn report(&self, now: OffsetDateTime) -> AdminServerMonitorBackups {
        let active_job = self
            .inner
            .active_job
            .lock()
            .ok()
            .and_then(|active| active.clone());
        scan_backup_root(
            &self.inner.config.backup_root,
            now,
            self.inner.config.health_max_age_hours,
            active_job,
        )
        .report
    }

    pub fn artifact(&self, id: &str) -> Result<BackupArtifact, BackupDoctorError> {
        let id = id.trim();
        if id.is_empty() {
            return Err(BackupDoctorError::NotFound);
        }
        let scan = scan_backup_root(
            &self.inner.config.backup_root,
            OffsetDateTime::now_utc(),
            self.inner.config.health_max_age_hours,
            None,
        );
        if let Some(artifact) = scan.artifacts.get(id) {
            return Ok(artifact.clone());
        }
        if scan
            .report
            .snapshots
            .iter()
            .any(|snapshot| snapshot.id == id)
        {
            return Err(BackupDoctorError::NotReady);
        }
        Err(BackupDoctorError::NotFound)
    }

    fn start_backup(
        &self,
        source: &str,
        requested_by: String,
    ) -> Result<AdminServerMonitorBackupSnapshot, BackupDoctorError> {
        let database_url = self
            .inner
            .config
            .database_url
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .ok_or(BackupDoctorError::NotConfigured)?;
        if database_url.trim().is_empty() {
            return Err(BackupDoctorError::NotConfigured);
        }
        if !self.inner.config.script_path.is_file() {
            return Err(BackupDoctorError::EngineUnavailable);
        }
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| BackupDoctorError::RuntimeUnavailable)?;
        let mut active = self
            .inner
            .active_job
            .lock()
            .map_err(|_| BackupDoctorError::Storage)?;
        if active
            .as_ref()
            .is_some_and(|job| !terminal_status(&job.status))
        {
            return Err(BackupDoctorError::AlreadyRunning);
        }

        let now = OffsetDateTime::now_utc().unix_timestamp();
        let id = format!("backup-{now}-{:08x}", rand::random::<u32>());
        let job_dir = self.inner.config.backup_root.join(&id);
        fs::create_dir_all(&job_dir).map_err(|_| BackupDoctorError::Storage)?;
        let job = AdminServerMonitorBackupSnapshot {
            id,
            status: "queued".to_string(),
            source: source.to_string(),
            requested_by: requested_by.trim().to_string(),
            created_at_unix: now,
            ..Default::default()
        };
        write_manifest(&job_dir, &job).map_err(|_| BackupDoctorError::Storage)?;
        *active = Some(job.clone());
        drop(active);

        let doctor = self.clone();
        let spawned_job = job.clone();
        handle.spawn(async move {
            doctor.run_backup(spawned_job).await;
        });
        Ok(job)
    }

    async fn run_backup(&self, mut job: AdminServerMonitorBackupSnapshot) {
        job.status = "running".to_string();
        job.started_at_unix = OffsetDateTime::now_utc().unix_timestamp();
        self.publish_job(&job);

        if let Some(available_mb) = available_disk_mb(&self.inner.config.backup_root).await
            && available_mb < self.inner.config.min_available_mb
        {
            self.finish_failed(
                job,
                format!("backup uchun disk joyi yetarli emas: {available_mb} MiB mavjud"),
            );
            return;
        }

        let mut command = Command::new("bash");
        command
            .arg(&self.inner.config.script_path)
            .env(
                "MINI_ERP_DATABASE_URL",
                self.inner
                    .config
                    .database_url
                    .as_deref()
                    .unwrap_or_default(),
            )
            .env("MINI_ERP_BACKUP_DIR", &self.inner.config.backup_root)
            .env("MINI_ERP_BACKUP_TIMESTAMP", &job.id)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(admin_url) = &self.inner.config.admin_database_url {
            command.env("MINI_ERP_ADMIN_DATABASE_URL", admin_url);
        }
        let output = match timeout(self.inner.config.max_runtime, command.output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => {
                self.finish_failed(job, format!("backup engine ishga tushmadi: {error}"));
                return;
            }
            Err(_) => {
                self.finish_failed(job, "backup vaqti chegaradan oshdi".to_string());
                return;
            }
        };
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
            self.finish_failed(job, truncate_error(&error));
            return;
        }

        job.status = "verifying".to_string();
        self.publish_job(&job);
        let job_dir = self.inner.config.backup_root.join(&job.id);
        let Some(artifact_path) = preferred_artifact_in(&job_dir) else {
            self.finish_failed(job, "backup dump fayli yaratilmagan".to_string());
            return;
        };
        let checksum_path = artifact_path.clone();
        let checksum = match tokio::task::spawn_blocking(move || sha256_file(&checksum_path)).await
        {
            Ok(Ok(checksum)) => checksum,
            _ => {
                self.finish_failed(job, "backup checksum tekshiruvi bajarilmadi".to_string());
                return;
            }
        };
        let metadata = match fs::metadata(&artifact_path) {
            Ok(metadata) => metadata,
            Err(_) => {
                self.finish_failed(job, "backup dump metadata o‘qilmadi".to_string());
                return;
            }
        };
        job.status = "ready".to_string();
        job.completed_at_unix = OffsetDateTime::now_utc().unix_timestamp();
        job.size_bytes = metadata.len();
        job.artifact_name = artifact_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();
        job.checksum_sha256 = checksum;
        job.verified = true;
        job.error.clear();
        self.publish_job(&job);
        self.clear_active(&job.id);
        if self.inner.config.retention_enabled {
            apply_retention(&self.inner.config.backup_root, OffsetDateTime::now_utc());
        }
        tracing::info!(backup_id = %job.id, size_bytes = job.size_bytes, "backup doctor completed backup");
    }
}
