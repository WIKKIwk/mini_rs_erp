use sqlx::{PgPool, Postgres, Transaction};

use crate::core::production_map::{ProductionMapError, QueueActionActor};

pub(super) async fn lock_scope(
    tx: &mut Transaction<'_, Postgres>,
    apparatus: &str,
    actor: &QueueActionActor,
) -> Result<(), ProductionMapError> {
    // Serialize preference changes with output assignment, including an unset selection.
    let key = serde_json::json!([
        "active_rezka_paddon",
        actor.role.trim(),
        actor.ref_.trim(),
        apparatus.trim()
    ])
    .to_string();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(key)
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) async fn load(
    pool: &PgPool,
    apparatus: &str,
    actor: &QueueActionActor,
) -> Result<Option<String>, ProductionMapError> {
    sqlx::query_scalar::<_, Option<String>>(
        "SELECT paddon_code FROM mini_active_rezka_paddons WHERE actor_role=$1 AND actor_ref=$2 AND apparatus_id=$3"
    ).bind(actor.role.trim()).bind(actor.ref_.trim()).bind(apparatus.trim())
        .fetch_optional(pool).await.map(Option::flatten).map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn load_for_output(
    tx: &mut Transaction<'_, Postgres>,
    apparatus: &str,
    actor: &QueueActionActor,
) -> Result<Option<String>, ProductionMapError> {
    lock_scope(tx, apparatus, actor).await?;
    sqlx::query_scalar::<_, Option<String>>(
        "SELECT paddon_code FROM mini_active_rezka_paddons WHERE actor_role=$1 AND actor_ref=$2 AND apparatus_id=$3"
    ).bind(actor.role.trim()).bind(actor.ref_.trim()).bind(apparatus.trim())
        .fetch_optional(&mut **tx).await.map(Option::flatten).map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn save(
    pool: &PgPool,
    apparatus: &str,
    actor: &QueueActionActor,
    code: Option<&str>,
) -> Result<(), ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    lock_scope(&mut tx, apparatus, actor).await?;
    if let Some(code) = code {
        let receipt = sqlx::query_scalar::<_, Option<serde_json::Value>>(
            "SELECT to_jsonb(p)->'receipt_json' FROM mini_paddons p WHERE code=$1 FOR UPDATE",
        )
        .bind(code)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?
        .ok_or(ProductionMapError::PaddonNotFound)?;
        if receipt.is_some_and(|v| !v.is_null()) {
            return Err(ProductionMapError::PaddonInvalidInput);
        }
    }
    sqlx::query(
        "INSERT INTO mini_active_rezka_paddons (actor_role, actor_ref, apparatus_id, paddon_code)
        VALUES ($1,$2,$3,$4) ON CONFLICT (actor_role, actor_ref, apparatus_id)
        DO UPDATE SET paddon_code=EXCLUDED.paddon_code, updated_at=now()",
    )
    .bind(actor.role.trim())
    .bind(actor.ref_.trim())
    .bind(apparatus.trim())
    .bind(code)
    .execute(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::production_map::ProductionMapStorePort;
    use crate::db::postgres_production_map::PostgresProductionMapStore;

    #[tokio::test]
    async fn active_paddon_postgres_persists_scopes_validates_and_overrides_stale_output() {
        let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
        let admin = PgPool::connect(&url).await.unwrap();
        let schema = format!("active_paddon_test_{:016x}", rand::random::<u64>());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        let search_path = format!("SET search_path TO {schema}");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(3)
            .after_connect(move |conn, _| {
                let sql = search_path.clone();
                Box::pin(async move {
                    sqlx::query(&sql).execute(conn).await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .unwrap();
        sqlx::raw_sql("CREATE TABLE mini_paddons(id TEXT PRIMARY KEY, code TEXT UNIQUE, receipt_json JSONB);
            INSERT INTO mini_paddons VALUES ('a','00001',NULL),('b','00002',NULL),('c','00003','{}');")
            .execute(&pool).await.unwrap();
        sqlx::raw_sql(include_str!(
            "../../../../migrations/postgres/0125_active_rezka_paddon.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        let apparatus = "apparatus:default:asset-010";
        let actor = QueueActionActor {
            role: "aparatchi".into(),
            ref_: "worker-a".into(),
            display_name: "A".into(),
        };
        let store = PostgresProductionMapStore::new(pool.clone());
        assert_eq!(
            store.active_rezka_paddon(apparatus, &actor).await.unwrap(),
            None
        );
        store
            .set_active_rezka_paddon(apparatus, &actor, Some("00001"))
            .await
            .unwrap();
        let second_device = PostgresProductionMapStore::new(pool.clone());
        assert_eq!(
            second_device
                .active_rezka_paddon(apparatus, &actor)
                .await
                .unwrap()
                .as_deref(),
            Some("00001")
        );
        let mut other = actor.clone();
        other.ref_ = "worker-b".into();
        assert_eq!(
            store.active_rezka_paddon(apparatus, &other).await.unwrap(),
            None
        );
        other = actor.clone();
        other.role = "admin".into();
        assert_eq!(
            store.active_rezka_paddon(apparatus, &other).await.unwrap(),
            None
        );
        assert_eq!(
            store
                .active_rezka_paddon("another-apparatus", &actor)
                .await
                .unwrap(),
            None
        );
        for (code, error) in [
            ("missing", ProductionMapError::PaddonNotFound),
            ("00003", ProductionMapError::PaddonInvalidInput),
        ] {
            assert_eq!(
                store
                    .set_active_rezka_paddon(apparatus, &actor, Some(code))
                    .await,
                Err(error)
            );
            assert_eq!(
                store
                    .active_rezka_paddon(apparatus, &actor)
                    .await
                    .unwrap()
                    .as_deref(),
                Some("00001")
            );
        }
        second_device
            .set_active_rezka_paddon(apparatus, &actor, Some("00002"))
            .await
            .unwrap();
        let payload = serde_json::json!({"use_active_paddon":true,"output_paddon_code":"00001"});
        let mut tx = pool.begin().await.unwrap();
        let selected = super::super::output_paddon_assignment::selection_for_output(
            &mut tx, apparatus, &actor, &payload,
        )
        .await
        .unwrap();
        assert_eq!(selected.as_deref(), Some("00002"));
        tx.commit().await.unwrap();
        second_device
            .set_active_rezka_paddon(apparatus, &actor, None)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        assert_eq!(
            super::super::output_paddon_assignment::selection_for_output(
                &mut tx, apparatus, &actor, &payload
            )
            .await
            .unwrap(),
            None
        );
        tx.commit().await.unwrap();
        store
            .set_active_rezka_paddon(apparatus, &actor, Some("00001"))
            .await
            .unwrap();
        sqlx::query("DELETE FROM mini_paddons WHERE code='00001'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            second_device
                .active_rezka_paddon(apparatus, &actor)
                .await
                .unwrap(),
            None
        );
        pool.close().await;
        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
    }
}
