impl PostgresTrainingWorkspaceStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn maps(&self) -> Result<Vec<ProductionMapSaved>, TrainingWorkspaceError> {
        let rows = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT map_json
             FROM mini_training_production_maps
             ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;

        let mut maps = Vec::with_capacity(rows.len());
        for payload in rows {
            match saved_map_from_payload(payload) {
                Ok(saved) => maps.push(saved),
                Err(error) => {
                    tracing::warn!(%error, "skipping invalid training production map");
                }
            }
        }
        Ok(maps)
    }

    pub async fn map(
        &self,
        map_id: &str,
    ) -> Result<Option<ProductionMapSaved>, TrainingWorkspaceError> {
        let payload = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT map_json
             FROM mini_training_production_maps
             WHERE id = $1",
        )
        .bind(map_id.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;

        payload.map(saved_map_from_payload).transpose()
    }

    pub async fn template_for_order(
        &self,
        map_id: &str,
        order_number: &str,
    ) -> Result<Option<CalculateOrderTemplate>, TrainingWorkspaceError> {
        let payload = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT payload_json
             FROM mini_training_quick_order_templates
             WHERE payload_json->>'source_map_id' = $1
                OR ($2 <> '' AND payload_json->>'order_number' = $2)
             ORDER BY
                 CASE WHEN payload_json->>'source_map_id' = $1 THEN 0 ELSE 1 END,
                 saved_at DESC
             LIMIT 1",
        )
        .bind(map_id.trim())
        .bind(order_number.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;

        payload
            .map(|payload| {
                serde_json::from_value(payload).map_err(|_| TrainingWorkspaceError::StoreFailed)
            })
            .transpose()
    }

    pub async fn save_map(
        &self,
        map: ProductionMapDefinition,
    ) -> Result<ProductionMapSaved, TrainingWorkspaceError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        let map = prepare_map_for_save(&mut tx, map).await?;
        save_map_tx(&mut tx, &map).await?;
        tx.commit()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        saved_map_from_definition(map)
    }

    pub async fn save_map_with_order(
        &self,
        map: ProductionMapDefinition,
        template: CalculateOrderTemplate,
        owner_key: &str,
    ) -> Result<TrainingProductionMapSaveWithOrder, TrainingWorkspaceError> {
        validate_template(&template).map_err(training_calculate_error)?;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        let mut map = prepare_map_for_save(&mut tx, map).await?;
        map.customer_name = template.customer.trim().to_string();
        save_map_tx(&mut tx, &map).await?;

        let template = prepare_template_for_save(template, &map);
        save_template_tx(&mut tx, &template, owner_key).await?;
        tx.commit()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;

        Ok(TrainingProductionMapSaveWithOrder {
            saved: saved_map_from_definition(map)?,
            template: Some(template),
        })
    }

    pub async fn raw_material_assignments(
        &self,
        order_id: &str,
        apparatus: &str,
    ) -> Result<Vec<serde_json::Value>, TrainingWorkspaceError> {
        let rows = sqlx::query_as::<_, TrainingRawMaterialRow>(
            "SELECT order_id, canonical_apparatus_id AS apparatus, barcode, payload_json
             FROM mini_training_raw_material_assignments
             ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;

        let order_id = order_id.trim();
        let apparatus = if apparatus.trim().is_empty() {
            None
        } else {
            Some(canonical_training_apparatus(apparatus)?)
        };
        let rows = rows.into_iter().filter_map(|mut row| {
            row.apparatus = canonical_training_apparatus(&row.apparatus)
                .ok()?
                .to_string();
            Some(row)
        });
        Ok(rows
            .filter(|row| {
                (order_id.is_empty() || row.order_id == order_id)
                    && apparatus
                        .as_ref()
                        .is_none_or(|id| row.apparatus.eq_ignore_ascii_case(id.as_str()))
            })
            .map(|row| {
                let mut payload = match row.payload_json {
                    serde_json::Value::Object(object) => object,
                    _ => serde_json::Map::new(),
                };
                payload.insert("order_id".to_string(), serde_json::json!(row.order_id));
                payload.insert("apparatus".to_string(), serde_json::json!(row.apparatus));
                payload.insert("barcode".to_string(), serde_json::json!(row.barcode));
                serde_json::Value::Object(payload)
            })
            .collect())
    }

    pub async fn save_raw_material_assignment(
        &self,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, TrainingWorkspaceError> {
        let order_id = payload_string(&payload, "order_id");
        let apparatus = payload_string(&payload, "apparatus");
        let apparatus = canonical_training_apparatus(&apparatus)?;
        let barcode = payload_string(&payload, "barcode");
        if order_id.is_empty() || barcode.is_empty() {
            return Err(TrainingWorkspaceError::InvalidInput(
                "order_id, apparatus va barcode kerak".to_string(),
            ));
        }

        let duplicate = sqlx::query_scalar::<_, String>(
            "SELECT id
             FROM mini_training_raw_material_assignments
             WHERE order_id = $1
               AND canonical_apparatus_id = $2
               AND lower(barcode) = lower($3)
             LIMIT 1",
        )
        .bind(&order_id)
        .bind(apparatus.as_str())
        .bind(&barcode)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        if duplicate.is_some() {
            return Err(TrainingWorkspaceError::DuplicateRawMaterialAssignment);
        }

        let id = format!("training-assignment-{}", unix_micros());
        let mut normalized_payload = payload;
        if let serde_json::Value::Object(object) = &mut normalized_payload {
            object.insert(
                "apparatus".to_string(),
                serde_json::json!(apparatus.as_str()),
            );
        }
        sqlx::query(
            "INSERT INTO mini_training_raw_material_assignments
                (id, order_id, apparatus, canonical_apparatus_id, barcode, payload_json, updated_at)
             VALUES ($1, $2, COALESCE((SELECT name FROM mini_apparatus WHERE id = $3), $3), $3, $4, $5, now())",
        )
        .bind(id)
        .bind(order_id)
        .bind(apparatus.as_str())
        .bind(barcode)
        .bind(&normalized_payload)
        .execute(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(normalized_payload)
    }

    pub async fn delete_raw_material_assignment(
        &self,
        order_id: &str,
        apparatus: &str,
        barcode: &str,
    ) -> Result<bool, TrainingWorkspaceError> {
        let order_id = order_id.trim();
        let apparatus = canonical_training_apparatus(apparatus)?;
        let barcode = barcode.trim();
        if order_id.is_empty() || !order_id.starts_with("training-") || barcode.is_empty() {
            return Err(TrainingWorkspaceError::InvalidInput(
                "order_id, apparatus va barcode kerak".to_string(),
            ));
        }

        let result = sqlx::query(
            "DELETE FROM mini_training_raw_material_assignments
             WHERE order_id = $1
               AND canonical_apparatus_id = $2
               AND lower(barcode) = lower($3)",
        )
        .bind(order_id)
        .bind(apparatus.as_str())
        .bind(barcode)
        .execute(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn generate_training_input_batch(
        &self,
        order_id: &str,
        apparatus: &str,
        input_apparatus: &str,
    ) -> Result<TrainingInputBatchIdentity, TrainingWorkspaceError> {
        self.generate_training_input_batches(order_id, apparatus, input_apparatus, 1)
            .await?
            .into_iter()
            .next()
            .ok_or(TrainingWorkspaceError::StoreFailed)
    }

    pub async fn generate_training_input_batches(
        &self,
        order_id: &str,
        apparatus: &str,
        input_apparatus: &str,
        count: usize,
    ) -> Result<Vec<TrainingInputBatchIdentity>, TrainingWorkspaceError> {
        let order_id = order_id.trim();
        let apparatus = canonical_training_apparatus(apparatus)?;
        let input_apparatus = training_virtual_input_id(input_apparatus)?;
        if order_id.is_empty()
            || !order_id.starts_with("training-")
            || apparatus.as_str().is_empty()
            || input_apparatus.is_empty()
            || count == 0
        {
            return Err(TrainingWorkspaceError::InvalidInput(
                "training order, target va input apparati kerak".to_string(),
            ));
        }
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        let mut identities = Vec::with_capacity(count);
        for _ in 0..count {
            let batch_id = progress_batch_id(
                &input_apparatus,
                order_id,
                queue_state::ApparatusQueueAction::Complete,
            );
            let session_id = format!("training-input-session:{batch_id}");
            let qr_payload = progress_qr_payload(&batch_id);
            let row = sqlx::query_as::<_, (String, String, String, String, String)>(
                "INSERT INTO mini_training_input_batches
                    (order_id, apparatus, canonical_apparatus_id, batch_id, session_id, qr_payload, generated_at)
                 VALUES ($1, COALESCE((SELECT name FROM mini_apparatus WHERE id = $2), $2), $2, $3, $4, $5, now())
                 RETURNING order_id, canonical_apparatus_id AS apparatus, batch_id, session_id, qr_payload",
            )
            .bind(order_id)
            .bind(apparatus.as_str())
            .bind(batch_id)
            .bind(session_id)
            .bind(qr_payload)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
            identities.push(training_input_batch_identity_from_row(row)?);
        }
        tx.commit()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(identities)
    }

    pub async fn training_input_batch(
        &self,
        order_id: &str,
        apparatus: &str,
        input_apparatus: &str,
    ) -> Result<Option<TrainingInputBatchIdentity>, TrainingWorkspaceError> {
        let identities = self.training_input_batches(order_id, apparatus).await?;
        let apparatus = canonical_training_apparatus(apparatus)?;
        if identities.len() != 1 {
            return Ok(None);
        }
        let identity = identities
            .into_iter()
            .next()
            .ok_or(TrainingWorkspaceError::StoreFailed)?;
        if is_production_progress_qr(&identity.qr_payload) {
            return Ok(Some(identity));
        }
        sqlx::query(
            "DELETE FROM mini_training_input_batches
             WHERE order_id = $1 AND canonical_apparatus_id = $2",
        )
        .bind(order_id.trim())
        .bind(apparatus.as_str())
        .execute(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        self.generate_training_input_batch(order_id, apparatus.as_str(), input_apparatus)
            .await
            .map(Some)
    }

    pub async fn training_input_batches(
        &self,
        order_id: &str,
        apparatus: &str,
    ) -> Result<Vec<TrainingInputBatchIdentity>, TrainingWorkspaceError> {
        let apparatus = canonical_training_apparatus(apparatus)?;
        let rows = sqlx::query_as::<_, (String, String, String, String, String)>(
            "SELECT order_id, canonical_apparatus_id AS apparatus, batch_id, session_id, qr_payload
             FROM mini_training_input_batches
             WHERE order_id = $1 AND canonical_apparatus_id = $2
             ORDER BY generated_at ASC, batch_id ASC",
        )
        .bind(order_id.trim())
        .bind(apparatus.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        rows.into_iter()
            .map(training_input_batch_identity_from_row)
            .collect()
    }

    pub async fn training_input_batch_for_qr(
        &self,
        qr_payload: &str,
    ) -> Result<Option<TrainingInputBatchIdentity>, TrainingWorkspaceError> {
        let row = sqlx::query_as::<_, (String, String, String, String, String)>(
            "SELECT order_id, canonical_apparatus_id AS apparatus, batch_id, session_id, qr_payload
             FROM mini_training_input_batches
             WHERE lower(qr_payload) = lower($1)",
        )
        .bind(qr_payload.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        row.map(training_input_batch_identity_from_row).transpose()
    }
}
