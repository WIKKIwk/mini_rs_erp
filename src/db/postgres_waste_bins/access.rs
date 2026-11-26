use super::*;
use crate::core::apparatus_standard::ApparatusId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct WasteBinAccessInput {
    pub name: String,
    pub apparatus_ids: Vec<String>,
    pub operators: Vec<WasteBinOperator>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WasteBinOperator {
    pub role: String,
    pub r#ref: String,
}

impl WasteBinAccessInput {
    pub(super) fn normalized(&self) -> Result<Self, WasteBinError> {
        let name = self.name.trim();
        if !(1..=120).contains(&name.chars().count())
            || name.chars().any(char::is_control)
            || !(1..=128).contains(&self.apparatus_ids.len())
            || !(1..=128).contains(&self.operators.len())
        {
            return Err(WasteBinError::InvalidInput);
        }
        let mut apparatus_ids = self
            .apparatus_ids
            .iter()
            .map(|id| {
                ApparatusId::new(id.trim())
                    .map(ApparatusId::into_string)
                    .map_err(|_| WasteBinError::InvalidInput)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut operators = self
            .operators
            .iter()
            .map(|operator| {
                let role = operator.role.trim();
                if !matches!(
                    role,
                    "aparatchi"
                        | "rezka_master"
                        | "tayyorlov_masteri"
                        | "metalayzer"
                        | "homashyo_rezkachi"
                ) {
                    return Err(WasteBinError::InvalidInput);
                }
                Ok(WasteBinOperator {
                    role: role.to_owned(),
                    r#ref: identity(&operator.r#ref)?.to_owned(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        apparatus_ids.sort_unstable();
        apparatus_ids.dedup();
        operators.sort_unstable();
        operators.dedup();
        Ok(Self {
            name: name.to_owned(),
            apparatus_ids,
            operators,
        })
    }
}

pub(super) async fn write(
    db: &mut PgConnection,
    code: &str,
    access: &WasteBinAccessInput,
) -> Result<(), WasteBinError> {
    sqlx::query("UPDATE mini_waste_bins SET name = $2 WHERE code = $1")
        .bind(code)
        .bind(&access.name)
        .execute(&mut *db)
        .await?;
    sqlx::query("DELETE FROM mini_waste_bin_apparatuses WHERE bin_code = $1")
        .bind(code)
        .execute(&mut *db)
        .await?;
    sqlx::query(
        "INSERT INTO mini_waste_bin_apparatuses (bin_code, apparatus_id)
                 SELECT $1, id FROM unnest($2::text[]) AS ids(id)",
    )
    .bind(code)
    .bind(&access.apparatus_ids)
    .execute(&mut *db)
    .await?;
    sqlx::query("DELETE FROM mini_waste_bin_operators WHERE bin_code = $1")
        .bind(code)
        .execute(&mut *db)
        .await?;
    sqlx::query(
        "INSERT INTO mini_waste_bin_operators (bin_code, operator_role, operator_ref)
                 SELECT $1, role, ref FROM jsonb_to_recordset($2) AS o(role text, ref text)",
    )
    .bind(code)
    .bind(sqlx::types::Json(&access.operators))
    .execute(db)
    .await?;
    Ok(())
}

/// Automatic production entries must use the same scope as manual entries.
/// A producer outside that scope still records the entry as pending.
pub(super) async fn open_cycle_for_actor(
    db: &mut PgConnection,
    apparatus: &str,
    actor: &QueueActionActor,
) -> Result<Option<String>, WasteBinError> {
    Ok(sqlx::query_scalar(
        "SELECT c.id FROM mini_waste_bin_cycles c
         WHERE c.apparatus_id = $1 AND c.sealed_at IS NULL AND (
            (c.opened_by_role IS NULL AND c.opened_by_ref = $3)
            OR (
                EXISTS (SELECT 1 FROM mini_waste_bin_apparatuses a
                        WHERE a.bin_code = c.bin_code AND a.apparatus_id = $1)
                AND EXISTS (SELECT 1 FROM mini_waste_bin_operators o
                            WHERE o.bin_code = c.bin_code AND o.operator_role = $2 AND o.operator_ref = $3)
            )
         )",
    )
    .bind(apparatus)
    .bind(&actor.role)
    .bind(&actor.ref_)
    .fetch_optional(db)
    .await?)
}
