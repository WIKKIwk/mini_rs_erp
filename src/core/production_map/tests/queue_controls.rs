use std::sync::Arc;

use crate::core::apparatus_standard::QueueDiscipline;
use crate::core::apparatus_standard::test_support::{TestApparatusSpec, runtime_configuration};
use crate::core::production_map::*;

use super::fixtures::{canonical_apparatus_stage_map, canonical_two_stage_map};

const LAMINATION_ID: &str = "apparatus:default:asset-007";
const REZKA_ID: &str = "apparatus:default:asset-010";

#[tokio::test]
async fn free_pick_rezka_exposes_start_for_waiting_lamination_wip_outside_queue_head() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let lamination =
        runtime_configuration(TestApparatusSpec::laminate(LAMINATION_ID, "Laminatsiya 1"));
    let mut rezka = runtime_configuration(TestApparatusSpec::cut(REZKA_ID, "Rezka"));
    rezka.queue.discipline = QueueDiscipline::FreePick;
    let service = ProductionMapService::new(
        store,
        Arc::new(TestCanonicalApparatusResolver::new([lamination, rezka])),
    );

    let target_order = "zakaz-rezka-partial-wip";
    let queue_head = "zakaz-rezka-head";
    let mut target_map = canonical_two_stage_map(
        target_order,
        LAMINATION_ID,
        "Laminatsiya 1",
        REZKA_ID,
        "Rezka",
    );
    target_map
        .nodes
        .iter_mut()
        .find(|node| node.id == "second")
        .expect("Rezka node")
        .rezka_kadr_count = Some(1);
    service.upsert_map(target_map).await.expect("target map");
    let mut queue_head_map = canonical_apparatus_stage_map(queue_head, REZKA_ID, "Rezka");
    queue_head_map
        .nodes
        .iter_mut()
        .find(|node| node.id == "apparatus")
        .expect("queue-head Rezka node")
        .rezka_kadr_count = Some(1);
    service
        .upsert_map(queue_head_map)
        .await
        .expect("queue-head map");
    service
        .set_apparatus_sequence(LAMINATION_ID, vec![target_order.to_string()])
        .await
        .expect("lamination sequence");
    service
        .set_apparatus_sequence(
            REZKA_ID,
            vec![queue_head.to_string(), target_order.to_string()],
        )
        .await
        .expect("rezka sequence");

    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-rezka-partial-wip".to_string(),
        display_name: "Rezka partial WIP worker".to_string(),
    };
    service
        .apply_apparatus_queue_action_with_progress(
            LAMINATION_ID,
            target_order,
            queue_state::ApparatusQueueAction::Start,
            &[LAMINATION_ID.to_string()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await
        .expect("lamination start");
    service
        .apply_apparatus_queue_action_with_progress(
            LAMINATION_ID,
            target_order,
            queue_state::ApparatusQueueAction::Pause,
            &[LAMINATION_ID.to_string()],
            actor,
            QueueProgressInput {
                produced_qty: Some(100.0),
                uom: "m".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("lamination waiting WIP");

    let controls = service
        .queue_action_controls()
        .await
        .expect("queue controls");
    let rezka_control = controls
        .get(REZKA_ID)
        .and_then(|orders| orders.get(target_order))
        .expect("target Rezka control");

    assert!(!rezka_control.previous_stage_ready);
    assert_eq!(
        rezka_control.interaction.previous_wip_mode,
        ApparatusQueuePreviousWipMode::ScanRequired
    );
    assert_eq!(
        rezka_control.interaction.mode,
        ApparatusQueueInteractionMode::FreshStart
    );
    assert_eq!(rezka_control.interaction.blocking_reason_code, "");
    assert!(
        rezka_control
            .allowed_actions
            .contains(&queue_state::ApparatusQueueAction::Start)
    );
}
