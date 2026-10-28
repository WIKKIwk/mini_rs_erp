async fn lock_codes(tx: &mut Tx<'_>, codes: &[String]) -> Result<(), Error> {
    for code in codes
        .iter()
        .map(|s| s.trim().to_lowercase())
        .collect::<std::collections::BTreeSet<_>>()
    {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("qolip:{code}"))
            .execute(&mut **tx)
            .await
            .map_err(db)?;
    }
    Ok(())
}
fn check_available(mold: &Value) -> Result<(), Error> {
    for (field, error) in [
        ("archived", "qolip_archived"),
        ("is_in_use", "qolip_in_use"),
        ("is_in_rework", "qolip_in_rework"),
        ("is_transferring", "qolip_in_transfer"),
        ("is_custody_mismatch", "qolip_warehouse_mismatch"),
    ] {
        if mold[field].as_bool() == Some(true) {
            return Err(Error::Conflict(error));
        }
    }
    if mold["paddon_code"].as_str().is_some_and(|s| !s.is_empty()) {
        return Err(Error::Conflict("qolip_in_paddon"));
    }
    Ok(())
}
async fn event(
    tx: &mut Tx<'_>,
    code: &str,
    action: &str,
    principal: &Principal,
    payload: Value,
) -> Result<(), Error> {
    sqlx::query("INSERT INTO mini_qolip_paddon_events(id,paddon_code,action,actor_ref,actor_name,payload) VALUES($1,$2,$3,$4,$5,$6)").bind(id("qpe")).bind(code).bind(action).bind(&principal.ref_).bind(&principal.display_name).bind(payload).execute(&mut **tx).await.map_err(db)?;
    Ok(())
}
pub async fn action(
    pool: &PgPool,
    input: &Action,
    archive: bool,
    principal: &Principal,
    allowed: Option<&[String]>,
) -> Result<Value, Error> {
    let actor = format!("{}:{}", role_code(&principal.role), principal.ref_);
    let request = json!({"archive":archive,"input":input});
    let mut tx = pool.begin().await.map_err(db)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!(
            "qolip-storage-request:{actor}:{}",
            input.request_id
        ))
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    if let Some((old,response,warehouses))=sqlx::query_as::<_,(Value,Value,Vec<String>)>("SELECT request_json,response_json,warehouses FROM mini_qolip_storage_requests WHERE actor_key=$1 AND request_id=$2").bind(&actor).bind(&input.request_id).fetch_optional(&mut *tx).await.map_err(db)?{
  if old!=request{return Err(Error::Conflict("qolip_storage_request_conflict"));}
  for warehouse in &warehouses{owner(warehouse,allowed)?;}
  tx.commit().await.map_err(db)?;return Ok(response);
 }
    super::product_transfer::lock_composition(&mut tx).await?;
    let (response, warehouses) = if archive {
        archive_action(&mut tx, input, principal, allowed).await?
    } else {
        paddon_action(&mut tx, input, principal, allowed).await?
    };
    sqlx::query("INSERT INTO mini_qolip_storage_requests(actor_key,request_id,request_json,response_json,warehouses) VALUES($1,$2,$3,$4,$5)").bind(actor).bind(&input.request_id).bind(request).bind(&response).bind(warehouses).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(response)
}
async fn paddon_action(
    tx: &mut Tx<'_>,
    input: &Action,
    principal: &Principal,
    allowed: Option<&[String]>,
) -> Result<(Value, Vec<String>), Error> {
    if input.action == "create" {
        owner(&input.warehouse, allowed)?;
        lock_warehouse(tx, &input.warehouse).await?;
        let n: i64 = sqlx::query_scalar("SELECT nextval('mini_qolip_paddon_number')")
            .fetch_one(&mut **tx)
            .await
            .map_err(db)?;
        let code = format!("QP-{n:05}");
        sqlx::query("INSERT INTO mini_qolip_paddons(code,warehouse,location,note,created_by_role,created_by_ref,created_by_name) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(&code).bind(&input.warehouse).bind(&input.location).bind(&input.note).bind(role_code(&principal.role)).bind(&principal.ref_).bind(&principal.display_name).execute(&mut **tx).await.map_err(db)?;
        event(
            tx,
            &code,
            "create",
            principal,
            json!({"location":input.location,"note":input.note}),
        )
        .await?;
        return Ok((
            snapshot(tx, &code, allowed).await?,
            vec![input.warehouse.clone()],
        ));
    }
    let code = &input.code;
    let p = paddon(tx, code, allowed).await?;
    let warehouse = p["warehouse"].as_str().ok_or(Error::Store)?.to_owned();
    lock_warehouse(tx, &warehouse).await?;
    let current:Vec<String>=sqlx::query_scalar("SELECT qolip_code_key FROM mini_qolip_paddon_members WHERE paddon_code=$1 AND removed_at IS NULL ORDER BY qolip_code_key").bind(code).fetch_all(&mut **tx).await.map_err(db)?;
    let mut all = current.clone();
    all.extend(input.qolip_codes.iter().cloned());
    lock_codes(tx, &all).await?;
    sqlx::query("SELECT code FROM mini_qolip_paddons WHERE code=$1 FOR UPDATE")
        .bind(code)
        .fetch_one(&mut **tx)
        .await
        .map_err(db)?;
    if p["revision"].as_i64() != input.expected_revision {
        return Err(Error::Conflict("qolip_paddon_changed"));
    }
    let archived = p["archived"].as_bool() == Some(true);
    let locked = p["locked"].as_bool() == Some(true);
    if archived && !matches!(input.action.as_str(), "restore" | "inspect") {
        return Err(Error::Conflict("qolip_archived"));
    }
    let mut payload = json!({"qolip_codes":input.qolip_codes,"note":input.note});
    let mut inspection = None;
    match input.action.as_str() {
        "add" => {
            if locked {
                return Err(Error::Conflict("qolip_paddon_locked"));
            }
            if current.len() + input.qolip_codes.len() > 1000 {
                return Err(Error::Conflict("qolip_paddon_capacity"));
            }
            let selected = molds(tx, Some(&input.qolip_codes), None, "", 1001, 0).await?;
            if selected.len() != input.qolip_codes.len() {
                return Err(Error::NotFound);
            }
            for m in &selected {
                owner(m["warehouse"].as_str().unwrap_or(""), allowed)?;
                if !m["warehouse"]
                    .as_str()
                    .unwrap_or("")
                    .eq_ignore_ascii_case(&warehouse)
                {
                    return Err(Error::Conflict("qolip_warehouse_mismatch"));
                }
                check_available(m)?;
            }
            for m in selected {
                sqlx::query("INSERT INTO mini_qolip_paddon_members(id,paddon_code,qolip_code,added_by_ref,added_by_name) VALUES($1,$2,$3,$4,$5)").bind(id("qpm")).bind(code).bind(m["qolip_code"].as_str().ok_or(Error::Store)?).bind(&principal.ref_).bind(&principal.display_name).execute(&mut **tx).await.map_err(db)?;
            }
        }
        "remove" => {
            if locked {
                return Err(Error::Conflict("qolip_paddon_locked"));
            }
            if input.qolip_codes.iter().any(|c| !current.contains(c)) {
                return Err(Error::Conflict("qolip_paddon_membership_changed"));
            }
            sqlx::query("UPDATE mini_qolip_paddon_members SET removed_at=now(),removed_by_ref=$3,removed_by_name=$4 WHERE paddon_code=$1 AND qolip_code_key=ANY($2) AND removed_at IS NULL").bind(code).bind(&input.qolip_codes).bind(&principal.ref_).bind(&principal.display_name).execute(&mut **tx).await.map_err(db)?;
        }
        "lock" => {
            if locked {
                return Err(Error::Conflict("qolip_paddon_locked"));
            }
            if current.is_empty() {
                return Err(Error::Conflict("qolip_paddon_empty"));
            }
            sqlx::query("UPDATE mini_qolip_paddons SET locked_at=now() WHERE code=$1")
                .bind(code)
                .execute(&mut **tx)
                .await
                .map_err(db)?;
        }
        "unlock" => {
            if !locked {
                return Err(Error::Conflict("qolip_paddon_not_locked"));
            }
            sqlx::query("UPDATE mini_qolip_paddons SET locked_at=NULL WHERE code=$1")
                .bind(code)
                .execute(&mut **tx)
                .await
                .map_err(db)?;
        }
        "archive" => {
            if current.is_empty() {
                return Err(Error::Conflict("qolip_paddon_empty"));
            }
            let snap = snapshot(tx, code, allowed).await?;
            // Packaged stock is guarded, but revalidate physical availability in case legacy data is inconsistent.
            for m in snap["items"].as_array().ok_or(Error::Store)? {
                if m["is_in_use"] == true
                    || m["is_in_rework"] == true
                    || m["is_transferring"] == true
                    || m["is_custody_mismatch"] == true
                {
                    return Err(Error::Conflict("qolip_in_use"));
                }
            }
            let entry =
                insert_archive(tx, "paddon", code, &warehouse, snap, &input.note, principal)
                    .await?;
            payload["entry_id"] = entry["id"].clone();
            sqlx::query("UPDATE mini_qolip_paddons SET archived_at=now() WHERE code=$1")
                .bind(code)
                .execute(&mut **tx)
                .await
                .map_err(db)?;
        }
        "restore" => {
            if !archived {
                return Err(Error::Conflict("qolip_not_archived"));
            }
            let entry:Value=sqlx::query_scalar("SELECT to_jsonb(a) FROM mini_qolip_archive_entries a WHERE kind='paddon' AND code=$1 AND restored_at IS NULL FOR UPDATE").bind(code).fetch_optional(&mut **tx).await.map_err(db)?.ok_or(Error::Conflict("qolip_archive_changed"))?;
            restore_entry(tx, entry["id"].as_str().ok_or(Error::Store)?, principal).await?;
            payload["entry_id"] = entry["id"].clone();
            sqlx::query("UPDATE mini_qolip_paddons SET archived_at=NULL WHERE code=$1")
                .bind(code)
                .execute(&mut **tx)
                .await
                .map_err(db)?;
        }
        "inspect" => {
            let expected = current
                .iter()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>();
            let observed = input
                .qolip_codes
                .iter()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>();
            let result = json!({"expected":expected,"found":expected.intersection(&observed).cloned().collect::<Vec<_>>(),"missing":expected.difference(&observed).cloned().collect::<Vec<_>>(),"unexpected":observed.difference(&expected).cloned().collect::<Vec<_>>()});
            payload = result.clone();
            inspection = Some(result);
        }
        _ => return Err(Error::Invalid("qolip_storage_invalid_action")),
    }
    sqlx::query("UPDATE mini_qolip_paddons SET revision=revision+1 WHERE code=$1")
        .bind(code)
        .execute(&mut **tx)
        .await
        .map_err(db)?;
    event(tx, code, &input.action, principal, payload).await?;
    let mut response = snapshot(tx, code, allowed).await?;
    if let Some(result) = inspection {
        response["inspection"] = result;
    }
    Ok((response, vec![warehouse]))
}

async fn lock_warehouse(tx: &mut Tx<'_>, warehouse: &str) -> Result<(), Error> {
    let exists: Option<String> = sqlx::query_scalar(
        "SELECT name FROM mini_warehouses WHERE lower(btrim(name))=lower(btrim($1)) FOR SHARE",
    )
    .bind(warehouse)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db)?;
    if exists.is_none() {
        return Err(Error::Conflict("qolip_storage_warehouse_changed"));
    }
    Ok(())
}
