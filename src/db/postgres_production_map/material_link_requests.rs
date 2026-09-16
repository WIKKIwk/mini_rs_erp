//! Approval is one transaction: selected assignments, decision and durable card revision.
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};

use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::chat::{ChatPrincipalInput, ChatService};
use crate::core::production_map::{ProductionMapError, RawMaterialAssignment};
use crate::db::postgres_chat::PostgresChatStore;

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("material_link_store_failed")]
    Store(#[from] sqlx::Error),
    #[error("material_link_invalid_payload")]
    Json(#[from] serde_json::Error),
    #[error("material_link_not_found")]
    NotFound,
    #[error("material_link_forbidden")]
    Forbidden,
    #[error("material_link_selection_required")]
    Selection,
    #[error(transparent)]
    Production(#[from] ProductionMapError),
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct LinkCandidate {
    pub stock_id: String,
    pub barcode: String,
    pub item_name: String,
    pub qty: f64,
    pub uom: String,
    pub location_id: String,
    pub location_name: String,
    pub placement_version: i64,
    pub mover_ref: String,
    pub mover_display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkRequest {
    pub request_id: String,
    pub status: String,
    pub event_sequence: i64,
    pub order_id: String,
    pub order_number: String,
    pub order_title: String,
    pub apparatus_id: String,
    pub apparatus_name: String,
    pub requester_ref: String,
    pub requester_display_name: String,
    pub mover_ref: String,
    pub mover_display_name: String,
    pub candidates: Vec<LinkCandidate>,
    pub selected_barcodes: Vec<String>,
    pub decided_by_name: String,
    pub decided_by_role: String,
    pub decided_at_unix: i64,
    pub reason: String,
    pub requested_at_unix: i64,
    pub expires_at_unix: i64,
}

impl LinkRequest {
    pub fn can_decide(&self, principal: &Principal) -> bool {
        principal.role == PrincipalRole::Admin
            || (principal.role == PrincipalRole::MaterialTaminotchi
                && principal.ref_ == self.mover_ref)
    }

    pub fn can_read(&self, principal: &Principal) -> bool {
        self.can_decide(principal)
            || (principal.role == PrincipalRole::Aparatchi && principal.ref_ == self.requester_ref)
    }

    pub fn validate_selection(&self, barcodes: &[String]) -> Result<(), LinkError> {
        let selected: BTreeSet<_> = barcodes.iter().map(|b| b.trim().to_lowercase()).collect();
        if selected.is_empty()
            || selected.len() != barcodes.len()
            || selected.iter().any(|b| {
                !self
                    .candidates
                    .iter()
                    .any(|c| c.barcode.eq_ignore_ascii_case(b))
            })
        {
            return Err(LinkError::Selection);
        }
        Ok(())
    }

    fn finish(&mut self, status: &str, reason: &str) {
        self.status = status.into();
        self.reason = reason.into();
        self.decided_at_unix = time::OffsetDateTime::now_utc().unix_timestamp();
        self.event_sequence += 1;
    }
}

const CANDIDATES_SQL: &str = r#"
SELECT stock.id AS stock_id, stock.barcode, stock.item_name, stock.qty::float8 AS qty,
       stock.uom, location.id AS location_id, location.name AS location_name,
       placement.version AS placement_version, placement.updated_by_ref AS mover_ref,
       placement.updated_by_name AS mover_display_name
FROM mini_raw_material_stock stock
JOIN mini_inventory_placements placement
  ON placement.asset_kind = 'raw_material' AND placement.asset_ref = stock.id
JOIN mini_inventory_locations location ON location.id = placement.physical_location_id
JOIN mini_factory_location_apparatus_links link ON link.location_id = location.factory_location_id
JOIN mini_apparatus apparatus ON apparatus.id = link.apparatus_id
WHERE link.apparatus_id = $1 AND location.kind = 'state' AND location.active
  AND stock.status = 'available' AND stock.qty > 0 AND stock.reserved_order_id = ''
  AND COALESCE(stock.payload_json->>'inventory_transfer_id', '') = ''
  AND placement.updated_by_role = 'material_taminotchi' AND placement.updated_by_ref <> ''
  AND NOT EXISTS (SELECT 1 FROM mini_raw_material_assignments assignment
                  WHERE lower(assignment.barcode) = lower(stock.barcode))
ORDER BY stock.id
"#;

#[derive(Clone)]
pub struct MaterialLinkStore {
    pool: PgPool,
}

impl MaterialLinkStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn candidates(&self, apparatus: &str) -> Result<Vec<LinkCandidate>, LinkError> {
        Ok(sqlx::query_as(CANDIDATES_SQL)
            .bind(apparatus)
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn get(&self, id: &str) -> Result<LinkRequest, LinkError> {
        let mut tx = self.pool.begin().await?;
        let mut request = load_tx(&mut tx, id).await?;
        reconcile_tx(&mut tx, &mut request).await?;
        tx.commit().await?;
        Ok(request)
    }

    pub async fn list(
        &self,
        order_id: &str,
        apparatus: &str,
        requester: &str,
    ) -> Result<Vec<LinkRequest>, LinkError> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM mini_material_link_requests WHERE order_id=$1 AND apparatus_id=$2
             AND requester_ref=$3 ORDER BY created_at DESC LIMIT 50",
        )
        .bind(order_id)
        .bind(apparatus)
        .bind(requester)
        .fetch_all(&self.pool)
        .await?;
        let mut result = Vec::new();
        for id in ids {
            result.push(self.get(&id).await?);
        }
        Ok(result)
    }

    pub async fn create(&self, mut request: LinkRequest) -> Result<LinkRequest, LinkError> {
        let mut tx = self.pool.begin().await?;
        // Serialize duplicate submits, including retries from another process.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!(
                "material-link:{}:{}:{}:{}",
                request.order_id, request.apparatus_id, request.requester_ref, request.mover_ref
            ))
            .execute(&mut *tx)
            .await?;
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id FROM mini_material_link_requests WHERE order_id=$1 AND apparatus_id=$2
             AND requester_ref=$3 AND mover_ref=$4 AND status='pending'",
        )
        .bind(&request.order_id)
        .bind(&request.apparatus_id)
        .bind(&request.requester_ref)
        .bind(&request.mover_ref)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(id) = existing {
            let mut existing = load_tx(&mut tx, &id).await?;
            reconcile_tx(&mut tx, &mut existing).await?;
            if existing.status == "pending" {
                tx.commit().await?;
                return Ok(existing);
            }
        }
        sqlx::query(
            "INSERT INTO mini_material_link_requests
            (id,order_id,apparatus_id,requester_ref,mover_ref,status,payload_json,expires_at)
            VALUES ($1,$2,$3,$4,$5,'pending',$6,to_timestamp($7::double precision))",
        )
        .bind(&request.request_id)
        .bind(&request.order_id)
        .bind(&request.apparatus_id)
        .bind(&request.requester_ref)
        .bind(&request.mover_ref)
        .bind(serde_json::to_value(&request)?)
        .bind(request.expires_at_unix)
        .execute(&mut *tx)
        .await?;
        reconcile_tx(&mut tx, &mut request).await?;
        tx.commit().await?;
        Ok(request)
    }

    pub async fn decide(
        &self,
        id: &str,
        principal: &Principal,
        approve: bool,
        assignments: Vec<RawMaterialAssignment>,
    ) -> Result<LinkRequest, LinkError> {
        // Same lock order as queue actions, then the request, then stock/placement rows.
        let initial = self.get(id).await?;
        if !initial.can_decide(principal) {
            return Err(LinkError::Forbidden);
        }
        let mut tx = self.pool.begin().await?;
        super::transaction_locks::lock_order_and_apparatuses_tx(
            &mut tx,
            &initial.order_id,
            &[&initial.apparatus_id],
        )
        .await?;
        let mut request = load_tx(&mut tx, id).await?;
        if request.status != "pending" {
            tx.commit().await?;
            return Ok(request);
        }
        if approve {
            let barcodes: Vec<_> = assignments.iter().map(|a| a.barcode.clone()).collect();
            request.validate_selection(&barcodes)?;
            if assignments.iter().any(|a| {
                a.order_id != request.order_id || a.apparatus_id.as_str() != request.apparatus_id
            }) {
                return Err(LinkError::Selection);
            }
            // Lock all snapshot stock rows in stable order before checking for stale inputs.
            let ids: Vec<_> = request
                .candidates
                .iter()
                .map(|c| c.stock_id.clone())
                .collect();
            sqlx::query(
                "SELECT id FROM mini_raw_material_stock WHERE id=ANY($1) ORDER BY id FOR UPDATE",
            )
            .bind(&ids)
            .fetch_all(&mut *tx)
            .await?;
            sqlx::query(
                "SELECT asset_ref FROM mini_inventory_placements WHERE asset_kind='raw_material'
                         AND asset_ref=ANY($1) ORDER BY asset_ref FOR UPDATE",
            )
            .bind(&ids)
            .fetch_all(&mut *tx)
            .await?;
            reconcile_tx(&mut tx, &mut request).await?;
            if request.status != "pending" {
                tx.commit().await?;
                return Ok(request);
            }
            for assignment in &assignments {
                super::material_helpers::save_raw_material_assignment_tx(&mut tx, assignment)
                    .await?;
            }
            request.selected_barcodes = barcodes;
            request.finish("approved", "");
        } else {
            request.finish("cancelled", "");
        }
        request.decided_by_name = principal.display_name.clone();
        request.decided_by_role = if principal.role == PrincipalRole::Admin {
            "admin"
        } else {
            "material_taminotchi"
        }
        .into();
        save_tx(&mut tx, &request).await?;
        tx.commit().await?;
        Ok(request)
    }

    pub async fn cancel(&self, id: &str, principal: &Principal) -> Result<LinkRequest, LinkError> {
        let mut tx = self.pool.begin().await?;
        let mut request = load_tx(&mut tx, id).await?;
        if principal.role != PrincipalRole::Aparatchi || principal.ref_ != request.requester_ref {
            return Err(LinkError::Forbidden);
        }
        if request.status == "pending" {
            request.finish("cancelled", "Worker so‘rovni bekor qildi");
            request.decided_by_name = principal.display_name.clone();
            request.decided_by_role = "aparatchi".into();
            save_tx(&mut tx, &request).await?;
        }
        tx.commit().await?;
        Ok(request)
    }

    pub async fn invalidate(&self, id: &str, reason: &str) -> Result<LinkRequest, LinkError> {
        let mut tx = self.pool.begin().await?;
        let mut request = load_tx(&mut tx, id).await?;
        if request.status == "pending" {
            request.finish("stale", reason);
            save_tx(&mut tx, &request).await?;
        }
        tx.commit().await?;
        Ok(request)
    }

    pub fn start_worker(&self, chat: ChatService) {
        let store = self.clone();
        tokio::spawn(async move {
            loop {
                if let Err(error) = store.deliver_pending(&chat).await {
                    tracing::warn!(%error, "material link card delivery failed");
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        });
    }

    pub async fn deliver_pending(&self, chat: &ChatService) -> Result<(), LinkError> {
        let pending: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM mini_material_link_requests WHERE status='pending' ORDER BY created_at",
        )
        .fetch_all(&self.pool)
        .await?;
        for id in pending {
            self.get(&id).await?;
        }
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM mini_material_link_requests WHERE delivered_revision < revision
             AND retry_at <= now() ORDER BY retry_at LIMIT 50",
        )
        .fetch_all(&self.pool)
        .await?;
        for id in ids {
            let request = self.get(&id).await?;
            let mut delivered = true;
            for (role, ref_, name) in [
                (
                    PrincipalRole::Admin,
                    "admin".to_string(),
                    "Admin".to_string(),
                ),
                (
                    PrincipalRole::MaterialTaminotchi,
                    request.mover_ref.clone(),
                    request.mover_display_name.clone(),
                ),
            ] {
                let result = async {
                    let conversation = chat
                        .create_or_get_dm(
                            ChatPrincipalInput {
                                role: PrincipalRole::Aparatchi,
                                ref_: request.requester_ref.clone(),
                                display_name: request.requester_display_name.clone(),
                                avatar_url: String::new(),
                            },
                            ChatPrincipalInput {
                                role,
                                ref_,
                                display_name: name,
                                avatar_url: String::new(),
                            },
                        )
                        .await?;
                    PostgresChatStore::new(self.pool.clone())
                        .upsert_material_link_card(
                            &Principal {
                                role: PrincipalRole::Aparatchi,
                                ref_: request.requester_ref.clone(),
                                display_name: request.requester_display_name.clone(),
                                legal_name: String::new(),
                                phone: String::new(),
                                avatar_url: String::new(),
                            },
                            &conversation.conversation_id,
                            &request,
                        )
                        .await
                }
                .await;
                if let Err(error) = result {
                    delivered = false;
                    tracing::warn!(%error, request_id=%id, "material link card retry scheduled");
                }
            }
            if delivered {
                sqlx::query("UPDATE mini_material_link_requests SET delivered_revision=GREATEST(delivered_revision,$2)
                             WHERE id=$1")
                    .bind(&id).bind(request.event_sequence).execute(&self.pool).await?;
            } else {
                sqlx::query("UPDATE mini_material_link_requests SET retry_at=now()+interval '10 seconds' WHERE id=$1")
                    .bind(&id).execute(&self.pool).await?;
            }
        }
        Ok(())
    }
}

async fn load_tx(tx: &mut Transaction<'_, Postgres>, id: &str) -> Result<LinkRequest, LinkError> {
    let value: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json FROM mini_material_link_requests WHERE id=$1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(LinkError::NotFound)?;
    Ok(serde_json::from_value(value)?)
}

async fn save_tx(
    tx: &mut Transaction<'_, Postgres>,
    request: &LinkRequest,
) -> Result<(), LinkError> {
    sqlx::query("UPDATE mini_material_link_requests SET status=$2,revision=$3,payload_json=$4,retry_at=now() WHERE id=$1")
        .bind(&request.request_id).bind(&request.status).bind(request.event_sequence)
        .bind(serde_json::to_value(request)?).execute(&mut **tx).await?;
    Ok(())
}

async fn reconcile_tx(
    tx: &mut Transaction<'_, Postgres>,
    request: &mut LinkRequest,
) -> Result<(), LinkError> {
    if request.status != "pending" {
        return Ok(());
    }
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM mini_production_maps map WHERE id=$1
         AND lifecycle_status IN ('released','in_progress')
         AND NOT EXISTS(SELECT 1 FROM mini_order_control_states c WHERE c.order_id=map.id AND c.state <> 'active'))")
        .bind(&request.order_id).fetch_one(&mut **tx).await?;
    let candidates: Vec<LinkCandidate> = sqlx::query_as(CANDIDATES_SQL)
        .bind(&request.apparatus_id)
        .fetch_all(&mut **tx)
        .await?;
    let stale = request.candidates.is_empty()
        || request.candidates.iter().any(|old| {
            !candidates.iter().any(|now| {
                now.stock_id == old.stock_id
                    && now.barcode == old.barcode
                    && now.placement_version == old.placement_version
                    && now.location_id == old.location_id
                    && now.mover_ref == old.mover_ref
            })
        });
    if !active {
        request.finish("stale", "Order holati o‘zgardi. So‘rov yopildi.");
    } else if stale {
        request.finish(
            "stale",
            "Rulon boshqa orderga ulangan yoki joyi/holati o‘zgargan. So‘rov yopildi.",
        );
    } else if time::OffsetDateTime::now_utc().unix_timestamp() >= request.expires_at_unix {
        request.finish(
            "expired",
            "So‘rov muddati tugadi. Zarur bo‘lsa qayta yuboring.",
        );
    } else {
        return Ok(());
    }
    save_tx(tx, request).await
}
