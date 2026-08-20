use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use sqlx::{PgPool, Postgres, Row, Transaction};

use crate::core::apparatus_groups::{
    ApparatusCapabilityProfile, ApparatusCatalogEntry, ApparatusGroup, ApparatusGroupError,
    ApparatusGroupStorePort, ApparatusMasterData, ApparatusSource,
};
use crate::core::apparatus_standard::{
    ApparatusFamily, ApparatusId, ApparatusKind, CanonicalApparatus, CapabilityCode,
    QueuePolicy,
};

#[cfg(test)]
use crate::core::apparatus_groups::custom_apparatus_id;

#[derive(Clone)]
pub struct PostgresApparatusGroupStore {
    pool: PgPool,
}

impl PostgresApparatusGroupStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ApparatusGroupStorePort for PostgresApparatusGroupStore {
    async fn groups(&self) -> Result<Vec<ApparatusGroup>, ApparatusGroupError> {
        let rows = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT payload_json
             FROM mini_apparatus_groups
             ORDER BY lower(name) ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| ApparatusGroupError::StoreFailed)?;

        rows.into_iter()
            .map(|payload| {
                serde_json::from_value::<ApparatusGroup>(payload)
                    .map_err(|_| ApparatusGroupError::StoreFailed)
            })
            .collect()
    }

    async fn put_group(&self, group: ApparatusGroup) -> Result<(), ApparatusGroupError> {
        let name = group.name.trim();
        let group_id = group_id(name);
        let payload = serde_json::to_value(&group).map_err(|_| ApparatusGroupError::StoreFailed)?;

        sqlx::query(
            "INSERT INTO mini_apparatus_groups (id, name, payload_json)
             VALUES ($1, $2, $3)
             ON CONFLICT ((lower(name))) DO UPDATE SET
               name = excluded.name,
               payload_json = excluded.payload_json,
               updated_at = now()",
        )
        .bind(group_id)
        .bind(name)
        .bind(payload)
        .execute(&self.pool)
        .await
        .map_err(|_| ApparatusGroupError::StoreFailed)?;

        Ok(())
    }

    async fn apparatus(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<String>, ApparatusGroupError> {
        let needle = query.trim().to_lowercase();
        let pattern = format!("%{needle}%");
        sqlx::query_scalar::<_, String>(
            "SELECT name
             FROM mini_apparatus
             WHERE ($1 = '' OR lower(name) LIKE $2)
             ORDER BY lower(name) ASC
             LIMIT $3",
        )
        .bind(needle)
        .bind(pattern)
        .bind(limit.max(1) as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| ApparatusGroupError::StoreFailed)
    }

    async fn apparatus_catalog(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ApparatusCatalogEntry>, ApparatusGroupError> {
        let needle = query.trim().to_lowercase();
        let pattern = format!("%{needle}%");
        let rows = sqlx::query(
            "SELECT id, name, payload_json,
                    row_number() OVER (ORDER BY lower(name), id) - 1 AS catalog_order
             FROM mini_apparatus
             WHERE ($1 = '' OR lower(name) LIKE $2)
             ORDER BY lower(name) ASC
             LIMIT $3",
        )
        .bind(needle)
        .bind(pattern)
        .bind(limit.max(1) as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| ApparatusGroupError::StoreFailed)?;
        rows.into_iter()
            .map(|row| {
                let name: String = row.get("name");
                let payload: serde_json::Value = row.get("payload_json");
                let master = serde_json::from_value::<ApparatusMasterData>(payload)
                    .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
                let id: String = row.get("id");
                ApparatusId::new(id.clone()).map_err(|_| ApparatusGroupError::InvalidApparatus)?;
                Ok(ApparatusCatalogEntry {
                    id,
                    name,
                    source: ApparatusSource::Custom,
                    sort_order: row.get::<i64, _>("catalog_order").max(0) as usize,
                    master,
                })
            })
            .collect::<Result<Vec<_>, ApparatusGroupError>>()
    }

    async fn canonical_apparatus_by_id(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<CanonicalApparatus>, ApparatusGroupError> {
        let payload = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT payload_json
             FROM mini_apparatus
             WHERE id = $1
             LIMIT 1",
        )
        .bind(apparatus_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| ApparatusGroupError::StoreFailed)?;

        let Some(payload) = payload else {
            return Ok(None);
        };
        let Some(canonical) = payload.get("canonical_apparatus") else {
            // Legacy master payloads are intentionally not promoted to live
            // canonical configuration at read time.
            return Ok(None);
        };
        let canonical = serde_json::from_value::<CanonicalApparatus>(canonical.clone())
            .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
        if canonical.identity.id != *apparatus_id {
            return Err(ApparatusGroupError::InvalidApparatus);
        }
        canonical
            .validate()
            .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
        Ok(Some(canonical))
    }

    async fn put_canonical_apparatus(
        &self,
        expected_revision: u64,
        canonical: &CanonicalApparatus,
    ) -> Result<(), ApparatusGroupError> {
        canonical
            .validate()
            .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
        if canonical.versioning.revision
            != expected_revision
                .checked_add(1)
                .ok_or(ApparatusGroupError::Conflict)?
        {
            return Err(ApparatusGroupError::Conflict);
        }
        let payload = storage_payload(canonical)?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        let updated = sqlx::query(
            "UPDATE mini_apparatus
             SET name = $2,
                 base_name = $2,
                 kind = $3,
                 payload_json = $4,
                 updated_at = now()
             WHERE id = $1
               AND payload_json #>> '{canonical_apparatus,versioning,revision}' = $5",
        )
        .bind(canonical.identity.id.as_str())
        .bind(canonical.identity.display.display_name.trim())
        .bind(kind_name(canonical.classification.kind))
        .bind(payload)
        .bind(expected_revision.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|_| ApparatusGroupError::StoreFailed)?;
        if updated.rows_affected() != 1 {
            return Err(ApparatusGroupError::Conflict);
        }
        sync_canonical_projections_tx(&mut tx, canonical).await?;
        tx.commit()
            .await
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        Ok(())
    }

    async fn put_apparatus_with_canonical(
        &self,
        expected_revision: Option<u64>,
        requested_id: Option<&str>,
        name: &str,
        _master: &ApparatusMasterData,
        canonical: &CanonicalApparatus,
    ) -> Result<String, ApparatusGroupError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ApparatusGroupError::MissingApparatus);
        }
        canonical
            .validate()
            .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
        if let Some(expected_revision) = expected_revision
            && canonical.versioning.revision
                != expected_revision
                    .checked_add(1)
                    .ok_or(ApparatusGroupError::Conflict)?
        {
            return Err(ApparatusGroupError::Conflict);
        }
        let id = requested_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(|id| {
                ApparatusId::new(id.to_string()).map_err(|_| ApparatusGroupError::InvalidApparatus)
            })
            .transpose()?
            .unwrap_or_else(|| canonical.identity.id.clone());
        if id != canonical.identity.id {
            return Err(ApparatusGroupError::InvalidApparatus);
        }
        let payload = storage_payload(canonical)?;
        let display_name = canonical.identity.display.display_name.trim();
        let canonical_master = master_data_from_canonical(canonical);
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ApparatusGroupError::StoreFailed)?;

        let id = if let Some(expected_revision) = expected_revision {
            sqlx::query_scalar::<_, String>(
                "UPDATE mini_apparatus
                 SET name = $2,
                     base_name = $2,
                     kind = $3,
                     payload_json = $4,
                     updated_at = now()
                 WHERE id = $1
                   AND payload_json #>> '{canonical_apparatus,versioning,revision}' = $5
                 RETURNING id",
            )
            .bind(id.as_str())
            .bind(display_name)
            .bind(canonical_master.kind.as_str())
            .bind(payload)
            .bind(expected_revision.to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| ApparatusGroupError::StoreFailed)?
            .ok_or(ApparatusGroupError::Conflict)?
        } else {
            sqlx::query_scalar::<_, String>(
                "INSERT INTO mini_apparatus (id, name, base_name, kind, payload_json)
                 VALUES ($1, $2, $2, $3, $4)
                 ON CONFLICT (id) DO NOTHING
                 RETURNING id",
            )
            .bind(id.as_str())
            .bind(display_name)
            .bind(canonical_master.kind.as_str())
            .bind(payload)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| ApparatusGroupError::StoreFailed)?
            .ok_or(ApparatusGroupError::Conflict)?
        };
        sync_canonical_projections_tx(&mut tx, canonical).await?;
        tx.commit()
            .await
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        Ok(id)
    }
}

async fn sync_canonical_projections_tx(
    tx: &mut Transaction<'_, Postgres>,
    canonical: &CanonicalApparatus,
) -> Result<(), ApparatusGroupError> {
    let apparatus_id = canonical.identity.id.as_str();
    let display_name = canonical.identity.display.display_name.trim();
    let queue_policy = queue_policy_name(canonical.policies.queue);
    let queue_payload = serde_json::json!({
        "policy": queue_policy,
    });
    sqlx::query(
        "INSERT INTO mini_apparatus_queue_policies (
            apparatus, canonical_apparatus_id, policy, actor_role, actor_ref,
            actor_display_name, payload_json, updated_at
         ) VALUES ($1, $2, $3, '', '', '', $4, now())
         ON CONFLICT (canonical_apparatus_id) DO UPDATE SET
            apparatus = EXCLUDED.apparatus,
            policy = EXCLUDED.policy,
            payload_json = jsonb_set(
                CASE
                    WHEN jsonb_typeof(mini_apparatus_queue_policies.payload_json) = 'object'
                        THEN mini_apparatus_queue_policies.payload_json
                    ELSE '{}'::jsonb
                END,
                '{policy}',
                to_jsonb(EXCLUDED.policy),
                true
            ),
            updated_at = EXCLUDED.updated_at",
    )
    .bind(display_name)
    .bind(apparatus_id)
    .bind(queue_policy)
    .bind(queue_payload)
    .execute(&mut **tx)
    .await
    .map_err(|_| ApparatusGroupError::StoreFailed)?;

    let (capabilities, capability_levels) = capacity_projection_values(canonical)?;
    let working_windows = serde_json::to_value(&canonical.capacity.working_windows)
        .map_err(|_| ApparatusGroupError::StoreFailed)?;
    let setup_minutes = i32::try_from(canonical.capacity.setup_minutes)
        .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
    let cleanup_minutes = i32::try_from(canonical.capacity.cleanup_minutes)
        .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
    sqlx::query(
        "DELETE FROM mini_apparatus_capacity_profiles
         WHERE canonical_apparatus_id = $1
           AND apparatus_id <> $1",
    )
    .bind(apparatus_id)
    .execute(&mut **tx)
    .await
    .map_err(|_| ApparatusGroupError::StoreFailed)?;
    sqlx::query(
        "INSERT INTO mini_apparatus_capacity_profiles (
            canonical_apparatus_id, apparatus_id, apparatus, capacity_slots,
            setup_minutes, cleanup_minutes, efficiency_percent, finite_capacity,
            working_windows, capabilities, capability_levels, notes, updated_at
         ) VALUES ($1, $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, '', now())
         ON CONFLICT (canonical_apparatus_id) DO UPDATE SET
            apparatus_id = EXCLUDED.apparatus_id,
            apparatus = EXCLUDED.apparatus,
            capacity_slots = EXCLUDED.capacity_slots,
            setup_minutes = EXCLUDED.setup_minutes,
            cleanup_minutes = EXCLUDED.cleanup_minutes,
            efficiency_percent = EXCLUDED.efficiency_percent,
            finite_capacity = EXCLUDED.finite_capacity,
            working_windows = EXCLUDED.working_windows,
            capabilities = EXCLUDED.capabilities,
            capability_levels = EXCLUDED.capability_levels,
            updated_at = EXCLUDED.updated_at",
    )
    .bind(apparatus_id)
    .bind(display_name)
    .bind(i32::from(canonical.capacity.capacity_slots))
    .bind(setup_minutes)
    .bind(cleanup_minutes)
    .bind(i32::from(canonical.capacity.efficiency_percent))
    .bind(canonical.capacity.finite_capacity)
    .bind(working_windows)
    .bind(capabilities)
    .bind(capability_levels)
    .execute(&mut **tx)
    .await
    .map_err(|_| ApparatusGroupError::StoreFailed)?;

    let material = &canonical.policies.material;
    let item_groups = serde_json::to_value(&material.item_groups)
        .map_err(|_| ApparatusGroupError::StoreFailed)?;
    let requirement_groups = serde_json::to_value(&material.requirement_groups)
        .map_err(|_| ApparatusGroupError::StoreFailed)?;
    let material_payload = serde_json::json!({
        "apparatus_id": apparatus_id,
        "apparatus": display_name,
        "requires_material": material.requires_material,
        "start_policy": material.start_policy,
        "item_groups": material.item_groups,
        "requirement_groups": material.requirement_groups,
    });
    sqlx::query(
        "INSERT INTO mini_apparatus_material_rules (
            apparatus, canonical_apparatus_id, item_groups, requirement_groups,
            requires_material, payload_json, updated_at
         ) VALUES ($1, $2, $3, $4, $5, $6, now())
         ON CONFLICT (canonical_apparatus_id) DO UPDATE SET
            apparatus = EXCLUDED.apparatus,
            item_groups = EXCLUDED.item_groups,
            requirement_groups = EXCLUDED.requirement_groups,
            requires_material = EXCLUDED.requires_material,
            payload_json = EXCLUDED.payload_json,
            updated_at = EXCLUDED.updated_at",
    )
    .bind(display_name)
    .bind(apparatus_id)
    .bind(item_groups)
    .bind(requirement_groups)
    .bind(material.requires_material)
    .bind(material_payload)
    .execute(&mut **tx)
    .await
    .map_err(|_| ApparatusGroupError::StoreFailed)?;

    Ok(())
}

fn storage_payload(canonical: &CanonicalApparatus) -> Result<serde_json::Value, ApparatusGroupError> {
    let mut payload = serde_json::to_value(master_data_from_canonical(canonical))
        .map_err(|_| ApparatusGroupError::StoreFailed)?;
    payload["canonical_apparatus"] =
        serde_json::to_value(canonical).map_err(|_| ApparatusGroupError::StoreFailed)?;
    Ok(payload)
}

fn master_data_from_canonical(canonical: &CanonicalApparatus) -> ApparatusMasterData {
    ApparatusMasterData {
        family: family_name(canonical.classification.family).to_string(),
        kind: kind_name(canonical.classification.kind).to_string(),
        capabilities: canonical
            .capabilities
            .iter()
            .map(|code| capability_code_name(*code).to_string())
            .collect(),
        capability_profiles: canonical
            .capability_profiles
            .iter()
            .map(|profile| ApparatusCapabilityProfile {
                code: capability_code_name(profile.code).to_string(),
                level: profile.level,
                valid_from_unix: profile.valid_from_unix,
                valid_to_unix: profile.valid_to_unix,
                enabled: profile.enabled,
            })
            .collect(),
        color_stations: canonical.classification.color_stations,
        factory_map_object_id: canonical
            .placement
            .as_ref()
            .map(|placement| placement.factory_map_object_id.clone()),
        training_enabled: canonical.training.enabled,
        tooling_policy: Some(canonical.policies.tooling),
        capacity: Some(canonical.capacity.clone()),
    }
}

fn capacity_projection_values(
    canonical: &CanonicalApparatus,
) -> Result<(serde_json::Value, serde_json::Value), ApparatusGroupError> {
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or_default();
    let mut capabilities = Vec::new();
    let mut capability_levels = BTreeMap::new();
    for capability in &canonical.capabilities {
        let code = capability_code_name(*capability).to_string();
        let profiles = canonical
            .capability_profiles
            .iter()
            .filter(|profile| profile.code == *capability)
            .collect::<Vec<_>>();
        if profiles.is_empty() {
            capabilities.push(code.clone());
            capability_levels.insert(code, 1u16);
            continue;
        }
        let active = profiles
            .into_iter()
            .filter(|profile| {
                profile.enabled
                    && profile
                        .valid_from_unix
                        .is_none_or(|starts_at| now_unix >= starts_at)
                    && profile
                        .valid_to_unix
                        .is_none_or(|ends_at| now_unix < ends_at)
            })
            .collect::<Vec<_>>();
        if active.len() > 1 {
            return Err(ApparatusGroupError::InvalidApparatus);
        }
        if let Some(profile) = active.first() {
            capabilities.push(code.clone());
            capability_levels.insert(code, profile.level);
        }
    }
    Ok((
        serde_json::to_value(capabilities).map_err(|_| ApparatusGroupError::StoreFailed)?,
        serde_json::to_value(capability_levels)
            .map_err(|_| ApparatusGroupError::StoreFailed)?,
    ))
}

fn family_name(family: ApparatusFamily) -> &'static str {
    match family {
        ApparatusFamily::Pechat => "pechat",
        ApparatusFamily::Laminatsiya => "laminatsiya",
        ApparatusFamily::Rezka => "rezka",
        ApparatusFamily::Paket => "paket",
        ApparatusFamily::Kley => "kley",
        ApparatusFamily::Other => "other",
    }
}

fn kind_name(kind: ApparatusKind) -> &'static str {
    match kind {
        ApparatusKind::ColorPechat => "color_pechat",
        ApparatusKind::Flexo => "flexo",
        ApparatusKind::Laminatsiya => "laminatsiya",
        ApparatusKind::ExtruderLaminatsiya => "extruder_laminatsiya",
        ApparatusKind::Rezka => "rezka",
        ApparatusKind::Paket => "paket",
        ApparatusKind::HolodniyKley => "holodniy_kley",
        ApparatusKind::Other => "other",
    }
}

fn capability_code_name(code: CapabilityCode) -> &'static str {
    match code {
        CapabilityCode::Print => "print",
        CapabilityCode::Pechat => "pechat",
        CapabilityCode::Flexo => "flexo",
        CapabilityCode::Laminate => "laminate",
        CapabilityCode::Cut => "cut",
        CapabilityCode::Package => "package",
        CapabilityCode::Glue => "glue",
        CapabilityCode::Apparatus => "apparatus",
    }
}

fn queue_policy_name(policy: QueuePolicy) -> &'static str {
    match policy {
        QueuePolicy::StrictSequence => "strict_sequence",
        QueuePolicy::FreePick => "free_pick",
    }
}

#[cfg(test)]
impl PostgresApparatusGroupStore {
    pub(crate) async fn put_apparatus_with_id(
        &self,
        requested_id: Option<&str>,
        name: &str,
        master: &ApparatusMasterData,
    ) -> Result<String, ApparatusGroupError> {
        self.save_apparatus(requested_id, name, master).await
    }

    async fn save_apparatus(
        &self,
        requested_id: Option<&str>,
        name: &str,
        master: &ApparatusMasterData,
    ) -> Result<String, ApparatusGroupError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ApparatusGroupError::MissingApparatus);
        }
        let requested_id = requested_id
            .map(|id| {
                ApparatusId::new(id.trim().to_string())
                    .map_err(|_| ApparatusGroupError::InvalidApparatus)
            })
            .transpose()?;
        let payload = serde_json::to_value(master).map_err(|_| ApparatusGroupError::StoreFailed)?;
        let existing_id = if let Some(requested_id) = requested_id.as_ref() {
            sqlx::query_scalar::<_, String>(
                "SELECT id
                 FROM mini_apparatus
                 WHERE id = $1
                 LIMIT 1",
            )
            .bind(requested_id.as_str())
            .fetch_optional(&self.pool)
            .await
        } else {
            Ok(None)
        }
        .map_err(|_| ApparatusGroupError::StoreFailed)?;

        if let Some(id) = existing_id {
            return sqlx::query_scalar::<_, String>(
                "UPDATE mini_apparatus
                 SET name = $2,
                     payload_json = CASE
                         WHEN jsonb_typeof(payload_json) = 'object'
                              AND payload_json ? 'canonical_apparatus'
                         THEN jsonb_set(
                             CASE
                                 WHEN jsonb_typeof($3::jsonb) = 'object' THEN $3::jsonb
                                 ELSE '{}'::jsonb
                             END,
                             '{canonical_apparatus}',
                             payload_json->'canonical_apparatus',
                             true
                         )
                         ELSE CASE
                             WHEN jsonb_typeof($3::jsonb) = 'object' THEN $3::jsonb
                             ELSE '{}'::jsonb
                         END
                     END,
                     updated_at = now()
                 WHERE id = $1
                 RETURNING id",
            )
            .bind(id)
            .bind(name)
            .bind(payload)
            .fetch_one(&self.pool)
            .await
            .map_err(|_| ApparatusGroupError::StoreFailed);
        }

        let id = requested_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| custom_apparatus_id(name));
        sqlx::query_scalar::<_, String>(
            "INSERT INTO mini_apparatus (id, name, base_name, kind, payload_json)
             VALUES ($1, $2, $2, 'custom', $3)
             RETURNING id",
        )
        .bind(id)
        .bind(name)
        .bind(payload)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| ApparatusGroupError::StoreFailed)
    }
}

fn group_id(name: &str) -> String {
    format!("apparatus_group:{}", name.trim().to_lowercase())
}
