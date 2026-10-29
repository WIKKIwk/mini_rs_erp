use super::*;

const BASE: &str = include_str!("production_journal.sql");
const ORDER_JSON: &str = "jsonb_build_object('order_id',o.order_id,'order_code',o.order_code,
    'order_title',o.order_title,'customer_name',o.customer_name,'active_batch_count',o.active_batch_count,
    'batch_count',o.batch_count,'roll_count',o.roll_count,'total_net_kg',o.total_net_kg::text,
    'total_length_m',o.total_length_m::text,'last_activity_at',o.last_activity_at)";
const BATCH_JSON: &str = "jsonb_build_object('batch_id',b.batch_id,'order_id',b.order_id,
    'item_code',b.item_code,'item_name',b.item_name,'warehouse',b.warehouse,'active',b.active,
    'started_at',b.started_at,'ended_at',b.ended_at,'last_activity_at',b.last_activity_at,
    'roll_count',b.roll_count,'total_net_kg',b.total_net_kg::text,'total_length_m',b.total_length_m::text,
    'preparation_formula',b.preparation_formula)";

impl PostgresPreparationStore {
    pub async fn production_journal_orders(
        &self,
        owner: &str,
        search: &str,
        status: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Value, PreparationError> {
        let limit = limit.clamp(1, 100);
        let rows: Vec<Value> = sqlx::query_scalar(&format!(
            "{BASE} SELECT {ORDER_JSON} FROM journal_orders o
             WHERE ($2='' OR position(lower($2) IN lower(concat_ws(' ',order_code,order_title,customer_name,materials_search)))>0)
             AND ($3='all' OR ($3='active' AND active_batch_count>0) OR ($3='completed' AND active_batch_count=0))
             ORDER BY last_activity_at DESC,order_id LIMIT $4 OFFSET $5"
        )).bind(owner).bind(search.trim()).bind(status).bind((limit+1) as i64).bind(offset as i64)
            .fetch_all(&self.pool).await?;
        Ok(page("orders", rows, offset, limit))
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
        let order: Option<Value> = sqlx::query_scalar(&format!(
            "{BASE} SELECT {ORDER_JSON} FROM journal_orders o WHERE order_id=$2"
        ))
        .bind(owner)
        .bind(order_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(order) = order else { return Ok(None) };
        let batches: Vec<Value> = sqlx::query_scalar(&format!(
            "{BASE} SELECT {BATCH_JSON} FROM journal_batches b WHERE order_id=$2
             ORDER BY last_activity_at DESC,batch_id LIMIT $3 OFFSET $4"
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
        let batch: Option<Value> = sqlx::query_scalar(&format!(
            "{BASE} SELECT {BATCH_JSON} FROM journal_batches b WHERE batch_id=$2"
        ))
        .bind(owner)
        .bind(batch_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(batch) = batch else { return Ok(None) };
        let rolls: Vec<Value> = sqlx::query_scalar(&format!(
            "{BASE} SELECT jsonb_build_object('epc',epc,'receipt_name',receipt_name,
                'item_code',item_code,'item_name',item_name,'warehouse',warehouse,
                'net_kg',net_kg::text,'gross_kg',gross_kg::text,'length_m',length_m::text,
                'width_mm',width_mm::text,'micron',micron::text,'printed_at',printed_at,
                'preparation_formula',preparation_formula)
             FROM rolls WHERE batch_id=$2 ORDER BY printed_at DESC,epc LIMIT $3 OFFSET $4"
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
