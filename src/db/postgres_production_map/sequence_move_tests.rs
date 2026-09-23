use super::sequence_move::commit;
use crate::core::apparatus_standard::{ApparatusId, RuntimeApparatusConfiguration};
use crate::core::production_map::*;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{collections::BTreeMap, sync::Arc};

const APP: &str = "apparatus:default:bosma_8";

async fn fixture() -> (PgPool, Arc<RuntimeApparatusConfiguration>, SequenceMove) {
    let url = std::env::var("MINI_ERP_REORDER_TEST_DATABASE_URL")
        .expect("use an isolated local PostgreSQL database");
    let parsed = reqwest::Url::parse(&url);
    // No production host is permitted even if someone passes the wrong env.
    assert!(parsed.is_ok());
    let parsed = parsed.unwrap();
    assert!(matches!(parsed.host_str(), Some("127.0.0.1" | "localhost")));
    let schema = format!("queue_reorder_test_{:032x}", rand::random::<u128>());
    let bootstrap = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&bootstrap)
        .await
        .unwrap();
    bootstrap.close().await;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .after_connect(move |conn, _| {
            let statement =
                format!("SELECT set_config('search_path','{schema}',false), set_config('application_name','{schema}',false)");
            Box::pin(async move {
                sqlx::query(&statement).execute(conn).await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    sqlx::raw_sql("
        CREATE TABLE mini_apparatus (id TEXT PRIMARY KEY, name TEXT NOT NULL);
        CREATE TABLE mini_production_maps (id TEXT PRIMARY KEY, map_json JSONB NOT NULL, updated_at TIMESTAMPTZ DEFAULT now());
        CREATE TABLE mini_queue_sequences (apparatus TEXT, canonical_apparatus_id TEXT UNIQUE, order_ids JSONB, updated_at TIMESTAMPTZ);
        CREATE TABLE mini_queue_states (canonical_apparatus_id TEXT, order_id TEXT, state TEXT);
        CREATE TABLE mini_order_control_states (order_id TEXT PRIMARY KEY, state TEXT);
        CREATE TABLE mini_print_preflight_holds (canonical_apparatus_id TEXT, order_id TEXT, status TEXT);
    ").execute(&pool).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/postgres/0132_queue_reorder_commands.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO mini_apparatus VALUES ($1,'8 color')")
        .bind(APP)
        .execute(&pool)
        .await
        .unwrap();
    let mut maps = Vec::new();
    for id in ["a", "b", "c", "frozen"] {
        let map: ProductionMapDefinition = serde_json::from_value(serde_json::json!({
            "id":id,"product_code":id,"title":id,"code":id,
            "nodes":[{"id":"start","kind":"start","title":"Start"},
                {"id":"print","kind":"apparatus","title":"Print","apparatus_id":APP},
                {"id":"end","kind":"end","title":"End"}],
            "edges":[{"from":"start","to":"print"},{"from":"print","to":"end"}]
        }))
        .unwrap();
        sqlx::query("INSERT INTO mini_production_maps (id,map_json) VALUES ($1,$2)")
            .bind(id)
            .bind(serde_json::json!(map))
            .execute(&pool)
            .await
            .unwrap();
        maps.push(map);
    }
    let stored = vec!["a".into(), "b".into(), "c".into(), "frozen".into()];
    sqlx::query("INSERT INTO mini_queue_sequences VALUES ('8 color',$1,$2,now())")
        .bind(APP)
        .bind(serde_json::json!(stored))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO mini_order_control_states VALUES ('frozen','frozen')")
        .execute(&pool)
        .await
        .unwrap();
    let canonical = TestCanonicalApparatusResolver::standard()
        .resolve(&ApparatusId::new(APP).unwrap())
        .await
        .unwrap()
        .unwrap();
    let state = SequenceMoveState::from_data(
        &canonical,
        &maps,
        &stored,
        &BTreeMap::new(),
        &std::collections::BTreeSet::from(["frozen".into()]),
        &Default::default(),
    );
    let command = SequenceMove {
        apparatus: APP.into(),
        order_id: "c".into(),
        before_order_id: Some("a".into()),
        after_order_id: None,
        expected_version: state.version(),
        idempotency_key: "move-1".into(),
    };
    (pool, canonical, command)
}

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "admin".into(),
        ref_: "test".into(),
        display_name: "Test".into(),
    }
}

#[tokio::test]
#[ignore = "requires isolated local PostgreSQL; never uses the application DB"]
async fn postgres_reorder_parallel_retry_and_atomic_rollback() {
    let (pool, canonical, command) = fixture().await;
    let user = actor();
    let (one, two) = tokio::join!(
        commit(&pool, &canonical, &command, &user),
        commit(&pool, &canonical, &command, &user)
    );
    assert_eq!(one, two);
    let result = one.unwrap();
    assert_eq!(result.order_ids, vec!["c", "a", "b"]);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_queue_reorder_commands")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let mut second = command.clone();
    second.idempotency_key = "move-2".into();
    second.expected_version = result.version;
    second.before_order_id = None;
    // Receipt failure MUST roll back the queue update too.
    sqlx::query("ALTER TABLE mini_queue_reorder_commands ADD CONSTRAINT reject_second CHECK (idempotency_key <> 'move-2')")
        .execute(&pool).await.unwrap();
    assert_eq!(
        commit(&pool, &canonical, &second, &actor()).await,
        Err(ProductionMapError::StoreFailed)
    );
    let stored: serde_json::Value =
        sqlx::query_scalar("SELECT order_ids FROM mini_queue_sequences")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, serde_json::json!(["c", "a", "b"]));
    second.idempotency_key = "move-3".into();
    commit(&pool, &canonical, &second, &actor()).await.unwrap();
    commit(&pool, &canonical, &command, &actor()).await.unwrap(); // delayed retry cannot undo move-3
    let stored: serde_json::Value =
        sqlx::query_scalar("SELECT order_ids FROM mini_queue_sequences")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, serde_json::json!(["a", "b", "c"]));
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT state FROM mini_order_control_states WHERE order_id='frozen'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        "frozen"
    );
    pool.close().await;
}

#[tokio::test]
#[ignore = "requires isolated local PostgreSQL; never uses the application DB"]
async fn postgres_reorder_distinct_clients_conflict_without_lost_update() {
    let (pool, canonical, one) = fixture().await;
    let mut two = one.clone();
    two.idempotency_key = "different-client".into();
    two.before_order_id = Some("b".into());
    let user = actor();
    let (a, b) = tokio::join!(
        commit(&pool, &canonical, &one, &user),
        commit(&pool, &canonical, &two, &user)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert_eq!(
        if a.is_err() { a } else { b },
        Err(ProductionMapError::QueueReorderConflict)
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mini_queue_reorder_commands")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    pool.close().await;
}

#[tokio::test]
#[ignore = "requires isolated local PostgreSQL; never uses the application DB"]
async fn postgres_reorder_revalidates_changes_committed_while_waiting_for_locks() {
    for change in ["start", "freeze", "hold", "map"] {
        let (pool, canonical, command) = fixture().await;
        let mut writer = pool.begin().await.unwrap();
        super::transaction_locks::lock_order_and_apparatuses_tx(&mut writer, "a", &[APP])
            .await
            .unwrap();
        let pending = {
            let pool = pool.clone();
            tokio::spawn(async move { commit(&pool, &canonical, &command, &actor()).await })
        };
        // Verify the move really is waiting before committing the changed
        // state. This catches a transaction snapshot taken before lock wait.
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let waiting: bool = sqlx::query_scalar(
                    "SELECT EXISTS (
                    SELECT 1 FROM pg_stat_activity WHERE application_name=current_schema()
                    AND wait_event_type='Lock' AND wait_event='advisory')",
                )
                .fetch_one(&pool)
                .await
                .unwrap();
                if waiting {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("move must wait for the existing mutation lock");
        match change {
            "start" => {
                sqlx::query("INSERT INTO mini_queue_states VALUES ($1,'a','in_progress')")
                    .bind(APP)
                    .execute(&mut *writer)
                    .await
                    .unwrap();
            }
            "freeze" => {
                sqlx::query("INSERT INTO mini_order_control_states VALUES ('a','frozen')")
                    .execute(&mut *writer)
                    .await
                    .unwrap();
            }
            "hold" => {
                sqlx::query("INSERT INTO mini_print_preflight_holds VALUES ($1,'a','running')")
                    .bind(APP)
                    .execute(&mut *writer)
                    .await
                    .unwrap();
            }
            "map" => {
                sqlx::query(
                    "UPDATE mini_production_maps SET map_json=jsonb_set(map_json,
                '{nodes,1,apparatus_id}', '\"apparatus:default:asset-007\"'::jsonb) WHERE id='a'",
                )
                .execute(&mut *writer)
                .await
                .unwrap();
            }
            _ => unreachable!(),
        }
        writer.commit().await.unwrap();
        assert_eq!(
            pending.await.unwrap(),
            Err(ProductionMapError::QueueReorderConflict),
            "{change}"
        );
        let stored: serde_json::Value =
            sqlx::query_scalar("SELECT order_ids FROM mini_queue_sequences")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            stored,
            serde_json::json!(["a", "b", "c", "frozen"]),
            "{change}"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mini_queue_reorder_commands")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0,
            "{change}"
        );
        pool.close().await;
    }
}
