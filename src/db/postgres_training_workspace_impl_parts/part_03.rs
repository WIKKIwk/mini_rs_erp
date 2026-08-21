impl PostgresTrainingWorkspaceStore {

    pub async fn latest_queue_events(
        &self,
    ) -> Result<Vec<TrainingQueueEvent>, TrainingWorkspaceError> {
        let rows = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                String,
                String,
                String,
                String,
                String,
                i64,
            ),
        >(
            "SELECT event_id, canonical_apparatus_id AS apparatus, order_id, action, from_state, to_state,
                    actor_ref, actor_display_name,
                    EXTRACT(EPOCH FROM created_at)::bigint AS created_at_unix
             FROM (
                SELECT DISTINCT ON (canonical_apparatus_id, order_id)
                    event_id, canonical_apparatus_id, order_id, action, from_state, to_state,
                    actor_ref, actor_display_name, created_at
                FROM mini_training_queue_events
                ORDER BY canonical_apparatus_id, order_id, created_at DESC, event_id DESC
             ) latest
             ORDER BY created_at DESC, event_id DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        rows.into_iter()
            .map(training_queue_event_from_row)
            .collect()
    }

    pub async fn completed_queue_events_for_actor(
        &self,
        actor_ref: &str,
        limit: usize,
    ) -> Result<Vec<TrainingQueueEvent>, TrainingWorkspaceError> {
        let actor_ref = actor_ref.trim();
        if actor_ref.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let limit = i64::try_from(limit.min(500)).unwrap_or(500);
        let rows = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                String,
                String,
                String,
                String,
                String,
                i64,
            ),
        >(
            "SELECT event_id, canonical_apparatus_id AS apparatus, order_id, action, from_state, to_state,
                    actor_ref, actor_display_name,
                    EXTRACT(EPOCH FROM created_at)::bigint AS created_at_unix
             FROM (
                SELECT DISTINCT ON (order_id, canonical_apparatus_id)
                    event_id, canonical_apparatus_id, order_id, action, from_state, to_state,
                    actor_ref, actor_display_name, created_at
                FROM mini_training_queue_events
                WHERE actor_ref = $1
                  AND action IN ('pause', 'detach_roll', 'roll_complete', 'complete')
                ORDER BY order_id, canonical_apparatus_id, created_at DESC, event_id DESC
             ) latest
             ORDER BY created_at DESC, event_id DESC
             LIMIT $2",
        )
        .bind(actor_ref)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        rows.into_iter()
            .map(training_queue_event_from_row)
            .collect()
    }

    pub async fn reset_queue_states(&self, apparatus: &str) -> Result<u64, TrainingWorkspaceError> {
        let apparatus = if apparatus.trim().is_empty() {
            None
        } else {
            Some(canonical_training_apparatus(apparatus)?)
        };
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        let result = (if apparatus.is_none() {
            sqlx::query("DELETE FROM mini_training_queue_states")
                .execute(&mut *tx)
                .await
        } else {
            sqlx::query(
                "DELETE FROM mini_training_queue_states
                 WHERE canonical_apparatus_id = $1",
            )
            .bind(
                apparatus
                    .as_ref()
                    .map(ApparatusId::as_str)
                    .unwrap_or_default(),
            )
            .execute(&mut *tx)
            .await
        })
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        if apparatus.is_none() {
            sqlx::query("DELETE FROM mini_training_queue_events")
                .execute(&mut *tx)
                .await
                .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        } else {
            sqlx::query(
                "DELETE FROM mini_training_queue_events
                 WHERE canonical_apparatus_id = $1",
            )
            .bind(
                apparatus
                    .as_ref()
                    .map(ApparatusId::as_str)
                    .unwrap_or_default(),
            )
            .execute(&mut *tx)
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        }
        tx.commit()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(result.rows_affected())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn save_returned_paint_report(
        &self,
        order_id: &str,
        apparatus: &str,
        action: &str,
        items: &[ReturnedPaintItem],
        image_id: &str,
        return_ink_kg: Option<f64>,
        calculation: Option<&ReturnedPaintCalculation>,
    ) -> Result<serde_json::Value, TrainingWorkspaceError> {
        let order_id = order_id.trim();
        let apparatus = canonical_training_apparatus(apparatus)?;
        let action = action.trim();
        let image_id = image_id.trim();
        if order_id.is_empty() || action.is_empty() {
            return Err(TrainingWorkspaceError::InvalidInput(
                "training qaytarilgan bo‘yoq hisoboti uchun order, aparat va amal kerak"
                    .to_string(),
            ));
        }
        let id = format!("training-returned-paint-{}", unix_micros());
        let created_at_unix = (unix_micros() / 1_000_000) as i64;
        let items_json =
            serde_json::to_value(items).map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        let calculation_json = calculation
            .map(serde_json::to_value)
            .transpose()
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        let return_ink_kg = return_ink_kg.filter(|value| value.is_finite() && *value >= 0.0);

        sqlx::query(
            "INSERT INTO mini_training_returned_paint_reports
                (id, order_id, apparatus, canonical_apparatus_id, action, items_json, image_id,
                 return_ink_kg, calculation_json, created_at)
             VALUES ($1, $2, $3, $3, $4, $5, $6, $7, $8, to_timestamp($9))",
        )
        .bind(&id)
        .bind(order_id)
        .bind(apparatus.as_str())
        .bind(action)
        .bind(&items_json)
        .bind(image_id)
        .bind(return_ink_kg)
        .bind(calculation_json.clone())
        .bind(created_at_unix as f64)
        .execute(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;

        Ok(serde_json::json!({
            "id": id,
            "order_id": order_id,
            "apparatus": apparatus,
            "action": action,
            "items": items_json,
            "image_id": image_id,
            "return_ink_kg": return_ink_kg,
            "calculation": calculation_json,
            "created_at_unix": created_at_unix,
        }))
    }

    pub async fn raw_material_barcodes_for_order_apparatus(
        &self,
        order_id: &str,
        apparatus: &str,
    ) -> Result<Vec<String>, TrainingWorkspaceError> {
        let apparatus = canonical_training_apparatus(apparatus)?;
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT barcode
             FROM mini_training_raw_material_assignments
             WHERE order_id = $1
               AND canonical_apparatus_id = $2
             ORDER BY updated_at ASC",
        )
        .bind(order_id.trim())
        .bind(apparatus.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(rows
            .into_iter()
            .map(|(barcode,)| barcode.trim().to_string())
            .filter(|barcode| !barcode.is_empty())
            .collect())
    }

    pub async fn save_image(
        &self,
        owner_key: &str,
        image: TrainingImage,
    ) -> Result<TrainingImage, TrainingWorkspaceError> {
        sqlx::query(
            "INSERT INTO mini_training_order_images
                (owner_key, image_id, image_name, image_mime, image_size_bytes, body, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, now())
             ON CONFLICT (owner_key, image_id) DO UPDATE SET
                 image_name = excluded.image_name,
                 image_mime = excluded.image_mime,
                 image_size_bytes = excluded.image_size_bytes,
                 body = excluded.body,
                 created_at = now()",
        )
        .bind(owner_key.trim())
        .bind(&image.image_id)
        .bind(&image.image_name)
        .bind(&image.image_mime)
        .bind(image.image_size_bytes as i64)
        .bind(&image.body)
        .execute(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(image)
    }

    pub async fn image(
        &self,
        owner_key: &str,
        image_id: &str,
    ) -> Result<Option<TrainingImage>, TrainingWorkspaceError> {
        let row = sqlx::query_as::<_, TrainingImageRow>(
            "SELECT image_id, image_name, image_mime, image_size_bytes, body
             FROM mini_training_order_images
             WHERE owner_key = $1 AND image_id = $2",
        )
        .bind(owner_key.trim())
        .bind(image_id.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;

        Ok(row.map(|row| TrainingImage {
            image_id: row.image_id,
            image_name: row.image_name,
            image_mime: row.image_mime,
            image_size_bytes: row.image_size_bytes.max(0) as u64,
            body: row.body,
        }))
    }

    pub async fn delete_image(
        &self,
        owner_key: &str,
        image_id: &str,
    ) -> Result<bool, TrainingWorkspaceError> {
        let deleted = sqlx::query_scalar::<_, String>(
            "DELETE FROM mini_training_order_images
             WHERE owner_key = $1 AND image_id = $2
             RETURNING image_id",
        )
        .bind(owner_key.trim())
        .bind(image_id.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(deleted.is_some())
    }
}
