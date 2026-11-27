use super::*;
use crate::core::auth::models::PrincipalRole;

use super::transfer_receipt_scope::pending_lots_sql;

pub(in crate::db) async fn pending_counts(
    pool: &PgPool,
    transfer_ids: &[String],
    owner: &str,
) -> Result<BTreeMap<String, i64>, sqlx::Error> {
    if transfer_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let rows = sqlx::query_as(&format!(
        "SELECT transfer_id, count(*) FROM ({}) pending GROUP BY transfer_id",
        pending_lots_sql("SELECT unnest($1::text[])")
    ))
    .bind(transfer_ids)
    .bind(owner)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

impl PostgresPreparationStore {
    pub async fn accept_received_transfer(
        &self,
        actor: &Principal,
        transfer_id: &str,
    ) -> Result<Value, PreparationError> {
        if actor.role != PrincipalRole::TayyorlovMasteri {
            return Err(PreparationError::Forbidden);
        }
        let mut tx = self.pool.begin().await?;
        let warehouse: String = sqlx::query_scalar(
            "SELECT w.name FROM mini_inventory_transfers t
             JOIN mini_warehouses w ON w.id=t.destination_warehouse_id
             WHERE t.id=$1 AND t.status='received' FOR UPDATE OF t",
        )
        .bind(transfer_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(PreparationError::Conflict(
            "Qabul qilingan transfer topilmadi",
        ))?;
        let warehouse = assigned_warehouse(&mut tx, &actor.ref_, &warehouse).await?;
        // Keep material scope edits from changing eligibility between the check and the receipt.
        sqlx::query("LOCK TABLE mini_items, mini_preparation_materials IN ROW SHARE MODE")
            .execute(&mut *tx)
            .await?;

        let ids = [transfer_id.to_owned()];
        let pending_sql = pending_lots_sql("SELECT unnest($1::text[])");
        let locked: Vec<(String, String)> =
            sqlx::query_as(&format!("{pending_sql} ORDER BY s.id FOR UPDATE OF s"))
                .bind(&ids[..])
                .bind(&actor.ref_)
                .fetch_all(&mut *tx)
                .await?;
        // A concurrent placement, reservation, or receipt can have changed while waiting.
        let current: Vec<(String, String)> = sqlx::query_as(&pending_sql)
            .bind(&ids[..])
            .bind(&actor.ref_)
            .fetch_all(&mut *tx)
            .await?;
        let mut accepted = false;
        for (id, stock_id) in locked {
            if current.contains(&(id, stock_id.clone())) {
                paddons::received_preparation_lot(
                    &mut tx,
                    actor,
                    &stock_id,
                    &warehouse,
                    Some(transfer_id),
                )
                .await?
                .ok_or(PreparationError::Conflict("Transferdagi Seriyo o‘zgargan"))?;
                accepted = true;
            }
        }
        if !accepted {
            let recorded: bool = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM mini_preparation_operations
                    WHERE owner_ref=$1 AND kind='receipt'
                      AND request_json->>'kind'='warehouse_transfer'
                      AND request_json->>'transfer_id'=$2
                )",
            )
            .bind(&actor.ref_)
            .bind(transfer_id)
            .fetch_one(&mut *tx)
            .await?;
            if !recorded {
                return Err(PreparationError::Conflict(
                    "Tayyorlovga qabul qilinadigan Seriyo qolmagan",
                ));
            }
        }
        tx.commit().await?;
        Ok(json!({"transfer_id":transfer_id,"warehouse":warehouse}))
    }
}
