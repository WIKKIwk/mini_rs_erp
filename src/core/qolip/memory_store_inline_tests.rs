#[cfg(test)]
mod tests {
    use super::*;

    fn product(code: &str, name: &str) -> QolipProduct {
        QolipProduct {
            code: code.to_string(),
            name: name.to_string(),
            item_group: "Tayyor mahsulot".to_string(),
            customer_names: Vec::new(),
            qolip_code: String::new(),
            size: 0,
            has_qolip_spec: false,
            is_in_use: false,
        }
    }

    fn product_spec(item_code: &str, qolip_code: &str) -> QolipProductSpec {
        QolipProductSpec {
            item_code: item_code.to_string(),
            item_name: "Kross qolip".to_string(),
            item_group: "Tayyor mahsulot".to_string(),
            qolip_code: qolip_code.to_string(),
            size: 42,
            created_by_role: "qolipchi".to_string(),
            created_by_ref: "qolipchi-1".to_string(),
            created_by_name: "Qolipchi".to_string(),
        }
    }

    fn location(id: &str, item_code: &str, quantity: i32) -> QolipLocation {
        QolipLocation {
            id: id.to_string(),
            block: "A".to_string(),
            warehouse: "Qolip ombor".to_string(),
            item_code: item_code.to_string(),
            item_name: item_code.to_string(),
            qolip_code: "Q-1".to_string(),
            size: 40,
            quantity,
            row_letter: "C".to_string(),
            column_number: Some(2),
            location_label: "C2".to_string(),
            created_by_role: "admin".to_string(),
            created_by_ref: "admin".to_string(),
            created_by_name: "Admin".to_string(),
        }
    }

    fn checkout(id: &str, location_id: &str, item_code: &str, status: &str) -> QolipCheckout {
        QolipCheckout {
            id: id.to_string(),
            location_id: location_id.to_string(),
            block: "A".to_string(),
            warehouse: "Qolip ombor".to_string(),
            item_code: item_code.to_string(),
            item_name: item_code.to_string(),
            item_group: String::new(),
            qolip_code: "Q-1".to_string(),
            size: 40,
            quantity: 2,
            row_letter: "C".to_string(),
            column_number: Some(2),
            location_label: "C2".to_string(),
            issued_to_ref: "worker".to_string(),
            issued_to_name: "Worker".to_string(),
            status: status.to_string(),
            issued_by_role: "admin".to_string(),
            issued_by_ref: "admin".to_string(),
            issued_by_name: "Admin".to_string(),
            issued_at: "1970-01-01T00:00:00Z".to_string(),
        }
    }

    #[tokio::test]
    async fn products_returns_multiple_qolip_codes_for_same_container_item() {
        let store = MemoryQolipStore::default();
        store
            .seed_products(vec![product("ITEM-001", "Kross qolip")])
            .await;
        store
            .put_product_spec(product_spec("ITEM-001", "QOLIP-0001"))
            .await
            .expect("first spec");
        store
            .put_product_spec(product_spec("ITEM-001", "QOLIP-0002"))
            .await
            .expect("second spec");

        let products = store.products("Kross", 20, true).await.expect("products");

        assert_eq!(
            products
                .iter()
                .map(|product| product.qolip_code.as_str())
                .collect::<Vec<_>>(),
            vec!["QOLIP-0001", "QOLIP-0002"]
        );
    }

    #[tokio::test]
    async fn legacy_location_is_listed_and_deleted_without_product_spec() {
        let store = MemoryQolipStore::default();
        let mut legacy_product = product("ITEM-LEGACY", "Legacy kross");
        legacy_product.customer_names = vec!["Legacy customer".to_string()];
        store.seed_products(vec![legacy_product]).await;
        let mut legacy_location = location("legacy-location", "ITEM-LEGACY", 1);
        legacy_location.item_name = "Legacy kross".to_string();
        legacy_location.qolip_code = "Q-LEGACY".to_string();
        store
            .put_location(legacy_location)
            .await
            .expect("legacy location");

        let products = store
            .products("legacy customer", 20, true)
            .await
            .expect("legacy catalog");
        assert_eq!(products.len(), 1);
        assert_eq!(products[0].qolip_code, "Q-LEGACY");
        assert!(products[0].has_qolip_spec);
        assert!(
            store
                .product_spec_by_qolip_code("Q-LEGACY")
                .await
                .expect("legacy spec lookup")
                .is_some()
        );

        let deleted = store
            .delete_product_specs(&["Q-LEGACY".to_string()])
            .await
            .expect("delete legacy location");
        assert_eq!(deleted, 1);
        assert!(store.locations("A").await.expect("locations").is_empty());
    }

    #[tokio::test]
    async fn legacy_open_checkout_stays_visible_as_in_use() {
        let store = MemoryQolipStore::default();
        store
            .seed_products(vec![product("ITEM-DEBT", "Legacy debt qolip")])
            .await;
        let mut open_checkout = checkout("legacy-debt", "gone-location", "ITEM-DEBT", "open");
        open_checkout.item_name = "Legacy debt qolip".to_string();
        open_checkout.qolip_code = "Q-LEGACY-DEBT".to_string();
        store.checkouts.write().await.push(open_checkout);

        let products = store
            .products("legacy debt", 20, true)
            .await
            .expect("legacy debt catalog");
        assert_eq!(products.len(), 1);
        assert_eq!(products[0].qolip_code, "Q-LEGACY-DEBT");
        assert!(products[0].is_in_use);
        assert!(
            store
                .product_spec_by_qolip_code("Q-LEGACY-DEBT")
                .await
                .expect("legacy debt spec lookup")
                .is_some()
        );
        assert_eq!(
            store
                .delete_product_specs(&["Q-LEGACY-DEBT".to_string()])
                .await
                .expect_err("open legacy checkout cannot be deleted"),
            QolipError::QolipInUse
        );
    }

    #[tokio::test]
    async fn renaming_block_preserves_locations_checkouts_and_cell_qr() {
        let store = MemoryQolipStore::default();
        store
            .seed_blocks(vec![QolipBlock {
                name: "A".to_string(),
                warehouse: "Qolip ombor".to_string(),
            }])
            .await;
        store
            .put_location(location("location-a", "ITEM-A", 2))
            .await
            .expect("location");
        store
            .checkouts
            .write()
            .await
            .push(checkout("checkout-a", "location-a", "ITEM-A", "open"));
        let original_cell = QolipCellQr {
            id: "cell-a-c2".to_string(),
            block: "A".to_string(),
            warehouse: "Qolip ombor".to_string(),
            row_letter: "C".to_string(),
            column_number: 2,
            location_label: "C2".to_string(),
            qr_payload: "4002-ORIGINAL".to_string(),
            created_by_role: "admin".to_string(),
            created_by_ref: "admin".to_string(),
            created_by_name: "Admin".to_string(),
        };
        store
            .get_or_create_cell_qr(original_cell.clone())
            .await
            .expect("cell qr");

        let renamed = store
            .rename_block("A", "B", "Qolip ombor")
            .await
            .expect("rename block");

        assert_eq!(renamed.name, "B");
        assert!(store.locations("A").await.expect("old locations").is_empty());
        assert_eq!(store.locations("B").await.expect("new locations").len(), 1);
        assert_eq!(
            store
                .checkouts(Some("B"), None, "open", 20)
                .await
                .expect("renamed checkouts")
                .len(),
            1
        );
        let scanned = store
            .cell_qr_by_payload("4002-ORIGINAL")
            .await
            .expect("cell lookup")
            .expect("cell");
        assert_eq!(scanned.block, "B");

        let reused = store
            .get_or_create_cell_qr(QolipCellQr {
                id: "new-derived-cell-id".to_string(),
                block: "B".to_string(),
                ..original_cell
            })
            .await
            .expect("reuse cell qr");
        assert_eq!(reused.qr_payload, "4002-ORIGINAL");
    }

    #[tokio::test]
    async fn delete_product_specs_is_atomic_and_rejects_open_checkout() {
        let store = MemoryQolipStore::default();
        store
            .seed_products(vec![product("ITEM-001", "Kross qolip")])
            .await;
        store
            .put_product_spec(product_spec("ITEM-001", "Q-1"))
            .await
            .expect("free spec");
        store
            .put_product_spec(product_spec("ITEM-001", "Q-2"))
            .await
            .expect("used spec");
        store
            .put_location(location("free-location", "ITEM-001", 1))
            .await
            .expect("free location");
        let mut used_location = location("used-location", "ITEM-001", 2);
        used_location.qolip_code = "Q-2".to_string();
        store
            .put_location(used_location)
            .await
            .expect("used location");
        let mut open_checkout = checkout("checkout-1", "used-location", "ITEM-001", "open");
        open_checkout.qolip_code = "Q-2".to_string();
        store
            .issue_checkout(open_checkout)
            .await
            .expect("open checkout");

        let products = store.products("", 20, true).await.expect("products");
        assert!(
            !products
                .iter()
                .find(|item| item.qolip_code == "Q-1")
                .unwrap()
                .is_in_use
        );
        assert!(
            products
                .iter()
                .find(|item| item.qolip_code == "Q-2")
                .unwrap()
                .is_in_use
        );

        let error = store
            .delete_product_specs(&["Q-1".to_string(), "Q-2".to_string()])
            .await
            .expect_err("batch containing open checkout must fail");
        assert_eq!(error, QolipError::QolipInUse);
        assert_eq!(store.products("", 20, true).await.unwrap().len(), 2);

        let deleted = store
            .delete_product_specs(&["Q-1".to_string()])
            .await
            .expect("delete free spec");
        assert_eq!(deleted, 1);
        assert!(store.location_by_qolip_code("Q-1").await.unwrap().is_none());
        let remaining = store.products("", 20, true).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].qolip_code, "Q-2");
    }

    #[tokio::test]
    async fn issue_checkout_rejects_location_identity_mismatch() {
        let store = MemoryQolipStore::default();
        store
            .locations
            .write()
            .await
            .push(location("loc-1", "ITEM-A", 5));

        let result = store
            .issue_checkout(checkout("checkout-1", "loc-1", "ITEM-B", "open"))
            .await;

        assert!(matches!(result, Err(QolipError::LocationIdentityMismatch)));
    }

    #[tokio::test]
    async fn put_location_moves_existing_qolip_code_to_new_cell() {
        let store = MemoryQolipStore::default();
        store
            .locations
            .write()
            .await
            .push(location("qolip:a:item_a:q_1:40:c:2", "ITEM-A", 1));
        let mut next = location("qolip:a:item_a:q_1:40:d:3", "ITEM-A", 1);
        next.row_letter = "D".to_string();
        next.column_number = Some(3);
        next.location_label = "D3".to_string();

        let saved = store.put_location(next).await.expect("saved location");

        assert_eq!(saved.location_label, "D3");
        assert_eq!(saved.quantity, 1);
        let locations = store.locations("A").await.expect("locations");
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].location_label, "D3");
        assert_eq!(locations[0].qolip_code, "Q-1");
    }

    #[tokio::test]
    async fn return_checkout_rejects_location_identity_mismatch() {
        let store = MemoryQolipStore::default();
        store
            .checkouts
            .write()
            .await
            .push(checkout("checkout-1", "loc-1", "ITEM-A", "open"));
        store
            .locations
            .write()
            .await
            .push(location("qolip:a:item_a:q_1:40:c:2", "ITEM-B", 5));

        let result = store.return_checkout("checkout-1", "", None).await;

        assert!(matches!(result, Err(QolipError::LocationIdentityMismatch)));
        let saved = store
            .checkout_by_id("checkout-1")
            .await
            .expect("checkout lookup")
            .expect("checkout");
        assert_eq!(saved.status, "open");
    }

    #[tokio::test]
    async fn move_location_rejects_target_identity_mismatch() {
        let store = MemoryQolipStore::default();
        store.locations.write().await.extend([
            location("source", "ITEM-A", 5),
            location("qolip:a:item_a:q_1:40:d:3", "ITEM-B", 4),
        ]);

        let result = store
            .move_location("source", "A", "Qolip ombor", "D", 3, 2)
            .await;

        assert!(matches!(result, Err(QolipError::LocationIdentityMismatch)));
    }
}
