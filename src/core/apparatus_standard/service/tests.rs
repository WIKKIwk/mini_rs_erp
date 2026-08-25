use super::*;

const EXPECTED_FACTORY_DEFAULTS: [(&str, &str); 10] = [
    ("apparatus:default:asset-004", "Extruder laminatsiya"),
    ("apparatus:default:asset-005", "Flexo pechat"),
    ("apparatus:default:asset-007", "Laminatsiya 1"),
    ("apparatus:default:asset-008", "Laminatsiya 2"),
    ("apparatus:default:asset-010", "Rezka"),
    ("apparatus:default:bosma_7", "7 ta rangli bosma aparat"),
    ("apparatus:default:bosma_8", "8 ta rangli bosma aparat"),
    ("apparatus:default:bosma_9", "9 ta rangli bosma aparat"),
    ("apparatus:default:holodniy_kley", "Holodniy kley aparat"),
    ("apparatus:default:paket", "Paket aparat"),
];

#[tokio::test]
async fn factory_default_bootstrap_populates_an_empty_repository() {
    let service = CanonicalApparatusService::memory();

    let created = service
        .bootstrap_factory_defaults()
        .await
        .expect("bootstrap factory defaults");

    assert_eq!(created, EXPECTED_FACTORY_DEFAULTS.len());
    let actual = service
        .list_runtime_projections()
        .await
        .expect("list factory defaults")
        .into_iter()
        .map(|projection| {
            (
                projection.apparatus_id.as_str().to_string(),
                projection.display.display_name,
            )
        })
        .collect::<Vec<_>>();
    let expected = EXPECTED_FACTORY_DEFAULTS
        .into_iter()
        .map(|(id, name)| (id.to_string(), name.to_string()))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn factory_default_bootstrap_is_restart_safe() {
    let service = CanonicalApparatusService::memory();
    service
        .bootstrap_factory_defaults()
        .await
        .expect("first bootstrap");
    let before = service
        .list_runtime_projections()
        .await
        .expect("list before restart");

    let created = service
        .bootstrap_factory_defaults()
        .await
        .expect("second bootstrap");
    let after = service
        .list_runtime_projections()
        .await
        .expect("list after restart");

    assert_eq!(created, 0);
    assert_eq!(after, before);
}
