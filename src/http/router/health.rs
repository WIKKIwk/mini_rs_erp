use crate::app::AppState;
use crate::core::admin::models::AdminServerMonitorResponse;
use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{Response, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::any;

const HEALTHZ_BODY: &str = r#"{"ok":true}"#;

pub(super) fn routes(state: AppState) -> Router {
    Router::new()
        .route("/healthz", any(healthz))
        .route("/readyz", axum::routing::get(readyz))
        .with_state(state)
}

async fn readyz(State(state): State<AppState>) -> Response<Body> {
    let report = state.system_monitor_hub.subscribe().borrow().clone();
    let ready = is_ready(
        report.as_ref(),
        time::OffsetDateTime::now_utc().unix_timestamp(),
    );
    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({"ok": ready})),
    )
        .into_response()
}

fn is_ready(report: Option<&AdminServerMonitorResponse>, now: i64) -> bool {
    report.is_some_and(|report| {
        let sampled_at = report.doctor.checked_at_unix;
        report.database.configured
            && report.database.reachable
            && !report.doctor.protected
            && !report
                .doctor
                .checks
                .iter()
                .any(|check| check.id == "domain" && check.critical)
            && !report
                .doctor
                .checks
                .iter()
                .any(|check| check.id == "database" && check.critical)
            && sampled_at > 0
            && now >= sampled_at
            && now.saturating_sub(sampled_at) <= 15
    })
}

async fn healthz() -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(HEALTHZ_BODY))
        .expect("static health response is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_requires_fresh_reachable_database_and_no_resource_protection() {
        assert!(!is_ready(None, 100));
        let mut report = AdminServerMonitorResponse::default();
        report.database.configured = true;
        report.database.reachable = true;
        report.doctor.checked_at_unix = 100;
        assert!(is_ready(Some(&report), 100));
        assert!(is_ready(Some(&report), 115));
        assert!(!is_ready(Some(&report), 116));
        assert!(!is_ready(Some(&report), 99));
        report
            .doctor
            .checks
            .push(crate::core::system_doctor::DoctorCheck {
                id: "database".to_string(),
                status: "recovering".to_string(),
                critical: true,
                ..Default::default()
            });
        assert!(!is_ready(Some(&report), 100));
        report.doctor.checks.clear();
        report.doctor.protected = true;
        assert!(!is_ready(Some(&report), 100));
        report.doctor.protected = false;
        report
            .doctor
            .checks
            .push(crate::core::system_doctor::DoctorCheck {
                id: "domain".to_string(),
                status: "critical".to_string(),
                critical: true,
                ..Default::default()
            });
        assert!(!is_ready(Some(&report), 100));
        report.doctor.checks.pop();
        report.database.reachable = false;
        assert!(!is_ready(Some(&report), 100));
    }
}
