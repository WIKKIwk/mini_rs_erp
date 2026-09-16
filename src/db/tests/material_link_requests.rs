use std::sync::Arc;

use crate::core::apparatus_standard::{ApparatusId, test_support::TestApparatusSpec};
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::chat::{ChatService, ChatStorePort, OrderFreezeChatEvent};
use crate::core::production_map::{ProductionMapStorePort, RawMaterialAssignment};
use crate::db::postgres::{apply_foundation_migration, postgres_test_database_options};
use crate::db::postgres_chat::PostgresChatStore;
use crate::db::postgres_production_map::PostgresProductionMapStore;
use crate::db::postgres_production_map::material_link_requests::{
    LinkCandidate, LinkError, LinkRequest, MaterialLinkStore,
};

const APPARATUS: &str = "apparatus:material-link:test";

fn actor(role: PrincipalRole, ref_: &str) -> Principal {
    Principal {
        role,
        ref_: ref_.into(),
        display_name: ref_.into(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    }
}

fn request(id: &str, candidates: Vec<LinkCandidate>) -> LinkRequest {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    LinkRequest {
        request_id: id.into(),
        status: "pending".into(),
        event_sequence: 1,
        order_id: "order-link".into(),
        order_number: "L001".into(),
        order_title: "Order".into(),
        apparatus_id: APPARATUS.into(),
        apparatus_name: "Machine".into(),
        requester_ref: "worker".into(),
        requester_display_name: "Worker".into(),
        mover_ref: "mover".into(),
        mover_display_name: "Mover".into(),
        candidates,
        selected_barcodes: vec![],
        decided_by_name: String::new(),
        decided_by_role: String::new(),
        decided_at_unix: 0,
        reason: String::new(),
        requested_at_unix: now,
        expires_at_unix: now + 1800,
    }
}

fn assignment(barcode: &str, order: &str) -> RawMaterialAssignment {
    RawMaterialAssignment {
        order_id: order.into(),
        apparatus_id: ApparatusId::new(APPARATUS).unwrap(),
        apparatus: APPARATUS.into(),
        barcode: barcode.into(),
        item_code: "FILM".into(),
        item_name: "Film".into(),
        item_group: "Rulon".into(),
        assigned_by_role: "admin".into(),
        assigned_by_ref: "admin".into(),
        assigned_by_display_name: "Admin".into(),
        assigned_at: "2026-09-16T00:00:00Z".into(),
    }
}

#[test]
fn material_link_permissions_and_explicit_selection() {
    let request = request(
        "permissions",
        vec![LinkCandidate {
            stock_id: "s1".into(),
            barcode: "R1".into(),
            item_name: "Film".into(),
            qty: 10.0,
            uom: "kg".into(),
            location_id: "state".into(),
            location_name: "State".into(),
            placement_version: 1,
            mover_ref: "mover".into(),
            mover_display_name: "Mover".into(),
        }],
    );
    assert!(request.can_decide(&actor(PrincipalRole::Admin, "admin")));
    assert!(request.can_decide(&actor(PrincipalRole::MaterialTaminotchi, "mover")));
    assert!(!request.can_decide(&actor(PrincipalRole::MaterialTaminotchi, "unrelated")));
    assert!(!request.can_decide(&actor(PrincipalRole::Aparatchi, "worker")));
    assert!(!request.can_decide(&actor(PrincipalRole::Supplier, "mover")));
    assert!(!request.can_read(&actor(PrincipalRole::Aparatchi, "unrelated")));
    assert!(request.validate_selection(&[]).is_err());
    assert!(request.validate_selection(&["OTHER".into()]).is_err());
    assert!(
        request
            .validate_selection(&["R1".into(), "r1".into()])
            .is_err()
    );
    assert!(request.validate_selection(&["R1".into()]).is_ok());
}

#[tokio::test]
async fn postgres_material_link_selection_shared_cards_and_stale_recovery() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let database = format!(
        "mini_rs_erp_test_material_link_{:016x}",
        rand::random::<u64>()
    );
    let admin = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("test admin connection");
    sqlx::query(&format!("CREATE DATABASE \"{database}\""))
        .execute(&admin)
        .await
        .unwrap();
    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, &database))
        .await
        .unwrap();
    apply_foundation_migration(&pool)
        .await
        .expect("apply migrations in isolated database");
    apply_foundation_migration(&pool)
        .await
        .expect("migration replay");
    super::seed_canonical_apparatus(&pool, TestApparatusSpec::laminate(APPARATUS, "Machine")).await;
    sqlx::raw_sql(r#"
        INSERT INTO mini_item_groups(name,parent_item_group) VALUES ('All Item Groups',NULL) ON CONFLICT DO NOTHING;
        INSERT INTO mini_item_groups(name,parent_item_group) VALUES ('Rulon','All Item Groups') ON CONFLICT DO NOTHING;
        INSERT INTO mini_items(code,name,item_group) VALUES ('FILM','Film','Rulon');
        INSERT INTO mini_production_maps(id,product_code,title,map_json) VALUES
            ('order-link','FILM','Order','{}'),('order-other','FILM','Other','{}');
        INSERT INTO mini_warehouses(id,name) VALUES ('wh','Warehouse');
        INSERT INTO mini_factory_locations(id,name) VALUES ('floor','Floor');
        INSERT INTO mini_factory_location_apparatus_links(location_id,apparatus_id)
            VALUES ('floor','apparatus:material-link:test');
        INSERT INTO mini_inventory_locations(id,kind,name,factory_location_id)
            VALUES ('link-state','state','State','floor')
            ON CONFLICT (factory_location_id) WHERE factory_location_id IS NOT NULL DO NOTHING;
        INSERT INTO mini_raw_material_stock(id,warehouse,item_code,item_name,barcode,qty,uom,status,payload_json)
            SELECT 'stock-'||i,'Warehouse','FILM','Film','R'||i,10,'kg','available','{}' FROM generate_series(1,6) i;
        INSERT INTO mini_inventory_placements(asset_kind,asset_ref,physical_location_id,updated_by_role,updated_by_ref,updated_by_name)
            SELECT 'raw_material','stock-'||i,location.id,'material_taminotchi',
                CASE WHEN i=6 THEN 'other-mover' ELSE 'mover' END,'Mover'
            FROM generate_series(1,6) i,mini_inventory_locations location WHERE location.factory_location_id='floor'
            ON CONFLICT(asset_kind,asset_ref) DO UPDATE SET physical_location_id=excluded.physical_location_id,
                updated_by_role=excluded.updated_by_role,updated_by_ref=excluded.updated_by_ref,updated_by_name=excluded.updated_by_name;
    "#).execute(&pool).await.unwrap();
    let store = MaterialLinkStore::new(pool.clone());
    let chat = ChatService::new(Arc::new(PostgresChatStore::new(pool.clone())));
    let admin_actor = actor(PrincipalRole::Admin, "admin");
    let mover = actor(PrincipalRole::MaterialTaminotchi, "mover");
    let candidates = store.candidates(APPARATUS).await.unwrap();
    assert_eq!(candidates.len(), 6);
    let first = request(
        "subset",
        candidates
            .iter()
            .filter(|c| matches!(c.barcode.as_str(), "R1" | "R2"))
            .cloned()
            .collect(),
    );
    let first = store.create(first).await.unwrap();
    assert_eq!(first.status, "pending");
    let replay = store
        .create(request("duplicate", first.candidates.clone()))
        .await
        .unwrap();
    assert_eq!(replay.request_id, first.request_id);
    assert!(matches!(
        store
            .decide(
                &first.request_id,
                &actor(PrincipalRole::MaterialTaminotchi, "other-mover"),
                false,
                vec![]
            )
            .await,
        Err(LinkError::Forbidden)
    ));
    assert!(matches!(
        store
            .decide(&first.request_id, &admin_actor, true, vec![])
            .await,
        Err(LinkError::Selection)
    ));
    // Even the admin may only approve rolls included in the worker's request.
    assert!(matches!(
        store
            .decide(
                &first.request_id,
                &admin_actor,
                true,
                vec![assignment("R3", "order-link")],
            )
            .await,
        Err(LinkError::Selection)
    ));

    // A partial delivery must be durable and retry without duplicating the admin card.
    sqlx::raw_sql("CREATE FUNCTION reject_test_card() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN IF NEW.message_type='material_link_request' AND EXISTS (
            SELECT 1 FROM mini_chat_conversation_members m JOIN mini_chat_principals p USING(principal_id)
            WHERE m.conversation_id=NEW.conversation_id AND p.principal_role='material_taminotchi'
        ) THEN RAISE EXCEPTION 'injected card failure'; END IF; RETURN NEW; END $$;
        CREATE TRIGGER reject_test_card BEFORE INSERT ON mini_chat_messages FOR EACH ROW EXECUTE FUNCTION reject_test_card();")
        .execute(&pool).await.unwrap();
    store.deliver_pending(&chat).await.unwrap();
    let delivered: i64 = sqlx::query_scalar(
        "SELECT delivered_revision FROM mini_material_link_requests WHERE id='subset'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(delivered, 0);
    sqlx::raw_sql(
        "DROP TRIGGER reject_test_card ON mini_chat_messages;
        UPDATE mini_material_link_requests SET retry_at=now() WHERE id='subset';",
    )
    .execute(&pool)
    .await
    .unwrap();
    store
        .deliver_pending(&chat)
        .await
        .expect("both cards delivered after retry");
    store
        .deliver_pending(&chat)
        .await
        .expect("delivery idempotent");
    let card_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mini_chat_messages WHERE metadata_json->>'request_id'='subset'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(card_count, 2);
    let requested_rolls: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT metadata_json->'candidates' FROM mini_chat_messages
         WHERE metadata_json->>'request_id'='subset'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    for rolls in requested_rolls {
        let barcodes: Vec<_> = rolls
            .as_array()
            .unwrap()
            .iter()
            .map(|roll| roll["barcode"].as_str().unwrap())
            .collect();
        assert_eq!(barcodes, ["R1", "R2"]);
    }
    let recipients: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT p.principal_role,p.principal_ref
        FROM mini_chat_conversation_members m JOIN mini_chat_principals p USING(principal_id)
        WHERE p.principal_role <> 'aparatchi' ORDER BY p.principal_role",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        recipients,
        vec![
            ("admin".into(), "admin".into()),
            ("material_taminotchi".into(), "mover".into())
        ]
    );

    // A multi-roll failure must roll back the first assignment and the decision.
    sqlx::raw_sql("CREATE FUNCTION reject_test_roll() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN IF NEW.barcode='R2' THEN RAISE EXCEPTION 'injected failure'; END IF; RETURN NEW; END $$;
        CREATE TRIGGER reject_test_roll BEFORE INSERT ON mini_raw_material_assignments FOR EACH ROW EXECUTE FUNCTION reject_test_roll();")
        .execute(&pool).await.unwrap();
    assert!(
        store
            .decide(
                "subset",
                &admin_actor,
                true,
                vec![
                    assignment("R1", "order-link"),
                    assignment("R2", "order-link")
                ]
            )
            .await
            .is_err()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_raw_material_assignments")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(store.get("subset").await.unwrap().status, "pending");
    sqlx::query("DROP TRIGGER reject_test_roll ON mini_raw_material_assignments")
        .execute(&pool)
        .await
        .unwrap();

    let approved = store
        .decide(
            "subset",
            &admin_actor,
            true,
            vec![assignment("R1", "order-link")],
        )
        .await
        .unwrap();
    assert_eq!(approved.status, "approved");
    assert_eq!(approved.selected_barcodes, vec!["R1"]);
    let barcodes: Vec<String> =
        sqlx::query_scalar("SELECT barcode FROM mini_raw_material_assignments")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(barcodes, vec!["R1"]); // R2 and all other state rolls remain unlinked.
    assert_eq!(
        store
            .decide("subset", &mover, false, vec![])
            .await
            .unwrap()
            .status,
        "approved"
    );
    store.deliver_pending(&chat).await.unwrap();
    let statuses: Vec<String> = sqlx::query_scalar("SELECT metadata_json->>'status' FROM mini_chat_messages WHERE metadata_json->>'request_id'='subset'")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(statuses, vec!["approved", "approved"]);
    // Delayed creation replay must not overwrite a newer decision in either chat.
    let conversation: String = sqlx::query_scalar("SELECT conversation_id FROM mini_chat_messages WHERE metadata_json->>'request_id'='subset' LIMIT 1")
        .fetch_one(&pool).await.unwrap();
    PostgresChatStore::new(pool.clone())
        .upsert_material_link_card(
            &actor(PrincipalRole::Aparatchi, "worker"),
            &conversation,
            &first,
        )
        .await
        .unwrap();
    let pending_cards: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mini_chat_messages WHERE metadata_json->>'status'='pending'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pending_cards, 0);

    // The shared SQL helper must retain the existing freeze-card type and update contract.
    let freeze = OrderFreezeChatEvent {
        event_sequence: 1,
        event_id: "freeze-event".into(),
        request_id: "freeze-regression".into(),
        status: "pending".into(),
        order_id: "order-link".into(),
        order_number: "L001".into(),
        order_title: "Order".into(),
        requester_role: "aparatchi".into(),
        requester_ref: "worker".into(),
        requester_display_name: "Worker".into(),
        target_session_id: "session-1".into(),
        target_apparatus: APPARATUS.into(),
        target_worker_role: "aparatchi".into(),
        target_worker_ref: "worker".into(),
        target_worker_display_name: "Worker".into(),
        requested_at_unix: 1,
        transitioned_at_unix: 1,
        attempts: 0,
    };
    let chat_store = PostgresChatStore::new(pool.clone());
    let worker = actor(PrincipalRole::Aparatchi, "worker");
    let inserted = chat_store
        .upsert_order_freeze_card(&worker, &conversation, &freeze)
        .await
        .unwrap();
    assert_eq!(inserted.message.message_type, "order_freeze_request");
    let terminal = OrderFreezeChatEvent {
        event_sequence: 2,
        status: "cancelled".into(),
        ..freeze.clone()
    };
    let updated = chat_store
        .upsert_order_freeze_card(&worker, &conversation, &terminal)
        .await
        .unwrap();
    let delayed = chat_store
        .upsert_order_freeze_card(&worker, &conversation, &freeze)
        .await
        .unwrap();
    assert_eq!(updated.message.message_id, inserted.message.message_id);
    assert_eq!(delayed.message.metadata["status"], "cancelled");
    assert_eq!(updated.message.message_type, "order_freeze_request");

    let r2 = candidates
        .iter()
        .find(|c| c.barcode == "R2")
        .unwrap()
        .clone();
    store
        .create(request("external", vec![r2.clone()]))
        .await
        .unwrap();
    PostgresProductionMapStore::new(pool.clone())
        .put_raw_material_assignment(assignment("R2", "order-other"))
        .await
        .unwrap();
    store.deliver_pending(&chat).await.unwrap(); // Background reconciliation, without worker polling.
    assert_eq!(store.get("external").await.unwrap().status, "stale");
    assert!(
        !store
            .candidates(APPARATUS)
            .await
            .unwrap()
            .iter()
            .any(|c| c.barcode == "R2")
    );
    let stale_cards: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_chat_messages WHERE metadata_json->>'request_id'='external' AND metadata_json->>'status'='stale'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(stale_cards, 2);

    let r3 = candidates
        .iter()
        .find(|c| c.barcode == "R3")
        .unwrap()
        .clone();
    store
        .create(request("race", vec![r3.clone()]))
        .await
        .unwrap();
    let (approved, rejected) = tokio::join!(
        store.decide(
            "race",
            &admin_actor,
            true,
            vec![assignment("R3", "order-link")]
        ),
        store.decide("race", &mover, false, vec![])
    );
    let approved = approved.unwrap();
    let rejected = rejected.unwrap();
    assert_eq!(approved.status, rejected.status);
    assert_eq!(approved.event_sequence, 2);
    store.deliver_pending(&chat).await.unwrap();

    let r4 = candidates
        .iter()
        .find(|c| c.barcode == "R4")
        .unwrap()
        .clone();
    store
        .create(request("reject", vec![r4.clone()]))
        .await
        .unwrap();
    assert_eq!(
        store
            .decide("reject", &mover, false, vec![])
            .await
            .unwrap()
            .status,
        "cancelled"
    );
    store.deliver_pending(&chat).await.unwrap();
    let cancelled_cards: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_chat_messages WHERE metadata_json->>'request_id'='reject' AND metadata_json->>'status'='cancelled'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(cancelled_cards, 2);
    let retry = store
        .create(request("retry", vec![r4.clone()]))
        .await
        .unwrap();
    assert_eq!(retry.request_id, "retry");
    assert_eq!(retry.status, "pending");
    store
        .cancel("retry", &actor(PrincipalRole::Aparatchi, "worker"))
        .await
        .unwrap();
    let mut expired = request("expired", vec![r4.clone()]);
    expired.expires_at_unix = 1;
    assert_eq!(store.create(expired).await.unwrap().status, "expired");
    store.create(request("moved", vec![r4])).await.unwrap();
    sqlx::query("UPDATE mini_inventory_placements SET version=version+1 WHERE asset_ref='stock-4'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(store.get("moved").await.unwrap().status, "stale");

    let r5 = candidates
        .iter()
        .find(|c| c.barcode == "R5")
        .unwrap()
        .clone();
    store.create(request("closed", vec![r5])).await.unwrap();
    sqlx::query(
        "UPDATE mini_production_maps SET lifecycle_status='cancelled' WHERE id='order-link'",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(store.get("closed").await.unwrap().status, "stale");
    pool.close().await;
    assert!(database.starts_with("mini_rs_erp_test_material_link_"));
    sqlx::query(&format!("DROP DATABASE \"{database}\""))
        .execute(&admin)
        .await
        .expect("remove this test's isolated database");
    admin.close().await;
}
