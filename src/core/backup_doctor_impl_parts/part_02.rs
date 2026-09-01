impl BackupDoctor {

    async fn run_import(&self, mut job: AdminServerMonitorBackupSnapshot, staged_path: PathBuf) {
        job.status = "running".to_string();
        job.started_at_unix = OffsetDateTime::now_utc().unix_timestamp();
        self.publish_job(&job);

        let safety_backup = match self.create_pre_restore_backup(&job).await {
            Ok(backup) => backup,
            Err(error) => {
                self.finish_failed(job, error);
                return;
            }
        };

        if let Err(error) = self.run_restore_script(&staged_path).await {
            self.finish_failed(
                job,
                format!(
                    "restore bajarilmadi: {error}. O‘zgarish boshlanishidan oldingi safety backup: {}",
                    safety_backup.job.id
                ),
            );
            return;
        }

        if self.inner.config.auto_migrate_after_restore
            && let Err(error) = self.run_restore_migrations().await
        {
            let error = self
                .rollback_after_restore(
                    &safety_backup,
                    format!("restore’dan keyingi schema migration bajarilmadi: {error}"),
                )
                .await;
            self.finish_failed(job, error);
            return;
        }

        job.status = "verifying".to_string();
        self.publish_job(&job);

        let artifact_path = staged_path
            .parent()
            .map(|directory| directory.join(&job.artifact_name))
            .unwrap_or(staged_path.clone());
        if let Err(error) = tokio::fs::rename(&staged_path, &artifact_path).await {
            let error = self
                .rollback_after_restore(
                    &safety_backup,
                    format!("import backup fayli saqlanmadi: {error}"),
                )
                .await;
            self.finish_failed(job, error);
            return;
        }
        let checksum_path = artifact_path.clone();
        let checksum = match tokio::task::spawn_blocking(move || sha256_file(&checksum_path)).await
        {
            Ok(Ok(checksum)) => checksum,
            _ => {
                let error = self
                    .rollback_after_restore(
                        &safety_backup,
                        "import checksum tekshiruvi bajarilmadi".to_string(),
                    )
                    .await;
                self.finish_failed(job, error);
                return;
            }
        };
        let metadata = match tokio::fs::metadata(&artifact_path).await {
            Ok(metadata) => metadata,
            Err(_) => {
                let error = self
                    .rollback_after_restore(
                        &safety_backup,
                        "import backup metadata o‘qilmadi".to_string(),
                    )
                    .await;
                self.finish_failed(job, error);
                return;
            }
        };
        job.status = "ready".to_string();
        job.completed_at_unix = OffsetDateTime::now_utc().unix_timestamp();
        job.size_bytes = metadata.len();
        job.checksum_sha256 = checksum;
        job.verified = true;
        job.error.clear();
        self.publish_job(&job);
        self.clear_active(&job.id);
        tracing::info!(
            backup_id = %job.id,
            safety_backup_id = %safety_backup.job.id,
            size_bytes = job.size_bytes,
            "backup doctor completed database import"
        );
    }

    async fn create_pre_restore_backup(
        &self,
        import_job: &AdminServerMonitorBackupSnapshot,
    ) -> Result<PreRestoreBackup, String> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let id = format!("backup-pre-restore-{now}-{:08x}", rand::random::<u32>());
        let job_dir = self.inner.config.backup_root.join(&id);
        fs::create_dir_all(&job_dir)
            .map_err(|error| format!("restore oldidan backup papkasi yaratilmadi: {error}"))?;
        let mut job = AdminServerMonitorBackupSnapshot {
            id,
            status: "running".to_string(),
            source: "pre_restore".to_string(),
            requested_by: format!("Restore oldidan: {}", import_job.requested_by),
            created_at_unix: now,
            started_at_unix: now,
            ..Default::default()
        };
        self.persist_standalone_job(&job)?;

        if let Some(available_mb) = available_disk_mb(&self.inner.config.backup_root).await
            && available_mb < self.inner.config.min_available_mb
        {
            let error = format!(
                "restore oldidan safety backup uchun disk joyi yetarli emas: {available_mb} MiB mavjud"
            );
            self.finish_standalone_failed(job, error.clone());
            return Err(error);
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
                let error = format!("restore oldidan backup engine ishga tushmadi: {error}");
                self.finish_standalone_failed(job, error.clone());
                return Err(error);
            }
            Err(_) => {
                let error = "restore oldidan backup vaqti chegaradan oshdi".to_string();
                self.finish_standalone_failed(job, error.clone());
                return Err(error);
            }
        };
        if !output.status.success() {
            let error = command_failure("restore oldidan safety backup", &output);
            self.finish_standalone_failed(job, error.clone());
            return Err(error);
        }

        job.status = "verifying".to_string();
        self.persist_standalone_job(&job)?;
        let Some(artifact_path) = preferred_artifact_in(&job_dir) else {
            let error = "restore oldidan safety backup dump fayli yaratilmagan".to_string();
            self.finish_standalone_failed(job, error.clone());
            return Err(error);
        };
        let checksum_path = artifact_path.clone();
        let checksum = match tokio::task::spawn_blocking(move || sha256_file(&checksum_path)).await
        {
            Ok(Ok(checksum)) => checksum,
            _ => {
                let error = "restore oldidan safety backup checksum tekshiruvi bajarilmadi";
                self.finish_standalone_failed(job, error.to_string());
                return Err(error.to_string());
            }
        };
        let metadata = match fs::metadata(&artifact_path) {
            Ok(metadata) => metadata,
            Err(error) => {
                let error = format!("restore oldidan safety backup metadata o‘qilmadi: {error}");
                self.finish_standalone_failed(job, error.clone());
                return Err(error);
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
        self.persist_standalone_job(&job)?;
        let artifact_name = job.artifact_name.clone();

        Ok(PreRestoreBackup {
            job,
            artifact: BackupArtifact {
                path: artifact_path,
                filename: artifact_name,
                size_bytes: metadata.len(),
            },
        })
    }

    async fn run_restore_script(&self, dump: &std::path::Path) -> Result<(), String> {
        let database_url = self
            .inner
            .config
            .database_url
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "restore uchun database URL sozlanmagan".to_string())?;
        let mut command = Command::new("bash");
        command
            .arg(&self.inner.config.restore_script_path)
            .env("MINI_ERP_DATABASE_URL", database_url)
            .env("MINI_ERP_RESTORE_DUMP", dump)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(migration_url) = &self.inner.config.migration_database_url {
            command
                .env("MINI_ERP_MIGRATION_DATABASE_URL", migration_url)
                .env("MINI_ERP_RESTORE_DATABASE_URL", migration_url);
        }
        let output = match timeout(self.inner.config.max_runtime, command.output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => return Err(format!("restore engine ishga tushmadi: {error}")),
            Err(_) => return Err("restore vaqti chegaradan oshdi".to_string()),
        };
        if output.status.success() {
            Ok(())
        } else {
            Err(command_failure("restore engine", &output))
        }
    }

    async fn run_restore_migrations(&self) -> Result<(), String> {
        let database_url = self
            .inner
            .config
            .database_url
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                "restore’dan keyingi migration uchun database URL sozlanmagan".to_string()
            })?;
        match timeout(
            self.inner.config.max_runtime,
            crate::db::postgres::migrate_database(
                database_url,
                self.inner.config.migration_database_url.as_deref(),
            ),
        )
        .await
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(error.to_string()),
            Err(_) => {
                Err("restore’dan keyingi schema migration vaqti chegaradan oshdi".to_string())
            }
        }
    }

    async fn rollback_after_restore(
        &self,
        safety_backup: &PreRestoreBackup,
        reason: impl Into<String>,
    ) -> String {
        let reason = reason.into();
        match self.run_restore_script(&safety_backup.artifact.path).await {
            Ok(()) => format!(
                "{reason}; database restore oldidagi holatga qaytarildi: {}",
                safety_backup.job.id
            ),
            Err(error) => format!(
                "{reason}; rollback ham bajarilmadi. Safety backup qo‘lda tiklash uchun tayyor: {}. Rollback xatosi: {error}",
                safety_backup.job.id
            ),
        }
    }

    fn persist_standalone_job(&self, job: &AdminServerMonitorBackupSnapshot) -> Result<(), String> {
        let job_dir = self.inner.config.backup_root.join(&job.id);
        write_manifest(&job_dir, job)
            .map_err(|error| format!("backup manifest saqlanmadi: {error}"))
    }

    fn finish_standalone_failed(&self, mut job: AdminServerMonitorBackupSnapshot, error: String) {
        job.status = "failed".to_string();
        job.completed_at_unix = OffsetDateTime::now_utc().unix_timestamp();
        job.error = truncate_error(&error);
        job.verified = false;
        if let Err(write_error) = self.persist_standalone_job(&job) {
            tracing::warn!(%write_error, backup_id = %job.id, "safety backup failure manifest write failed");
        }
        tracing::error!(backup_id = %job.id, error = %job.error, "pre-restore safety backup failed");
    }

    fn publish_job(&self, job: &AdminServerMonitorBackupSnapshot) {
        let job_dir = self.inner.config.backup_root.join(&job.id);
        if let Err(error) = write_manifest(&job_dir, job) {
            tracing::warn!(%error, backup_id = %job.id, "backup doctor manifest write failed");
        }
        if let Ok(mut active) = self.inner.active_job.lock() {
            *active = Some(job.clone());
        }
    }

    fn finish_failed(&self, mut job: AdminServerMonitorBackupSnapshot, error: String) {
        job.status = "failed".to_string();
        job.completed_at_unix = OffsetDateTime::now_utc().unix_timestamp();
        job.error = truncate_error(&error);
        job.verified = false;
        self.publish_job(&job);
        self.clear_active(&job.id);
        tracing::error!(backup_id = %job.id, error = %job.error, "backup doctor backup failed");
    }

    fn clear_active(&self, id: &str) {
        if let Ok(mut active) = self.inner.active_job.lock()
            && active.as_ref().is_some_and(|job| job.id == id)
        {
            *active = None;
        }
    }

    fn maybe_start_scheduled_backup(&self) {
        let offset_seconds = self.inner.config.utc_offset_minutes.saturating_mul(60);
        let offset = UtcOffset::from_whole_seconds(offset_seconds).unwrap_or(UtcOffset::UTC);
        let now = OffsetDateTime::now_utc();
        let local_now = now.to_offset(offset);
        if (local_now.hour(), local_now.minute())
            < (
                self.inner.config.schedule_hour,
                self.inner.config.schedule_minute,
            )
        {
            return;
        }
        let report = self.report(now);
        let already_ready = report.snapshots.iter().any(|snapshot| {
            snapshot.status == "ready"
                && snapshot.completed_at_unix > 0
                && OffsetDateTime::from_unix_timestamp(snapshot.completed_at_unix)
                    .ok()
                    .is_some_and(|completed| completed.to_offset(offset).date() == local_now.date())
        });
        if already_ready {
            return;
        }
        let recent_attempt = report.snapshots.iter().any(|snapshot| {
            snapshot.source == "automatic"
                && now
                    .unix_timestamp()
                    .saturating_sub(snapshot.created_at_unix)
                    < DEFAULT_AUTO_RETRY_MINUTES * 60
        });
        if recent_attempt {
            return;
        }
        match self.start_backup("automatic", "Backup Doctor".to_string()) {
            Ok(job) => tracing::info!(backup_id = %job.id, "backup doctor scheduled backup"),
            Err(BackupDoctorError::AlreadyRunning) => {}
            Err(error) => tracing::warn!(%error, "backup doctor could not schedule backup"),
        }
    }

    fn reconcile_interrupted_jobs(&self) {
        let root = &self.inner.config.backup_root;
        if !root.is_dir() {
            return;
        }
        let mut manifests = Vec::new();
        collect_manifest_paths(root, &mut manifests);
        for manifest_path in manifests {
            let Ok(body) = fs::read(&manifest_path) else {
                continue;
            };
            let Ok(mut snapshot) =
                serde_json::from_slice::<AdminServerMonitorBackupSnapshot>(&body)
            else {
                continue;
            };
            if terminal_status(&snapshot.status) {
                continue;
            }
            snapshot.status = "failed".to_string();
            snapshot.completed_at_unix = OffsetDateTime::now_utc().unix_timestamp();
            snapshot.error = "server qayta ishga tushgani uchun backup uzildi".to_string();
            snapshot.verified = false;
            if let Some(directory) = manifest_path.parent() {
                let _ = write_manifest(directory, &snapshot);
            }
        }
    }
}
