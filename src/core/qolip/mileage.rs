use std::collections::BTreeMap;

use serde::Serialize;
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::QolipError;
use crate::core::quantity::{erp_quantity_from_units, erp_quantity_to_units};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QolipMileage {
    pub qolip_code: String,
    pub tracking_enabled: bool,
    pub current_meter: Option<f64>,
    pub cycles: Vec<QolipMileageCycle>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QolipMileageCycle {
    pub id: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub meter: f64,
    pub rework_id: String,
}

pub(crate) fn cycle_id(rework_id: &str, qolip_code: &str) -> String {
    let identity = serde_json::json!([rework_id.trim(), qolip_code.trim().to_lowercase()]);
    format!("qolip-mileage:{:x}", Sha256::digest(identity.to_string()))
}

pub(crate) fn meter_units(meter: f64) -> Result<i64, QolipError> {
    erp_quantity_to_units(meter)
        .filter(|units| *units >= 0)
        .ok_or(QolipError::StoreFailed)
}

fn timestamp(value: OffsetDateTime) -> Result<String, QolipError> {
    value.format(&Rfc3339).map_err(|_| QolipError::StoreFailed)
}

#[derive(Clone)]
struct MemoryCycle {
    id: String,
    rework_id: String,
    started_at: OffsetDateTime,
    ended_at: Option<OffsetDateTime>,
    meter_units: i64,
}

#[derive(Clone)]
struct MemoryContribution {
    cycle_id: Option<String>,
    meter_units: i64,
}

#[derive(Clone, Default)]
pub(crate) struct MemoryQolipMileageState {
    cycles: BTreeMap<String, Vec<MemoryCycle>>,
    #[cfg_attr(not(test), allow(dead_code))]
    contributions: BTreeMap<(String, String), MemoryContribution>,
}

pub(crate) fn begin_memory_cycle(
    state: &mut MemoryQolipMileageState,
    rework_id: &str,
    qolip_code: &str,
    returned_at: OffsetDateTime,
) -> Result<(), QolipError> {
    if rework_id.trim().is_empty() || qolip_code.trim().is_empty() {
        return Err(QolipError::StoreFailed);
    }
    let cycles = state
        .cycles
        .entry(qolip_code.trim().to_lowercase())
        .or_default();
    if cycles
        .iter()
        .any(|cycle| cycle.rework_id == rework_id.trim())
    {
        return Ok(());
    }
    if let Some(previous) = cycles.last_mut() {
        if returned_at < previous.started_at {
            return Err(QolipError::StoreFailed);
        }
        previous.ended_at = Some(returned_at);
    }
    cycles.push(MemoryCycle {
        id: cycle_id(rework_id, qolip_code),
        rework_id: rework_id.trim().to_string(),
        started_at: returned_at,
        ended_at: None,
        meter_units: 0,
    });
    Ok(())
}

pub(crate) fn memory_mileage(
    state: &MemoryQolipMileageState,
    qolip_code: &str,
) -> Result<QolipMileage, QolipError> {
    let cycles = state.cycles.get(&qolip_code.trim().to_lowercase());
    let current = cycles.and_then(|cycles| cycles.last());
    Ok(QolipMileage {
        qolip_code: qolip_code.trim().to_string(),
        tracking_enabled: current.is_some(),
        current_meter: current.map(|cycle| erp_quantity_from_units(cycle.meter_units)),
        cycles: cycles
            .into_iter()
            .flatten()
            .map(|cycle| {
                Ok(QolipMileageCycle {
                    id: cycle.id.clone(),
                    started_at: timestamp(cycle.started_at)?,
                    ended_at: cycle.ended_at.map(timestamp).transpose()?,
                    meter: erp_quantity_from_units(cycle.meter_units),
                    rework_id: cycle.rework_id.clone(),
                })
            })
            .collect::<Result<_, QolipError>>()?,
    })
}

/// A source retains the cycle chosen at actual printing work, even while off.
/// Reprints and later corrections cannot enroll it into a newer return cycle.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn record_memory_output(
    state: &mut MemoryQolipMileageState,
    batch_id: &str,
    captured_cycles: &BTreeMap<String, Option<String>>,
    meter: f64,
) -> Result<(), QolipError> {
    let units = meter_units(meter)?;
    let mut staged = state.clone();
    for (code, captured_cycle) in captured_cycles {
        let identity = (batch_id.to_string(), code.trim().to_lowercase());
        let existing = staged.contributions.get(&identity);
        let cycle_id = existing
            .map(|entry| entry.cycle_id.clone())
            .unwrap_or_else(|| captured_cycle.clone());
        let previous = existing.map(|entry| entry.meter_units).unwrap_or(0);
        if let Some(id) = &cycle_id {
            let cycle = staged
                .cycles
                .values_mut()
                .flatten()
                .find(|cycle| &cycle.id == id)
                .ok_or(QolipError::StoreFailed)?;
            cycle.meter_units = cycle
                .meter_units
                .checked_sub(previous)
                .and_then(|total| total.checked_add(units))
                .ok_or(QolipError::StoreFailed)?;
        }
        staged.contributions.insert(
            identity,
            MemoryContribution {
                cycle_id,
                meter_units: units,
            },
        );
    }
    *state = staged;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_return_activates_and_retry_does_not_reset() {
        let mut state = MemoryQolipMileageState::default();
        assert_eq!(memory_mileage(&state, "A").unwrap().current_meter, None);
        let at = OffsetDateTime::from_unix_timestamp(100).unwrap();
        begin_memory_cycle(&mut state, "return-1", "A", at).unwrap();
        let id = cycle_id("return-1", "A");
        record_memory_output(
            &mut state,
            "wip",
            &BTreeMap::from([("a".into(), Some(id))]),
            10.0,
        )
        .unwrap();
        begin_memory_cycle(&mut state, "return-1", "a", at).unwrap();
        let mileage = memory_mileage(&state, "A").unwrap();
        assert_eq!(mileage.current_meter, Some(10.0));
        assert_eq!(mileage.cycles.len(), 1);
    }

    #[test]
    fn each_mold_gets_full_meter_and_only_returned_mold_resets() {
        let mut state = MemoryQolipMileageState::default();
        let at = OffsetDateTime::from_unix_timestamp(100).unwrap();
        for code in ["A", "B"] {
            begin_memory_cycle(&mut state, "first", code, at).unwrap();
        }
        let captured = BTreeMap::from([
            ("a".into(), Some(cycle_id("first", "A"))),
            ("b".into(), Some(cycle_id("first", "B"))),
            ("untracked".into(), None),
        ]);
        record_memory_output(&mut state, "wip-1", &captured, 1000.0).unwrap();
        record_memory_output(&mut state, "wip-1", &captured, 1000.0).unwrap();
        assert_eq!(
            memory_mileage(&state, "A").unwrap().current_meter,
            Some(1000.0)
        );
        assert_eq!(
            memory_mileage(&state, "B").unwrap().current_meter,
            Some(1000.0)
        );
        begin_memory_cycle(&mut state, "next", "A", at + time::Duration::seconds(20)).unwrap();
        assert_eq!(
            memory_mileage(&state, "A").unwrap().current_meter,
            Some(0.0)
        );
        assert_eq!(
            memory_mileage(&state, "B").unwrap().current_meter,
            Some(1000.0)
        );
        record_memory_output(&mut state, "wip-1", &captured, 1100.0).unwrap();
        let mileage = memory_mileage(&state, "A").unwrap();
        assert_eq!(mileage.current_meter, Some(0.0));
        assert_eq!(mileage.cycles[0].meter, 1100.0);
        assert!(mileage.cycles[0].ended_at.is_some());
        assert_eq!(
            memory_mileage(&state, "B").unwrap().current_meter,
            Some(1100.0)
        );
    }

    #[test]
    fn pre_tracking_output_never_backfills_on_correction() {
        let mut state = MemoryQolipMileageState::default();
        record_memory_output(
            &mut state,
            "old",
            &BTreeMap::from([("a".into(), None)]),
            100.0,
        )
        .unwrap();
        begin_memory_cycle(&mut state, "return", "A", OffsetDateTime::UNIX_EPOCH).unwrap();
        record_memory_output(
            &mut state,
            "old",
            &BTreeMap::from([("a".into(), Some(cycle_id("return", "A")))]),
            150.0,
        )
        .unwrap();
        assert_eq!(
            memory_mileage(&state, "A").unwrap().current_meter,
            Some(0.0)
        );
    }
}
