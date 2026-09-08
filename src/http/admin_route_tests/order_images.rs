use super::*;
use crate::core::calculate_orders::OrderImageLookup;
use crate::core::production_map::ProductionMapDefinition;
use crate::db::postgres_calculate_order::PostgresCalculateOrderStore;

#[tokio::test]
async fn postgres_order_image_legacy_photo_is_identical_for_admin_and_all_operators() {
    // Real PostgreSQL SQL/schema, but connection-local tables only. No ERP
    // records, databases or running services are changed by this regression.
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let pool = sqlx::postgres::PgPoolOptions::new().max_connections(1)
        .connect(&url).await.expect("PostgreSQL regression connection");
    let migration = include_str!("../../../migrations/postgres/0001_mini_erp_foundation.sql");
    for table in ["mini_quick_order_templates", "mini_quick_order_images"] {
        let prefix = format!("CREATE TABLE IF NOT EXISTS {table}");
        let statement = migration.split(&prefix).nth(1).unwrap().split(';').next().unwrap();
        sqlx::query(&format!("CREATE TEMP TABLE {table}{statement}"))
            .execute(&pool).await.expect("isolated tables with actual migrated schema");
    }
    let mut state = test_state();
    state.calculate_orders = Arc::new(PostgresCalculateOrderStore::new(pool.clone()));
    let rgb = image::RgbImage::from_fn(1200, 600, |x, y|
        image::Rgb([(x % 251) as u8, (y % 239) as u8, 128]));
    let full_bytes = webp::Encoder::from_rgb(rgb.as_raw(), 1200, 600).encode(82.0).to_vec();
    state.calculate_orders.save_image("admin:admin", CalculateOrderImage {
        image_id: "legacy-photo".into(), image_name: "photo.webp".into(),
        image_mime: "image/webp".into(), body: full_bytes.clone(),
        ..CalculateOrderImage::default()
    }).await.unwrap();
    sqlx::query("INSERT INTO mini_quick_order_templates
        (id,owner_key,code,name,item_code,product_name,quick_key,payload_json)
        VALUES ('template','admin:admin','Z-old','Mono 007','mono 007','Mono 007','fixture',
        '{\"source_map_id\":\"template-zakaz-0003\",\"order_number\":\"\",
          \"image_id\":\"legacy-photo\",\"width_mm\":41}')")
        .execute(&pool).await.unwrap();
    let mut map: ProductionMapDefinition = serde_json::from_str(&pechat_order_map_json(
        "zakaz-0012", "Mono 007", "0012", "apparatus:default:bosma_7")).unwrap();
    map.product_code = "mono 007".into();
    map.width_mm = Some(41.0);
    assert!(map.image_id.is_empty(), "reproduce a pre-linkage/reused-template order");
    state.production_maps.upsert_map(map.clone()).await.unwrap();

    let mut expected_thumb = None;
    for (role, viewer, apparatus) in [
        (PrincipalRole::Admin, "admin", ""),
        (PrincipalRole::Aparatchi, "print-worker", "apparatus:default:bosma_7"),
        (PrincipalRole::Aparatchi, "lamination-worker", "apparatus:default:asset-007"),
        (PrincipalRole::Aparatchi, "rezka-worker", "apparatus:default:asset-010"),
        (PrincipalRole::Aparatchi, "flexo-worker", "apparatus:default:asset-005"),
    ] {
        if !apparatus.is_empty() {
            state.admin.upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
                principal_role: role, principal_ref: viewer.into(), role_id: "aparatchi".into(),
                assigned_apparatus: vec![apparatus.into()], assigned_item_groups: vec![],
            }).await.unwrap();
        }
        let token = session_for(&state, role, viewer).await;
        for thumbnail in [true, false] {
            let path = format!("/v1/mobile/admin/production-maps/order-image/view?order_id=zakaz-0012{}",
                if thumbnail { "&variant=thumb-v1" } else { "" });
            let response = build_router(state.clone()).oneshot(request("GET", &path, &token)).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{viewer}: {path}");
            let etag = response.headers()[header::ETAG].clone();
            let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            if thumbnail {
                assert_eq!(image::load_from_memory(&bytes).unwrap().width(), 256);
                if let Some(expected) = &expected_thumb { assert_eq!(&bytes, expected); }
                expected_thumb = Some(bytes);
            } else {
                assert_eq!(bytes.as_ref(), full_bytes, "{viewer} must receive full source pixels");
            }
            let response = build_router(state.clone()).oneshot(Request::builder().uri(&path)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::IF_NONE_MATCH, etag).body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        }
    }
    // Exercise JSON order-number and legacy template-map identity separately
    // from product matching, so a nonexistent SQL column cannot regress again.
    map.product_code = "unrelated".into();
    map.title = "unrelated".into();
    map.nodes.clear();
    for (source, number) in [("", "0012"), ("template-zakaz-0012", "")] {
        sqlx::query("UPDATE mini_quick_order_templates SET payload_json =
            payload_json || jsonb_build_object('source_map_id',$1::text,'order_number',$2::text)")
            .bind(source).bind(number).execute(&pool).await.unwrap();
        assert_eq!(state.calculate_orders.image_id_for_order(&OrderImageLookup::for_map(&map))
            .await.unwrap().as_deref(), Some("legacy-photo"));
    }
    let unauthorized = build_router(state).oneshot(Request::builder()
        .uri("/v1/mobile/admin/production-maps/order-image/view?order_id=zakaz-0012")
        .body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    pool.close().await; // PostgreSQL removes our temporary tables on disconnect.
}
