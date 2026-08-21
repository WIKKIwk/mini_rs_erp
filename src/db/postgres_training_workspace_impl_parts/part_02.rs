impl PostgresTrainingWorkspaceStore {

    pub async fn delete_training_input_batch(
        &self,
        order_id: &str,
        apparatus: &str,
        qr_payload: &str,
    ) -> Result<Vec<String>, TrainingWorkspaceError> {
        let order_id = order_id.trim();
        let apparatus = if apparatus.trim().is_empty() {
            None
        } else {
            Some(canonical_training_apparatus(apparatus)?)
        };
        let qr_payload = qr_payload.trim();
        if order_id.is_empty() || !order_id.starts_with("training-") {
            return Err(TrainingWorkspaceError::InvalidInput(
                "training order id kerak".to_string(),
            ));
        }
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        let deleted_batch_ids = sqlx::query_scalar::<_, String>(
            "DELETE FROM mini_training_input_batches
             WHERE order_id = $1
               AND ($2 = '' OR canonical_apparatus_id = $2)
               AND ($3 = '' OR lower(qr_payload) = lower($3))
             RETURNING batch_id",
        )
        .bind(order_id)
        .bind(
            apparatus
                .as_ref()
                .map(ApparatusId::as_str)
                .unwrap_or_default(),
        )
        .bind(qr_payload)
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        for batch_id in &deleted_batch_ids {
            sqlx::query("DELETE FROM mini_training_progress_batches WHERE batch_id = $1")
                .bind(batch_id)
                .execute(&mut *tx)
                .await
                .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        }
        tx.commit()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(deleted_batch_ids)
    }

    pub async fn training_input_batch_generated(
        &self,
        order_id: &str,
        apparatus: &str,
    ) -> Result<bool, TrainingWorkspaceError> {
        let apparatus = canonical_training_apparatus(apparatus)?;
        let generated = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                 SELECT 1
                 FROM mini_training_input_batches
                 WHERE order_id = $1 AND canonical_apparatus_id = $2
             )",
        )
        .bind(order_id.trim())
        .bind(apparatus.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(generated)
    }

    pub async fn training_input_batch_set_started(
        &self,
        order_id: &str,
        apparatus: &str,
    ) -> Result<bool, TrainingWorkspaceError> {
        let apparatus = canonical_training_apparatus(apparatus)?;
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                 SELECT 1
                 FROM mini_training_queue_events
                 WHERE order_id = $1
                   AND canonical_apparatus_id = $2
                   AND action = 'start'
             )",
        )
        .bind(order_id.trim())
        .bind(apparatus.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)
    }

    pub async fn training_input_batch_orders(
        &self,
    ) -> Result<Vec<(String, String)>, TrainingWorkspaceError> {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT order_id, canonical_apparatus_id AS apparatus
             FROM mini_training_input_batches
             ORDER BY generated_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(rows
            .into_iter()
            .filter_map(|(order_id, apparatus)| {
                canonical_training_apparatus(&apparatus)
                    .ok()
                    .map(|id| (order_id, id.to_string()))
            })
            .collect())
    }

    pub async fn put_training_progress_batches(
        &self,
        progress_batches: &[OrderProgressBatch],
    ) -> Result<(), TrainingWorkspaceError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        for batch in progress_batches {
            let apparatus = canonical_training_apparatus(&batch.apparatus)?;
            let payload = training_progress_payload(batch)?;
            sqlx::query(
                "INSERT INTO mini_training_progress_batches
                    (batch_id, order_id, apparatus, canonical_apparatus_id, qr_payload, payload_json, generated_at)
                 VALUES ($1, $2, COALESCE((SELECT name FROM mini_apparatus WHERE id = $3), $3), $3, $4, $5, now())
                 ON CONFLICT (batch_id) DO UPDATE SET
                     order_id = excluded.order_id,
                     apparatus = excluded.apparatus,
                     canonical_apparatus_id = excluded.canonical_apparatus_id,
                     qr_payload = excluded.qr_payload,
                     payload_json = excluded.payload_json,
                     generated_at = now()",
            )
            .bind(batch.batch_id.trim())
            .bind(batch.order_id.trim())
            .bind(apparatus.as_str())
            .bind(batch.qr_payload.trim())
            .bind(payload)
            .execute(&mut *tx)
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        }
        tx.commit()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(())
    }

    pub async fn training_progress_batch_for_key(
        &self,
        batch_id: &str,
        qr_payload: &str,
    ) -> Result<Option<OrderProgressBatch>, TrainingWorkspaceError> {
        let row = sqlx::query_as::<_, (String, serde_json::Value)>(
            "SELECT canonical_apparatus_id, payload_json
             FROM mini_training_progress_batches
             WHERE ($1 <> '' AND lower(batch_id) = lower($1))
                OR ($2 <> '' AND lower(qr_payload) = lower($2))
             LIMIT 1",
        )
        .bind(batch_id.trim())
        .bind(qr_payload.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        row.map(training_progress_batch_from_row).transpose()
    }

    pub async fn training_progress_batches_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<OrderProgressBatch>, TrainingWorkspaceError> {
        let rows = sqlx::query_as::<_, (String, serde_json::Value)>(
            "SELECT canonical_apparatus_id, payload_json
             FROM mini_training_progress_batches
             WHERE order_id = $1
             ORDER BY generated_at ASC, batch_id ASC",
        )
        .bind(order_id.trim())
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        rows.into_iter()
            .map(training_progress_batch_from_row)
            .collect()
    }

    pub async fn apparatus_modes(&self) -> Result<BTreeMap<String, bool>, TrainingWorkspaceError> {
        let rows = sqlx::query_as::<_, (String, bool)>(
            "SELECT canonical_apparatus_id AS apparatus, enabled
             FROM mini_training_apparatus_modes
             ORDER BY canonical_apparatus_id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(rows
            .into_iter()
            .filter_map(|(apparatus, enabled)| {
                canonical_training_apparatus(&apparatus)
                    .map(|id| (id.to_string(), enabled))
                    .map_err(|error| {
                        tracing::warn!(%error, apparatus, "skipping invalid training apparatus mode");
                        error
                    })
                    .ok()
            })
            .collect())
    }

    pub async fn set_apparatus_mode(
        &self,
        apparatus: &str,
        enabled: bool,
    ) -> Result<(), TrainingWorkspaceError> {
        let apparatus = canonical_training_apparatus(apparatus)?;
        sqlx::query(
            "INSERT INTO mini_training_apparatus_modes
                (apparatus, canonical_apparatus_id, enabled, updated_at)
             VALUES (COALESCE((SELECT name FROM mini_apparatus WHERE id = $1), $1), $1, $2, now())
             ON CONFLICT (canonical_apparatus_id) DO UPDATE SET
                 apparatus = excluded.apparatus,
                 enabled = excluded.enabled,
                 updated_at = now()",
        )
        .bind(apparatus.as_str())
        .bind(enabled)
        .execute(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(())
    }

    pub async fn queue_states(
        &self,
    ) -> Result<BTreeMap<String, BTreeMap<String, String>>, TrainingWorkspaceError> {
        let rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT canonical_apparatus_id AS apparatus, order_id, state
             FROM mini_training_queue_states
             ORDER BY updated_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;

        let mut states = BTreeMap::new();
        for (apparatus, order_id, state) in rows {
            let Ok(apparatus) = canonical_training_apparatus(&apparatus) else {
                continue;
            };
            let order_id = order_id.trim();
            if apparatus.as_str().is_empty() || order_id.is_empty() {
                continue;
            }
            states
                .entry(apparatus.to_string())
                .or_insert_with(BTreeMap::new)
                .insert(order_id.to_string(), state.trim().to_string());
        }
        Ok(states)
    }

    pub async fn queue_state_records(
        &self,
    ) -> Result<Vec<TrainingQueueStateRecord>, TrainingWorkspaceError> {
        let rows = sqlx::query_as::<_, (String, String, String, i64)>(
            "SELECT canonical_apparatus_id AS apparatus, order_id, state,
                    EXTRACT(EPOCH FROM updated_at)::bigint AS updated_at_unix
             FROM mini_training_queue_states
             ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;

        Ok(rows
            .into_iter()
            .filter_map(|(apparatus, order_id, state, updated_at_unix)| {
                let apparatus = canonical_training_apparatus(&apparatus).ok()?.to_string();
                let order_id = order_id.trim().to_string();
                let state = state.trim().to_string();
                (!apparatus.is_empty() && !order_id.is_empty() && !state.is_empty()).then_some(
                    TrainingQueueStateRecord {
                        apparatus,
                        order_id,
                        state,
                        updated_at_unix,
                    },
                )
            })
            .collect())
    }

    pub async fn put_queue_state(
        &self,
        apparatus: &str,
        order_id: &str,
        state: &str,
    ) -> Result<(), TrainingWorkspaceError> {
        let apparatus = canonical_training_apparatus(apparatus)?;
        let order_id = order_id.trim();
        let state = state.trim();
        if apparatus.as_str().is_empty() || order_id.is_empty() || state.is_empty() {
            return Err(TrainingWorkspaceError::InvalidInput(
                "apparatus, order_id va state kerak".to_string(),
            ));
        }
        sqlx::query(
            "INSERT INTO mini_training_queue_states
                (apparatus, canonical_apparatus_id, order_id, state, updated_at)
             VALUES (COALESCE((SELECT name FROM mini_apparatus WHERE id = $1), $1), $1, $2, $3, now())
             ON CONFLICT (canonical_apparatus_id, order_id) DO UPDATE SET
                 apparatus = excluded.apparatus,
                 state = excluded.state,
                 updated_at = now()",
        )
        .bind(apparatus.as_str())
        .bind(order_id)
        .bind(state)
        .execute(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn put_queue_state_with_event(
        &self,
        apparatus: &str,
        order_id: &str,
        state: &str,
        event_id: &str,
        action: &str,
        from_state: &str,
        actor_ref: &str,
        actor_display_name: &str,
        progress_batches: &[OrderProgressBatch],
    ) -> Result<(), TrainingWorkspaceError> {
        let apparatus = canonical_training_apparatus(apparatus)?;
        let order_id = order_id.trim();
        let state = state.trim();
        let event_id = event_id.trim();
        let action = action.trim();
        let from_state = from_state.trim();
        if apparatus.as_str().is_empty()
            || order_id.is_empty()
            || state.is_empty()
            || event_id.is_empty()
            || action.is_empty()
            || from_state.is_empty()
        {
            return Err(TrainingWorkspaceError::InvalidInput(
                "training queue event uchun aparat, order, state va amal kerak".to_string(),
            ));
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        sqlx::query(
            "INSERT INTO mini_training_queue_states
                (apparatus, canonical_apparatus_id, order_id, state, updated_at)
             VALUES (COALESCE((SELECT name FROM mini_apparatus WHERE id = $1), $1), $1, $2, $3, now())
             ON CONFLICT (canonical_apparatus_id, order_id) DO UPDATE SET
                 apparatus = excluded.apparatus,
                 state = excluded.state,
                 updated_at = now()",
        )
        .bind(apparatus.as_str())
        .bind(order_id)
        .bind(state)
        .execute(&mut *tx)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        sqlx::query(
            "INSERT INTO mini_training_queue_events
                (event_id, apparatus, canonical_apparatus_id, order_id, action, from_state, to_state,
                 actor_ref, actor_display_name, created_at)
             VALUES ($1, COALESCE((SELECT name FROM mini_apparatus WHERE id = $2), $2), $2,
                     $3, $4, $5, $6, $7, $8, now())",
        )
        .bind(event_id)
        .bind(apparatus.as_str())
        .bind(order_id)
        .bind(action)
        .bind(from_state)
        .bind(state)
        .bind(actor_ref.trim())
        .bind(actor_display_name.trim())
        .execute(&mut *tx)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        for batch in progress_batches {
            let apparatus = canonical_training_apparatus(&batch.apparatus)?;
            let payload = training_progress_payload(batch)?;
            sqlx::query(
                "INSERT INTO mini_training_progress_batches
                    (batch_id, order_id, apparatus, canonical_apparatus_id, qr_payload, payload_json, generated_at)
                 VALUES ($1, $2, COALESCE((SELECT name FROM mini_apparatus WHERE id = $3), $3), $3, $4, $5, now())
                 ON CONFLICT (batch_id) DO UPDATE SET
                     order_id = excluded.order_id,
                     apparatus = excluded.apparatus,
                     canonical_apparatus_id = excluded.canonical_apparatus_id,
                     qr_payload = excluded.qr_payload,
                     payload_json = excluded.payload_json,
                     generated_at = now()",
            )
            .bind(batch.batch_id.trim())
            .bind(batch.order_id.trim())
            .bind(apparatus.as_str())
            .bind(batch.qr_payload.trim())
            .bind(payload)
            .execute(&mut *tx)
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        }
        tx.commit()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(())
    }
}
