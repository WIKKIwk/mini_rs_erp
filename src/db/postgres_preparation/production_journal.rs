use super::*;
use super::output_notes::output_note_json;
use super::production_notes::note_json_sql;

pub(super) const BASE: &str = include_str!("production_journal.sql");
pub(crate) const MAX_JOURNAL_WINDOW: usize = 10_000;
const ORDER_JSON: &str = "jsonb_build_object('order_id',o.order_id,'order_code',o.order_code,
    'order_title',o.order_title,'customer_name',o.customer_name,'active_batch_count',o.active_batch_count,
    'batch_count',o.batch_count,'roll_count',o.roll_count,'total_net_kg',o.total_net_kg::text,
    'total_length_m',o.total_length_m::text,'last_activity_at',o.last_activity_at)";
const BATCH_JSON: &str = "jsonb_build_object('batch_id',b.batch_id,'order_id',b.order_id,
    'batch_code',b.batch_code,'order_code',COALESCE(o.order_code,''),'order_title',COALESCE(o.order_title,''),
    'item_code',b.item_code,'item_name',b.item_name,'warehouse',b.warehouse,'active',b.active,
    'started_at',b.started_at,'ended_at',b.ended_at,'last_activity_at',b.last_activity_at,
    'roll_count',b.roll_count,'total_net_kg',b.total_net_kg::text,'total_length_m',b.total_length_m::text,
    'preparation_formula',b.preparation_formula)";

const BATCH_NOTE_JOIN: &str = "LEFT JOIN mini_preparation_output_notes bn ON bn.owner_ref=$1
    AND bn.target_kind='batch' AND bn.target_id=b.batch_id";
const ROLL_NOTE_JOINS: &str = "LEFT JOIN mini_preparation_output_notes bn ON bn.owner_ref=$1
    AND bn.target_kind='batch' AND bn.target_id=r.recorded_batch_id
    LEFT JOIN mini_preparation_output_notes rn ON rn.owner_ref=$1
    AND rn.target_kind='roll' AND rn.target_id=r.receipt_name";

fn roll_json() -> String {
    format!("jsonb_build_object('epc',r.epc,'receipt_name',r.receipt_name,
        'batch_id',r.batch_id,'order_id',r.order_id,'order_code',COALESCE(o.order_code,''),
        'order_title',COALESCE(o.order_title,''),'item_code',r.item_code,'item_name',r.item_name,'warehouse',r.warehouse,
        'net_kg',r.net_kg::text,'gross_kg',r.gross_kg::text,'length_m',r.length_m::text,
        'width_mm',r.width_mm::text,'micron',r.micron::text,'printed_at',r.printed_at,
        'preparation_formula',r.preparation_formula,
        'on_paddon', EXISTS (
            SELECT 1 FROM mini_raw_material_stock stock
            JOIN mini_raw_material_paddon_units unit ON unit.stock_id=stock.id
                AND unit.issue_id IS NULL AND unit.prepared_roll
            WHERE lower(stock.barcode)=lower(r.epc)
                AND stock.source_receipt_id=r.receipt_name AND stock.status='on_paddon' AND stock.qty>0
        ),
        'batch_note',{},'roll_note',{})", output_note_json("bn"), output_note_json("rn"))
}

impl PostgresPreparationStore {
    pub async fn owns_submitted_production_receipt(
        &self,
        owner: &str,
        barcode: &str,
    ) -> Result<bool, PreparationError> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM mini_gscale_receipts
             WHERE lower(barcode) = lower($2) AND status = 'submitted'
               AND payload_json->>'actor_role' = 'tayyorlov_masteri'
               AND payload_json->>'actor_ref' = $1)",
        )
        .bind(owner.trim())
        .bind(barcode.trim())
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn production_journal_orders(
        &self,
        owner: &str,
        search: &str,
        status: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Value, PreparationError> {
        let limit = limit.clamp(1, MAX_JOURNAL_WINDOW);
        let note = note_json_sql("o.order_id");
        let rows: Vec<Value> = sqlx::query_scalar(&format!(
            "{BASE} SELECT {ORDER_JSON} || jsonb_build_object('preparation_note',{note}) FROM journal_orders o
             LEFT JOIN mini_preparation_production_order_notes note ON note.order_id=o.order_id
             WHERE ($2='' OR position(lower($2) IN lower(concat_ws(' ',o.order_id,order_code,order_title,customer_name,materials_search)))>0
                OR EXISTS (SELECT 1 FROM rolls r WHERE r.order_id=o.order_id AND position(lower($2) IN lower(r.epc))>0))
             AND ($3='all' OR ($3='active' AND active_batch_count>0) OR ($3='completed' AND active_batch_count=0))
             ORDER BY last_activity_at DESC,o.order_id LIMIT $4 OFFSET $5"
        )).bind(owner).bind(search.trim()).bind(status).bind((limit+1) as i64).bind(offset as i64)
            .fetch_all(&self.pool).await?;
        Ok(page("orders", rows, offset, limit))
    }

    pub async fn production_journal_batches(
        &self, owner: &str, search: &str, status: &str, offset: usize, limit: usize,
    ) -> Result<Value, PreparationError> {
        let limit = limit.clamp(1, MAX_JOURNAL_WINDOW);
        let note = output_note_json("bn");
        let rows: Vec<Value> = sqlx::query_scalar(&format!(
            "{BASE} SELECT {BATCH_JSON} || jsonb_build_object('batch_note',{note})
             FROM production_batches b LEFT JOIN journal_orders o ON o.order_id=b.order_id
             {BATCH_NOTE_JOIN}
             WHERE starts_with(b.batch_id,'batch:')
             AND ($2='' OR position(lower($2) IN lower(concat_ws(' ',b.batch_code,b.item_code,b.item_name,
                 b.order_id,o.order_code,o.order_title,o.customer_name,b.preparation_formula->>'variant_name')))>0
                 OR EXISTS (SELECT 1 FROM rolls r WHERE r.batch_id=b.batch_id AND position(lower($2) IN lower(r.epc))>0))
             AND ($3='all' OR ($3='active' AND b.active) OR ($3='completed' AND NOT b.active))
             ORDER BY b.last_activity_at DESC,b.batch_id LIMIT $4 OFFSET $5"
        )).bind(owner).bind(search.trim()).bind(status).bind((limit+1) as i64).bind(offset as i64)
            .fetch_all(&self.pool).await?;
        Ok(page("batches", rows, offset, limit))
    }

    pub async fn production_journal_rolls(
        &self, owner: &str, search: &str, status: &str, offset: usize, limit: usize,
    ) -> Result<Value, PreparationError> {
        let limit = limit.clamp(1, MAX_JOURNAL_WINDOW);
        let roll = roll_json();
        let rows: Vec<Value> = sqlx::query_scalar(&format!(
            "{BASE} SELECT {roll} FROM rolls r LEFT JOIN journal_orders o ON o.order_id=r.order_id
             LEFT JOIN batches b ON b.batch_id=r.recorded_batch_id {ROLL_NOTE_JOINS}
             WHERE ($2='' OR position(lower($2) IN lower(concat_ws(' ',r.epc,r.item_code,r.item_name,r.order_id,
                    o.order_code,o.order_title,o.customer_name,r.preparation_formula->>'variant_name',b.batch_code)))>0)
             AND ($3='all' OR ($3='active' AND b.active) OR ($3='completed' AND NOT COALESCE(b.active,false)))
             ORDER BY r.printed_at DESC,r.epc LIMIT $4 OFFSET $5"
        )).bind(owner).bind(search.trim()).bind(status).bind((limit+1) as i64).bind(offset as i64)
            .fetch_all(&self.pool).await?;
        Ok(page("rolls", rows, offset, limit))
    }

    pub async fn production_journal_order(
        &self,
        owner: &str,
        order_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Option<Value>, PreparationError> {
        let limit = limit.clamp(1, 100);
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let note = note_json_sql("o.order_id");
        let order: Option<Value> = sqlx::query_scalar(&format!(
            "{BASE} SELECT {ORDER_JSON} || jsonb_build_object('preparation_note',{note}) FROM journal_orders o
             LEFT JOIN mini_preparation_production_order_notes note ON note.order_id=o.order_id WHERE o.order_id=$2"
        ))
        .bind(owner)
        .bind(order_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(order) = order else { return Ok(None) };
        let note = note_json_sql("b.order_id");
        let batch_note = output_note_json("bn");
        let batches: Vec<Value> = sqlx::query_scalar(&format!(
            "{BASE} SELECT {BATCH_JSON} || jsonb_build_object('preparation_note',{note},'batch_note',{batch_note})
             FROM journal_batches b LEFT JOIN journal_orders o ON o.order_id=b.order_id
             LEFT JOIN mini_preparation_production_order_notes note ON note.order_id=b.order_id {BATCH_NOTE_JOIN}
             WHERE b.order_id=$2 ORDER BY b.last_activity_at DESC,b.batch_id LIMIT $3 OFFSET $4"
        ))
        .bind(owner)
        .bind(order_id)
        .bind((limit + 1) as i64)
        .bind(offset as i64)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        let mut result = page("batches", batches, offset, limit);
        result["order"] = order;
        Ok(Some(result))
    }

    pub async fn production_journal_batch(
        &self,
        owner: &str,
        batch_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Option<Value>, PreparationError> {
        let limit = limit.clamp(1, 100);
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let note = note_json_sql("b.order_id");
        let batch_note = output_note_json("bn");
        let batch: Option<Value> = sqlx::query_scalar(&format!(
            "{BASE} SELECT {BATCH_JSON} || jsonb_build_object('preparation_note',CASE WHEN b.order_id<>'' THEN {note} END,
                'batch_note',{batch_note}) FROM production_batches b LEFT JOIN journal_orders o ON o.order_id=b.order_id
             LEFT JOIN mini_preparation_production_order_notes note ON note.order_id=b.order_id {BATCH_NOTE_JOIN}
             WHERE b.batch_id=$2"
        ))
        .bind(owner)
        .bind(batch_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(batch) = batch else { return Ok(None) };
        let note = note_json_sql("r.order_id");
        let roll = roll_json();
        let rolls: Vec<Value> = sqlx::query_scalar(&format!(
            "{BASE} SELECT {roll} || jsonb_build_object('preparation_note',CASE WHEN r.order_id<>'' THEN {note} END)
             FROM rolls r LEFT JOIN journal_orders o ON o.order_id=r.order_id
             LEFT JOIN mini_preparation_production_order_notes note ON note.order_id=r.order_id {ROLL_NOTE_JOINS}
             WHERE r.batch_id=$2 OR r.order_batch_id=$2 ORDER BY r.printed_at DESC,r.epc LIMIT $3 OFFSET $4"
        ))
        .bind(owner)
        .bind(batch_id)
        .bind((limit + 1) as i64)
        .bind(offset as i64)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        let mut result = page("rolls", rolls, offset, limit);
        result["batch"] = batch;
        Ok(Some(result))
    }
}

fn page(key: &str, mut rows: Vec<Value>, offset: usize, limit: usize) -> Value {
    let has_more = rows.len() > limit;
    rows.truncate(limit);
    let next_offset = offset + rows.len();
    let mut result =
        json!({"has_more":has_more,"next_offset":if has_more {Some(next_offset)} else {None}});
    result[key] = json!(rows);
    result
}
