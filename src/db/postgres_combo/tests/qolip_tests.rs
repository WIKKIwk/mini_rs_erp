use std::sync::Arc;

use sqlx::PgPool;

use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::combo::ComboReference;
use crate::core::qolip::{QolipError, QolipProductSpecUpsert, QolipService, QolipStorePort};
use crate::db::postgres_qolip::PostgresQolipStore;

pub(super) async fn assert_combo_mold_attachment(
    admin: &PgPool,
    runtime: &PgPool,
    combo: &ComboReference,
) {
    sqlx::raw_sql(
        "INSERT INTO mini_warehouses(id,name,parent_warehouse)
         VALUES ('combo-qolip','Qolip ombori','');
         INSERT INTO mini_warehouse_assignments
             (warehouse,warehouse_name,principal_role,principal_ref,assignment_kind)
         VALUES ('Qolip ombori','Qolip ombori','qolipchi','combo-clerk','warehouse');",
    )
    .execute(admin)
    .await
    .unwrap();
    let actor = Principal {
        role: PrincipalRole::Qolipchi,
        ref_: "combo-clerk".into(),
        display_name: "Qolipchi".into(),
        legal_name: "Qolipchi".into(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    let store = Arc::new(PostgresQolipStore::new(runtime.clone()));
    let service = QolipService::new(store.clone());
    let payload_before: serde_json::Value =
        sqlx::query_scalar("SELECT payload_json FROM mini_items WHERE code=$1")
            .bind(&combo.item_code)
            .fetch_one(runtime)
            .await
            .unwrap();
    let input = |code: &str, color: &str| QolipProductSpecUpsert {
        item_code: combo.item_code.clone(),
        item_name: format!("{} · v{}", combo.name, combo.revision),
        item_group: "Kombo".into(),
        warehouse: "Qolip ombori".into(),
        qolip_code: code.into(),
        size: 120,
        color: color.into(),
        ..Default::default()
    };
    // The task sheet saves all selected colors in one transaction, using the
    // real runtime database role (including the immutable catalog trigger).
    let saved = service
        .upsert_product_specs(
            vec![
                input("213213-1", "RED"),
                input("213213-2", "ORANGE"),
                input("213213-3", "YELLOW"),
            ],
            &actor,
        )
        .await
        .expect("attach a three-color mold set to an immutable combo revision");
    assert_eq!(saved.len(), 3);
    assert!(!saved[0].qolip_set_id.is_empty());
    assert!(saved.iter().all(|spec| spec.item_code == combo.item_code
        && spec.qolip_set_id == saved[0].qolip_set_id
        && spec.created_by_role == "qolipchi"));
    service
        .upsert_product_spec(input("213213-4", "GREEN"), &actor)
        .await
        .expect("single mold attachment also preserves combo identity");
    assert_eq!(
        service
            .upsert_product_specs(
                vec![input("213213-5", "BLUE"), input("213213-1", "RED")],
                &actor,
            )
            .await,
        Err(QolipError::QolipCodeConflict)
    );
    assert_eq!(
        store.product_specs(&combo.item_code).await.unwrap().len(),
        4
    );
    assert!(store.product_specs("A").await.unwrap().is_empty());
    assert!(store.product_specs("B").await.unwrap().is_empty());
    assert_eq!(
        service
            .required_qolips_for_order(&combo.item_code, &saved[0].item_name)
            .await
            .unwrap()
            .len(),
        4
    );
    for allowed in [None, Some(vec!["Qolip ombori".to_string()])] {
        let products = store
            .products("", 100, true, allowed.as_deref())
            .await
            .unwrap();
        let molds = products
            .iter()
            .filter(|product| product.code == combo.item_code)
            .collect::<Vec<_>>();
        assert_eq!(molds.len(), 4);
        assert!(
            molds
                .iter()
                .all(|product| product.first_qolip_code == "213213-1")
        );
    }
    let payload_after: serde_json::Value =
        sqlx::query_scalar("SELECT payload_json FROM mini_items WHERE code=$1")
            .bind(&combo.item_code)
            .fetch_one(runtime)
            .await
            .unwrap();
    assert_eq!(payload_after, payload_before);

    // Ordinary products retain their existing first-code cache behavior.
    let mut ordinary = input("ORDINARY-1", "RED");
    ordinary.item_code = "A".into();
    ordinary.item_name = "A".into();
    ordinary.item_group = "Tayyor mahsulot".into();
    service.upsert_product_spec(ordinary, &actor).await.unwrap();
    let cached: String = sqlx::query_scalar(
        "SELECT payload_json->>'qolip_first_code' FROM mini_items WHERE code='A'",
    )
    .fetch_one(runtime)
    .await
    .unwrap();
    assert_eq!(cached, "ORDINARY-1");
}
