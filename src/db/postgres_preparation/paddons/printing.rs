use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum PrintAction {
    Start,
    Confirm,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaddonLabelPrint {
    issue_id: String,
    barcode: String,
    attempt: String,
    #[serde(default)]
    previous_attempt: Option<String>,
    action: PrintAction,
}

impl PostgresPreparationStore {
    pub async fn track_paddon_print(
        &self,
        actor: &Principal,
        input: PaddonLabelPrint,
    ) -> Result<Value, PreparationError> {
        if !(8..=128).contains(&input.attempt.len())
            || input.barcode.len() > 128
            || input.issue_id.len() > 128
        {
            return Err(PreparationError::Invalid("Yorliq ma’lumoti noto‘g‘ri"));
        }
        let mut tx = self.pool.begin().await?;
        let allowed: bool=sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM mini_raw_material_paddon_issues i
            JOIN mini_raw_material_paddons p ON p.id=i.paddon_id JOIN mini_warehouses w ON w.id=p.warehouse_id
            JOIN mini_warehouse_assignments a ON lower(a.warehouse_name)=lower(w.name)
            WHERE i.id=$1 AND p.owner_role=$2 AND p.owner_ref=$3 AND NOT w.is_group
              AND a.assignment_kind='warehouse' AND a.principal_role=$2 AND a.principal_ref=$3
              AND i.response_json->'labels' @> jsonb_build_array(jsonb_build_object('barcode',$4::text)))")
            .bind(&input.issue_id).bind(role_code(actor)?).bind(&actor.ref_).bind(&input.barcode).fetch_one(&mut *tx).await?;
        if !allowed {
            return Err(PreparationError::Forbidden);
        }
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("paddon-print:{}:{}", input.issue_id, input.barcode))
            .execute(&mut *tx)
            .await?;
        let saved: Option<(String, bool)> = sqlx::query_as(
            "SELECT attempt,printed FROM mini_raw_material_paddon_label_prints
            WHERE issue_id=$1 AND barcode=$2",
        )
        .bind(&input.issue_id)
        .bind(&input.barcode)
        .fetch_optional(&mut *tx)
        .await?;
        let conflict =
            || PreparationError::Conflict("Yorliqning chop holati o‘zgargan. Tarixni yangilang");
        let printed = match input.action {
            PrintAction::Start if saved.as_ref().is_some_and(|(a, _)| a == &input.attempt) => {
                saved.unwrap().1
            }
            PrintAction::Start => {
                if saved.as_ref().map(|(a, _)| a) != input.previous_attempt.as_ref() {
                    return Err(conflict());
                }
                sqlx::query("INSERT INTO mini_raw_material_paddon_label_prints(issue_id,barcode,attempt) VALUES($1,$2,$3)
                    ON CONFLICT(issue_id,barcode) DO UPDATE SET attempt=excluded.attempt,printed=false")
                    .bind(&input.issue_id).bind(&input.barcode).bind(&input.attempt).execute(&mut *tx).await?;
                false
            }
            PrintAction::Confirm => {
                if !saved.is_some_and(|(a, _)| a == input.attempt) {
                    return Err(conflict());
                }
                sqlx::query("UPDATE mini_raw_material_paddon_label_prints SET printed=true WHERE issue_id=$1 AND barcode=$2")
                    .bind(&input.issue_id).bind(&input.barcode).execute(&mut *tx).await?;
                true
            }
        };
        tx.commit().await?;
        Ok(json!({"attempt":input.attempt,"printed":printed}))
    }
}
