use super::*;
use crate::core::apparatus_standard::{
    ProcessTechnology,
    factory_defaults::{
        FLEXO_DEFAULT_APPARATUS_ID, FLEXO_DEFAULT_MAX_ROLL_COUNT, FLEXO_DEFAULT_MAX_WEB_WIDTH_MM,
        FLEXO_DEFAULT_MIN_WEB_WIDTH_MM,
    },
    test_support::{TestApparatusSpec, canonical_draft},
};

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
    let projections = service
        .list_runtime_projections()
        .await
        .expect("list factory defaults");
    let flexo = projections
        .iter()
        .find(|projection| projection.apparatus_id.as_str() == FLEXO_DEFAULT_APPARATUS_ID)
        .expect("Flexo factory default");
    assert_eq!(
        flexo.execution_profile.color_station_count,
        Some(FLEXO_DEFAULT_MAX_ROLL_COUNT)
    );
    assert_eq!(
        flexo.execution_profile.min_web_width_mm,
        Some(FLEXO_DEFAULT_MIN_WEB_WIDTH_MM)
    );
    assert_eq!(
        flexo.execution_profile.max_web_width_mm,
        Some(FLEXO_DEFAULT_MAX_WEB_WIDTH_MM)
    );
    let actual = projections
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
async fn factory_default_bootstrap_upgrades_legacy_flexo_limits_once() {
    let service = CanonicalApparatusService::memory();
    let legacy = TestApparatusSpec::print(
        FLEXO_DEFAULT_APPARATUS_ID,
        "Flexo pechat",
        ProcessTechnology::Flexographic,
        None,
    )
    .requiring_tooling();
    service
        .seed_for_test(
            ApparatusId::new(FLEXO_DEFAULT_APPARATUS_ID).expect("Flexo ID"),
            canonical_draft(&legacy),
        )
        .await
        .expect("seed legacy Flexo");

    assert_eq!(
        service
            .bootstrap_factory_defaults()
            .await
            .expect("upgrade legacy Flexo"),
        EXPECTED_FACTORY_DEFAULTS.len()
    );
    let flexo = service
        .current_projection(&ApparatusId::new(FLEXO_DEFAULT_APPARATUS_ID).expect("Flexo ID"))
        .await
        .expect("read upgraded Flexo")
        .expect("upgraded Flexo projection");
    assert_eq!(flexo.source_revision, 2);
    assert_eq!(
        flexo.execution_profile.color_station_count,
        Some(FLEXO_DEFAULT_MAX_ROLL_COUNT)
    );
    assert_eq!(
        flexo.execution_profile.min_web_width_mm,
        Some(FLEXO_DEFAULT_MIN_WEB_WIDTH_MM)
    );
    assert_eq!(
        flexo.execution_profile.max_web_width_mm,
        Some(FLEXO_DEFAULT_MAX_WEB_WIDTH_MM)
    );
    assert_eq!(
        service
            .bootstrap_factory_defaults()
            .await
            .expect("restart after Flexo upgrade"),
        0
    );
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
