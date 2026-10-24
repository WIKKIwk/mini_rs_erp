//! Read-only output reporting. PostgreSQL aggregates quantities as NUMERIC;
//! Rust constructs calendar boundaries without loading individual output rows.
use chrono::{
    DateTime, Datelike, Duration, LocalResult, NaiveDateTime, Offset, SecondsFormat, TimeZone,
    Timelike, Utc,
};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::ProductionMapService;
use crate::core::apparatus_standard::ApparatusId;

pub const MAX_INTERVAL_DAYS: i64 = 366;
pub const MAX_BUCKETS: usize = 1_500;
pub const MAX_POINTS: usize = 50_000;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OutputBucket {
    Hour,
    Day,
    Week,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputStatsQuery {
    pub from: Option<String>,
    pub to: Option<String>,
    pub bucket: Option<String>,
    pub timezone: Option<String>,
    pub apparatus: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OutputStatsRequest {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub bucket: OutputBucket,
    pub timezone: Tz,
    pub apparatus: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OutputStatsError {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("output_stats_store_failed")]
    StoreFailed,
}

/// PostgreSQL serializes the final JSON directly so six-place NUMERIC sums do
/// not pass through f64. Only the reporting store constructs this response.
#[derive(Debug)]
pub struct OutputStatsReport(pub String);

#[derive(Debug, Serialize)]
pub struct OutputStatsPointBoundary {
    pub start: String,
    pub end: String,
    pub is_partial: bool,
    pub start_utc: String,
    pub end_utc: String,
}

pub fn utc_string(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

impl TryFrom<OutputStatsQuery> for OutputStatsRequest {
    type Error = OutputStatsError;

    fn try_from(query: OutputStatsQuery) -> Result<Self, Self::Error> {
        let parse = |value: Option<String>, code| {
            let value = value.ok_or(OutputStatsError::Invalid(code))?;
            let instant = DateTime::parse_from_rfc3339(&value)
                .map_err(|_| OutputStatsError::Invalid(code))?
                .with_timezone(&Utc);
            // Match PostgreSQL's microsecond resolution without moving a bound.
            if instant.nanosecond() >= 1_000_000_000
                || instant.nanosecond() % 1_000 != 0
                || !(1..=9999).contains(&instant.year())
            {
                return Err(OutputStatsError::Invalid(code));
            }
            Ok(instant)
        };
        let from = parse(query.from, "output_stats_invalid_from")?;
        let to = parse(query.to, "output_stats_invalid_to")?;
        if from >= to || to - from > Duration::days(MAX_INTERVAL_DAYS) {
            return Err(OutputStatsError::Invalid("output_stats_invalid_interval"));
        }
        let bucket = match query.bucket.as_deref() {
            Some("hour") => OutputBucket::Hour,
            Some("day") => OutputBucket::Day,
            Some("week") => OutputBucket::Week,
            _ => return Err(OutputStatsError::Invalid("output_stats_invalid_bucket")),
        };
        let timezone = query
            .timezone
            .and_then(|v| v.parse::<Tz>().ok())
            .ok_or(OutputStatsError::Invalid("output_stats_invalid_timezone"))?;
        let apparatus = query
            .apparatus
            .map(|id| {
                ApparatusId::new(id.clone())
                    .map_err(|_| OutputStatsError::Invalid("output_stats_invalid_apparatus"))?;
                Ok(id)
            })
            .transpose()?;
        let request = Self {
            from,
            to,
            bucket,
            timezone,
            apparatus,
        };
        request.boundaries(to)?;
        Ok(request)
    }
}

impl OutputStatsRequest {
    pub fn boundaries(
        &self,
        as_of: DateTime<Utc>,
    ) -> Result<Vec<OutputStatsPointBoundary>, OutputStatsError> {
        let local_from = self.from.with_timezone(&self.timezone).naive_local();
        let local_to = self.to.with_timezone(&self.timezone).naive_local();
        let padding = if self.bucket == OutputBucket::Week {
            14
        } else {
            2
        };
        let mut local = floor(local_from.min(local_to), self.bucket) - Duration::days(padding);
        let last = floor(local_from.max(local_to), self.bucket) + Duration::days(padding);
        let step = match self.bucket {
            OutputBucket::Hour => Duration::hours(1),
            OutputBucket::Day => Duration::days(1),
            OutputBucket::Week => Duration::weeks(1),
        };
        let mut instants = BTreeSet::new();
        while local <= last {
            match resolve_local(self.timezone, local)? {
                LocalResult::Single(value) => {
                    instants.insert(value.with_timezone(&Utc));
                }
                LocalResult::Ambiguous(a, b) => {
                    instants.insert(a.min(b).with_timezone(&Utc));
                    // A repeated hour has two buckets. A repeated midnight
                    // still belongs to one calendar day/week.
                    if self.bucket == OutputBucket::Hour {
                        instants.insert(a.max(b).with_timezone(&Utc));
                    }
                }
                LocalResult::None => unreachable!("resolved gap"),
            }
            local += step;
        }
        if self.bucket == OutputBucket::Hour {
            // A half-hour rollback can repeat only the *end* of a wall hour,
            // so from_local_datetime(hour:00) alone does not reveal it.
            let nominal: Vec<_> = instants.iter().copied().collect();
            for pair in nominal.windows(2) {
                let offset = |t: i64| {
                    self.timezone
                        .offset_from_utc_datetime(
                            &DateTime::from_timestamp(t, 0)
                                .expect("boundary timestamp")
                                .naive_utc(),
                        )
                        .fix()
                        .local_minus_utc()
                };
                let (mut lo, mut hi) = (pair[0].timestamp(), pair[1].timestamp() - 1);
                let before = offset(lo);
                if offset(hi) >= before {
                    continue;
                }
                while hi - lo > 1 {
                    let mid = lo + (hi - lo) / 2;
                    if offset(mid) == before {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                instants.insert(DateTime::from_timestamp(hi, 0).expect("transition timestamp"));
            }
        }
        let instants: Vec<_> = instants.into_iter().collect();
        let mut points = Vec::new();
        let mut requested_count = 0;
        for pair in instants.windows(2) {
            let (start, end) = (pair[0], pair[1]);
            if end <= self.from || start >= self.to {
                continue;
            }
            requested_count += 1;
            if requested_count > MAX_BUCKETS {
                return Err(OutputStatsError::Invalid("output_stats_too_many_buckets"));
            }
            if start >= as_of || self.from >= as_of {
                continue;
            }
            points.push(OutputStatsPointBoundary {
                start: start.with_timezone(&self.timezone).to_rfc3339(),
                end: end.with_timezone(&self.timezone).to_rfc3339(),
                is_partial: start < self.from || end > self.to || end > as_of,
                start_utc: utc_string(start),
                end_utc: utc_string(end),
            });
        }
        Ok(points)
    }
}

fn floor(local: NaiveDateTime, bucket: OutputBucket) -> NaiveDateTime {
    let date = if bucket == OutputBucket::Week {
        local.date() - Duration::days(local.weekday().num_days_from_monday().into())
    } else {
        local.date()
    };
    date.and_hms_opt(
        if bucket == OutputBucket::Hour {
            local.hour()
        } else {
            0
        },
        0,
        0,
    )
    .expect("calendar boundary")
}

/// Resolve missing wall times to the first real instant after the gap.
/// Bisection also handles half-hour shifts and skipped calendar dates.
fn resolve_local(
    tz: Tz,
    local: NaiveDateTime,
) -> Result<LocalResult<DateTime<Tz>>, OutputStatsError> {
    let result = tz.from_local_datetime(&local);
    if !matches!(result, LocalResult::None) {
        return Ok(result);
    }
    let mut high = 1_i64;
    while matches!(
        tz.from_local_datetime(&(local + Duration::seconds(high))),
        LocalResult::None
    ) {
        high *= 2;
        if high > 172_800 {
            return Err(OutputStatsError::Invalid(
                "output_stats_invalid_timezone_boundary",
            ));
        }
    }
    let mut low = 0;
    while high - low > 1 {
        let mid = (high + low) / 2;
        if matches!(
            tz.from_local_datetime(&(local + Duration::seconds(mid))),
            LocalResult::None
        ) {
            low = mid;
        } else {
            high = mid;
        }
    }
    Ok(tz.from_local_datetime(&(local + Duration::seconds(high))))
}

impl ProductionMapService {
    pub async fn output_stats(
        &self,
        request: &OutputStatsRequest,
        scope: Option<&[String]>,
    ) -> Result<OutputStatsReport, OutputStatsError> {
        self.store.output_stats(request, scope).await
    }
}

#[cfg(test)]
mod tests;
