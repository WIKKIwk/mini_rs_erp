use std::collections::BTreeSet;

use super::{WorkerGroupError, WorkerGroupRecord, WorkerGroupUpsert, default_work_days_per_week};

pub(super) fn normalize_input(
    input: WorkerGroupUpsert,
) -> Result<WorkerGroupRecord, WorkerGroupError> {
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

pub(super) fn ensure_workers_not_duplicated(
    groups: &[WorkerGroupRecord],
) -> Result<(), WorkerGroupError> {
    let mut seen = BTreeSet::new();
    for worker_id in groups.iter().flat_map(|group| group.worker_ids.iter()) {
        if !seen.insert(worker_id.to_lowercase()) {
            return Err(WorkerGroupError::DuplicateWorker);
        }
    }
    Ok(())
}

pub(super) fn sort_groups(mut groups: Vec<WorkerGroupRecord>) -> Vec<WorkerGroupRecord> {
    groups.sort_by(|left, right| {
        left.apparatus
            .to_lowercase()
            .cmp(&right.apparatus.to_lowercase())
            .then(left.group_code.cmp(&right.group_code))
    });
    groups
}
