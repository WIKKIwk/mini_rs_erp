from __future__ import annotations

import unittest

from audit import SourceFile, audit_source


def finding(source: str):
    _, findings = audit_source(SourceFile("src/example.rs", source.encode()))
    if len(findings) != 1:
        raise AssertionError(f"expected one finding, got {len(findings)}")
    return findings[0]


class MigrationAuditTests(unittest.TestCase):
    def test_classifies_static_single_request_as_automatic(self) -> None:
        result = finding(
            r"""
#[tokio::test]
async fn me_requires_auth() {
    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/me"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
"""
        )

        self.assertEqual(result.classification, "automatic_contract")
        self.assertEqual(result.signals.http_methods, ["GET"])
        self.assertEqual(result.signals.literal_uris, ["/v1/mobile/me"])
        self.assertEqual(result.signals.status_codes, ["UNAUTHORIZED"])

    def test_classifies_multi_request_flow_as_scenario(self) -> None:
        result = finding(
            r"""
#[tokio::test]
async fn create_then_read() {
    let created = build_router(state.clone())
        .oneshot(request("POST", "/v1/mobile/items"))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::OK);
    let read = build_router(state)
        .oneshot(request("GET", "/v1/mobile/items/1"))
        .await
        .unwrap();
    assert_eq!(read.status(), StatusCode::OK);
}
"""
        )

        self.assertEqual(result.classification, "scenario_contract")
        self.assertIn("contains 2 HTTP executions", result.reasons)

    def test_keeps_database_test_in_rust(self) -> None:
        result = finding(
            r"""
#[sqlx::test]
async fn transaction_rolls_back(pool: PgPool) {
    let mut tx = pool.begin().await.unwrap();
    assert!(save(&mut tx).await.is_ok());
    tx.rollback().await.unwrap();
}
"""
        )

        self.assertEqual(result.classification, "rust_native")
        self.assertTrue(result.signals.database_access)

    def test_keeps_internal_unit_test_in_rust(self) -> None:
        result = finding(
            r"""
#[test]
fn normalizes_title() {
    assert_eq!(normalize(" A "), "A");
}
"""
        )

        self.assertEqual(result.classification, "rust_native")
        self.assertEqual(result.reasons, ["no router HTTP execution"])

    def test_marks_custom_fixture_as_scenario(self) -> None:
        result = finding(
            r"""
#[tokio::test]
async fn lookup_uses_fake() {
    let state = test_state(Some(Arc::new(FakeLookup::found())));
    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/lookup"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
"""
        )

        self.assertEqual(result.classification, "scenario_contract")
        self.assertTrue(result.signals.custom_fixture)

    def test_allows_default_test_state_for_automatic_contract(self) -> None:
        result = finding(
            r"""
#[tokio::test]
async fn lookup_uses_test_state() {
    let state = test_state();
    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/lookup"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
"""
        )

        self.assertEqual(result.classification, "automatic_contract")
        self.assertFalse(result.signals.fixture_setup)

    def test_marks_parameterized_test_state_as_scenario(self) -> None:
        result = finding(
            r"""
#[tokio::test]
async fn lookup_uses_custom_test_state() {
    let state = test_state(None);
    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/lookup"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}
"""
        )

        self.assertEqual(result.classification, "scenario_contract")
        self.assertTrue(result.signals.fixture_setup)

    def test_marks_formatted_uri_as_scenario(self) -> None:
        result = finding(
            r"""
#[tokio::test]
async fn reads_dynamic_item() {
    let response = build_router(state)
        .oneshot(request("GET", &format!("/v1/mobile/items/{item_id}")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
"""
        )

        self.assertEqual(result.classification, "scenario_contract")
        self.assertTrue(result.signals.dynamic_request)


if __name__ == "__main__":
    unittest.main()
