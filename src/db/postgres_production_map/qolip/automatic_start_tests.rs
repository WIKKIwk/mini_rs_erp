use super::*;
use crate::core::production_map::{AutomaticQolipConfirmation, automatic_qolip_set_fingerprint};
use crate::core::qolip::QolipProductSpec;

/// This test uses only an explicitly supplied disposable database and temporary
/// domain tables. It verifies the last authoritative boundary, after the
/// optimistic read and preparations have already succeeded.
#[tokio::test]
#[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL on a disposable PostgreSQL server"]
async fn automatic_fingerprint_and_common_product_binding_are_rechecked_at_commit() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .expect("explicit disposable PostgreSQL test URL");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::raw_sql("CREATE FUNCTION mini_qolip_assigned_warehouse(text,text) RETURNS text LANGUAGE SQL AS 'SELECT ''''::text';
        CREATE TEMP TABLE mini_production_maps(id text,product_code text,map_json jsonb) ON COMMIT DROP;
        CREATE TEMP TABLE mini_qolip_product_specs(qolip_code text,qolip_set_id text,payload_json jsonb,size integer) ON COMMIT DROP;
        CREATE TEMP TABLE mini_qolip_effective_product_molds(qolip_code text,item_code_key text,is_extension boolean) ON COMMIT DROP;
        CREATE TEMP TABLE mini_qolip_locations(qolip_code text,item_code text,size integer,payload_json jsonb,created_by_role text,created_by_ref text) ON COMMIT DROP;
        CREATE TEMP TABLE mini_qolip_checkouts(qolip_code text,item_code text,size integer,payload_json jsonb,status text,issued_by_role text,issued_by_ref text) ON COMMIT DROP;
        INSERT INTO mini_production_maps VALUES('order','ITEM','{\"products\":[{\"item_code\":\"ITEM\"},{\"item_code\":\"SECOND\"}]}');
        INSERT INTO mini_qolip_product_specs VALUES('PLATE','950100000001','{\"warehouse\":\"Warehouse\",\"color\":\"BLACK\"}',420);
        INSERT INTO mini_qolip_effective_product_molds VALUES('PLATE','item',false),('PLATE','second',false);")
        .execute(&mut *tx).await.unwrap();
    let spec = QolipProductSpec {
        qolip_code: "PLATE".into(),
        qolip_set_id: "950100000001".into(),
        warehouse: "Warehouse".into(),
        size: 420,
        color: "BLACK".into(),
        ..Default::default()
    };
    let confirmation = AutomaticQolipConfirmation {
        method: "automatic".into(),
        set_id: spec.set_id(),
        fingerprint: automatic_qolip_set_fingerprint(&spec.set_id(), &[spec.clone()]),
        policy_revision: 1,
    };
    let session = OrderRunSession {
        session_id: "start".into(),
        apparatus: "apparatus:default:bosma_7".into(),
        order_id: "order".into(),
        stage_node_id: "print".into(),
        status: OrderRunStatus::Active,
        worker_role: "aparatchi".into(),
        worker_ref: "worker".into(),
        worker_display_name: "Worker".into(),
        started_at_unix: 1,
        updated_at_unix: 1,
        payload_json: serde_json::json!({
            "qolip_lock_owner":true,"qolip_codes":["PLATE"],"qolip_set_id":spec.set_id(),
            "qolip_confirmation":confirmation,
            "qolip_set_snapshot":{"members":[{"qolip_code":"PLATE","size":420,"color":"BLACK"}]},
        }),
    };
    validate_automatic_qolip_set_tx(&mut tx, &session)
        .await
        .unwrap();
    for change in [
        "UPDATE mini_qolip_product_specs SET size=421",
        "UPDATE mini_qolip_product_specs SET payload_json=jsonb_set(payload_json,'{color}','\"CYAN\"')",
        "UPDATE mini_qolip_product_specs SET payload_json=jsonb_set(payload_json,'{warehouse}','\"Moved\"')",
        "UPDATE mini_qolip_effective_product_molds SET is_extension=true WHERE item_code_key='item'",
        "DELETE FROM mini_qolip_effective_product_molds WHERE item_code_key='second'",
    ] {
        sqlx::query("SAVEPOINT changed_catalog")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query(change).execute(&mut *tx).await.unwrap();
        assert_eq!(
            validate_automatic_qolip_set_tx(&mut tx, &session).await,
            Err(ProductionMapError::QolipSetChanged),
            "{change}"
        );
        sqlx::query("ROLLBACK TO SAVEPOINT changed_catalog")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    let reference = serde_json::json!({"id":"combo-master","item_code":"ITEM",
        "name":"Combined","revision":1,"layout_fingerprint":"a".repeat(64)});
    sqlx::raw_sql("CREATE TEMP TABLE mini_combo_revisions(item_code text,reference_json jsonb) ON COMMIT DROP;
        DELETE FROM mini_qolip_effective_product_molds WHERE item_code_key='second';")
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO mini_combo_revisions VALUES ('ITEM',$1)")
        .bind(&reference)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE mini_production_maps SET map_json=jsonb_set(map_json,'{combo}',$1)")
        .bind(&reference)
        .execute(&mut *tx)
        .await
        .unwrap();
    validate_automatic_qolip_set_tx(&mut tx, &session)
        .await
        .unwrap();
    sqlx::query("UPDATE mini_production_maps SET map_json=jsonb_set(map_json,'{combo,item_code}','\"CHILD\"')")
        .execute(&mut *tx).await.unwrap();
    assert_eq!(
        validate_automatic_qolip_set_tx(&mut tx, &session).await,
        Err(ProductionMapError::QolipCodeMismatch)
    );
    tx.rollback().await.unwrap();
    pool.close().await;
}
