from __future__ import annotations

import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

import audit
from extract_contracts import ExtractionFailure, extract_case, verify_removals


def extract(
    source: str,
    package_version: str | None = None,
    allow_scenario: bool = False,
):
    source_file = audit.SourceFile("src/example.rs", source.encode())
    tree = audit.RUST_PARSER.parse(source_file.content)
    functions = list(audit.test_functions(source_file.content, tree.root_node))
    if len(functions) != 1:
        raise AssertionError(f"expected one test, got {len(functions)}")
    return extract_case(
        source_file,
        functions[0],
        package_version,
        allow_scenario=allow_scenario,
    ).case


class ContractExtractionTests(unittest.TestCase):
    def test_selected_manifest_can_explicitly_extract_scenario_contract(self) -> None:
        source = r"""
#[tokio::test]
async fn fake_lookup_error_contract() {
    let state = test_state(Some(FakeLookup));
    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/items"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}
"""
        with self.assertRaises(ExtractionFailure):
            extract(source)

        case = extract(source, allow_scenario=True)
        self.assertEqual(case["expect"]["status"], 500)

    def test_removal_guard_rejects_selected_function_restoration(self) -> None:
        manifest = {
            "remove_selected_tests": True,
            "tests": ["src/example.rs::migrated"],
        }
        with TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "src" / "example.rs"
            source.parent.mkdir()
            source.write_text(
                "#[test]\nfn migrated() {}\n\n#[test]\nfn retained() {}\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                ExtractionFailure, "test functions were restored"
            ):
                verify_removals(root, manifest)

            source.write_text("#[test]\nfn retained() {}\n", encoding="utf-8")
            verify_removals(root, manifest)

    def test_extracts_anonymous_json_request_and_body_oracle(self) -> None:
        case = extract(
            r"""
#[tokio::test]
async fn login_rejects_wrong_phone() {
    let response = build_router(test_state())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/auth/login")
                .body(Body::from(r#"{"phone":"bad"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json_body(response).await["error"], "invalid credentials");
}
"""
        )

        self.assertEqual(
            case["request"],
            {
                "method": "POST",
                "uri": "/v1/mobile/auth/login",
                "fixture": "isolated",
                "body": {"phone": "bad"},
            },
        )
        self.assertEqual(
            case["expect"],
            {"status": 401, "body": {"error": "invalid credentials"}},
        )

    def test_extracts_supplier_role_from_session_helper(self) -> None:
        case = extract(
            r"""
#[tokio::test]
async fn supplier_read_fails_without_provider() {
    let state = test_state();
    let token = supplier_session(&state).await;
    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/supplier/history", &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json_body(response).await["error"], "supplier history failed");
}
"""
        )

        self.assertEqual(case["request"]["role"], "supplier")
        self.assertEqual(case["expect"]["status"], 500)

    def test_extracts_bound_status_and_indexed_body_oracles(self) -> None:
        case = extract(
            r"""
#[tokio::test]
async fn calculate_contract() {
    let response = build_router(test_state())
        .oneshot(request("POST", "/v1/mobile/calculate", r#"{"kg":1}"#))
        .await
        .unwrap();
    let status = response.status();
    let body = json_body(response).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["results"][0]["value"], 3.5);
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
    assert!(body["score"].as_f64().unwrap_or(0.0) > 0.0);
    let code = body["code"].as_str().expect("code");
    assert!(code.starts_with("30"));
    assert!(body["detail"].as_str().unwrap_or_default().contains("required"));
}
"""
        )

        self.assertEqual(
            case["expect"]["body_paths"],
            [
                {"path": ["results", 0, "value"], "equals": 3.5},
                {"path": ["items"], "length": 2},
                {"path": ["score"], "greater_than": 0.0},
                {"path": ["code"], "starts_with": "30"},
                {"path": ["detail"], "contains": "required"},
            ],
        )

    def test_preserves_invalid_json_as_raw_body(self) -> None:
        case = extract(
            r"""
#[tokio::test]
async fn invalid_json() {
    let response = build_router(test_state())
        .oneshot(request("POST", "/v1/mobile/items", "{not-json"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
"""
        )

        self.assertEqual(case["request"]["raw_body"], "{not-json")

    def test_extracts_default_get_and_cargo_package_version(self) -> None:
        case = extract(
            r"""
#[tokio::test]
async fn handshake_identifies_version() {
    let response = build_router(test_state())
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/server/handshake")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
}
""",
            package_version="1.2.3",
        )

        self.assertEqual(case["request"]["method"], "GET")
        self.assertEqual(case["expect"]["body"]["version"], "1.2.3")

    def test_refuses_to_drop_unknown_assertion(self) -> None:
        with self.assertRaisesRegex(
            ExtractionFailure, "unsupported response assertion"
        ):
            extract(
                r"""
#[tokio::test]
async fn response_has_rows() {
    let response = build_router(test_state())
        .oneshot(request("GET", "/v1/mobile/items"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(json_body(response).await.as_array().unwrap().len() > 2);
}
"""
            )


if __name__ == "__main__":
    unittest.main()
