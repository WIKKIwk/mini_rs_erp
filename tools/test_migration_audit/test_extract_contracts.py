from __future__ import annotations

import unittest

import audit
from extract_contracts import ExtractionFailure, extract_case


def extract(source: str):
    source_file = audit.SourceFile("src/example.rs", source.encode())
    tree = audit.RUST_PARSER.parse(source_file.content)
    functions = list(audit.test_functions(source_file.content, tree.root_node))
    if len(functions) != 1:
        raise AssertionError(f"expected one test, got {len(functions)}")
    return extract_case(source_file, functions[0]).case


class ContractExtractionTests(unittest.TestCase):
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
