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
    let body = br#"{"order":"zakaz-001","quantities":[1,250],"metadata":{"a":1,"a":2}}"#;
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

#[test]
fn deep_known_and_ignored_fields_keep_the_baseline_depth_behavior() {
    #[derive(Deserialize, Debug, PartialEq)]
    struct Ignoring {
        order_id: String,
    }
    for depth in [126, 127, 128, 129, 254, 512, 2000] {
        let nested = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        let ignored = format!(r#"{{"ignored":{nested},"order_id":"001"}}"#);
        let baseline = serde_json::from_str::<Value>(&nested);
        let baseline_ignored = serde_json::from_str::<Ignoring>(&ignored);
        for engine in engines() {
            assert_eq!(
                engine.from_slice::<Value>(nested.as_bytes()).is_ok(),
                baseline.is_ok()
            );
            let parsed = engine.from_slice::<Ignoring>(ignored.as_bytes());
            assert_eq!(parsed.is_ok(), baseline_ignored.is_ok());
            if let (Ok(actual), Ok(expected)) = (parsed, &baseline_ignored) {
                assert_eq!(&actual, expected);
            }
        }
    }
}

#[test]
fn real_map_count_validation_and_nonfinite_output_match_the_baseline() {
    use mini_rs_erp::core::production_map::ProductionMapDefinition;
    for count in ["7", "7.0", "7.25", "-1", "18446744073709551615"] {
        let body = format!(r#"{{"id":"001","product_code":"P","title":"T","roll_count":{count}}}"#);
        let baseline = serde_json::from_str::<ProductionMapDefinition>(&body);
        for engine in engines() {
            let parsed = engine.from_slice::<ProductionMapDefinition>(body.as_bytes());
            assert_eq!(
                parsed.is_ok(),
                baseline.is_ok(),
                "{} {count}",
                engine.name()
            );
            if let (Ok(actual), Ok(expected)) = (parsed, &baseline) {
                assert_eq!(&actual, expected);
            }
        }
    }
    for quantity in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -0.0,
        f64::MIN_POSITIVE,
        f64::MAX,
    ] {
        let baseline = serde_json::to_vec(&quantity).unwrap();
        for engine in engines() {
            assert_eq!(engine.to_vec(&quantity).unwrap(), baseline);
        }
    }
}

#[test]
fn numeric_rounding_keeps_the_current_serde_json_behavior() {
    for raw in [
        "4563.952087548593714563e-91",
        "2968.802212938538312968e-90",
        "9931.541946641252159931e-83",
        "1745.865272634031931745e-81",
        "7004.585233762587617004e-78",
        "123456789012345678901234567890",
        "1.0000000000000002",
        "-0",
        "9007199254740993",
        "-9007199254740993",
        "-0.0",
        "4.9406564584124654e-324",
    ] {
        let expected: f64 = serde_json::from_str(raw).unwrap();
        for engine in engines() {
            let actual: f64 = engine.from_slice(raw.as_bytes()).unwrap();
            assert_eq!(
                actual.to_bits(),
                expected.to_bits(),
                "{}: {raw}",
                engine.name()
            );
        }
    }
    // Escaped quotes and numeric-looking strings do not confuse the guard.
    let body = br#"{"order_id":"digits 12345678901234567890 and \"[e.\"","quantities":[7]}"#;
    for engine in engines() {
        assert_eq!(
            engine.from_slice::<Request>(body).unwrap(),
            serde_json::from_slice::<Request>(body).unwrap()
        );
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
