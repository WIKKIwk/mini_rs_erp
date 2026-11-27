use super::*;
use crate::core::apparatus_standard::ApparatusId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct WasteBinAccessInput {
    pub name: String,
    pub apparatus_ids: Vec<String>,
}

impl WasteBinAccessInput {
    pub(super) fn normalized(&self) -> Result<Self, WasteBinError> {
        let name = self.name.trim();
        if !(1..=120).contains(&name.chars().count())
            || name.chars().any(char::is_control)
            || !(1..=128).contains(&self.apparatus_ids.len())
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
        apparatus_ids.sort_unstable();
        apparatus_ids.dedup();
        Ok(Self {
            name: name.to_owned(),
            apparatus_ids,
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
    .execute(db)
    .await?;
    Ok(())
}

// Both manual and production entries go to the apparatus's open cycle.
pub(super) async fn open_cycle(
    db: &mut PgConnection,
    apparatus: &str,
) -> Result<Option<String>, WasteBinError> {
    Ok(sqlx::query_scalar(
        "SELECT c.id FROM mini_waste_bin_cycles c
         WHERE c.apparatus_id = $1 AND c.sealed_at IS NULL AND (
            c.opened_by_role IS NULL
            OR EXISTS (SELECT 1 FROM mini_waste_bin_apparatuses a
                       WHERE a.bin_code = c.bin_code AND a.apparatus_id = $1)
         )",
    )
    .bind(apparatus)
    .fetch_optional(db)
    .await?)
}
