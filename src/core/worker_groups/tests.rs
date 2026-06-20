use std::sync::Arc;

use super::*;

#[tokio::test]
async fn worker_group_accepts_custom_codes_schedule_and_rejects_duplicate_workers() {
    let service = WorkerGroupService::new(Arc::new(MemoryWorkerGroupStore::new()));
    let saved = service
        .upsert_group(WorkerGroupUpsert {
            apparatus: "Laminatsiya 1".to_string(),
            group_code: "b guruh".to_string(),
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
            apparatus: "Laminatsiya 1".to_string(),
            group_code: "ba".to_string(),
            shift: "kunduz".to_string(),
            worker_ids: vec!["w1".to_string()],
            ..WorkerGroupUpsert::default()
        })
        .await;
    assert_eq!(duplicate, Err(WorkerGroupError::DuplicateWorker));

    service
        .upsert_group(WorkerGroupUpsert {
            apparatus: "Laminatsiya 1".to_string(),
            group_code: "dd".to_string(),
            shift: "tungi".to_string(),
            worker_ids: vec!["w2".to_string()],
            ..WorkerGroupUpsert::default()
        })
        .await
        .expect("save second custom group");

    let groups = service
        .worker_groups(Some("Laminatsiya 1"))
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
            apparatus: "Laminatsiya 2".to_string(),
            group_code: "b guruh".to_string(),
            shift: "kechki".to_string(),
            worker_ids: vec!["w1".to_string()],
            ..WorkerGroupUpsert::default()
        })
        .await
        .expect("move group to another apparatus");

    let old_apparatus_groups = service
        .worker_groups(Some("Laminatsiya 1"))
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
        .worker_groups(Some("Laminatsiya 2"))
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
