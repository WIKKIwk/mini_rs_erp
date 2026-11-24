use super::*;

fn source(kind: &str, id: &str) -> LayerRemainderSource {
    LayerRemainderSource {
        source_kind: kind.into(),
        source_id: id.into(),
        is_current_roll: false,
        session_id: "session".into(),
        revision: 7,
        qr_payload: format!("qr-{id}"),
        item_code: id.into(),
        item_name: id.into(),
        source_net_kg: 50.0,
        source_meter: 1500.0,
        tare_kg: 2.0,
        producer_name: "original owner".into(),
    }
}

fn weights(
    sources: &[LayerRemainderSource],
    wip: f64,
    film: f64,
) -> LaminationRemainderWeights {
    LaminationRemainderWeights {
        wip_net_kg: wip,
        film_net_kg: film,
        wip_source_id: None,
        film_source_id: None,
        expected_sources: sources
            .iter()
            .map(|s| LaminationRemainderSourceRevision {
                source_kind: s.source_kind.clone(),
                source_id: s.source_id.clone(),
                expected_revision: s.revision,
            })
            .collect(),
    }
}

#[test]
fn two_kg_fields_bind_to_their_original_rolls_and_preserve_zero_answers() {
    let sources = [
        source("raw_material", "film"),
        source("progress_batch", "printed"),
    ];
    let layers = weights(&sources, 20.25, 0.0)
        .dispositions(&sources)
        .unwrap();
    assert_eq!(layers[0].remaining_net_kg, 0.0);
    assert_eq!(layers[1].source_id, "printed");
    assert_eq!(layers[1].remaining_net_kg, 20.25);
    assert_eq!(layers[1].expected_revision, 7);
    assert!(
        weights(&sources, 0.0, 0.0)
            .dispositions(&sources)
            .unwrap()
            .iter()
            .all(|layer| layer.remaining_net_kg == 0.0)
    );
    assert!(weights(&sources, 51.0, 0.0).dispositions(&sources).is_err());
    for kg in [-1.0, f64::NAN, f64::INFINITY, 0.0000001] {
        assert!(weights(&sources, kg, 0.0).dispositions(&sources).is_err());
    }
}

#[test]
fn kg_fields_never_guess_missing_ambiguous_or_changed_sources() {
    let mut sources = vec![source("progress_batch", "printed")];
    assert_eq!(
        weights(&sources, 0.0, 1.0)
            .dispositions(&sources)
            .unwrap_err(),
        ProductionMapError::LaminationRemainderSourceMissing
    );
    let old = weights(&sources, 1.0, 0.0);
    sources[0].revision += 1;
    assert_eq!(
        old.dispositions(&sources).unwrap_err(),
        ProductionMapError::ProgressBatchCorrectionConflict
    );
    sources.push(source("progress_batch", "other"));
    assert_eq!(
        weights(&sources, 1.0, 0.0)
            .dispositions(&sources)
            .unwrap_err(),
        ProductionMapError::LaminationRemainderSourceAmbiguous
    );
    sources.truncate(1);
    sources[0].source_meter = 0.0;
    assert_eq!(
        weights(&sources, 1.0, 0.0)
            .dispositions(&sources)
            .unwrap_err(),
        ProductionMapError::LaminationRemainderSourceMissing
    );
    assert!(weights(&[], 0.0, 0.0).dispositions(&[]).unwrap().is_empty());
}

#[test]
fn physical_roll_binding_survives_history_and_rejects_a_stale_roll_id() {
    for wip_kind in ["progress_batch", "opening_wip"] {
        let mut sources = vec![
            source("raw_material", "a-current-film"),
            source(wip_kind, "current-printed"),
            source("raw_material", "z-old-film"),
            source("progress_batch", "old-printed"),
        ];
        sources[0].is_current_roll = true;
        sources[1].is_current_roll = true;
        let mut input = weights(&sources, 20.0, 8.0);
        input.wip_source_id = Some("current-printed".into());
        input.film_source_id = Some("a-current-film".into());
        let layers = input.dispositions(&sources).unwrap();
        assert_eq!(layers[0].remaining_net_kg, 8.0);
        assert_eq!(layers[1].remaining_net_kg, 20.0);
        assert_eq!(layers[2].remaining_net_kg, 0.0);
        assert_eq!(layers[3].remaining_net_kg, 0.0);
        assert_eq!(
            weights(&sources, 20.0, 8.0).dispositions(&sources).unwrap(),
            layers
        );
        for id in ["z-old-film", "current-printed", "not-a-source"] {
            input.film_source_id = Some(id.into());
            assert_eq!(
                input.dispositions(&sources).unwrap_err(),
                ProductionMapError::ProgressBatchCorrectionConflict
            );
        }
        input.film_source_id = Some("a-current-film".into());
        sources[0].is_current_roll = false;
        sources[2].is_current_roll = true;
        assert_eq!(
            input.dispositions(&sources).unwrap_err(),
            ProductionMapError::ProgressBatchCorrectionConflict
        );
        input.film_net_kg = 0.0;
        assert_eq!(
            input.dispositions(&sources).unwrap_err(),
            ProductionMapError::ProgressBatchCorrectionConflict
        );
        input.film_source_id = Some("z-old-film".into());
        assert!(
            input
                .dispositions(&sources)
                .unwrap()
                .iter()
                .filter(|layer| layer.source_kind == "raw_material")
                .all(|layer| layer.remaining_net_kg == 0.0)
        );
    }
}

#[test]
fn glue_is_not_a_roll_and_an_invalid_current_roll_never_uses_an_older_passport() {
    let mut sources = vec![
        source("raw_material", "film"),
        source("raw_material", "glue"),
    ];
    sources[1].source_meter = 0.0;
    let layers = weights(&sources, 0.0, 8.0).dispositions(&sources).unwrap();
    assert_eq!(layers[0].remaining_net_kg, 8.0);
    assert_eq!(layers[1].remaining_net_kg, 0.0);
    sources[1].is_current_roll = true;
    assert_eq!(
        weights(&sources, 0.0, 8.0)
            .dispositions(&sources)
            .unwrap_err(),
        ProductionMapError::LaminationRemainderSourceMissing
    );
}

#[test]
fn opening_wip_uses_the_existing_wip_kg_field_and_label_switch() {
    let sources = [
        source("opening_wip", "opening"),
        source("raw_material", "film"),
    ];
    let layers = weights(&sources, 20.0, 0.0).dispositions(&sources).unwrap();
    assert_eq!(layers[0].source_kind, "opening_wip");
    assert_eq!(layers[0].remaining_net_kg, 20.0);
    assert_eq!(layers[1].remaining_net_kg, 0.0);
    for enabled in [false, true] {
        let settings = LaminationSettings {
            print_wip_remainder_labels: enabled,
            print_film_remainder_labels: !enabled,
            ..Default::default()
        };
        assert_eq!(settings.prints_remainder("opening_wip"), enabled);
    }
    let batch: OpeningWipBatch = serde_json::from_value(serde_json::json!({
        "batch_id":"opening","intake_id":"intake","order_id":"order","sequence_no":1,
        "qr_payload":"opening-qr","quantity_basis":"measured","quantity":1500,"uom":"m",
        "finished_goods_meter":1500,"finished_goods_kg":52,"bobina_kg":2,
        "wip_status":"waiting","label_item_code":"PET","label_item_name":"Printed PET",
        "created_at_unix":1,"updated_at_unix":1,
    }))
    .unwrap();
    assert_eq!(
        layer_remainder_opening_basis(&batch).unwrap(),
        (50.0, 1500.0, 2.0)
    );
    assert_eq!(
        layer_remainder_meter(50.0, 1500.0, 20.0).unwrap(),
        (20.0, 600.0)
    );
    let mut missing = batch;
    missing.finished_goods_kg = None;
    assert!(layer_remainder_opening_basis(&missing).is_err());
}

#[test]
fn label_switches_are_independent_and_only_select_their_source_kind() {
    for wip in [false, true] {
        for film in [false, true] {
            let mut settings = LaminationSettings::default();
            settings.apply(LaminationLabelSettingsPatch {
                print_wip_remainder_labels: Some(wip),
                print_film_remainder_labels: None,
            });
            settings.apply(LaminationLabelSettingsPatch {
                print_wip_remainder_labels: None,
                print_film_remainder_labels: Some(film),
            });
            assert_eq!(settings.prints_remainder("progress_batch"), wip);
            assert_eq!(settings.prints_remainder("raw_material"), film);
            assert!(!settings.prints_remainder("unknown"));
        }
    }
}
#[test]
fn net_weight_ratio_and_rounding() {
    assert_eq!(
        layer_remainder_meter(50.0, 1500.0, 20.0).unwrap(),
        (20.0, 600.0)
    );
    assert_eq!(
        layer_remainder_meter(48.0, 1500.0, 18.0).unwrap(),
        (18.0, 562.5)
    );
    assert_eq!(
        layer_remainder_meter(3.0, 10.0, 1.0).unwrap(),
        (1.0, 3.333333)
    );
    assert_eq!(
        layer_remainder_meter(20.0, 600.0, 5.0).unwrap(),
        (5.0, 150.0)
    );
}
#[test]
fn invalid_quantities_never_create_stock() {
    for remaining in [0.0, -1.0, 51.0, f64::NAN, f64::INFINITY, 0.0000001] {
        assert!(layer_remainder_meter(50.0, 1500.0, remaining).is_err());
    }
    assert!(layer_remainder_meter(50.0, 0.0, 20.0).is_err());
}
