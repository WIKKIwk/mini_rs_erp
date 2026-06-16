use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
#[cfg(test)]
use tokio::sync::RwLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerGroupRecord {
    pub apparatus: String,
    pub group_code: String,
    pub shift: String,
    #[serde(default)]
    pub start_time: String,
    #[serde(default)]
    pub end_time: String,
    #[serde(default = "default_work_days_per_week")]
    pub work_days_per_week: i32,
    #[serde(default)]
    pub start_day: String,
    #[serde(default)]
    pub accounting_enabled: bool,
    #[serde(default)]
    pub worker_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerGroupUpsert {
    pub apparatus: String,
    pub group_code: String,
    #[serde(default)]
    pub shift: String,
    #[serde(default)]
    pub start_time: String,
    #[serde(default)]
    pub end_time: String,
    #[serde(default = "default_work_days_per_week")]
    pub work_days_per_week: i32,
    #[serde(default)]
    pub start_day: String,
    #[serde(default)]
    pub accounting_enabled: bool,
    #[serde(default)]
    pub worker_ids: Vec<String>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WorkerGroupError {
    #[error("apparatus is required")]
    MissingApparatus,
    #[error("worker group is invalid")]
    InvalidGroup,
    #[error("worker shift is invalid")]
    InvalidShift,
    #[error("worker schedule is invalid")]
    InvalidSchedule,
    #[error("worker is duplicated in apparatus groups")]
    DuplicateWorker,
    #[error("worker group store failed")]
    StoreFailed,
}

#[async_trait]
pub trait WorkerGroupStorePort: Send + Sync {
    async fn worker_groups(
        &self,
        apparatus: Option<&str>,
    ) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError>;
    async fn put_apparatus_worker_groups(
        &self,
        apparatus: &str,
        groups: Vec<WorkerGroupRecord>,
    ) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError>;
}

#[derive(Clone)]
pub struct WorkerGroupService {
    store: Arc<dyn WorkerGroupStorePort>,
}

impl WorkerGroupService {
    pub fn new(store: Arc<dyn WorkerGroupStorePort>) -> Self {
        Self { store }
    }

    pub fn unavailable() -> Self {
        Self::new(Arc::new(UnavailableWorkerGroupStore))
    }

    pub async fn worker_groups(
        &self,
        apparatus: Option<&str>,
    ) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError> {
        self.store.worker_groups(apparatus).await
    }

    pub async fn upsert_group(
        &self,
        input: WorkerGroupUpsert,
    ) -> Result<WorkerGroupRecord, WorkerGroupError> {
        let next = normalize_input(input)?;
        let all_groups = self.store.worker_groups(None).await?;
        let source_apparatuses = all_groups
            .iter()
            .filter(|group| {
                group.group_code == next.group_code
                    && !group.apparatus.eq_ignore_ascii_case(&next.apparatus)
            })
            .map(|group| group.apparatus.clone())
            .collect::<BTreeSet<_>>();

        for source_apparatus in source_apparatuses {
            let remaining = all_groups
                .iter()
                .filter(|group| {
                    group.apparatus.eq_ignore_ascii_case(&source_apparatus)
                        && group.group_code != next.group_code
                })
                .cloned()
                .collect::<Vec<_>>();
            self.store
                .put_apparatus_worker_groups(&source_apparatus, sort_groups(remaining))
                .await?;
        }

        let mut groups = self
            .store
            .worker_groups(Some(&next.apparatus))
            .await?
            .into_iter()
            .filter(|group| group.apparatus.eq_ignore_ascii_case(&next.apparatus))
            .collect::<Vec<_>>();

        if groups.iter().any(|group| {
            group.group_code != next.group_code
                && group.worker_ids.iter().any(|worker_id| {
                    next.worker_ids
                        .iter()
                        .any(|next_id| next_id.eq_ignore_ascii_case(worker_id))
                })
        }) {
            return Err(WorkerGroupError::DuplicateWorker);
        }

        if let Some(existing) = groups
            .iter_mut()
            .find(|group| group.group_code == next.group_code)
        {
            *existing = next.clone();
        } else {
            groups.push(next.clone());
        }

        ensure_workers_not_duplicated(&groups)?;

        let saved = self
            .store
            .put_apparatus_worker_groups(&next.apparatus, sort_groups(groups))
            .await?;
        saved
            .into_iter()
            .find(|group| group.group_code == next.group_code)
            .ok_or(WorkerGroupError::StoreFailed)
    }
}

fn normalize_input(input: WorkerGroupUpsert) -> Result<WorkerGroupRecord, WorkerGroupError> {
    let apparatus = input.apparatus.trim().to_string();
    if apparatus.is_empty() {
        return Err(WorkerGroupError::MissingApparatus);
    }
    let group_code = normalize_group_code(&input.group_code)?;
    let shift = normalize_shift(&input.shift)?;
    let start_time = normalize_time(&input.start_time, "08:00")?;
    let end_time = normalize_time(&input.end_time, "20:00")?;
    let work_days_per_week = normalize_work_days(input.work_days_per_week)?;
    let start_day = normalize_start_day(&input.start_day)?;
    let mut seen = BTreeSet::new();
    let worker_ids = input
        .worker_ids
        .into_iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .filter(|id| seen.insert(id.to_lowercase()))
        .collect();

    Ok(WorkerGroupRecord {
        apparatus,
        group_code,
        shift,
        start_time,
        end_time,
        work_days_per_week,
        start_day,
        accounting_enabled: input.accounting_enabled,
        worker_ids,
    })
}

fn normalize_group_code(value: &str) -> Result<String, WorkerGroupError> {
    let upper = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase();
    if upper.is_empty()
        || upper.len() > 64
        || upper
            .chars()
            .any(|ch| !(ch.is_alphanumeric() || ch.is_whitespace()))
    {
        return Err(WorkerGroupError::InvalidGroup);
    }
    Ok(upper)
}

fn normalize_shift(value: &str) -> Result<String, WorkerGroupError> {
    let shift = value.trim();
    if shift.is_empty() {
        return Ok("kunduz".to_string());
    }
    if shift.len() > 64 {
        return Err(WorkerGroupError::InvalidShift);
    }
    Ok(shift.to_string())
}

fn normalize_time(value: &str, default: &str) -> Result<String, WorkerGroupError> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(default.to_string());
    }
    let Some((hour, minute)) = value.split_once(':') else {
        return Err(WorkerGroupError::InvalidSchedule);
    };
    let hour = hour
        .parse::<u8>()
        .map_err(|_| WorkerGroupError::InvalidSchedule)?;
    let minute = minute
        .parse::<u8>()
        .map_err(|_| WorkerGroupError::InvalidSchedule)?;
    if hour > 23 || minute > 59 {
        return Err(WorkerGroupError::InvalidSchedule);
    }
    Ok(format!("{hour:02}:{minute:02}"))
}

fn normalize_work_days(value: i32) -> Result<i32, WorkerGroupError> {
    match value {
        0 => Ok(default_work_days_per_week()),
        1..=7 => Ok(value),
        _ => Err(WorkerGroupError::InvalidSchedule),
    }
}

fn normalize_start_day(value: &str) -> Result<String, WorkerGroupError> {
    let day = value.trim().to_lowercase();
    if day.is_empty() {
        return Ok("monday".to_string());
    }
    match day.as_str() {
        "monday" | "tuesday" | "wednesday" | "thursday" | "friday" | "saturday" | "sunday" => {
            Ok(day)
        }
        _ => Err(WorkerGroupError::InvalidSchedule),
    }
}

fn default_work_days_per_week() -> i32 {
    6
}

fn ensure_workers_not_duplicated(groups: &[WorkerGroupRecord]) -> Result<(), WorkerGroupError> {
    let mut seen = BTreeSet::new();
    for worker_id in groups.iter().flat_map(|group| group.worker_ids.iter()) {
        if !seen.insert(worker_id.to_lowercase()) {
            return Err(WorkerGroupError::DuplicateWorker);
        }
    }
    Ok(())
}

fn sort_groups(mut groups: Vec<WorkerGroupRecord>) -> Vec<WorkerGroupRecord> {
    groups.sort_by(|left, right| {
        left.apparatus
            .to_lowercase()
            .cmp(&right.apparatus.to_lowercase())
            .then(left.group_code.cmp(&right.group_code))
    });
    groups
}

struct UnavailableWorkerGroupStore;

#[async_trait]
impl WorkerGroupStorePort for UnavailableWorkerGroupStore {
    async fn worker_groups(
        &self,
        _apparatus: Option<&str>,
    ) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError> {
        Err(WorkerGroupError::StoreFailed)
    }

    async fn put_apparatus_worker_groups(
        &self,
        _apparatus: &str,
        _groups: Vec<WorkerGroupRecord>,
    ) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError> {
        Err(WorkerGroupError::StoreFailed)
    }
}

#[derive(Default)]
#[cfg(test)]
pub struct MemoryWorkerGroupStore {
    groups: RwLock<Vec<WorkerGroupRecord>>,
}

#[cfg(test)]
impl MemoryWorkerGroupStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
#[cfg(test)]
impl WorkerGroupStorePort for MemoryWorkerGroupStore {
    async fn worker_groups(
        &self,
        apparatus: Option<&str>,
    ) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError> {
        let apparatus = apparatus.unwrap_or("").trim().to_lowercase();
        let mut groups = self
            .groups
            .read()
            .await
            .iter()
            .filter(|group| apparatus.is_empty() || group.apparatus.to_lowercase() == apparatus)
            .cloned()
            .collect::<Vec<_>>();
        groups = sort_groups(groups);
        Ok(groups)
    }

    async fn put_apparatus_worker_groups(
        &self,
        apparatus: &str,
        groups: Vec<WorkerGroupRecord>,
    ) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError> {
        let key = apparatus.trim().to_lowercase();
        let mut stored = self.groups.write().await;
        stored.retain(|group| group.apparatus.to_lowercase() != key);
        stored.extend(groups.clone());
        *stored = sort_groups(stored.clone());
        Ok(sort_groups(groups))
    }
}

#[cfg(test)]
mod tests {
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
}
