# Audit fixes

Branch `audit-fixes` starts at `f093beb7`. The table follows the requested commit order.

| ID | Change | Regression coverage |
| --- | --- | --- |
| S01 | Supplier directory reads return warehouse and unit defaults only. The detail response omits the login code. Authorized regeneration retains its existing response shape. | `supplier_directory_read_does_not_disclose_credentials_or_grant_code_management` exercises both reads and denies regeneration to a directory reader. |
| S02 | Avatar URLs contain a five-minute ticket bound to the issuing session, target and version. Header-authenticated fetches remain supported. Both avatar routes reject a session bearer in the query. Mobile request spans log the path. | Profile route tests fetch a returned image URL and reject query bearers. Scoped-ticket tests cover target/version changes, expiry, tampering and revocation. |
| S09 | Telegram Bot API and file requests strip the URL from Reqwest errors before producing loggable diagnostics. | `bot_transport_diagnostics_strip_private_request_urls` checks the formatted error keeps its class and excludes the private URL. |
| P01 | Login filters directory candidates by phone before fetching verifiers. Built-in identities use their specific key. PostgreSQL retrieves only matching account rows through a bound array. | `login_reads_verifiers_only_for_matching_phones` records the exact keys requested for an unknown phone, a supplier and a built-in account. |
| S10 | Every HTTP/1 listener has a Tokio timer and a 15-second header deadline. | `header_deadline_closes_an_idle_connection` runs the production builder against an in-memory connection and checks Hyper's timeout error. |
| S11 | Push registration rejects malformed or oversized tokens. FCM diagnostics use a short SHA-256 fingerprint, including for legacy Unicode tokens. | `registration_rejects_malformed_tokens_before_storage` and `log_identifiers_are_safe_for_legacy_unicode_and_short_tokens` cover both paths. |
| P07 | Migration `0205_gscale_receipt_barcode_lookup` indexes `lower(barcode), updated_at DESC` and permits historical case variants. | A registry test checks the migration. The PostgreSQL receipt test checks newest-match selection and uses `EXPLAIN` with sequential scans disabled to verify index eligibility. |
| P11 | Werka MIME detection borrows the multipart image. Only the buffer transferred to the AI service is copied. Order-image optimization moves the existing HTTP body into the blocking task. | `parsed_upload_borrows_the_original_multipart_image_for_mime_detection` checks the buffer address and MIME inferred from its bytes. The order-image route tests exercise upload, WebP storage, owner-scoped reads and invalid input. |
| P12 | The gateway installs the configured connection deadline and read/write timeouts on every upstream peer. | `configured_peer_uses_operator_connection_and_io_timeouts` verifies all three options using nondefault values. |
| S07 | Browser CORS permission uses an exact origin allowlist. Responses vary on `Origin` so caches keep permissions separate. | The CORS tests cover allowed and unlisted origins, missing and null origins, actual requests, preflights and the required headers. |
| S04 | Six private live routes resolve the bearer header or path-bound ticket to one session before applying role restrictions and upgrade-specific capability checks. They reject raw query bearers and mixed credential sources. | `private_live_streams_enforce_the_same_scope_for_headers_and_path_tickets` exercises the issuance and upgrade paths over loopback. |
| S05 | Chat keeps its one-use ticket and binds it to the issuing session. All seven authenticated streams recheck that session and the stream’s current access requirements before sending data and while idle. A revoked stream sends only its fixed denial notice, where its protocol defines one, and closes. | A supply-task test checks the 401 frame and immediate close. Loopback monitor and chat tests cover logout, revocation of an outstanding ticket, replay, expiry, capability removal, and a second session for the same account remaining usable. |
| S14 | A finite session TTL rejects and removes restored records missing their expiry. At the exact expiry instant the session is invalid. Explicitly unlimited sessions retain their existing semantics. JSON session writers lock a sibling `.lock` file while applying changes to the latest snapshot. | `expiry_policy_tests` exercises cached and persisted records, bearer lookup/update, session handles and scoped tickets on JSON and LMDB, including a stale cache and concurrent JSON writers. |

## Client configuration

Set `MINI_ERP_CORS_ALLOWED_ORIGINS` to comma-separated browser origins, including the scheme and any nondefault port. The default grants no cross-origin browser access. Same-origin and native clients can continue using their existing requests.

For the six private, non-chat WebSocket routes, native clients can send an `Authorization: Bearer …` header during the upgrade. Browser clients should first post `{"path":"<exact live route path>"}` to `/v1/mobile/auth/live-ticket` with their bearer header, then use the returned `ticket` as a query parameter within 30 seconds. Refresh the ticket when reconnecting. Avatar consumers can display the URL returned by the API or fetch using their existing authorization header.

## Verification

All 13 requested findings have regression coverage. The suite used a disposable PostgreSQL 18.4 cluster on a private Unix socket in the Samsung checkout. Database fixtures used a separate access-code key.

The first workspace run exposed two regressions. The Qolip cache test now checks the native response without an Origin header, and the allowlisted browser test verifies ETag and conditional requests. These test-only adjustments are included with final verification in the S14 commit. The S05 commit preserves the supply-task protocol's terminal 401 notice and close frame. Both corrected cases passed in the full rerun.

| Check | Result |
| --- | --- |
| Workspace tests | `cargo test --workspace` was run. The final unfiltered rerun used `--no-fail-fast` to execute later targets after library failures. Across 72 harness results it reported **2,050 passed, 12 failed, 64 ignored**. All audit-fix regressions passed. |
| Integration reruns | Two of the 12 failures passed with the same compiled binaries and disposable fixtures. The voice-media test passed with the SSD FFmpeg and FFprobe on PATH. The order-reset test passed with the SSD PostgreSQL tools on PATH and an isolated backup-runtime directory. It completed the dump and database snapshot comparison. |
| Opt-in coverage | The normally ignored PostgreSQL credential-cutover test passed when run explicitly against a separate disposable auth database. The remaining opt-in tests kept their normal skip conditions. |
| Clippy | `cargo clippy --workspace --all-targets` exited 101 on two existing `clippy::unused_io_amount` errors in the Wi-Fi probe test fixtures at `src/core/system_doctor/wifi/probe.rs:95` and `:145`. Both lines match the starting revision. The same all-target check passed with only this lint allowed. |

## Ten remaining workspace test failures

| Existing check | Observed result |
| --- | --- |
| `rust_source_files_do_not_exceed_500_lines` | The identical 89 source paths exceed 500 lines at `f093beb7` and on this branch. |
| `admin_approves_zero_output_completion_request_and_closes_order_with_issue_history` | Returns `partially_completed` where the test expects `completed_with_issue`. |
| `laminatsiya_queue_exposes_start_for_waiting_print_wip_behind_unready_queue_head` | Returns `not_required` where the test expects `scan_required`. |
| `rezka_astatka_http_captures_session_and_replays_request` | Returns HTTP 400 where the test expects 200. |
| `material_taminotchi_rps_batch_can_link_active_order` | Returns HTTP 500 with `catalog_read_failed` where the test expects 200. |
| `rezka_output_report_reads_one_legacy_input_for_missing_contained_kadr_count` and `rezka_output_report_reads_only_the_active_card_and_preserves_saved_slots` | Both return HTTP 409 where the tests expect 200. |
| `admin_system_user_list_preserves_blocked_state_with_one_snapshot` and `admin_worker_user_list_uses_one_state_snapshot_and_batch_profiles` | Both count four reads where the tests expect one. |
| `service_generates_batch_code_once_and_preserves_it_through_stop` | The RPS integration fixture omits `length_m`. The existing service rejects it with `length_m_required` before generating a batch code. The fixture and the RPS service are identical to `f093beb7`. |

For the eight library runtime assertions above, a saved binary built immediately after S01 reproduced the same failures. S01's supplier-directory changes do not participate in those paths. The size check was compared directly against the starting Git tree.

All requested fixes are included. The unrelated failing assertions and the two existing Clippy errors are outside the requested findings.
