use std::sync::Arc;

use crate::core::apparatus_standard::ApparatusId;

use super::*;

fn apparatus_id(value: &str) -> ApparatusId {
    ApparatusId::new(value.to_string()).expect("canonical apparatus id")
}

const LAMINATSIYA_1_ID: &str = "apparatus:default:asset-007";
const LAMINATSIYA_2_ID: &str = "apparatus:default:asset-008";
const SHARED_TITLE_1_ID: &str = "apparatus:test:asset-101";
const SHARED_TITLE_2_ID: &str = "apparatus:test:asset-102";

#[tokio::test]
async fn worker_group_accepts_custom_codes_schedule_and_rejects_duplicate_workers() {
    let service = WorkerGroupService::new(Arc::new(MemoryWorkerGroupStore::new()));
    let saved = service
        .upsert_group(WorkerGroupUpsert {
            apparatus_id: Some(apparatus_id(LAMINATSIYA_1_ID)),
            apparatus: "Laminatsiya 1".to_string(),
            group_code: "b guruh".to_string(),
            previous_apparatus: None,
            previous_apparatus_id: None,
            previous_group_code: None,
            shift: "kechki".to_string(),
            start_time: "08:30".to_string(),
            end_time: "20:30".to_string(),
            work_days_per_week: 6,
            start_day: "monday".to_string(),
            accounting_enabled: true,
            worker_ids: vec!["w1".to_string()],
        })
        .await
        .expect("save custom group");

    assert_eq!(saved.group_code, "B GURUH");
    assert_eq!(saved.shift, "kechki");
    assert_eq!(saved.start_time, "08:30");
    assert_eq!(saved.end_time, "20:30");
    assert_eq!(saved.work_days_per_week, 6);
    assert_eq!(saved.start_day, "monday");
    assert!(saved.accounting_enabled);

    let duplicate = service
        .upsert_group(WorkerGroupUpsert {
            apparatus_id: Some(apparatus_id(LAMINATSIYA_1_ID)),
            apparatus: "Laminatsiya 1".to_string(),
            group_code: "ba".to_string(),
            shift: "kunduz".to_string(),
            worker_ids: vec!["w1".to_string()],
            ..WorkerGroupUpsert::default()
        })
        .await;
    assert_eq!(duplicate, Err(WorkerGroupError::DuplicateWorker));

    let duplicate_across_apparatus = service
        .upsert_group(WorkerGroupUpsert {
            apparatus_id: Some(apparatus_id(LAMINATSIYA_2_ID)),
            apparatus: "Laminatsiya 2".to_string(),
            group_code: "cross apparatus".to_string(),
            shift: "kunduz".to_string(),
            worker_ids: vec!["w1".to_string()],
            ..WorkerGroupUpsert::default()
        })
        .await;
    assert_eq!(
        duplicate_across_apparatus,
        Err(WorkerGroupError::DuplicateWorker)
    );

    service
        .upsert_group(WorkerGroupUpsert {
            apparatus_id: Some(apparatus_id(LAMINATSIYA_1_ID)),
            apparatus: "Laminatsiya 1".to_string(),
            group_code: "dd".to_string(),
            shift: "tungi".to_string(),
            worker_ids: vec!["w2".to_string()],
            ..WorkerGroupUpsert::default()
        })
        .await
        .expect("save second custom group");

    let groups = service
        .worker_groups(Some(&apparatus_id(LAMINATSIYA_1_ID)))
        .await
        .expect("groups");
    assert_eq!(
        groups
            .iter()
            .map(|group| group.group_code.as_str())
            .collect::<Vec<_>>(),
        vec!["B GURUH", "DD"]
    );

    service
        .upsert_group(WorkerGroupUpsert {
            apparatus_id: Some(apparatus_id(LAMINATSIYA_2_ID)),
            apparatus: "Laminatsiya 2".to_string(),
            group_code: "b guruh".to_string(),
            shift: "kechki".to_string(),
            worker_ids: vec!["w1".to_string()],
            ..WorkerGroupUpsert::default()
        })
        .await
        .expect("move group to another apparatus");

    let old_apparatus_groups = service
        .worker_groups(Some(&apparatus_id(LAMINATSIYA_1_ID)))
        .await
        .expect("old apparatus groups");
    assert_eq!(
        old_apparatus_groups
            .iter()
            .map(|group| group.group_code.as_str())
            .collect::<Vec<_>>(),
        vec!["DD"]
    );

    let moved_groups = service
        .worker_groups(Some(&apparatus_id(LAMINATSIYA_2_ID)))
        .await
        .expect("moved apparatus groups");
    assert_eq!(
        moved_groups
            .iter()
            .map(|group| group.group_code.as_str())
            .collect::<Vec<_>>(),
        vec!["B GURUH"]
    );
}

#[tokio::test]
async fn worker_group_can_be_renamed_without_leaving_the_old_group() {
    let service = WorkerGroupService::new(Arc::new(MemoryWorkerGroupStore::new()));

    service
        .upsert_group(WorkerGroupUpsert {
            apparatus_id: Some(apparatus_id(LAMINATSIYA_1_ID)),
            apparatus: "Original laminator".to_string(),
            group_code: "A guruh".to_string(),
            shift: "kunduz".to_string(),
            worker_ids: vec!["w1".to_string()],
            ..WorkerGroupUpsert::default()
        })
        .await
        .expect("create group");

    let renamed = service
        .upsert_group(WorkerGroupUpsert {
            apparatus_id: Some(apparatus_id(LAMINATSIYA_1_ID)),
            apparatus: "Renamed laminator".to_string(),
            group_code: "A laminatsiya".to_string(),
            previous_apparatus: Some("Old title that must not be used".to_string()),
            previous_apparatus_id: Some(apparatus_id(LAMINATSIYA_1_ID)),
            previous_group_code: Some("A guruh".to_string()),
            shift: "kunduz".to_string(),
            worker_ids: vec!["w1".to_string()],
            ..WorkerGroupUpsert::default()
        })
        .await
        .expect("rename group");

    assert_eq!(renamed.group_code, "A LAMINATSIYA");
    let groups = service
        .worker_groups(Some(&apparatus_id(LAMINATSIYA_1_ID)))
        .await
        .expect("list groups");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].group_code, "A LAMINATSIYA");
    assert_eq!(groups[0].worker_ids, vec!["w1"]);
    assert_eq!(groups[0].apparatus_id, apparatus_id(LAMINATSIYA_1_ID));
    assert_eq!(groups[0].apparatus, "Renamed laminator");
}

#[tokio::test]
async fn worker_group_scope_is_exact_id_even_when_display_titles_match() {
    let service = WorkerGroupService::new(Arc::new(MemoryWorkerGroupStore::new()));
    for (id, worker_id) in [(SHARED_TITLE_1_ID, "w1"), (SHARED_TITLE_2_ID, "w2")] {
        service
            .upsert_group(WorkerGroupUpsert {
                apparatus_id: Some(apparatus_id(id)),
                apparatus: "Same display title".to_string(),
                group_code: "A guruh".to_string(),
                worker_ids: vec![worker_id.to_string()],
                ..WorkerGroupUpsert::default()
            })
            .await
            .expect("save group");
    }

    service
        .upsert_group(WorkerGroupUpsert {
            apparatus_id: Some(apparatus_id(SHARED_TITLE_2_ID)),
            apparatus: "Same display title".to_string(),
            group_code: "A guruh".to_string(),
            worker_ids: vec!["w3".to_string()],
            ..WorkerGroupUpsert::default()
        })
        .await
        .expect("update second apparatus group");

    let first = service
        .worker_groups(Some(&apparatus_id(SHARED_TITLE_1_ID)))
        .await
        .expect("first apparatus groups");
    let second = service
        .worker_groups(Some(&apparatus_id(SHARED_TITLE_2_ID)))
        .await
        .expect("second apparatus groups");
    assert_eq!(first[0].worker_ids, vec!["w1"]);
    assert_eq!(second[0].worker_ids, vec!["w3"]);
}

#[tokio::test]
async fn worker_group_rejects_title_without_canonical_id() {
    let service = WorkerGroupService::new(Arc::new(MemoryWorkerGroupStore::new()));
    let result = service
        .upsert_group(WorkerGroupUpsert {
            apparatus: "Laminatsiya 1".to_string(),
            group_code: "A guruh".to_string(),
            ..WorkerGroupUpsert::default()
        })
        .await;
    assert_eq!(result, Err(WorkerGroupError::MissingApparatus));
}

#[tokio::test]
async fn concurrent_edits_of_different_groups_preserve_both_changes() {
    let service = Arc::new(WorkerGroupService::new(Arc::new(
        MemoryWorkerGroupStore::new(),
    )));
    for (group_code, worker_id) in [("A guruh", "w1"), ("B guruh", "w2")] {
        service
            .upsert_group(WorkerGroupUpsert {
                apparatus_id: Some(apparatus_id(LAMINATSIYA_1_ID)),
                apparatus: "Laminatsiya 1".to_string(),
                group_code: group_code.to_string(),
                shift: "kunduz".to_string(),
                worker_ids: vec![worker_id.to_string()],
                ..WorkerGroupUpsert::default()
            })
            .await
            .expect("seed group");
    }

    let edit_a = service.upsert_group(WorkerGroupUpsert {
        apparatus_id: Some(apparatus_id(LAMINATSIYA_1_ID)),
        apparatus: "Laminatsiya 1".to_string(),
        group_code: "A guruh".to_string(),
        previous_apparatus: Some("Laminatsiya 1".to_string()),
        previous_apparatus_id: Some(apparatus_id(LAMINATSIYA_1_ID)),
        previous_group_code: Some("A guruh".to_string()),
        shift: "tungi-a".to_string(),
        worker_ids: vec!["w1".to_string()],
        ..WorkerGroupUpsert::default()
    });
    let edit_b = service.upsert_group(WorkerGroupUpsert {
        apparatus_id: Some(apparatus_id(LAMINATSIYA_1_ID)),
        apparatus: "Laminatsiya 1".to_string(),
        group_code: "B guruh".to_string(),
        previous_apparatus: Some("Laminatsiya 1".to_string()),
        previous_apparatus_id: Some(apparatus_id(LAMINATSIYA_1_ID)),
        previous_group_code: Some("B guruh".to_string()),
        shift: "tungi-b".to_string(),
        worker_ids: vec!["w2".to_string()],
        ..WorkerGroupUpsert::default()
    });
    let (saved_a, saved_b) = tokio::join!(edit_a, edit_b);
    saved_a.expect("edit A group");
    saved_b.expect("edit B group");

    let groups = service
        .worker_groups(Some(&apparatus_id(LAMINATSIYA_1_ID)))
        .await
        .expect("load groups");
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].group_code, "A GURUH");
    assert_eq!(groups[0].shift, "tungi-a");
    assert_eq!(groups[1].group_code, "B GURUH");
    assert_eq!(groups[1].shift, "tungi-b");
}
