use sqlx::{Postgres, Transaction};

use crate::core::production_map::ProductionMapError;

/// Revalidate every effective mold in the order under the same resource locks
/// as peredelka dispatch. An intact alternative cannot hide an away mold.
pub(super) async fn require_order_qolips_available_tx(
    tx: &mut Transaction<'_, Postgres>,
    order_id: &str,
) -> Result<(), ProductionMapError> {
    sqlx::query(
        "SELECT pg_advisory_xact_lock_shared(hashtextextended('qolip:product-composition', 0))",
    )
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    let Some((product_code, map)) = sqlx::query_as::<_, (String, serde_json::Value)>(
        "SELECT product_code, map_json FROM mini_production_maps WHERE id = $1",
    )
    .bind(order_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    else {
        return Err(ProductionMapError::MapNotFound);
    };
    let mut products = std::collections::BTreeSet::new();
    if !product_code.trim().is_empty() {
        products.insert(product_code.trim().to_lowercase());
    }
    let named_combo =
        super::qolip_session_helpers::combo_primary_revision_tx(tx, &map, &product_code).await?;
    for product in map
        .get("products")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|_| !named_combo)
    {
        if let Some(code) = product.get("item_code").and_then(serde_json::Value::as_str)
            && !code.trim().is_empty()
        {
            products.insert(code.trim().to_lowercase());
        }
    }
    if products.is_empty() {
        return Ok(());
    }
    let codes = sqlx::query_scalar::<_, String>(
        "SELECT qolip_code_key FROM mini_qolip_effective_product_molds
         WHERE item_code_key = ANY($1)
         UNION
         SELECT lower(btrim(location.qolip_code)) FROM mini_qolip_locations location
         WHERE lower(btrim(location.item_code)) = ANY($1)
           AND NOT EXISTS (SELECT 1 FROM mini_qolip_product_specs spec
                           WHERE lower(btrim(spec.qolip_code)) = lower(btrim(location.qolip_code)))
         UNION
         SELECT lower(btrim(checkout.qolip_code)) FROM mini_qolip_checkouts checkout
         WHERE lower(btrim(checkout.item_code)) = ANY($1) AND checkout.status = 'open'
           AND NOT EXISTS (SELECT 1 FROM mini_qolip_product_specs spec
                           WHERE lower(btrim(spec.qolip_code)) = lower(btrim(checkout.qolip_code)))
         ORDER BY 1",
    )
    .bind(products.into_iter().collect::<Vec<_>>())
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    for code in &codes {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("qolip:{code}"))
            .execute(&mut **tx)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    if codes.is_empty() {
        return Ok(());
    }
    let unavailable: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM mini_qolip_reworks
         WHERE returned_at IS NULL AND qolip_code_key = ANY($1))",
    )
    .bind(codes)
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if unavailable {
        Err(ProductionMapError::QolipInRework)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> sqlx::PgPool {
        let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .expect("explicit disposable PostgreSQL test URL");
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("isolated test PostgreSQL")
    }

    async fn fixture(tx: &mut Transaction<'_, Postgres>) {
        sqlx::raw_sql(
            "CREATE TEMP TABLE mini_production_maps
                (id text, product_code text, map_json jsonb) ON COMMIT DROP;
             CREATE TEMP TABLE mini_qolip_effective_product_molds
                (item_code_key text, qolip_code_key text) ON COMMIT DROP;
             CREATE TEMP TABLE mini_qolip_product_specs (qolip_code text) ON COMMIT DROP;
             CREATE TEMP TABLE mini_qolip_locations
                (item_code text, qolip_code text) ON COMMIT DROP;
             CREATE TEMP TABLE mini_qolip_checkouts
                (item_code text, qolip_code text, status text) ON COMMIT DROP;
             CREATE TEMP TABLE mini_qolip_reworks
                (qolip_code_key text, returned_at timestamptz) ON COMMIT DROP;",
        )
        .execute(&mut **tx)
        .await
        .unwrap();
    }

    #[tokio::test]
    #[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL on a disposable PostgreSQL server"]
    async fn shared_code_blocks_all_orders_even_with_an_intact_alternative() {
        let pool = pool().await;
        let mut tx = pool.begin().await.unwrap();
        fixture(&mut tx).await;
        sqlx::raw_sql(
            "INSERT INTO mini_production_maps VALUES
                ('order-one', 'FIRST', '{}'),
                ('order-two', 'SECOND', '{}'),
                ('unrelated', 'THIRD', '{}');
             INSERT INTO mini_qolip_effective_product_molds VALUES
                ('first', 'shared'), ('first', 'intact-alternative'),
                ('second', 'shared'), ('second', 'second-mold'),
                ('third', 'unrelated-mold');
             INSERT INTO mini_qolip_reworks VALUES ('shared', NULL);",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        for order in ["order-one", "order-two"] {
            assert_eq!(
                require_order_qolips_available_tx(&mut tx, order).await,
                Err(ProductionMapError::QolipInRework),
            );
        }
        require_order_qolips_available_tx(&mut tx, "unrelated")
            .await
            .unwrap();
        sqlx::query("UPDATE mini_qolip_reworks SET returned_at = now()")
            .execute(&mut *tx)
            .await
            .unwrap();
        for order in ["order-one", "order-two"] {
            require_order_qolips_available_tx(&mut tx, order)
                .await
                .unwrap();
        }
        tx.rollback().await.unwrap();
        pool.close().await;
    }

    #[tokio::test]
    #[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL on a disposable PostgreSQL server"]
    async fn partial_return_keeps_multi_product_and_legacy_requirements_blocked() {
        let pool = pool().await;
        let mut tx = pool.begin().await.unwrap();
        fixture(&mut tx).await;
        sqlx::raw_sql(
            "INSERT INTO mini_production_maps VALUES
                ('mixed-order', 'FIRST', '{\"products\":[{\"item_code\":\"SECOND\"}]}');
             INSERT INTO mini_qolip_effective_product_molds VALUES ('second', 'second-mold');
             INSERT INTO mini_qolip_locations VALUES ('FIRST', 'Legacy-Mold');
             INSERT INTO mini_qolip_reworks VALUES ('legacy-mold', NULL), ('second-mold', NULL);",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        assert_eq!(
            require_order_qolips_available_tx(&mut tx, "mixed-order").await,
            Err(ProductionMapError::QolipInRework),
        );
        sqlx::query("UPDATE mini_qolip_reworks SET returned_at = now() WHERE qolip_code_key = 'legacy-mold'")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            require_order_qolips_available_tx(&mut tx, "mixed-order").await,
            Err(ProductionMapError::QolipInRework),
        );
        sqlx::query("UPDATE mini_qolip_reworks SET returned_at = now() WHERE qolip_code_key = 'second-mold'")
            .execute(&mut *tx)
            .await
            .unwrap();
        require_order_qolips_available_tx(&mut tx, "mixed-order")
            .await
            .unwrap();
        tx.rollback().await.unwrap();
        pool.close().await;
    }

    #[tokio::test]
    #[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL on a disposable PostgreSQL server"]
    async fn named_combo_checks_own_molds_without_children_rework() {
        let pool = pool().await;
        let mut tx = pool.begin().await.unwrap();
        fixture(&mut tx).await;
        let reference = serde_json::json!({"id":"combo-master","item_code":"COMBO",
            "name":"Combined","revision":1,"layout_fingerprint":"a".repeat(64)});
        sqlx::raw_sql("CREATE TEMP TABLE mini_combo_revisions(item_code text,reference_json jsonb) ON COMMIT DROP;
            INSERT INTO mini_qolip_effective_product_molds VALUES ('combo','combo-mold'),('child','child-mold');
            INSERT INTO mini_qolip_reworks VALUES ('child-mold',NULL);")
            .execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO mini_combo_revisions VALUES ('COMBO',$1)")
            .bind(&reference)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO mini_production_maps VALUES ('named','COMBO',$1)")
            .bind(serde_json::json!({"combo":reference,"products":[{"item_code":"CHILD"}]}))
            .execute(&mut *tx)
            .await
            .unwrap();
        require_order_qolips_available_tx(&mut tx, "named")
            .await
            .unwrap();
        sqlx::query("INSERT INTO mini_qolip_reworks VALUES ('combo-mold',NULL)")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            require_order_qolips_available_tx(&mut tx, "named").await,
            Err(ProductionMapError::QolipInRework)
        );
        tx.rollback().await.unwrap();
        pool.close().await;
    }
}
