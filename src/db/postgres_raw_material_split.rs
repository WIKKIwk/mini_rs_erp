use crate::core::gscale::{epc::GscaleEpcGenerator, ports::EpcSource};
use crate::core::{auth::models::Principal, raw_material_split::*};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};

impl From<sqlx::Error> for SplitError {
    fn from(error: sqlx::Error) -> Self {
        tracing::error!(%error, "raw material split storage failed");
        Self::StoreFailed
    }
}
// Identical stock eligibility to other inventory consumers; row locks serialize
// splitting with assignment, correction, transfer and physical movement.
const AVAILABLE: &str = "s.status='available' AND s.reserved_order_id='' AND s.uom='kg'
 AND s.qty > 0 AND s.width_mm > 0 AND s.micron > 0
 AND btrim(COALESCE(s.payload_json->>'inventory_transfer_id',''))=''
 AND NOT EXISTS(SELECT 1 FROM mini_raw_material_assignments a WHERE lower(a.barcode)=lower(s.barcode))
 AND NOT EXISTS(SELECT 1 FROM mini_inventory_placements p
 JOIN mini_inventory_locations l ON l.id=p.physical_location_id
 LEFT JOIN mini_warehouses w ON w.id=l.warehouse_id
 WHERE p.asset_kind='raw_material' AND lower(p.asset_ref)=lower(s.id)
 AND (l.kind<>'warehouse' OR w.name IS NULL OR lower(w.name)<>lower(s.warehouse)))";
const SOURCE_JSON: &str = "jsonb_build_object('stock_id',s.id,'barcode',s.barcode,
 'revision',extract(epoch FROM s.updated_at)::text,
 'warehouse',s.warehouse,'item_code',s.item_code,'item_name',s.item_name,
 'kg',trim_scale(s.qty)::text,'width_mm',trim_scale(s.width_mm)::text,'micron',trim_scale(s.micron)::text)";
const SCOPE: &str = "EXISTS(SELECT 1 FROM mini_warehouse_assignments a JOIN mini_warehouses w
 ON lower(w.name)=lower(a.warehouse_name) WHERE a.assignment_kind='warehouse'
 AND a.principal_role='homashyo_rezkachi' AND a.principal_ref=$1
 AND NOT w.is_group AND lower(w.name)=lower(s.warehouse))";

#[derive(Clone)]
pub struct PostgresRawMaterialSplitStore {
    pool: PgPool,
}
impl PostgresRawMaterialSplitStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
    pub async fn snapshot(&self, owner: &str) -> Result<Value, SplitError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let warehouses: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT w.name FROM mini_warehouses w
            JOIN mini_warehouse_assignments a ON lower(a.warehouse_name)=lower(w.name)
            WHERE a.assignment_kind='warehouse' AND a.principal_role='homashyo_rezkachi'
            AND a.principal_ref=$1 AND NOT w.is_group ORDER BY w.name",
        )
        .bind(owner)
        .fetch_all(&mut *tx)
        .await?;
        let history: Vec<Value>=sqlx::query_scalar("SELECT response_json || jsonb_build_object('created_at',created_at)
            FROM mini_raw_material_splits WHERE owner_ref=$1 ORDER BY created_at DESC,id DESC LIMIT 100")
            .bind(owner).fetch_all(&mut *tx).await?;
        let issues: Vec<Value> = sqlx::query_scalar(
            "SELECT response_json FROM mini_raw_material_split_issues
            WHERE owner_ref=$1 ORDER BY created_at DESC,id DESC LIMIT 100",
        )
        .bind(owner)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(json!({"warehouses":warehouses,"history":history,"issues":issues}))
    }
    pub async fn source(&self, owner: &str, barcode: &str) -> Result<Value, SplitError> {
        sqlx::query_scalar(&format!(
            "SELECT {SOURCE_JSON} FROM mini_raw_material_stock s
            WHERE lower(s.barcode)=lower($2) AND {SCOPE} AND {AVAILABLE}"
        ))
        .bind(owner)
        .bind(barcode.trim())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(SplitError::Forbidden)
    }
    pub async fn saved(&self, owner: &str, id: &str) -> Result<Value, SplitError> {
        sqlx::query_scalar(
            "SELECT response_json FROM mini_raw_material_splits WHERE owner_ref=$1 AND id=$2",
        )
        .bind(owner)
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(SplitError::Forbidden)
    }
    pub async fn split(&self, actor: &Principal, input: SplitCreate) -> Result<Value, SplitError> {
        if actor.role != crate::core::auth::models::PrincipalRole::HomashyoRezkachi {
            return Err(SplitError::Forbidden);
        }
        let request = json!(&input);
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout='5s'")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!(
                "raw-material-split:{}:{}",
                actor.ref_, input.request_id
            ))
            .execute(&mut *tx)
            .await?;
        if let Some(row)=sqlx::query("SELECT request_json,response_json FROM mini_raw_material_splits WHERE owner_ref=$1 AND request_id=$2")
            .bind(&actor.ref_).bind(&input.request_id).fetch_optional(&mut *tx).await? {
            if row.try_get::<Value,_>("request_json")? != request {
                return Err(SplitError::Conflict("So‘rov raqami boshqa operatsiyada ishlatilgan"));
            }
            return Ok(row.try_get("response_json")?);
        }
        // Legacy saved commands remain replayable, but every new operation must
        // carry measured gross/core weights and an explicitly entered waste.
        let (source_kg, waste, quantities) = input.validate()?;
        // Lock before testing availability so concurrent consumers see the latest committed state.
        let row = sqlx::query(&format!(
            "SELECT {SOURCE_JSON} AS source,s.source_receipt_id,s.payload_json
            FROM mini_raw_material_stock s WHERE lower(s.barcode)=lower($1) FOR UPDATE OF s"
        ))
        .bind(input.source_barcode.trim())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(SplitError::Forbidden)?;
        let source: Value = row.try_get("source")?;
        let parent = source["stock_id"].as_str().ok_or(SplitError::StoreFailed)?;
        let warehouse = source["warehouse"]
            .as_str()
            .ok_or(SplitError::StoreFailed)?;
        let authorized: Option<String> = sqlx::query_scalar(
            "SELECT w.name FROM mini_warehouses w
            JOIN mini_warehouse_assignments a ON lower(a.warehouse_name)=lower(w.name)
            WHERE a.assignment_kind='warehouse' AND a.principal_role='homashyo_rezkachi'
            AND a.principal_ref=$1 AND lower(w.name)=lower($2) AND NOT w.is_group FOR SHARE OF w,a",
        )
        .bind(&actor.ref_)
        .bind(warehouse)
        .fetch_optional(&mut *tx)
        .await?;
        if authorized.is_none() {
            return Err(SplitError::Forbidden);
        }
        let available: bool = sqlx::query_scalar(&format!(
            "SELECT EXISTS(SELECT 1 FROM mini_raw_material_stock s WHERE s.id=$1 AND {AVAILABLE})"
        ))
        .bind(parent)
        .fetch_one(&mut *tx)
        .await?;
        if !available {
            return Err(SplitError::Conflict(
                "Rulon band, ko‘chirilmoqda yoki allaqachon ishlatilgan",
            ));
        }
        if source["revision"].as_str() != Some(input.expected_revision.as_str()) {
            return Err(SplitError::Conflict(
                "Rulon ma’lumotlari o‘zgargan. Qayta skanerlang",
            ));
        }
        for (field, expected) in [
            ("kg", &input.expected_kg),
            ("width_mm", &input.expected_width_mm),
            ("micron", &input.expected_micron),
        ] {
            if quantity(source[field].as_str().unwrap_or(""), false)? != quantity(expected, false)?
            {
                return Err(SplitError::Conflict(
                    "Rulon ma’lumotlari o‘zgargan. Qayta skanerlang",
                ));
            }
        }
        let id = data_encoding::HEXLOWER.encode(&rand::random::<[u8; 16]>());
        let code = source["item_code"]
            .as_str()
            .ok_or(SplitError::StoreFailed)?;
        let name = source["item_name"]
            .as_str()
            .ok_or(SplitError::StoreFailed)?;
        let micron = source["micron"].as_str().ok_or(SplitError::StoreFailed)?;
        let receipt: String = row.try_get("source_receipt_id")?;
        let owner: Value=sqlx::query_scalar("SELECT jsonb_build_object('role',owner_role,'ref',owner_ref,'name',owner_display_name)
            FROM mini_raw_material_events WHERE lower(barcode)=lower($1) AND owner_ref<>'' ORDER BY id DESC LIMIT 1")
            .bind(&input.source_barcode).fetch_optional(&mut *tx).await?.unwrap_or(json!({}));
        sqlx::query("UPDATE mini_raw_material_stock SET qty=0,status='consumed',
            payload_json=payload_json || jsonb_build_object('raw_material_split_id',$2::text),updated_at=now() WHERE id=$1")
            .bind(parent).bind(&id).execute(&mut *tx).await?;
        let epcs = GscaleEpcGenerator::new();
        let mut outputs = Vec::new();
        for line in &quantities {
            let kg = line.kg;
            let width = decimal_text(line.width_mm);
            let output_name = split_item_name(name, &width, micron);
            let barcode = epcs.next_epc();
            let stock_id = format!("raw:{}", barcode.to_lowercase());
            let output = json!({"stock_id":stock_id,"barcode":barcode,"warehouse":warehouse,"item_code":code,
                "item_name":output_name,"kg":decimal_text(kg),"width_mm":width,"micron":micron,
                "gross_kg":decimal_text(line.gross_kg),"bobina_kg":decimal_text(line.bobina_kg)});
            sqlx::query("INSERT INTO mini_raw_material_stock
                (id,warehouse,item_code,item_name,barcode,qty,width_mm,micron,uom,status,source_receipt_id,payload_json)
                VALUES ($1,$2,$3,$4,$5,$6::text::numeric,$7::text::numeric,$8::text::numeric,'kg','available',$9,$10)")
                .bind(&stock_id).bind(warehouse).bind(code).bind(&output_name).bind(&barcode)
                .bind(decimal_text(kg)).bind(&width).bind(micron).bind(&receipt)
                .bind(json!({"source":"raw_material_split","parent_stock_id":parent,"split_id":id,"initial_kg":decimal_text(kg),
                    "gross_qty":decimal_text(line.gross_kg),"net_qty":decimal_text(kg),"tare_kg":decimal_text(line.bobina_kg),"tare_enabled":true}))
                .execute(&mut *tx).await?;
            outputs.push(output);
        }
        let result = json!({"id":id,"warehouse":warehouse,"source":source,"source_kg":decimal_text(source_kg),
            "output_kg":decimal_text(source_kg-waste),"waste_kg":decimal_text(waste),"outputs":outputs});
        sqlx::query("INSERT INTO mini_raw_material_splits
            (id,owner_ref,request_id,parent_stock_id,source_kg,output_kg,waste_kg,request_json,response_json)
            VALUES($1,$2,$3,$4,$5::text::numeric,$6::text::numeric,$7::text::numeric,$8,$9)")
            .bind(&id).bind(&actor.ref_).bind(&input.request_id).bind(parent).bind(decimal_text(source_kg))
            .bind(decimal_text(source_kg-waste)).bind(decimal_text(waste)).bind(request).bind(&result).execute(&mut *tx).await?;
        for output in &outputs {
            sqlx::query(
                "INSERT INTO mini_raw_material_split_outputs(split_id,stock_id,kg,width_mm,gross_kg,bobina_kg)
                VALUES($1,$2,$3::text::numeric,$4::text::numeric,$5::text::numeric,$6::text::numeric)",
            )
            .bind(&id)
            .bind(output["stock_id"].as_str())
            .bind(output["kg"].as_str())
            .bind(output["width_mm"].as_str())
            .bind(output["gross_kg"].as_str())
            .bind(output["bobina_kg"].as_str())
            .execute(&mut *tx)
            .await?;
        }
        for (entry, consumed) in
            std::iter::once((&source, true)).chain(outputs.iter().map(|o| (o, false)))
        {
            let barcode = entry["barcode"].as_str().ok_or(SplitError::StoreFailed)?;
            let delta = if consumed {
                format!("-{}", decimal_text(source_kg))
            } else {
                entry["kg"].as_str().unwrap().to_string()
            };
            sqlx::query("INSERT INTO mini_raw_material_events
                (event_id,idempotency_key,event_type,warehouse,barcode,item_code,item_name,qty_delta,
                stock_status_before,stock_status_after,actor_role,actor_ref,actor_display_name,
                owner_role,owner_ref,owner_display_name,source_type,source_id,source_line_ref,correlation_id,payload_json)
                VALUES($1,$1,$2,$3,$4,$5,$6,$7::text::numeric,$8,$9,'homashyo_rezkachi',$10,$11,
                $12,$13,$14,'raw_material_split',$15,$4,$15,$16)")
                .bind(format!("raw-split:{id}:{barcode}"))
                .bind(if consumed {"split_consumed"} else {"split_produced"})
                .bind(warehouse).bind(barcode).bind(code).bind(entry["item_name"].as_str()).bind(delta)
                .bind(if consumed {Some("available")} else {None}).bind(if consumed {"consumed"} else {"available"})
                .bind(&actor.ref_).bind(&actor.display_name)
                .bind(owner["role"].as_str().unwrap_or("")).bind(owner["ref"].as_str().unwrap_or(""))
                .bind(owner["name"].as_str().unwrap_or("")).bind(&id)
                .bind(json!({"split_id":id,"parent_stock_id":parent,"waste_kg":decimal_text(waste),
                    "gross_qty":entry["gross_kg"],"net_qty":entry["kg"],"tare_kg":entry["bobina_kg"],
                    "width_mm":entry["width_mm"],"micron":entry["micron"]}))
                .execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(result)
    }

    pub async fn report_issue(
        &self,
        actor: &Principal,
        input: SplitIssueCreate,
    ) -> Result<Value, SplitError> {
        if actor.role != crate::core::auth::models::PrincipalRole::HomashyoRezkachi {
            return Err(SplitError::Forbidden);
        }
        let balance = input.validate()?;
        let command = &input.command;
        let request = json!(&input);
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout='5s'")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!(
                "raw-split-issue:{}:{}",
                actor.ref_, command.request_id
            ))
            .execute(&mut *tx)
            .await?;
        if let Some(row) = sqlx::query("SELECT request_json,response_json FROM mini_raw_material_split_issues WHERE owner_ref=$1 AND request_id=$2")
            .bind(&actor.ref_).bind(&command.request_id).fetch_optional(&mut *tx).await? {
            if row.try_get::<Value,_>("request_json")? != request {
                return Err(SplitError::Conflict("So‘rov raqami boshqa muammoga ishlatilgan"));
            }
            return Ok(row.try_get("response_json")?);
        }
        let source: Value = sqlx::query_scalar(&format!(
            "SELECT {SOURCE_JSON} FROM mini_raw_material_stock s
            WHERE lower(s.barcode)=lower($2) AND {SCOPE} AND {AVAILABLE} FOR SHARE OF s"
        ))
        .bind(&actor.ref_)
        .bind(command.source_barcode.trim())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(SplitError::Forbidden)?;
        if source["revision"].as_str() != Some(command.expected_revision.as_str()) {
            return Err(SplitError::Conflict(
                "Rulon ma’lumotlari o‘zgargan. Qayta skanerlang",
            ));
        }
        for (field, expected) in [
            ("kg", &command.expected_kg),
            ("width_mm", &command.expected_width_mm),
            ("micron", &command.expected_micron),
        ] {
            if quantity(source[field].as_str().unwrap_or(""), false)? != quantity(expected, false)?
            {
                return Err(SplitError::Conflict(
                    "Asl rulon ma’lumotlari mos kelmadi. Qayta skanerlang",
                ));
            }
        }
        let id = format!("raw-issue:{:032x}", rand::random::<u128>());
        let created_at: String = sqlx::query_scalar("SELECT to_jsonb(now()) #>> '{}'")
            .fetch_one(&mut *tx)
            .await?;
        let result = json!({"id":id,"request_id":command.request_id,"source":source,"outputs":command.outputs,
            "source_kg":decimal_text(balance.source),"output_kg":split_issue_decimal(balance.output),
            "entered_waste_kg":command.waste_kg,"waste_kg":balance.waste.map(decimal_text),
            "difference_kg":balance.difference.map(split_issue_decimal),"kind":balance.kind,
            "note":input.note.trim(),"actor_ref":actor.ref_,"actor_name":actor.display_name,"created_at":created_at});
        sqlx::query("INSERT INTO mini_raw_material_split_issues
            (id,owner_ref,request_id,parent_stock_id,source_kg,output_kg,waste_kg,difference_kg,note,request_json,response_json)
            VALUES($1,$2,$3,$4,$5::text::numeric,$6::text::numeric,$7::text::numeric,$8::text::numeric,$9,$10,$11)")
            .bind(&id).bind(&actor.ref_).bind(&command.request_id).bind(source["stock_id"].as_str().ok_or(SplitError::StoreFailed)?)
            .bind(decimal_text(balance.source)).bind(split_issue_decimal(balance.output))
            .bind(balance.waste.map(decimal_text)).bind(balance.difference.map(split_issue_decimal))
            .bind(input.note.trim()).bind(request).bind(&result).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(result)
    }
}
