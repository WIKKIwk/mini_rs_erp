use super::*;
use serde_json::json;

pub fn request(from: &str, to: &str, bucket: &str, timezone: &str) -> OutputStatsRequest {
    OutputStatsRequest::try_from(
        serde_json::from_value::<OutputStatsQuery>(json!({
            "from": from, "to": to, "bucket": bucket, "timezone": timezone,
        }))
        .unwrap(),
    )
    .unwrap()
}

fn at(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

#[test]
fn local_midnight_and_monday_boundaries_are_exclusive() {
    let daily = request(
        "2026-10-18T19:00:00Z",
        "2026-10-25T19:00:00Z",
        "day",
        "Asia/Tashkent",
    );
    let points = daily.boundaries(daily.to).unwrap();
    assert_eq!(points.len(), 7);
    assert_eq!(points[0].start, "2026-10-19T00:00:00+05:00");
    assert_eq!(points[6].end, "2026-10-26T00:00:00+05:00");
    assert!(points.iter().all(|p| !p.is_partial));
    let weekly = request(
        "2026-10-19T00:00:00+05:00",
        "2026-11-02T00:00:00+05:00",
        "week",
        "Asia/Tashkent",
    );
    let points = weekly.boundaries(weekly.to).unwrap();
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].end, points[1].start);
    assert_eq!(points[1].start, "2026-10-26T00:00:00+05:00");
}

#[test]
fn spring_gap_and_fall_repetition_preserve_real_hour_offsets() {
    for (from, to, count) in [
        ("2026-03-08T00:00:00-05:00", "2026-03-09T00:00:00-04:00", 23),
        ("2026-11-01T00:00:00-04:00", "2026-11-02T00:00:00-05:00", 25),
    ] {
        let req = request(from, to, "hour", "America/New_York");
        let points = req.boundaries(req.to).unwrap();
        assert_eq!(points.len(), count);
        assert!(points.windows(2).all(|p| p[0].end_utc == p[1].start_utc));
        assert!(points.iter().all(|p| !p.is_partial));
    }
    let req = request(
        "2026-11-01T00:00:00-04:00",
        "2026-11-02T00:00:00-05:00",
        "hour",
        "America/New_York",
    );
    let p = req.boundaries(req.to).unwrap();
    assert_eq!(p[1].start, "2026-11-01T01:00:00-04:00");
    assert_eq!(p[2].start, "2026-11-01T01:00:00-05:00");
}

#[test]
fn day_and_week_use_calendar_duration_through_dst() {
    let req = request(
        "2026-03-08T00:00:00-05:00",
        "2026-03-09T00:00:00-04:00",
        "day",
        "America/New_York",
    );
    let p = req.boundaries(req.to).unwrap();
    assert_eq!(p.len(), 1);
    assert_eq!(at(&p[0].end) - at(&p[0].start), Duration::hours(23));
    let req = request(
        "2026-10-26T00:00:00-04:00",
        "2026-11-02T00:00:00-05:00",
        "week",
        "America/New_York",
    );
    let p = req.boundaries(req.to).unwrap();
    assert_eq!(p.len(), 1);
    assert_eq!(at(&p[0].end) - at(&p[0].start), Duration::hours(169));
}

#[test]
fn half_hour_shifts_and_skipped_dates_have_no_gaps_or_overlap() {
    for (from, to, transition) in [
        (
            "2026-04-05T00:00:00+11:00",
            "2026-04-06T00:00:00+10:30",
            "2026-04-05T01:30:00+10:30",
        ),
        (
            "2026-10-04T00:00:00+10:30",
            "2026-10-05T00:00:00+11:00",
            "2026-10-04T02:30:00+11:00",
        ),
    ] {
        let req = request(from, to, "hour", "Australia/Lord_Howe");
        let p = req.boundaries(req.to).unwrap();
        assert!(p.iter().any(|p| p.start == transition));
        assert!(p.windows(2).all(|p| p[0].end_utc == p[1].start_utc));
        assert_eq!(at(&p.first().unwrap().start), req.from);
        assert_eq!(at(&p.last().unwrap().end), req.to);
    }
    let req = request(
        "2011-12-29T00:00:00-10:00",
        "2012-01-01T00:00:00+14:00",
        "day",
        "Pacific/Apia",
    );
    let p = req.boundaries(req.to).unwrap();
    assert_eq!(p.len(), 2);
    assert_eq!(p[0].end, "2011-12-31T00:00:00+14:00");
}

#[test]
fn partial_and_future_buckets_are_explicit() {
    let req = request(
        "2026-10-19T00:30:00+05:00",
        "2026-10-22T00:00:00+05:00",
        "day",
        "Asia/Tashkent",
    );
    let p = req.boundaries(at("2026-10-20T12:00:00+05:00")).unwrap();
    assert_eq!(p.len(), 2);
    assert!(p.iter().all(|p| p.is_partial));
    assert!(
        req.boundaries(req.from - Duration::minutes(1))
            .unwrap()
            .is_empty()
    );
    let req = request(
        "2026-10-19T10:00:00+05:00",
        "2026-10-19T11:30:00+05:00",
        "hour",
        "Asia/Tashkent",
    );
    let p = req.boundaries(req.to).unwrap();
    assert!(!p[0].is_partial);
    assert!(p[1].is_partial);
}

#[test]
fn validation_rejects_missing_invalid_and_excessive_requests() {
    let base = json!({"from":"2026-01-01T00:00:00Z","to":"2026-01-08T00:00:00Z","bucket":"day","timezone":"Asia/Tashkent"});
    for (key, value, code) in [
        ("from", json!("bad"), "output_stats_invalid_from"),
        (
            "from",
            json!("2026-01-01T00:00:00.0000001Z"),
            "output_stats_invalid_from",
        ),
        (
            "to",
            json!("2026-01-01T00:00:00Z"),
            "output_stats_invalid_interval",
        ),
        (
            "to",
            json!("2027-01-03T00:00:00Z"),
            "output_stats_invalid_interval",
        ),
        ("bucket", json!("month"), "output_stats_invalid_bucket"),
        ("timezone", json!("+05:00"), "output_stats_invalid_timezone"),
        (
            "timezone",
            json!("Fake/Zone"),
            "output_stats_invalid_timezone",
        ),
        (
            "apparatus",
            json!("Rezka"),
            "output_stats_invalid_apparatus",
        ),
    ] {
        let mut query = base.clone();
        query[key] = value;
        let error = OutputStatsRequest::try_from(
            serde_json::from_value::<OutputStatsQuery>(query).unwrap(),
        )
        .unwrap_err();
        assert_eq!(error, OutputStatsError::Invalid(code));
    }
    for key in ["from", "to", "bucket", "timezone"] {
        let mut query = base.clone();
        query.as_object_mut().unwrap().remove(key);
        assert!(
            OutputStatsRequest::try_from(
                serde_json::from_value::<OutputStatsQuery>(query).unwrap()
            )
            .is_err()
        );
    }
    assert_eq!(OutputStatsRequest::try_from(serde_json::from_value::<OutputStatsQuery>(json!({
        "from":"2026-01-01T00:00:00Z","to":"2026-04-01T00:00:00Z","bucket":"hour","timezone":"UTC"
    })).unwrap()).unwrap_err(), OutputStatsError::Invalid("output_stats_too_many_buckets"));
}
