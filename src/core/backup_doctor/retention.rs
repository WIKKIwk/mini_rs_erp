use std::collections::HashSet;
use std::fs;
use std::path::Path;

use time::OffsetDateTime;

use super::catalog::{collect_manifest_paths, read_manifest};

pub(super) fn apply_retention(root: &Path, now: OffsetDateTime) {
    let mut manifests = Vec::new();
    collect_manifest_paths(root, &mut manifests);
    let mut snapshots = manifests
        .into_iter()
        .filter_map(|path| read_manifest(&path).map(|snapshot| (path, snapshot)))
        .collect::<Vec<_>>();
    snapshots.sort_by_key(|(_, snapshot)| std::cmp::Reverse(snapshot.completed_at_unix));
    let mut kept_weeks = HashSet::new();
    let mut kept_months = HashSet::new();
    for (index, (manifest_path, snapshot)) in snapshots.into_iter().enumerate() {
        let completed = OffsetDateTime::from_unix_timestamp(snapshot.completed_at_unix).ok();
        let age_days = completed
            .map(|value| (now - value).whole_days().max(0))
            .unwrap_or(i64::MAX);
        let keep = if index == 0 || age_days <= 14 {
            true
        } else if age_days <= 56 {
            completed.is_some_and(|value| {
                kept_weeks.insert((value.year(), value.iso_week()))
            })
        } else if age_days <= 365 {
            completed.is_some_and(|value| {
                kept_months.insert((value.year(), u8::from(value.month())))
            })
        } else {
            false
        };
        let failed_expired = snapshot.status == "failed" && age_days > 7;
        if (!keep || failed_expired)
            && let Some(directory) = manifest_path.parent()
            && directory.parent() == Some(root)
            && fs::remove_dir_all(directory).is_ok()
        {
            tracing::info!(backup_id = %snapshot.id, "backup doctor pruned snapshot");
        }
    }
}
