use super::*;
use crate::core::apparatus_standard::{
    ProcessTechnology,
    factory_defaults::{
        FLEXO_DEFAULT_APPARATUS_ID, FLEXO_DEFAULT_MAX_ROLL_COUNT, FLEXO_DEFAULT_MAX_WEB_WIDTH_MM,
        FLEXO_DEFAULT_MIN_WEB_WIDTH_MM,
    },
    test_support::{TestApparatusSpec, canonical_draft},
};

const EXPECTED_FACTORY_DEFAULTS: [(&str, &str); 14] = [
    ("apparatus:default:asset-004", "Extruder laminatsiya"),
    ("apparatus:default:asset-005", "Flexo pechat"),
    ("apparatus:default:asset-007", "Laminatsiya 1"),
    ("apparatus:default:asset-008", "Laminatsiya 2"),
    ("apparatus:default:asset-010", "Rezka"),
    ("apparatus:default:asset-011", "Rezka 2"),
    ("apparatus:default:asset-012", "Rezka 3"),
    ("apparatus:default:asset-013", "Rezka 4"),
    ("apparatus:default:asset-014", "Rezka 5"),
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
        FLEXO_DEFAULT_MIN_WEB_WIDTH_MM
    );
    assert_eq!(
        flexo.execution_profile.max_web_width_mm,
        FLEXO_DEFAULT_MAX_WEB_WIDTH_MM
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

    let rezka_ids = actual
        .iter()
        .filter(|(id, _)| {
            matches!(
                id.as_str(),
                "apparatus:default:asset-010"
                    | "apparatus:default:asset-011"
                    | "apparatus:default:asset-012"
                    | "apparatus:default:asset-013"
                    | "apparatus:default:asset-014"
            )
        })
        .map(|(id, _)| id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(rezka_ids.len(), 5);
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
        FLEXO_DEFAULT_MIN_WEB_WIDTH_MM
    );
    assert_eq!(
        flexo.execution_profile.max_web_width_mm,
        FLEXO_DEFAULT_MAX_WEB_WIDTH_MM
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
async fn factory_default_bootstrap_upgrades_previous_flexo_width_limits() {
    let service = CanonicalApparatusService::memory();
    let mut previous = TestApparatusSpec::print(
        FLEXO_DEFAULT_APPARATUS_ID,
        "Flexo pechat",
        ProcessTechnology::Flexographic,
        Some(FLEXO_DEFAULT_MAX_ROLL_COUNT),
    )
    .requiring_tooling();
    previous.min_web_width_mm = Some(400);
    previous.max_web_width_mm = Some(800);
    service
        .seed_for_test(
            ApparatusId::new(FLEXO_DEFAULT_APPARATUS_ID).expect("Flexo ID"),
            canonical_draft(&previous),
        )
        .await
        .expect("seed previous Flexo default");

    assert_eq!(
        service
            .bootstrap_factory_defaults()
            .await
            .expect("upgrade previous Flexo default"),
        EXPECTED_FACTORY_DEFAULTS.len()
    );
    let flexo = service
        .current_projection(&ApparatusId::new(FLEXO_DEFAULT_APPARATUS_ID).expect("Flexo ID"))
        .await
        .expect("read upgraded Flexo projection")
        .expect("upgraded Flexo projection");
    assert_eq!(flexo.source_revision, 2);
    assert_eq!(
        flexo.execution_profile.min_web_width_mm,
        FLEXO_DEFAULT_MIN_WEB_WIDTH_MM
    );
    assert_eq!(
        flexo.execution_profile.max_web_width_mm,
        FLEXO_DEFAULT_MAX_WEB_WIDTH_MM
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
