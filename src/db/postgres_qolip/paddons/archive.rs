async fn insert_archive(
    tx: &mut Tx<'_>,
    kind: &str,
    code: &str,
    warehouse: &str,
    snapshot: Value,
    note: &str,
    principal: &Principal,
) -> Result<Value, Error> {
    sqlx::query_scalar("INSERT INTO mini_qolip_archive_entries AS a(id,kind,code,warehouse,snapshot,note,archived_by_role,archived_by_ref,archived_by_name) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING to_jsonb(a)")
 .bind(id("qa")).bind(kind).bind(code).bind(warehouse).bind(snapshot).bind(note).bind(role_code(&principal.role)).bind(&principal.ref_).bind(&principal.display_name).fetch_one(&mut **tx).await.map_err(db)
}
async fn restore_entry(tx: &mut Tx<'_>, id: &str, principal: &Principal) -> Result<Value, Error> {
    sqlx::query_scalar("UPDATE mini_qolip_archive_entries AS a SET restored_at=now(),restored_by_role=$2,restored_by_ref=$3,restored_by_name=$4 WHERE id=$1 AND restored_at IS NULL RETURNING to_jsonb(a)").bind(id).bind(role_code(&principal.role)).bind(&principal.ref_).bind(&principal.display_name).fetch_optional(&mut **tx).await.map_err(db)?.ok_or(Error::Conflict("qolip_archive_changed"))
}
async fn archive_action(
    tx: &mut Tx<'_>,
    input: &Action,
    principal: &Principal,
    allowed: Option<&[String]>,
) -> Result<(Value, Vec<String>), Error> {
    let mut entries = Vec::new();
    let mut warehouses = std::collections::BTreeSet::new();
    if input.action == "archive" {
        lock_codes(tx, &input.qolip_codes).await?;
        let selected = molds(tx, Some(&input.qolip_codes), None, "", 1001, 0).await?;
        if selected.len() != input.qolip_codes.len() {
            return Err(Error::NotFound);
        }
        let homes = selected
            .iter()
            .filter_map(|m| m["warehouse"].as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for home in homes {
            lock_warehouse(tx, home).await?;
        }
        for mold in &selected {
            owner(mold["warehouse"].as_str().unwrap_or(""), allowed)?;
            check_available(mold)?;
        }
        for mold in selected {
            let warehouse = mold["warehouse"].as_str().ok_or(Error::Store)?.to_owned();
            let code = mold["qolip_code"].as_str().ok_or(Error::Store)?.to_owned();
            warehouses.insert(warehouse.clone());
            entries.push(
                insert_archive(tx, "qolip", &code, &warehouse, mold, &input.note, principal)
                    .await?,
            );
        }
    } else {
        let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(a) FROM mini_qolip_archive_entries a WHERE id=ANY($1) ORDER BY code_key FOR UPDATE").bind(&input.entry_ids).fetch_all(&mut **tx).await.map_err(db)?;
        if rows.len() != input.entry_ids.len() {
            return Err(Error::NotFound);
        }
        let mut codes = Vec::new();
        for row in &rows {
            owner(row["warehouse"].as_str().unwrap_or(""), allowed)?;
            if row["kind"] != "qolip" {
                return Err(Error::Conflict("qolip_restore_paddon_required"));
            }
            if !row["restored_at"].is_null() {
                return Err(Error::Conflict("qolip_archive_changed"));
            }
            codes.push(
                row["code"]
                    .as_str()
                    .ok_or(Error::Store)?
                    .trim()
                    .to_lowercase(),
            );
        }
        lock_codes(tx, &codes).await?;
        // Historical archive identity remains reserved; never recreate or reset mileage.
        let current = molds(tx, Some(&codes), None, "", 1001, 0).await?;
        if current.len() != codes.len() {
            return Err(Error::Conflict("qolip_storage_identity_changed"));
        }
        let homes = current
            .iter()
            .filter_map(|m| m["warehouse"].as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for home in homes {
            lock_warehouse(tx, home).await?;
        }
        for mold in &current {
            owner(mold["warehouse"].as_str().unwrap_or(""), allowed)?;
            if mold["paddon_code"].as_str().is_some_and(|s| !s.is_empty())
                || mold["is_in_use"] == true
                || mold["is_in_rework"] == true
                || mold["is_transferring"] == true
                || mold["is_custody_mismatch"] == true
            {
                return Err(Error::Conflict("qolip_storage_changed"));
            }
        }
        for row in rows {
            warehouses.insert(row["warehouse"].as_str().ok_or(Error::Store)?.to_owned());
            entries
                .push(restore_entry(tx, row["id"].as_str().ok_or(Error::Store)?, principal).await?);
        }
    }
    Ok((
        json!({"ok":true,"entries":entries}),
        warehouses.into_iter().collect(),
    ))
}
