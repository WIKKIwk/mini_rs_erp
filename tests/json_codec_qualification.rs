//! Differential encoder qualification, including SIMD tails and startup policy.
use mini_rs_erp::http::json_research::{self as codec, Engine};

#[test]
fn configuration_child() {
    let Ok(expected) = std::env::var("JSON_CODEC_TEST_CHILD") else {
        return;
    };
    if expected == "error" {
        assert!(codec::initialize_from_env().is_err());
        return;
    }
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(32));
    let threads: Vec<_> = (0..32)
        .map(|_| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                codec::initialize_from_env().unwrap()
            })
        })
        .collect();
    for thread in threads {
        let settings = thread.join().unwrap();
        assert_eq!(settings.encoder.name(), expected);
        assert_eq!(settings.decoder, Engine::SerdeJson);
        assert_eq!(codec::settings(), settings);
    }
}

#[test]
fn startup_settings_are_validated_without_mutating_the_parent_environment() {
    use std::process::Command;
    let sonic = if cfg!(feature = "sonic-json") {
        "sonic_rs"
    } else {
        "error"
    };
    for (encoder, legacy, decoder, expected) in [
        (None, None, None, "serde_json"),
        (Some("serde_json"), None, None, "serde_json"),
        (Some("sonic_rs"), None, None, sonic),
        (None, Some("sonic_rs"), None, sonic),
        (Some("sonic_rs"), Some("sonic_rs"), None, sonic),
        (Some("serde_json"), Some("sonic_rs"), None, "error"),
        (Some("sonic_rs"), Some("serde_json"), None, "error"),
        (Some("unknown"), None, None, "error"),
        (None, Some("unknown"), None, "error"),
        (None, None, Some("unknown"), "error"),
    ] {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "configuration_child", "--nocapture"]);
        for key in [
            codec::OUTPUT_ENCODER_ENV,
            codec::ENCODER_ENV,
            codec::DECODER_ENV,
        ] {
            command.env_remove(key);
        }
        for (key, value) in [
            (codec::OUTPUT_ENCODER_ENV, encoder),
            (codec::ENCODER_ENV, legacy),
            (codec::DECODER_ENV, decoder),
        ] {
            if let Some(value) = value {
                command.env(key, value);
            }
        }
        let output = command
            .env("JSON_CODEC_TEST_CHILD", expected)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{encoder:?}/{legacy:?}/{decoder:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[cfg(not(feature = "sonic-json-research"))]
    {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "configuration_child"])
            .env_remove(codec::OUTPUT_ENCODER_ENV)
            .env_remove(codec::ENCODER_ENV)
            .env(codec::DECODER_ENV, "sonic_rs")
            .env("JSON_CODEC_TEST_CHILD", "error")
            .output()
            .unwrap();
        assert!(output.status.success());
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "configuration_child"])
            .env(
                codec::OUTPUT_ENCODER_ENV,
                std::ffi::OsString::from_vec(vec![255]),
            )
            .env_remove(codec::ENCODER_ENV)
            .env_remove(codec::DECODER_ENV)
            .env("JSON_CODEC_TEST_CHILD", "error")
            .output()
            .unwrap();
        assert!(output.status.success());
    }
}

#[test]
fn invalid_encoder_stops_the_actual_server_before_database_bootstrap() {
    for (encoder, legacy) in [("unknown", None), ("serde_json", Some("sonic_rs"))] {
        let directory = tempfile::tempdir().unwrap();
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_mini_rs_erp"));
        command
            .env_clear()
            .current_dir(directory.path())
            .env(codec::OUTPUT_ENCODER_ENV, encoder)
            // If validation were skipped, the database layer would fail on
            // this deliberately invalid URL instead of the encoder setting.
            .env("DATABASE_URL", "invalid-database-url");
        if let Some(legacy) = legacy {
            command.env(codec::ENCODER_ENV, legacy);
        }
        let output = command.output().unwrap();
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("InvalidConfig") && stderr.contains("JSON_ENCODER"),
            "{stderr}"
        );
    }
}

#[cfg(feature = "sonic-json")]
mod encoding {
    use super::*;
    use proptest::prelude::*;
    use proptest::test_runner::{Config, RngSeed};
    use serde::Serialize;
    use serde_json::Value;
    use std::collections::BTreeMap;

    fn identical<T: Serialize + ?Sized>(value: &T) {
        let expected = serde_json::to_vec(value).unwrap();
        assert_eq!(Engine::SonicRs.to_vec(value).unwrap(), expected);
        assert_eq!(
            Engine::SonicRs.to_string(value).unwrap().as_bytes(),
            expected
        );
    }

    fn strings() -> impl Strategy<Value = String> {
        prop::collection::vec(any::<char>(), 0..260).prop_map(|chars| chars.into_iter().collect())
    }

    fn values() -> impl Strategy<Value = Value> {
        let leaf = prop_oneof![
            Just(Value::Null),
            any::<bool>().prop_map(Value::Bool),
            any::<i64>().prop_map(|v| v.into()),
            any::<u64>().prop_map(|v| v.into()),
            any::<u64>().prop_map(|bits| serde_json::Number::from_f64(f64::from_bits(bits))
                .map(Value::Number)
                .unwrap_or(Value::Null)),
            strings().prop_map(Value::String),
        ];
        leaf.prop_recursive(6, 256, 12, |inner| {
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..12).prop_map(Value::Array),
                prop::collection::btree_map(strings(), inner, 0..12)
                    .prop_map(|map| Value::Object(map.into_iter().collect())),
            ]
        })
    }

    proptest! {
        #![proptest_config(Config { cases: 16_384, failure_persistence: None,
            rng_seed: RngSeed::Fixed(0xacc0_2026_1028), ..Config::default() })]
        #[test]
        fn numeric_bit_patterns_match_serde(f64_bits in any::<u64>(), f32_bits in any::<u32>(),
            signed in any::<i128>(), unsigned in any::<u128>()) {
            identical(&f64::from_bits(f64_bits));
            identical(&f32::from_bits(f32_bits));
            identical(&signed);
            identical(&unsigned);
        }
    }

    proptest! {
        #![proptest_config(Config { cases: 2_048, failure_persistence: None,
            rng_seed: RngSeed::Fixed(0xacc0_2026_1029), ..Config::default() })]
        #[test]
        fn nested_unicode_payloads_are_byte_identical(value in values()) {
            identical(&value);
        }
    }

    #[test]
    fn serde_representations_map_keys_and_errors_match() {
        #[derive(Serialize)]
        #[serde(tag = "kind", content = "data", rename_all = "snake_case")]
        enum Tagged {
            Unit,
            Row { id: String, quantity: f64 },
            Tuple(i64, bool),
        }
        #[derive(Serialize)]
        struct Flattened {
            #[serde(rename = "order")]
            id: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            absent: Option<String>,
            #[serde(flatten)]
            metadata: BTreeMap<String, Tagged>,
        }
        identical(&Flattened {
            id: "zakaz-🧪".into(),
            absent: None,
            metadata: BTreeMap::from([
                ("a".into(), Tagged::Unit),
                (
                    "b".into(),
                    Tagged::Row {
                        id: "O‘zbekcha".into(),
                        quantity: -0.0,
                    },
                ),
                ("c".into(), Tagged::Tuple(i64::MIN, false)),
            ]),
        });
        identical(&BTreeMap::from([
            (i128::MIN, "minimum"),
            (i128::MAX, "maximum"),
        ]));
        identical(&BTreeMap::from([(false, "no"), (true, "yes")]));
        let invalid = BTreeMap::from([((1, 2), "tuple key")]);
        assert!(serde_json::to_vec(&invalid).is_err());
        assert!(Engine::SonicRs.to_vec(&invalid).is_err());
        for length in [
            0, 1, 15, 16, 17, 31, 32, 33, 63, 64, 65, 127, 128, 129, 4096, 32768,
        ] {
            identical(&"\0\t\n\r\u{1f}\"\\é中🧪".repeat(length));
        }
    }

    #[test]
    fn parallel_encoding_keeps_buffers_and_output_independent() {
        std::thread::scope(|scope| {
            for worker in 0..16 {
                scope.spawn(move || {
                    for revision in 0..256 {
                        identical(&serde_json::json!({"worker":worker,"rev":revision,
                        "rows":[{"id":format!("zakaz-{worker}-{revision}"),
                        "label":"O‘zbekcha / Русский / العربية 🧪".repeat(revision)}]}));
                    }
                });
            }
        });
    }

    #[cfg(unix)]
    #[test]
    fn simd_string_tails_at_unreadable_page_boundaries() {
        // Both adjacent pages are inaccessible: a SIMD overread/underread
        // crashes this test process instead of silently reading mapped memory.
        unsafe {
            let page = usize::try_from(libc::sysconf(libc::_SC_PAGESIZE)).unwrap();
            let address = libc::mmap(
                std::ptr::null_mut(),
                page * 3,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            );
            assert_ne!(address, libc::MAP_FAILED);
            struct Mapping(*mut libc::c_void, usize);
            impl Drop for Mapping {
                fn drop(&mut self) {
                    unsafe {
                        libc::munmap(self.0, self.1);
                    }
                }
            }
            let _mapping = Mapping(address, page * 3);
            let readable = (address as *mut u8).add(page);
            assert_eq!(
                libc::mprotect(readable.cast(), page, libc::PROT_READ | libc::PROT_WRITE),
                0
            );
            for length in 0..260 {
                for pattern in ["x", "\"", "\\", "\0", "é", "中", "🧪"] {
                    let value = pattern.repeat(length);
                    for offset in [0, page - value.len()] {
                        std::ptr::copy_nonoverlapping(
                            value.as_ptr(),
                            readable.add(offset),
                            value.len(),
                        );
                        let slice = std::slice::from_raw_parts(readable.add(offset), value.len());
                        identical(std::str::from_utf8(slice).unwrap());
                    }
                }
            }
        }
    }
}
