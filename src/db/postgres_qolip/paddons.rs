//! Durable Qolip package and archive lifecycle. Home stock remains reserved;
//! database guards prevent using it while packaged or archived.
use crate::core::auth::models::Principal;
use crate::core::qolip::{
    QolipStorageAction as Action, QolipStorageError as Error, QolipStorageQuery as Query, role_code,
};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction};
type Tx<'a> = Transaction<'a, Postgres>;
fn db(error: sqlx::Error) -> Error {
    if let Some(e) = error.as_database_error() {
        let message = e.message();
        for code in [
            "qolip_archived",
            "qolip_in_paddon",
            "qolip_in_rework",
            "qolip_history_locked",
        ] {
            if message.contains(code) {
                return Error::Conflict(code);
            }
        }
        if matches!(
            e.code().as_deref(),
            Some("23505" | "23503" | "23514" | "40001" | "40P01")
        ) {
            return Error::Conflict("qolip_storage_changed");
        }
    }
    tracing::error!(error=%error,"qolip storage failure");
    Error::Store
}
fn id(prefix: &str) -> String {
    format!("{prefix}:{:032x}", rand::random::<u128>())
}
fn allowed_names(allowed: Option<&[String]>) -> Option<Vec<String>> {
    allowed.map(|a| a.iter().map(|s| s.trim().to_lowercase()).collect())
}
fn owner(warehouse: &str, allowed: Option<&[String]>) -> Result<(), Error> {
    if warehouse.trim().is_empty()
        || allowed.is_some_and(|a| {
            !a.iter()
                .any(|w| w.trim().eq_ignore_ascii_case(warehouse.trim()))
        })
    {
        Err(Error::Forbidden)
    } else {
        Ok(())
    }
}
const PADDON_SELECT: &str = "SELECT to_jsonb(p) || jsonb_build_object('locked',p.locked_at IS NOT NULL,'archived',p.archived_at IS NOT NULL,'archive_entry_id',(SELECT a.id FROM mini_qolip_archive_entries a WHERE a.kind='paddon' AND a.code=p.code AND a.restored_at IS NULL),'item_count',(SELECT count(*) FROM mini_qolip_paddon_members m WHERE m.paddon_code=p.code AND m.removed_at IS NULL),'total_quantity',(SELECT COALESCE(sum(GREATEST(COALESCE((SELECT sum(l.quantity) FROM mini_qolip_locations l WHERE lower(btrim(l.qolip_code))=m.qolip_code_key),0),1)),0) FROM mini_qolip_paddon_members m WHERE m.paddon_code=p.code AND m.removed_at IS NULL)) FROM mini_qolip_paddons p";
const MOLD_SELECT: &str = r#"SELECT s.payload_json || jsonb_build_object(
'qolip_code',s.qolip_code,'qolip_set_id',COALESCE(s.qolip_set_id,''),'item_code',s.item_code,'item_name',s.item_name,'item_group',s.item_group,'size',s.size,
'warehouse',COALESCE(s.payload_json->>'warehouse',''),'color',COALESCE(s.payload_json->>'color',''),
'products',COALESCE((SELECT jsonb_agg(jsonb_build_object('code',i.code,'name',i.name) ORDER BY i.code) FROM mini_qolip_effective_product_molds e JOIN mini_items i ON i.code=e.item_code WHERE e.qolip_code_key=lower(btrim(s.qolip_code))),'[]'::jsonb),
'paddon_code',COALESCE(m.paddon_code,''),'archived',(p.archived_at IS NOT NULL OR EXISTS(SELECT 1 FROM mini_qolip_archive_entries a WHERE a.kind='qolip' AND a.code_key=lower(btrim(s.qolip_code)) AND a.restored_at IS NULL)),
'is_in_rework',EXISTS(SELECT 1 FROM mini_qolip_reworks r WHERE r.qolip_code_key=lower(btrim(s.qolip_code)) AND r.returned_at IS NULL),
'is_custody_mismatch',EXISTS(SELECT 1 FROM mini_qolip_locations l WHERE lower(btrim(l.qolip_code))=lower(btrim(s.qolip_code)) AND lower(btrim(l.warehouse))<>lower(btrim(COALESCE(s.payload_json->>'warehouse','')))),
'is_transferring',EXISTS(SELECT 1 FROM mini_qolip_locations l WHERE lower(btrim(l.qolip_code))=lower(btrim(s.qolip_code)) AND btrim(l.inventory_transfer_id)<>''),
'is_in_use',(EXISTS(SELECT 1 FROM mini_qolip_checkouts c WHERE lower(btrim(c.qolip_code))=lower(btrim(s.qolip_code)) AND c.status='open') OR EXISTS(SELECT 1 FROM mini_order_run_sessions r WHERE r.status IN ('active','paused','frozen','roll_detached') AND r.payload_json->>'qolip_lock_owner'='true' AND (lower(btrim(r.payload_json->>'qolip_code'))=lower(btrim(s.qolip_code)) OR EXISTS(SELECT 1 FROM jsonb_array_elements_text(CASE WHEN jsonb_typeof(r.payload_json->'qolip_codes')='array' THEN r.payload_json->'qolip_codes' ELSE '[]'::jsonb END) c(value) WHERE lower(btrim(c.value))=lower(btrim(s.qolip_code)))))),
'quantity',GREATEST(COALESCE((SELECT sum(l.quantity) FROM mini_qolip_locations l WHERE lower(btrim(l.qolip_code))=lower(btrim(s.qolip_code))),0),1),
'locations',COALESCE((SELECT jsonb_agg(to_jsonb(l) ORDER BY l.id) FROM mini_qolip_locations l WHERE lower(btrim(l.qolip_code))=lower(btrim(s.qolip_code)) AND lower(btrim(l.warehouse))=lower(btrim(COALESCE(s.payload_json->>'warehouse','')))),'[]'::jsonb))
FROM mini_qolip_product_specs s
LEFT JOIN mini_qolip_paddon_members m ON m.qolip_code_key=lower(btrim(s.qolip_code)) AND m.removed_at IS NULL
LEFT JOIN mini_qolip_paddons p ON p.code=m.paddon_code
WHERE ($1::text[] IS NULL OR lower(btrim(s.qolip_code))=ANY($1))
AND ($2::text[] IS NULL OR lower(btrim(s.payload_json->>'warehouse'))=ANY($2))
AND ($3='' OR lower(s.qolip_code) LIKE $3 OR lower(s.item_name) LIKE $3 OR lower(COALESCE(s.qolip_set_id,'')) LIKE $3 OR EXISTS(SELECT 1 FROM mini_qolip_effective_product_molds e JOIN mini_items i ON i.code=e.item_code WHERE e.qolip_code_key=lower(btrim(s.qolip_code)) AND (lower(i.name) LIKE $3 OR lower(i.code) LIKE $3)))
ORDER BY lower(s.qolip_code) LIMIT $4 OFFSET $5"#;
async fn molds(
    tx: &mut Tx<'_>,
    codes: Option<&[String]>,
    allowed: Option<&[String]>,
    q: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<Value>, Error> {
    sqlx::query_scalar::<_, Value>(MOLD_SELECT)
        .bind(codes)
        .bind(allowed_names(allowed))
        .bind(if q.is_empty() {
            String::new()
        } else {
            format!("%{}%", q.to_lowercase())
        })
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&mut **tx)
        .await
        .map_err(db)
}
async fn paddon(tx: &mut Tx<'_>, code: &str, allowed: Option<&[String]>) -> Result<Value, Error> {
    let p: Value = sqlx::query_scalar(&format!("{PADDON_SELECT} WHERE p.code=$1"))
        .bind(code)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db)?
        .ok_or(Error::NotFound)?;
    owner(p["warehouse"].as_str().unwrap_or(""), allowed)?;
    Ok(p)
}
async fn snapshot(tx: &mut Tx<'_>, code: &str, allowed: Option<&[String]>) -> Result<Value, Error> {
    let p = paddon(tx, code, allowed).await?;
    let codes:Vec<String>=sqlx::query_scalar("SELECT qolip_code_key FROM mini_qolip_paddon_members WHERE paddon_code=$1 AND removed_at IS NULL ORDER BY qolip_code_key").bind(code).fetch_all(&mut **tx).await.map_err(db)?;
    let items = molds(tx, Some(&codes), allowed, "", 1001, 0).await?;
    if items.len() != codes.len() {
        return Err(Error::Conflict("qolip_storage_identity_changed"));
    }
    let history:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(e) FROM mini_qolip_paddon_events e WHERE paddon_code=$1 ORDER BY created_at DESC,id DESC LIMIT 201").bind(code).fetch_all(&mut **tx).await.map_err(db)?;
    let more = history.len() > 200;
    Ok(
        json!({"ok":true,"paddon":p,"items":items,"history":history.into_iter().take(200).collect::<Vec<_>>(),"history_has_more":more}),
    )
}
pub async fn read(
    pool: &PgPool,
    kind: &str,
    query: &Query,
    allowed: Option<&[String]>,
) -> Result<Value, Error> {
    query.validate()?;
    let selected_warehouses = if query.warehouse.trim().is_empty() {
        None
    } else {
        owner(query.warehouse.trim(), allowed)?;
        Some(vec![query.warehouse.trim().to_owned()])
    };
    let allowed = selected_warehouses.as_deref().or(allowed);
    let mut tx = pool.begin().await.map_err(db)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    let limit = query.limit.unwrap_or(100).clamp(1, 200);
    let offset = query.offset;
    let value = match kind {
        "detail" => snapshot(&mut tx, &query.code.trim().to_uppercase(), allowed).await?,
        "molds" => {
            let codes = (!query.code.trim().is_empty()).then(|| {
                vec![
                    crate::core::qolip::qolip_scan_code_from_qr(&query.code)
                        .trim()
                        .to_lowercase(),
                ]
            });
            let mut rows = molds(
                &mut tx,
                codes.as_deref(),
                allowed,
                query.q.trim(),
                limit + 1,
                offset,
            )
            .await?;
            let more = rows.len() > limit;
            rows.truncate(limit);
            json!({"ok":true,"molds":rows,"has_more":more,"next_offset":if more{Some(offset+limit)}else{None}})
        }
        "paddons" => {
            let mut rows:Vec<Value>=sqlx::query_scalar(&format!("{PADDON_SELECT} WHERE (p.archived_at IS NOT NULL)=$1 AND ($2::text[] IS NULL OR lower(btrim(p.warehouse))=ANY($2)) AND ($3='' OR lower(p.code) LIKE $3 OR lower(p.location) LIKE $3 OR lower(p.note) LIKE $3) ORDER BY p.created_at DESC,p.code DESC LIMIT $4 OFFSET $5")).bind(query.archived).bind(allowed_names(allowed)).bind(if query.q.is_empty(){String::new()}else{format!("%{}%",query.q.trim().to_lowercase())}).bind((limit+1)as i64).bind(offset as i64).fetch_all(&mut *tx).await.map_err(db)?;
            let more = rows.len() > limit;
            rows.truncate(limit);
            json!({"ok":true,"paddons":rows,"has_more":more,"next_offset":if more{Some(offset+limit)}else{None}})
        }
        "archive" => {
            let mut rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(a) FROM mini_qolip_archive_entries a WHERE ($1='all' OR ($1='returned' AND restored_at IS NOT NULL) OR ($1='open' AND restored_at IS NULL)) AND ($2::text[] IS NULL OR lower(btrim(warehouse))=ANY($2)) AND ($3='' OR lower(code) LIKE $3 OR lower(snapshot::text) LIKE $3) AND ($6::date IS NULL OR archived_at >= ($6::date::timestamp AT TIME ZONE 'Asia/Tashkent')) AND ($7::date IS NULL OR archived_at < (($7::date + 1)::timestamp AT TIME ZONE 'Asia/Tashkent')) ORDER BY archived_at DESC,id DESC LIMIT $4 OFFSET $5").bind(if query.status.is_empty(){"open"}else{&query.status}).bind(allowed_names(allowed)).bind(if query.q.is_empty(){String::new()}else{format!("%{}%",query.q.trim().to_lowercase())}).bind((limit+1)as i64).bind(offset as i64).bind(query.from.as_deref()).bind(query.to.as_deref()).fetch_all(&mut *tx).await.map_err(db)?;
            let more = rows.len() > limit;
            rows.truncate(limit);
            json!({"ok":true,"entries":rows,"has_more":more,"next_offset":if more{Some(offset+limit)}else{None}})
        }
        _ => return Err(Error::NotFound),
    };
    tx.commit().await.map_err(db)?;
    Ok(value)
}
include!("paddons/actions.rs");
include!("paddons/archive.rs");

#[cfg(test)]
#[path = "paddons/tests.rs"]
mod tests;
