use std::collections::{BTreeMap, BTreeSet};

use sqlx::{Postgres, Row, Transaction};

use crate::core::production_map::{OrderRunSession, ProductionMapError};

const MAX_PANTON_COUNT: i16 = 7;

pub(super) async fn assign_order_qolip_pantons_tx(
    tx: &mut Transaction<'_, Postgres>,
    session: &mut OrderRunSession,
) -> Result<(), ProductionMapError> {
    let requested = normalized_codes(
        session
            .payload_json
            .get("qolip_panton_codes")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str),
    );
    if requested.is_empty() {
        return Ok(());
    }

    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("qolip-panton:{}", session.order_id.trim()))
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;

    let rows = sqlx::query(
        "SELECT qolip_code_key, qolip_code, panton_number
         FROM mini_order_qolip_pantons
         WHERE order_id = $1
         ORDER BY panton_number
         FOR UPDATE",
    )
    .bind(session.order_id.trim())
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    let mut assigned = BTreeMap::<String, (String, i16)>::new();
    let mut used_numbers = BTreeSet::<i16>::new();
    for row in rows {
        let key: String = row
            .try_get("qolip_code_key")
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let code: String = row
            .try_get("qolip_code")
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let number: i16 = row
            .try_get("panton_number")
            .map_err(|_| ProductionMapError::StoreFailed)?;
        assigned.insert(key, (code, number));
        used_numbers.insert(number);
    }

    for code in requested {
        let key = code.to_ascii_lowercase();
        if assigned.contains_key(&key) {
            continue;
        }
        let Some(number) = (1..=MAX_PANTON_COUNT).find(|number| !used_numbers.contains(number))
        else {
            return Err(ProductionMapError::QolipPantonLimitExceeded);
        };
        sqlx::query(
            "INSERT INTO mini_order_qolip_pantons
                (order_id, qolip_code_key, qolip_code, panton_number)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(session.order_id.trim())
        .bind(&key)
        .bind(&code)
        .bind(number)
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        assigned.insert(key, (code, number));
        used_numbers.insert(number);
    }

    let pantons = assigned
        .values()
        .map(|(code, number)| (code.clone(), serde_json::json!(number)))
        .collect::<serde_json::Map<_, _>>();
    if !session.payload_json.is_object() {
        session.payload_json = serde_json::json!({});
    }
    session.payload_json["qolip_pantons"] = serde_json::Value::Object(pantons);
    Ok(())
}

fn normalized_codes<'a>(codes: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut normalized = Vec::new();
    for code in codes {
        let code = code.trim();
        if code.is_empty()
            || normalized
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(code))
        {
            continue;
        }
        normalized.push(code.to_string());
    }
    normalized
}
