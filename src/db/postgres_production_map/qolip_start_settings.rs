use crate::core::production_map::{
    AutomaticQolipConfirmation, ProductionMapError, QolipStartSettings, QueueActionActor,
};
use sqlx::{PgPool, Postgres, Transaction};

pub(super) async fn load(pool: &PgPool) -> Result<QolipStartSettings, ProductionMapError> {
    let (automatic_confirmation_enabled, revision) = sqlx::query_as(
        "SELECT automatic_confirmation_enabled, revision FROM mini_qolip_start_settings WHERE singleton",
    ).fetch_one(pool).await.map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(QolipStartSettings {
        automatic_confirmation_enabled,
        revision,
    })
}

pub(super) async fn update(
    pool: &PgPool,
    enabled: bool,
    actor: &QueueActionActor,
) -> Result<QolipStartSettings, ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let (previous, old_revision): (bool, i64) = sqlx::query_as(
        "SELECT automatic_confirmation_enabled, revision FROM mini_qolip_start_settings WHERE singleton FOR UPDATE",
    ).fetch_one(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    let revision = if previous == enabled {
        old_revision
    } else {
        let revision: i64 = sqlx::query_scalar(
            "UPDATE mini_qolip_start_settings SET automatic_confirmation_enabled=$1, revision=revision+1,
             updated_at=now(), updated_by_ref=$2, updated_by_display_name=$3 WHERE singleton RETURNING revision",
        ).bind(enabled).bind(actor.ref_.trim()).bind(actor.display_name.trim())
            .fetch_one(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        let event_id = format!("qolip-start-policy:{:032x}", rand::random::<u128>());
        sqlx::query("INSERT INTO mini_engine_events (event_id,domain,action,entity_id,actor_key,idempotency_key,payload_json)
            VALUES ($1,'qolip','start_policy','qolip-start-settings',$2,$1,$3)")
            .bind(&event_id).bind(format!("{}:{}", actor.role, actor.ref_))
            .bind(serde_json::json!({"before":previous,"automatic_confirmation_enabled":enabled,"revision":revision,"actor_name":actor.display_name}))
            .execute(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        revision
    };
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(QolipStartSettings {
        automatic_confirmation_enabled: enabled,
        revision,
    })
}

pub(super) async fn require_automatic_tx(
    tx: &mut Transaction<'_, Postgres>,
    confirmation: &AutomaticQolipConfirmation,
) -> Result<(), ProductionMapError> {
    // The policy writer takes FOR UPDATE. Keep its revision fixed until the
    // start, tooling allocation and history commit together.
    let (automatic_confirmation_enabled, revision) = sqlx::query_as(
        "SELECT automatic_confirmation_enabled,revision FROM mini_qolip_start_settings WHERE singleton FOR SHARE",
    ).fetch_one(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    confirmation.require_policy(&QolipStartSettings {
        automatic_confirmation_enabled,
        revision,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL on a disposable PostgreSQL server"]
    async fn automatic_policy_persists_and_toggle_waits_for_admitted_start() {
        let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .expect("explicit disposable PostgreSQL test URL");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(3)
            .connect(&url)
            .await
            .unwrap();
        sqlx::raw_sql("CREATE TABLE mini_engine_events(event_id text,domain text,action text,entity_id text,actor_key text,idempotency_key text,payload_json jsonb);
            CREATE FUNCTION mini_notify_production_live() RETURNS trigger LANGUAGE plpgsql AS 'BEGIN RETURN NULL; END';")
            .execute(&pool).await.unwrap();
        sqlx::raw_sql(include_str!(
            "../../../migrations/postgres/0168_qolip_start_settings.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(load(&pool).await.unwrap(), QolipStartSettings::default());
        let actor = QueueActionActor {
            role: "admin".into(),
            ref_: "policy-admin".into(),
            display_name: "Admin".into(),
        };
        let enabled = update(&pool, true, &actor).await.unwrap();
        assert_eq!(
            enabled,
            QolipStartSettings {
                automatic_confirmation_enabled: true,
                revision: 1
            }
        );
        assert_eq!(load(&pool).await.unwrap(), enabled);
        assert_eq!(update(&pool, true, &actor).await.unwrap(), enabled);
        let confirmation = AutomaticQolipConfirmation {
            method: "automatic".into(),
            set_id: "950100000001".into(),
            fingerprint: "a".repeat(64),
            policy_revision: 1,
        };
        let mut start = pool.begin().await.unwrap();
        require_automatic_tx(&mut start, &confirmation)
            .await
            .unwrap();
        let update_pool = pool.clone();
        let mut toggling = tokio::spawn(async move { update(&update_pool, false, &actor).await });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), &mut toggling)
                .await
                .is_err(),
            "a policy change must wait for the admitted start transaction"
        );
        start.commit().await.unwrap();
        let disabled = tokio::time::timeout(std::time::Duration::from_secs(5), toggling)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            disabled,
            QolipStartSettings {
                automatic_confirmation_enabled: false,
                revision: 2
            }
        );
        let mut stale = pool.begin().await.unwrap();
        assert_eq!(
            require_automatic_tx(&mut stale, &confirmation).await,
            Err(ProductionMapError::QolipAutomaticConfirmationDisabled)
        );
        stale.rollback().await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM mini_engine_events WHERE action='start_policy'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            2
        );
        pool.close().await;
    }
}
