//! Wire compatibility checks for the optional codec; no database or live server.
use std::collections::BTreeMap;

use axum::body::to_bytes;
use axum::http::{StatusCode, header};
use mini_rs_erp::http::json_research::{Engine, Settings};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

fn engines() -> Vec<Engine> {
    let mut engines = vec![Engine::SerdeJson];
    if cfg!(feature = "sonic-json-research") {
        engines.push(Engine::SonicRs);
    }
    engines
}

#[test]
fn research_is_explicit_and_encoder_decoder_are_independent() {
    assert_eq!(Settings::from_names("", "").unwrap(), Settings::default());
    assert!(Settings::from_names("auto", "serde_json").is_err());
    assert!(Settings::from_names("serde_json", "unknown").is_err());
    if cfg!(feature = "sonic-json-research") {
        assert_eq!(
            Settings::from_names("sonic_rs", "serde_json").unwrap(),
            Settings {
                encoder: Engine::SonicRs,
                decoder: Engine::SerdeJson,
            }
        );
        assert_eq!(
            Settings::from_names("serde_json", "sonic_rs").unwrap(),
            Settings {
                encoder: Engine::SerdeJson,
                decoder: Engine::SonicRs,
            }
        );
    } else {
        assert!(Settings::from_names("sonic_rs", "serde_json").is_err());
        assert!(Settings::from_names("serde_json", "sonic_rs").is_err());
        assert!(Engine::SonicRs.to_vec(&json!({})).is_err());
    }
}

#[test]
fn snapshot_delta_strings_and_quantities_keep_their_json_meaning() {
    let payload = json!({
        "ok":true,"type":"state_delta","base_rev":41,"rev":42,
        "epoch":"research","scope":"aparatchi/apparatus:default:asset-010",
        "patch":{
            "order_controls":{"upsert":{"zakaz-001":{
                "label":"O‘zbekcha — Русский — العربية — 中文 🧪",
                "note":"quote: \" / slash: \\ / newline:\n / tab:\t / null:\u{0}",
                "qty":1234.125,"minus_zero":-0.0,"largest_id":u64::MAX,
                "smallest_id":i64::MIN,"pending":null,"enabled":false,
            }},"remove":[]},
            "sequences":{"upsert":{"apparatus:default:asset-010":["zakaz-001","zakaz-002"]},"remove":[]}
        }
    });
    for engine in engines() {
        let text = engine.to_string(&payload).unwrap();
        let bytes = engine.to_vec(&payload).unwrap();
        assert_eq!(text.as_bytes(), bytes);
        assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), payload);
        assert_eq!(engine.from_slice::<Value>(&bytes).unwrap(), payload);
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Request {
    #[serde(alias = "order")]
    order_id: String,
    #[serde(default)]
    quantities: Vec<f64>,
    #[serde(default)]
    metadata: BTreeMap<String, Value>,
}

#[test]
fn serde_contract_and_duplicate_key_semantics_are_preserved() {
    let body = br#"{"order":"zakaz-001","quantities":[1.25,2.5e2],"metadata":{"a":1,"a":2}}"#;
    let expected: Request = serde_json::from_slice(body).unwrap();
    for engine in engines() {
        assert_eq!(engine.from_slice::<Request>(body).unwrap(), expected);
        let serialized = engine.to_vec(&expected).unwrap();
        assert_eq!(engine.from_slice::<Request>(&serialized).unwrap(), expected);
        assert!(
            engine
                .from_slice::<Request>(br#"{"order_id":"a","order_id":"b"}"#)
                .is_err()
        );
        assert!(
            engine
                .from_slice::<Request>(br#"{"order_id":"a","extra":true}"#)
                .is_err()
        );
        assert!(
            engine
                .from_slice::<Request>(br#"{"quantities":[]}"#)
                .is_err()
        );
    }
}

#[test]
fn external_input_is_validated_and_trailing_payloads_are_rejected() {
    let invalid: &[&[u8]] = &[
        b"",
        b"{",
        b"[1,]",
        b"{\"a\":1,}",
        b"{\"a\":NaN}",
        b"{}{}",
        b"[] garbage",
        b"{\"a\":\"\xff\"}",
        br#"{"a":"\uD800"}"#,
        br#"{"a":01}"#,
        br#"{"a":1e999}"#,
    ];
    for engine in engines() {
        for input in invalid {
            assert!(
                engine.from_slice::<Value>(input).is_err(),
                "{} accepted {input:?}",
                engine.name()
            );
        }
        assert_eq!(
            engine.from_slice::<Value>(b" \n {\"a\": 1}\t").unwrap(),
            json!({"a":1})
        );
        let nested = format!("{}0{}", "[".repeat(512), "]".repeat(512));
        assert!(engine.from_slice::<Value>(nested.as_bytes()).is_err());
    }
}

#[tokio::test]
async fn http_success_and_serialization_error_keep_status_and_content_type() {
    struct Failing;
    impl Serialize for Failing {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("fixture serialization failure"))
        }
    }
    for engine in engines() {
        let response = engine.response(json!({"ok":true,"rows":[{"id":"001"}]}));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({"ok":true,"rows":[{"id":"001"}]})
        );
        let response = engine.response(Failing);
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/plain; charset=utf-8"
        );
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"fixture serialization failure");
    }
}
