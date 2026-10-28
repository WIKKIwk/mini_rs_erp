use async_trait::async_trait;

use crate::core::auth::models::{Principal, PrincipalRole};

use super::memory_store::MemoryQolipStore;
use super::models::{
    QolipBlock, QolipCellQr, QolipCheckout, QolipError, QolipLocation, QolipLocationUpsert,
    QolipProduct, QolipProductSpec, QolipProductSpecUpsert,
};
use super::ports::QolipStorePort;
use super::service::QolipService;

#[tokio::test]
async fn named_combo_uses_only_its_revision_tooling_not_component_molds() {
    use crate::core::production_map::ProductionMapDefinition;
    let store = std::sync::Arc::new(MemoryQolipStore::new());
    let products = crate::core::order_layout::tests::products()[..2].to_vec();
    let mut catalog = products.iter().map(|p| QolipProduct {
        code: p.item_code.clone(), name: p.product.clone(),
        item_group: "Tayyor mahsulot".into(), ..Default::default()
    }).collect::<Vec<_>>();
    for code in ["COMBO-R1", "COMBO-R2"] {
        catalog.push(QolipProduct { code: code.into(), name: "Mixed layout".into(),
            item_group: "Kombo".into(), ..Default::default() });
    }
    store.seed_products(catalog).await;
    let spec = |item: &str, code: &str, set: &str| QolipProductSpec {
        item_code: item.into(), item_name: if item.starts_with("COMBO") { "Mixed layout".into() }
            else { products.iter().find(|p| p.item_code == item).unwrap().product.clone() },
        item_group: if item.starts_with("COMBO") { "Kombo".into() } else { "Tayyor mahsulot".into() },
        warehouse: "Molds".into(), qolip_set_id: set.into(), qolip_code: code.into(),
        color: "BLACK".into(), size: 650, ..Default::default()
    };
    for (index, product) in products.iter().enumerate() {
        store.put_product_spec(spec(&product.item_code, &format!("CHILD-{index}"), &format!("child-set-{index}")))
            .await.unwrap();
    }
    let mut map: ProductionMapDefinition = serde_json::from_value(serde_json::json!({
        "id":"zakaz-combo-tooling", "product_code":"COMBO-R1", "title":"Mixed layout", "products":products,
        "combo":{"id":"combo-master", "item_code":"COMBO-R1", "name":"Mixed layout", "revision":1,"layout_fingerprint":"test-layout"},
        "nodes":[],"edges":[]
    })).unwrap();
    let service = QolipService::new(store.clone());
    assert_eq!(service.required_qolips_for_production_map(&map).await,
        Err(QolipError::ComboQolipRequired), "child molds cannot satisfy a named combo");
    store.put_product_spec(spec("COMBO-R1", "COMBO-MOLD", "combo-set")).await.unwrap();
    let required = service.required_qolips_for_production_map(&map).await.unwrap();
    assert_eq!(required.iter().map(|s| s.qolip_code.as_str()).collect::<Vec<_>>(), vec!["COMBO-MOLD"]);
    map.combo.as_mut().unwrap().item_code = "COMBO-R2".into();
    map.combo.as_mut().unwrap().revision = 2;
    map.product_code = "COMBO-R2".into();
    assert_eq!(service.required_qolips_for_production_map(&map).await, Err(QolipError::ComboQolipRequired),
        "a revised layout cannot borrow an earlier revision's molds");
    map.combo = None;
    assert_eq!(service.required_qolips_for_production_map(&map).await, Err(QolipError::QolipCodeMismatch),
        "legacy multi-product layouts retain the existing common-set policy");
}

#[tokio::test]
async fn multi_product_requires_a_complete_shared_mold_set() {
    let store = std::sync::Arc::new(MemoryQolipStore::new());
    let products = crate::core::order_layout::tests::products()[..2].to_vec();
    store.seed_products(products.iter().map(|p| QolipProduct {
        code: p.item_code.clone(), name: p.product.clone(),
        item_group: "Tayyor mahsulot".into(), ..Default::default()
    }).collect()).await;
    store.put_product_specs(["BLACK", "CYAN"].into_iter().map(|color| QolipProductSpec {
        item_code: products[0].item_code.clone(), item_name: products[0].product.clone(),
        item_group: "Tayyor mahsulot".into(), warehouse: "Molds".into(),
        qolip_set_id: "mixed-set".into(), qolip_code: format!("MIX-{color}"),
        color: color.into(), size: 650, ..Default::default()
    }).collect()).await.unwrap();
    let service = QolipService::new(store.clone());
    assert_eq!(service.required_qolips_for_layout(&products, "", "").await,
        Err(QolipError::QolipCodeMismatch));
    let set = store.resolve_shared_set("MIX-BLACK").await.unwrap().unwrap();
    store.reuse_shared_set(&set.set_id, &products[1].item_code, &set.fingerprint,
        &principal(), &["Molds".into()]).await.unwrap();
    let required = service.required_qolips_for_layout(&products, "", "").await.unwrap();
    assert_eq!(required.len(), 2);
    assert!(required.iter().all(|spec| spec.qolip_code.starts_with("MIX-")));
}

#[tokio::test]
async fn exact_product_lookup_is_independent_of_catalog_pages() {
    let store = std::sync::Arc::new(MemoryQolipStore::new());
    let mut products = (0..115)
        .map(|n| QolipProduct {
            code: format!("Zizi-{n:03}"),
            name: format!("A product {n:03}"),
            item_group: "Tayyor mahsulot".into(),
            ..Default::default()
        })
        .collect::<Vec<_>>();
    products.push(QolipProduct {
        code: "Zizi".into(),
        name: "Zizi sponge".into(),
        item_group: "Tayyor mahsulot".into(),
        ..Default::default()
    });
    store.seed_products(products).await;
    store
        .put_product_specs(
            (1..=150)
                .map(|n| QolipProductSpec {
                    item_code: "Zizi".into(),
                    item_name: "Zizi sponge".into(),
                    item_group: "Tayyor mahsulot".into(),
                    warehouse: "Molds".into(),
                    qolip_code: format!("MOLD-{n:03}"),
                    size: 40,
                    ..Default::default()
                })
                .collect(),
        )
        .await
        .unwrap();
    let page = store.products("Zizi", 100, false, None).await.unwrap();
    assert_eq!(page.len(), 100);
    assert!(page.iter().all(|product| product.code != "Zizi"));
    let service = QolipService::new(store);
    let molds = service
        .required_qolips_for_order(" zIzI ", "")
        .await
        .unwrap();
    assert_eq!(molds.len(), 150);
    assert!(molds.iter().all(|spec| spec.item_code == "Zizi"));
    assert_eq!(
        service
            .required_qolips_for_order("missing", "Zizi sponge")
            .await,
        Err(QolipError::ProductNotFound)
    );
    assert_eq!(
        service
            .required_qolips_for_order("", "Zizi sponge")
            .await
            .unwrap()
            .len(),
        150
    );
}

#[tokio::test]
async fn batch_product_spec_save_is_atomic() {
    let store = std::sync::Arc::new(MemoryQolipStore::new());
    store
        .seed_products(vec![QolipProduct {
            is_extension: false,
            qolip_set_id: String::new(),
            warehouse: "Qolip ombor".to_string(),
            code: "ITEM-BATCH".to_string(),
            name: "Batch product".to_string(),
            item_group: "Tayyor mahsulot".to_string(),
            customer_names: Vec::new(),
            qolip_code: String::new(),
            first_qolip_code: String::new(),
            size: 0,
            color: String::new(),
            has_qolip_spec: false,
            is_in_use: false,
        }])
        .await;
    let service = QolipService::new(store);

    let batch = (1..=8)
        .map(|number| QolipProductSpecUpsert {
            warehouse: "Qolip ombor".to_string(),
            item_code: "ITEM-BATCH".to_string(),
            item_name: "Batch product".to_string(),
            item_group: "Tayyor mahsulot".to_string(),
            qolip_code: format!("BATCH-{number}"),
            previous_qolip_code: String::new(),
            size: 42,
            color: format!("#00000{number}"),
        })
        .collect();

    let saved = service
        .upsert_product_specs(batch, &principal())
        .await
        .expect("save batch product specs");
    assert_eq!(saved.len(), 8);
    assert!(saved.iter().all(|spec| spec.size == 42));

    let conflict = service
        .upsert_product_specs(
            vec![
                QolipProductSpecUpsert {
                    warehouse: "Qolip ombor".to_string(),
                    item_code: "ITEM-BATCH".to_string(),
                    item_name: "Batch product".to_string(),
                    item_group: "Tayyor mahsulot".to_string(),
                    qolip_code: "BATCH-1".to_string(),
                    previous_qolip_code: String::new(),
                    size: 42,
                    color: "#FFFFFF".to_string(),
                },
                QolipProductSpecUpsert {
                    warehouse: "Qolip ombor".to_string(),
                    item_code: "ITEM-BATCH".to_string(),
                    item_name: "Batch product".to_string(),
                    item_group: "Tayyor mahsulot".to_string(),
                    qolip_code: "BATCH-9".to_string(),
                    previous_qolip_code: String::new(),
                    size: 42,
                    color: "#FFFFFF".to_string(),
                },
            ],
            &principal(),
        )
        .await
        .expect_err("a conflicting batch must fail");
    assert_eq!(conflict, QolipError::QolipCodeConflict);

    let products = service
        .products("BATCH-9", 20, true)
        .await
        .expect("load products after rejected batch");
    assert!(products.is_empty());
}

#[tokio::test]
async fn order_start_accepts_slash_code_without_inventing_checkout() {
    let store = std::sync::Arc::new(MemoryQolipStore::new());
    store
        .seed_products(vec![QolipProduct {
            is_extension: false,
            qolip_set_id: String::new(),
            warehouse: "Qolip ombor".to_string(),
            code: "ITEM-ORDER".to_string(),
            name: "Order product".to_string(),
            item_group: "Tayyor mahsulot".to_string(),
            customer_names: Vec::new(),
            qolip_code: String::new(),
            first_qolip_code: String::new(),
            size: 0,
            color: String::new(),
            has_qolip_spec: false,
            is_in_use: false,
        }])
        .await;
    let service = QolipService::new(store);
    service
        .upsert_product_spec(
            QolipProductSpecUpsert {
                warehouse: "Qolip ombor".to_string(),
                item_code: "ITEM-ORDER".to_string(),
                item_name: "Order product".to_string(),
                item_group: "Tayyor mahsulot".to_string(),
                qolip_code: "Milano Premium 30/40".to_string(),
                previous_qolip_code: String::new(),
                size: 42,
                color: String::new(),
            },
            &principal(),
        )
        .await
        .expect("save product spec");

    let preparation = service
        .prepare_qolip_code_for_order_start(
            "Milano Premium 30/40",
            "ITEM-ORDER",
            "Order product",
            "worker-1",
            "Worker",
            &principal(),
        )
        .await
        .expect("a catalog qolip for the order product must be accepted");

    assert_eq!(preparation.spec.qolip_code, "Milano Premium 30/40");
    assert!(preparation.checkout.is_none());
    assert_eq!(service.prepare_qolip_code_for_order_reacquisition(
        "Milano Premium 30/40", "ITEM-ORDER", "Order product", "worker-1", "Worker", &principal(),
    ).await, Err(QolipError::CheckoutRequired));
    assert_eq!(
        service.product_spec_by_qolip_code("40").await.unwrap(),
        None,
    );
}

#[tokio::test]
async fn legacy_stocked_qolip_can_be_promoted_to_colored_spec() {
    let store = std::sync::Arc::new(MemoryQolipStore::new());
    store
        .seed_products(vec![QolipProduct {
            is_extension: false,
            qolip_set_id: String::new(),
            warehouse: "Qolip ombor".to_string(),
            code: "ITEM-LEGACY".to_string(),
            name: "Legacy product".to_string(),
            item_group: "Tayyor mahsulot".to_string(),
            customer_names: Vec::new(),
            qolip_code: String::new(),
            first_qolip_code: String::new(),
            size: 0,
            color: String::new(),
            has_qolip_spec: false,
            is_in_use: false,
        }])
        .await;
    let service = QolipService::new(store.clone());
    service
        .upsert_location(
            QolipLocationUpsert {
                block: "A".to_string(),
                warehouse: "Qolip ombor".to_string(),
                item_code: "ITEM-LEGACY".to_string(),
                item_name: "Legacy product".to_string(),
                item_group: "Tayyor mahsulot".to_string(),
                qolip_code: "Q-LEGACY".to_string(),
                size: 42,
                quantity: 1,
                row_letter: "A".to_string(),
                column_number: Some(1),
            },
            &principal(),
        )
        .await
        .expect("save legacy location");

    let saved = service
        .upsert_product_spec(
            QolipProductSpecUpsert {
                warehouse: "Qolip ombor".to_string(),
                item_code: "ITEM-LEGACY".to_string(),
                item_name: "Legacy product".to_string(),
                item_group: "Tayyor mahsulot".to_string(),
                qolip_code: "Q-LEGACY".to_string(),
                previous_qolip_code: "Q-LEGACY".to_string(),
                size: 42,
                color: "#E53935".to_string(),
            },
            &principal(),
        )
        .await
        .expect("promote legacy location to product spec");

    assert_eq!(saved.color, "#E53935");
    let products = service
        .products("Q-LEGACY", 20, true)
        .await
        .expect("colored legacy product");
    assert_eq!(products.first().expect("legacy product").color, "#E53935");
}

#[tokio::test]
async fn order_start_accepts_existing_checkout_for_same_worker() {
    let store = std::sync::Arc::new(MemoryQolipStore::new());
    store
        .seed_products(vec![QolipProduct {
            is_extension: false,
            qolip_set_id: String::new(),
            warehouse: "Qolip ombor".to_string(),
            code: "ITEM-ORDER".to_string(),
            name: "Order product".to_string(),
            item_group: "Tayyor mahsulot".to_string(),
            customer_names: Vec::new(),
            qolip_code: String::new(),
            first_qolip_code: String::new(),
            size: 0,
            color: String::new(),
            has_qolip_spec: false,
            is_in_use: false,
        }])
        .await;
    let service = QolipService::new(store);
    service
        .upsert_product_spec(
            QolipProductSpecUpsert {
                warehouse: "Qolip ombor".to_string(),
                item_code: "ITEM-ORDER".to_string(),
                item_name: "Order product".to_string(),
                item_group: "Tayyor mahsulot".to_string(),
                qolip_code: "QOLIP-CHECKED-OUT".to_string(),
                previous_qolip_code: String::new(),
                size: 42,
                color: String::new(),
            },
            &principal(),
        )
        .await
        .expect("save product spec");
    let location = service
        .upsert_location(
            QolipLocationUpsert {
                block: "A".to_string(),
                warehouse: "Qolip ombor".to_string(),
                item_code: "ITEM-ORDER".to_string(),
                item_name: "Order product".to_string(),
                item_group: "Tayyor mahsulot".to_string(),
                qolip_code: "QOLIP-CHECKED-OUT".to_string(),
                size: 42,
                quantity: 1,
                row_letter: "A".to_string(),
                column_number: Some(1),
            },
            &principal(),
        )
        .await
        .expect("save location");
    service
        .issue_checkout_from_location(location, 1, "worker-1", "Worker", &principal())
        .await
        .expect("issue checkout");

    let preparation = service
        .prepare_qolip_code_for_order_start(
            "QOLIP-CHECKED-OUT",
            "ITEM-ORDER",
            "Order product",
            "worker-1",
            "Worker",
            &principal(),
        )
        .await
        .expect("existing worker checkout must be accepted");

    assert_eq!(preparation.spec.qolip_code, "QOLIP-CHECKED-OUT");
    assert!(preparation.checkout.is_none());
    assert!(service.prepare_qolip_code_for_order_reacquisition(
        "QOLIP-CHECKED-OUT", "ITEM-ORDER", "Order product", "worker-1", "Worker", &principal(),
    ).await.is_ok());
}

#[tokio::test]
async fn assigned_blocks_include_direct_block_assignments() {
    let service = QolipService::new(std::sync::Arc::new(DirectBlockAssignmentStore));
    let blocks = service
        .assigned_blocks(&principal())
        .await
        .expect("assigned blocks");

    assert_eq!(
        blocks,
        vec![QolipBlock {
            name: "A".to_string(),
            warehouse: "Qolip ombor".to_string(),
        }]
    );
}

#[tokio::test]
async fn checkouts_without_block_are_limited_to_assigned_blocks() {
    let service = QolipService::new(std::sync::Arc::new(CheckoutLedgerStore));
    let checkouts = service
        .checkouts(&principal(), false, None, "open", 100)
        .await
        .expect("checkouts");

    assert_eq!(
        checkouts
            .iter()
            .map(|checkout| checkout.block.as_str())
            .collect::<Vec<_>>(),
        vec!["A"]
    );
}

#[tokio::test]
async fn checkouts_without_block_return_full_ledger_for_admin() {
    let service = QolipService::new(std::sync::Arc::new(CheckoutLedgerStore));
    let checkouts = service
        .checkouts(&principal(), true, None, "open", 100)
        .await
        .expect("checkouts");

    assert_eq!(
        checkouts
            .iter()
            .map(|checkout| checkout.block.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "B"]
    );
}

#[tokio::test]
async fn order_start_rejects_qolip_from_another_finished_product_group() {
    let store = std::sync::Arc::new(MemoryQolipStore::new());
    store
        .seed_products(vec![QolipProduct {
            is_extension: false,
            qolip_set_id: String::new(),
            warehouse: "Qolip ombor".to_string(),
            code: "ITEM-ORDER".to_string(),
            name: "Order product".to_string(),
            item_group: "Tayyor mahsulot A".to_string(),
            customer_names: Vec::new(),
            qolip_code: String::new(),
            first_qolip_code: String::new(),
            size: 0,
            color: String::new(),
            has_qolip_spec: false,
            is_in_use: false,
        }])
        .await;
    let service = QolipService::new(store);
    service
        .upsert_product_spec(
            QolipProductSpecUpsert {
                warehouse: "Qolip ombor".to_string(),
                item_code: "ITEM-ORDER".to_string(),
                item_name: "Order product".to_string(),
                item_group: "Tayyor mahsulot B".to_string(),
                qolip_code: "QOLIP-WRONG-GROUP".to_string(),
                previous_qolip_code: String::new(),
                size: 42,
                color: String::new(),
            },
            &principal(),
        )
        .await
        .expect("save mismatched spec");
    service
        .upsert_location(
            QolipLocationUpsert {
                block: "A".to_string(),
                warehouse: "Qolip ombor".to_string(),
                item_code: "ITEM-ORDER".to_string(),
                item_name: "Order product".to_string(),
                item_group: "Tayyor mahsulot B".to_string(),
                qolip_code: "QOLIP-WRONG-GROUP".to_string(),
                size: 42,
                quantity: 1,
                row_letter: "A".to_string(),
                column_number: Some(1),
            },
            &principal(),
        )
        .await
        .expect("save location");

    let error = service
        .prepare_qolip_code_for_order_start(
            "QOLIP-WRONG-GROUP",
            "ITEM-ORDER",
            "Order product",
            "worker-1",
            "Worker",
            &principal(),
        )
        .await
        .expect_err("wrong product group must be rejected");

    assert_eq!(error, QolipError::QolipCodeMismatch);
}

#[tokio::test]
async fn order_start_rejects_another_products_qolip_from_same_group() {
    let store = std::sync::Arc::new(MemoryQolipStore::new());
    store
        .seed_products(vec![
            QolipProduct {
                is_extension: false,
                qolip_set_id: String::new(),
                warehouse: "Qolip ombor".to_string(),
                code: "ITEM-ORDER".to_string(),
                name: "Order product".to_string(),
                item_group: "Tayyor mahsulot".to_string(),
                customer_names: Vec::new(),
                qolip_code: String::new(),
                first_qolip_code: String::new(),
                size: 0,
                color: String::new(),
                has_qolip_spec: false,
                is_in_use: false,
            },
            QolipProduct {
                is_extension: false,
                qolip_set_id: String::new(),
                warehouse: "Qolip ombor".to_string(),
                code: "ITEM-OTHER".to_string(),
                name: "Other product".to_string(),
                item_group: "Tayyor mahsulot".to_string(),
                customer_names: Vec::new(),
                qolip_code: String::new(),
                first_qolip_code: String::new(),
                size: 0,
                color: String::new(),
                has_qolip_spec: false,
                is_in_use: false,
            },
        ])
        .await;
    let service = QolipService::new(store);
    service
        .upsert_product_spec(
            QolipProductSpecUpsert {
                warehouse: "Qolip ombor".to_string(),
                item_code: "ITEM-OTHER".to_string(),
                item_name: "Other product".to_string(),
                item_group: "Tayyor mahsulot".to_string(),
                qolip_code: "QOLIP-OTHER".to_string(),
                previous_qolip_code: String::new(),
                size: 42,
                color: String::new(),
            },
            &principal(),
        )
        .await
        .expect("save other product spec");
    service
        .upsert_location(
            QolipLocationUpsert {
                block: "A".to_string(),
                warehouse: "Qolip ombor".to_string(),
                item_code: "ITEM-OTHER".to_string(),
                item_name: "Other product".to_string(),
                item_group: "Tayyor mahsulot".to_string(),
                qolip_code: "QOLIP-OTHER".to_string(),
                size: 42,
                quantity: 1,
                row_letter: "A".to_string(),
                column_number: Some(2),
            },
            &principal(),
        )
        .await
        .expect("save other product location");

    let error = service
        .prepare_qolip_code_for_order_start(
            "QOLIP-OTHER",
            "ITEM-ORDER",
            "Order product",
            "worker-1",
            "Worker",
            &principal(),
        )
        .await
        .expect_err("another product's qolip must be rejected");

    assert_eq!(error, QolipError::QolipCodeMismatch);
}

fn principal() -> Principal {
    Principal {
        role: PrincipalRole::Qolipchi,
        display_name: "Qolipchi".to_string(),
        legal_name: "Qolipchi".to_string(),
        ref_: "qolipchi-1".to_string(),
        phone: "+998901112233".to_string(),
        avatar_url: String::new(),
    }
}

fn checkout(id: &str, block: &str) -> QolipCheckout {
    QolipCheckout {
        id: id.to_string(),
        location_id: format!("location-{id}"),
        block: block.to_string(),
        warehouse: "Qolip ombor".to_string(),
        item_code: format!("ITEM-{id}"),
        item_name: format!("Qolip {id}"),
        item_group: String::new(),
        qolip_code: format!("Q-{id}"),
        size: 42,
        quantity: 1,
        row_letter: "A".to_string(),
        column_number: Some(1),
        location_label: "A1".to_string(),
        issued_to_ref: "worker".to_string(),
        issued_to_name: "Worker".to_string(),
        status: "open".to_string(),
        issued_by_role: "qolipchi".to_string(),
        issued_by_ref: "qolipchi-1".to_string(),
        issued_by_name: "Qolipchi".to_string(),
        issued_at: "2026-06-22T09:00:00Z".to_string(),
    }
}

struct DirectBlockAssignmentStore;

struct CheckoutLedgerStore;

#[async_trait]
impl QolipStorePort for CheckoutLedgerStore {
    async fn assigned_warehouses(&self, _principal: &Principal) -> Result<Vec<String>, QolipError> {
        Ok(Vec::new())
    }

    async fn assigned_blocks(&self, _principal: &Principal) -> Result<Vec<QolipBlock>, QolipError> {
        Ok(vec![QolipBlock {
            name: "A".to_string(),
            warehouse: "Qolip ombor".to_string(),
        }])
    }

    async fn all_blocks(&self) -> Result<Vec<QolipBlock>, QolipError> {
        Ok(vec![
            QolipBlock {
                name: "A".to_string(),
                warehouse: "Qolip ombor".to_string(),
            },
            QolipBlock {
                name: "B".to_string(),
                warehouse: "Qolip ombor".to_string(),
            },
        ])
    }

    async fn products(
        &self,
        _query: &str,
        _limit: usize,
        _with_qolip_only: bool,
        _allowed_blocks: Option<&[String]>,
    ) -> Result<Vec<QolipProduct>, QolipError> {
        Ok(Vec::new())
    }

    async fn product_spec(&self, _item_code: &str) -> Result<Option<QolipProductSpec>, QolipError> {
        Ok(None)
    }

    async fn put_product_spec(
        &self,
        spec: QolipProductSpec,
    ) -> Result<QolipProductSpec, QolipError> {
        Ok(spec)
    }

    async fn locations(&self, _block: &str) -> Result<Vec<QolipLocation>, QolipError> {
        Ok(Vec::new())
    }

    async fn location_by_id(
        &self,
        _location_id: &str,
    ) -> Result<Option<QolipLocation>, QolipError> {
        Ok(None)
    }

    async fn put_location(&self, location: QolipLocation) -> Result<QolipLocation, QolipError> {
        Ok(location)
    }

    async fn get_or_create_cell_qr(&self, cell: QolipCellQr) -> Result<QolipCellQr, QolipError> {
        Ok(cell)
    }

    async fn issue_checkout(&self, checkout: QolipCheckout) -> Result<QolipCheckout, QolipError> {
        Ok(checkout)
    }

    async fn checkouts(
        &self,
        _block: Option<&str>,
        allowed_blocks: Option<&[String]>,
        status: &str,
        limit: usize,
    ) -> Result<Vec<QolipCheckout>, QolipError> {
        let mut items = vec![checkout("1", "A"), checkout("2", "B")]
            .into_iter()
            .filter(|checkout| checkout.status.eq_ignore_ascii_case(status))
            .filter(|checkout| {
                allowed_blocks.is_none_or(|allowed| {
                    allowed
                        .iter()
                        .any(|block| checkout.block.eq_ignore_ascii_case(block))
                })
            })
            .collect::<Vec<_>>();
        items.truncate(limit);
        Ok(items)
    }

    async fn checkout_by_id(
        &self,
        _checkout_id: &str,
    ) -> Result<Option<QolipCheckout>, QolipError> {
        Ok(None)
    }

    async fn return_checkout(
        &self,
        _checkout_id: &str,
        _row_letter: &str,
        _column_number: Option<i32>,
    ) -> Result<QolipCheckout, QolipError> {
        Err(QolipError::CheckoutNotFound)
    }

    async fn move_location(
        &self,
        _location_id: &str,
        _block: &str,
        _warehouse: &str,
        _row_letter: &str,
        _column_number: i32,
        _quantity: i32,
    ) -> Result<QolipLocation, QolipError> {
        Err(QolipError::LocationNotFound)
    }

    async fn cell_qr_by_payload(
        &self,
        _qr_payload: &str,
    ) -> Result<Option<QolipCellQr>, QolipError> {
        Ok(None)
    }
}

#[async_trait]
impl QolipStorePort for DirectBlockAssignmentStore {
    async fn assigned_warehouses(&self, _principal: &Principal) -> Result<Vec<String>, QolipError> {
        Ok(vec!["A".to_string()])
    }

    async fn assigned_blocks(&self, _principal: &Principal) -> Result<Vec<QolipBlock>, QolipError> {
        Ok(Vec::new())
    }

    async fn all_blocks(&self) -> Result<Vec<QolipBlock>, QolipError> {
        Ok(vec![QolipBlock {
            name: "A".to_string(),
            warehouse: "Qolip ombor".to_string(),
        }])
    }

    async fn products(
        &self,
        _query: &str,
        _limit: usize,
        _with_qolip_only: bool,
        _allowed_blocks: Option<&[String]>,
    ) -> Result<Vec<QolipProduct>, QolipError> {
        Ok(Vec::new())
    }

    async fn product_spec(&self, _item_code: &str) -> Result<Option<QolipProductSpec>, QolipError> {
        Ok(None)
    }

    async fn put_product_spec(
        &self,
        spec: QolipProductSpec,
    ) -> Result<QolipProductSpec, QolipError> {
        Ok(spec)
    }

    async fn locations(&self, _block: &str) -> Result<Vec<QolipLocation>, QolipError> {
        Ok(Vec::new())
    }

    async fn location_by_id(
        &self,
        _location_id: &str,
    ) -> Result<Option<QolipLocation>, QolipError> {
        Ok(None)
    }

    async fn put_location(&self, location: QolipLocation) -> Result<QolipLocation, QolipError> {
        Ok(location)
    }

    async fn get_or_create_cell_qr(&self, cell: QolipCellQr) -> Result<QolipCellQr, QolipError> {
        Ok(cell)
    }

    async fn issue_checkout(&self, checkout: QolipCheckout) -> Result<QolipCheckout, QolipError> {
        Ok(checkout)
    }

    async fn checkouts(
        &self,
        _block: Option<&str>,
        _allowed_blocks: Option<&[String]>,
        _status: &str,
        _limit: usize,
    ) -> Result<Vec<QolipCheckout>, QolipError> {
        Ok(Vec::new())
    }

    async fn checkout_by_id(
        &self,
        _checkout_id: &str,
    ) -> Result<Option<QolipCheckout>, QolipError> {
        Ok(None)
    }

    async fn return_checkout(
        &self,
        _checkout_id: &str,
        _row_letter: &str,
        _column_number: Option<i32>,
    ) -> Result<QolipCheckout, QolipError> {
        Err(QolipError::CheckoutNotFound)
    }

    async fn move_location(
        &self,
        _location_id: &str,
        _block: &str,
        _warehouse: &str,
        _row_letter: &str,
        _column_number: i32,
        _quantity: i32,
    ) -> Result<QolipLocation, QolipError> {
        Err(QolipError::LocationNotFound)
    }

    async fn cell_qr_by_payload(
        &self,
        _qr_payload: &str,
    ) -> Result<Option<QolipCellQr>, QolipError> {
        Ok(None)
    }
}
