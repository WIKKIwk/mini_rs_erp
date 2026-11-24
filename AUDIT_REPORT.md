# Rust ERP performance and security audit

**24 November 2026 · `mini_rs_erp` · revision `15f90dc6da68`**

The supplier directory can return working credentials to a directory reader, and avatar links contain the same bearer token used for the rest of the account. On the public login route, the service reads every account’s credential row before comparing the phone number. This report covers 16 security and 12 performance findings. The dependency scan results appear separately below.

## Top five fixes

1. **Remove login credentials from supplier directory responses.** A read permission exposes a warehouse operator’s login and can reveal an individual supplier’s login. Use directory-specific response types, then rotate credentials that were disclosed. [S01](#s01)
2. **Remove full account tokens from URLs.** Use header authentication or short-lived, resource-scoped tickets for images and browser sockets. Redact query strings from request tracing. [S02](#s02)
3. **Replace predictable migrated supplier codes.** Identify the affected migration cohort, issue independent codes, and revoke its old sessions. [S03](#s03)
4. **Strip Telegram request URLs from errors.** Routine transport failures can write the bot credential into warning logs. Review retained logs and rotate affected tokens. [S09](#s09)
5. **Make login look up only the accounts it needs.** Check the built-in phone before fetching its state and use keyed or batched queries throughout role-specific login. This removes an account-table scan from unauthenticated requests. [P01](#p01)

## Quick map

| Component | Implementation |
|---|---|
| Main service | `mini_rs_erp` provides the HTTP API, domain services, persistence adapters and maintenance binaries. Major domains include users and sessions, production maps, orders, inventory, warehouses, chat and media. `src/lib.rs:1` and `src/main.rs:18`. |
| Workspace crates | `mini_rs_apparatus_contract` handles equipment identity and AASX. `mini_rs_domain_types` provides shared quantity types. `mini_rs_gscale_domain` handles scales, labels and receipts. `mini_rs_gateway` is the standalone Pingora reverse proxy. `Cargo.toml:10`. |
| HTTP and runtime | Axum 0.8 routers and Tower middleware run on Tokio’s multithread runtime, served by a custom Hyper HTTP/1 listener. The application upgrades WebSockets through a pinned local `sockudo-ws`. `Cargo.toml:33`, `src/main.rs:131` and `src/http/ws.rs:154`. |
| Storage | SQLx 0.8 and PostgreSQL hold transactional ERP data and login verifiers. Startup requires a migrated PostgreSQL instance. Local adapters include heed/LMDB for sessions and profiles, JSON stores, and rusqlite/SQLite calculation fallbacks. Private media can use local storage or R2. `src/main.rs:32`, `src/app_local_store.rs:41` and `src/app/builders.rs:243`. |
| Identity | Normalized phone plus access code, verified against salted Argon2id with a four-job blocking-work limit. ERP sessions are opaque URL-safe tokens from 24 random bytes. New production records default to 30 days and a session update renews expiry. Capabilities and account state are checked server-side. RS256 JWTs with one-hour expiry are outbound Google/FCM service assertions. `src/core/auth/password.rs:5`, `src/core/session/manager.rs:167`, `src/fcm/auth.rs:86` and `src/google_sheets/auth.rs:82`. |
| Other targets | `tools/iroh_erp_agent` is a separate Cargo workspace for the optional loopback ERP bridge. Its shipping lockfile was scanned separately. `fuzz/` holds a development harness. The patched WebSocket crate is a path dependency outside the workspace membership. `tools/runtime/up_domain.sh:269` and `Cargo.toml:18`. |

## Findings at a glance

“High” denotes substantial loss of account authority, continuing access to confidential content, or a broadly reachable scaling cost. Conditions such as required roles, optional integrations and historical migrations are stated in each finding. Source locations are relative to the ERP repository. Fix snippets illustrate the changed logic and include proposed types and methods where an interface needs to change.

| ID | Severity | Category | Finding | File:line |
|---|---|---|---|---|
| [S01](#s01) | High | Security | Supplier directory reads disclose login credentials | `src/http/handlers/admin/suppliers.rs:85` |
| [S02](#s02) | High | Security | Avatar URLs carry the account’s full bearer token | `src/http/handlers/auth.rs:230` |
| [S03](#s03) | High | Security | Credential migration creates predictable supplier logins | `src/db/postgres_auth.rs:87` |
| [S04](#s04) | Medium | Security | Warehouse live requests skip the sales role’s route restriction | `src/http/router/mobile/sales.rs:18` |
| [S05](#s05) | High | Security | Open sockets survive revocation of their issuing session | `src/http/handlers/admin/system/monitor.rs:42` |
| [S06](#s06) | Medium | Security | A tracked backup contains plaintext login data | `demo backup/db/data/mobile_admin_store.json:26` |
| [S07](#s07) | Low | Security | All web origins receive CORS permission | `src/http/router/cors.rs:20` |
| [S08](#s08) | Medium | Security | Telegram secret files receive private permissions after writing | `src/telegram/store.rs:561` |
| [S09](#s09) | High | Security | Telegram transport failures can log the bot token | `src/telegram/bot/parts/part_06.rs:397` |
| [S10](#s10) | Medium | Security | HTTP connections have no effective request-header deadline | `src/main.rs:135` |
| [S11](#s11) | Low | Security | A stored push token can panic the delivery task | `src/fcm/token_cleanup.rs:16` |
| [S12](#s12) | Low | Security | Receipt-print failures expose database diagnostics | `src/http/handlers/rps_batch.rs:458` |
| [S13](#s13) | Medium | Security | Image validation occurs after expensive decoding | `src/http/handlers/calculate_image.rs:56` |
| [S14](#s14) | Medium | Security | Restored sessions can omit an expiry | `src/core/session/models.rs:45` |
| [S15](#s15) | Medium | Security | A partial gateway TLS configuration silently enables plaintext | `crates/mini_rs_gateway/src/main.rs:110` |
| [S16](#s16) | Low | Security | Two public WebSocket library APIs have unenforced safety requirements | `third_party/sockudo-ws/src/protocol.rs:178` |
| [P01](#p01) | High | Perf | Login loads every account before it checks the phone | `src/core/auth/service/login.rs:178` |
| [P02](#p02) | Medium | Perf | Independent PostgreSQL pools | `src/app/postgres_pool.rs:11` |
| [P03](#p03) | Medium | Perf | Chat directory performs two credential queries per candidate | `src/http/handlers/chat/directory.rs:250` |
| [P04](#p04) | Medium | Perf | Order reconciliation reprocesses unchanged orders | `src/db/postgres_mini_order.rs:376` |
| [P05](#p05) | Medium | Perf | The calculation template list returns the entire visible archive | `src/db/postgres_calculate_order.rs:90` |
| [P06](#p06) | Medium | Perf | Raw-material intake reads assignments across all orders | `src/http/handlers/admin/production_maps/raw_materials.rs:1390` |
| [P07](#p07) | Low | Perf | Receipt confirmation lacks an index for its barcode lookup | `src/db/postgres_gscale_receipt_store.rs:167` |
| [P08](#p08) | Medium | Perf | Revoking one account scans all sessions 14 times | `src/app/session_security.rs:52` |
| [P09](#p09) | Medium | Perf | Abandoned sessions stay in the process cache | `src/core/session/manager.rs:80` |
| [P10](#p10) | Medium | Perf | Mobile requests repeat session queries and clone application state | `src/http/router/mobile/sales.rs:19` |
| [P11](#p11) | Low | Perf | Image handlers copy entire uploads unnecessarily | `src/http/handlers/werka/ai_search.rs:176` |
| [P12](#p12) | Medium | Perf | The gateway ignores configured upstream timeouts | `crates/mini_rs_gateway/src/main.rs:43` |

## Security findings

<a id="s01"></a>

### S01. Supplier directory reads disclose login credentials

`src/http/handlers/admin/suppliers.rs:71` permits supplier-directory readers to fetch a page that includes all admin settings at lines 85–100. `src/core/admin/service/mod.rs:215` includes the warehouse operator’s phone and line 218 includes the current login code. The PostgreSQL vault decrypts that code at `src/db/postgres_auth.rs:315`. The standalone settings endpoint requires `AdminSettingsRead` at `src/http/handlers/admin/suppliers.rs:23`. The default Brigader role has supplier-directory read and its route policy admits the supplier page (`src/core/authz_catalog.rs:220` and `src/http/router/mobile/brigader.rs:99`). A permitted directory reader can therefore obtain the warehouse login when the vault contains it. Supplier detail also returns the requested supplier’s phone and current code under the same read permission at `src/http/handlers/admin/suppliers.rs:228` and `src/core/admin/service/read/suppliers.rs:73`. The supplier-list field itself is blanked at `src/core/admin/service/mod.rs:432`. The disclosure comes from page settings and individual supplier detail.

```rust
// Selected fields from the settings object serialized on this page.
werka_phone: config.werka_phone.clone(),
werka_code: self.access_code("werka").await?,
```

**Fix.** Give directory pages a dedicated response type and fetch only its fields. Exclude login codes from directory detail types. If operators need credential recovery, expose it through a separately authorized and audited operation. Rotate codes that were disclosed.

```rust
#[derive(serde::Serialize)]
struct SupplierPageSettings {
    default_target_warehouse: String,
    default_uom: String,
}
```

<a id="s02"></a>

### S02. Avatar URLs carry the account’s full bearer token

Login and profile responses use `with_avatar_proxy` at `src/http/handlers/auth.rs:124` and `src/http/handlers/auth.rs:195`. When an avatar exists, lines 230–235 embed the same session credential in its URL. The directory variant does this at line 259. `src/http/handlers/profile.rs:206` accepts that query parameter and authenticates it against the ordinary session store at line 219. Someone given the URL can use the credential with the account’s permissions until revocation or expiry. The default TTL is 30 days at `src/config.rs:50`. Mobile routing also installs full-URI request tracing at `src/http/router/mobile.rs:17`, which can record the query when the request span’s debug level is enabled.

```rust
"{}://{}/v1/mobile/profile/avatar/view?token={}&v={}"
```

**Fix.** Return a relative URL without credentials and send the Authorization header on authenticated image fetches. For browser image elements that require a URL, issue a short-lived ticket scoped to the authorized avatar and viewer, following the media-ticket pattern at `src/http/handlers/chat/media.rs:47`. Record only the URI path in request spans and use scoped tickets for live routes that currently accept general query tokens.

```rust
principal.avatar_url = format!("/v1/mobile/profile/avatar/view?v={}",
    urlencoding::encode(avatar_version(&principal.avatar_url)));
// In avatar_view
let Some(token) = bearer_token(&headers) else { return unauthorized().into_response() };
```

<a id="s03"></a>

### S03. Credential migration creates predictable supplier logins

The migration at `src/db/postgres_auth.rs:84` fills missing supplier codes from directory data. `src/core/auth/access_codes.rs:19` derives them deterministically without a secret. The migration persists the result as the real credential at `src/db/postgres_auth.rs:124` and supplier login checks it at `src/core/auth/service/supplier_login.rs:57`. For accounts migrated through this branch, directory knowledge can enable impersonation until the code changes. Argon2 protects the stored verifier but cannot add unpredictability to its input.

```rust
if state.custom_code.trim().is_empty() {
    state.custom_code = supplier_access_code(&SupplierAccessInput { /* directory fields */ })?;
}
```

**Fix.** Share the random numeric generator already used for new suppliers at `src/core/admin/service/mutations/suppliers.rs:17` with the migration. Keep the configured prefix and distribute each code through the authorized provisioning channel. Identify and rotate previously derived codes and revoke the affected sessions.

```rust
if state.custom_code.trim().is_empty() {
    state.custom_code = random_code(supplier_prefix, &mut issued_codes);
}
```

<a id="s04"></a>

### S04. Warehouse live requests skip the sales role’s route restriction

`src/http/router/mobile/sales.rs:18` applies the sales allowlist to header credentials only. Warehouse live falls outside the GET allowlist at line 48. The live handler at `src/http/handlers/admin/warehouse_live.rs:16` accepts a query credential first at line 28 and requires `CatalogItemRead`. Sales holds that capability by default at `src/core/authz_catalog.rs:185`. The stream sends warehouse identifiers and reasons for changes from `src/core/warehouse_events.rs:59`. The same sales account therefore receives different access depending on the credential transport.

```rust
// Sales middleware
if let Some(token) = bearer_token(request.headers()) { /* enforce allowlist */ }
// Warehouse live authenticates independently
let principal = authenticated_principal_for_live(&state, &headers, &query.token).await?;
```

**Fix.** Resolve one authenticated identity before role checks. Reject conflicting credentials and use the same resolved identity in the upgrade handler. If browser sockets use tickets, resolve the ticket to its issuing session there as well.

```rust
// Shared credential resolver, before dispatch
let token = match (header_token, query_token) {
    (Some(h), Some(q)) if h != q => return Err(unauthorized()),
    (Some(h), _) => h,
    (None, Some(q)) => q,
    (None, None) => return Err(unauthorized()),
};
let actor = state.sessions.get(&token).await.map_err(|_| unauthorized())?;
// Apply the route allowlist to actor and pass actor to the handler.
```

<a id="s05"></a>

### S05. Open sockets survive revocation of their issuing session

`src/http/handlers/admin/system/monitor.rs:37` checks the session and admin capability at upgrade, then passes only the application state and socket at line 42. Lines 140–173 keep sending server and database snapshots after logout, account deactivation, expiry, or loss of admin access. Chat’s socket-ticket route drops the session identity at `src/http/handlers/chat/realtime.rs:67`. Chat does recheck account activity before events and on heartbeat at lines 132 and 151, but logging out a single session leaves its account active. That open socket continues receiving chat events, which include messages at `src/core/chat/models.rs:230`.

```rust
.on_upgrade(move |socket| system_monitor_live_socket(state, socket))
```

**Fix.** Bind every socket to the issuing session, including when redeeming a chat ticket. Check current session validity and the stream-specific privilege before initial delivery, subsequent sends, and on heartbeat. A server-side session identifier or revocation subscription can avoid retaining the raw bearer.

```rust
// Inside the monitor loop, before sending each snapshot
let Ok(actor) = state.sessions.get(&token).await else { break };
if !state.admin.principal_has_capability(&actor, Capability::AdminAccess).await {
    break;
}
```

<a id="s06"></a>

### S06. A tracked backup contains plaintext login data

Git tracks `demo backup/db/data/mobile_admin_store.json` and a populated worker table in `demo backup/db/postgres/mini_rs_erp_20260706_212155.sql:4975`. Line 11 contains a directory phone and line 26 contains a nonempty numeric `custom_code` for the corresponding identity. The same tracked backup includes session LMDB files and PostgreSQL exports with order and worker records. The code’s current validity and whether the records are synthetic are unverified. Anyone with repository access can read the stored values, and restoring the snapshot could import them as credentials through `src/db/postgres_auth.rs:124`.

```json
{"custom_code": "[redacted]"}
```

**Fix.** Remove deployed data from distributed snapshots, rotate any reused codes, and coordinate cleanup of repository history and existing copies. Build demo fixtures from synthetic records and provision fresh local credentials separately. Once the dump is untracked, keep that directory out of future commits.

```gitignore
/demo backup/db/
```

<a id="s07"></a>

### S07. All web origins receive CORS permission

`src/http/router.rs:30` applies custom CORS handling to the entire API. `src/http/router/cors.rs:5` answers preflights unconditionally. Lines 19–31 allow every origin, the Authorization header, and mutation methods. Arbitrary sites that can reach the deployment can read public responses and use credentials provided to their own origin. The application uses explicit bearer credentials and does not advertise credentialed CORS, so the wildcard alone does not grant access to another user’s session.

```rust
headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
```

**Fix.** Configure the deployed web-client origins. Echo only a matched origin on both actual and preflight responses, preserve `Vary: Origin`, and omit CORS permission for other origins.

```rust
headers.append(header::VARY, HeaderValue::from_static("Origin"));
if let Some(origin) = request_origin.filter(|o| allowed_origins.contains(*o)) {
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
}
```

<a id="s08"></a>

### S08. Telegram secret files receive private permissions after writing

`src/telegram/store.rs:559` serializes state containing the plaintext bot token and writes it to a fixed temporary file. Only after writing does line 564 request owner-only mode. The MTProto session encryption key follows the same sequence at lines 614–624. Permission failures merely log a warning at line 686. On Unix, a permissive umask and a traversable data directory can expose the new files to another local user during creation or longer if chmod fails.

```rust
tokio::fs::write(&tmp_path, raw).await.map_err(|_| TelegramStoreError::Write)?;
restrict_file_permissions(&tmp_path).await;
```

**Fix.** Reuse the existing private atomic writer for replaceable bot state (`src/store/private_file.rs:5`). For the encryption key, preserve creation without overwrite, using owner-only creation and atomic publication of the complete key so concurrent openers see the same key. The access-code vault already demonstrates that publication pattern at `src/core/auth/code_vault.rs:80`.

```rust
crate::store::private_file::write_private(&self.path, raw)
    .await.map_err(|_| TelegramStoreError::Write)?;
```

<a id="s09"></a>

### S09. Telegram transport failures can log the bot token

The configured polling worker starts at `src/main.rs:80`. It constructs a URL containing the token (`src/telegram/bot/parts/part_07.rs:203`), turns a request error into a string (`src/telegram/bot/parts/part_06.rs:397`), and logs that string at warning level (`src/telegram/bot/parts/part_01.rs:40`). In the locked Reqwest 0.12.28 source, a timeout or send failure can carry its request URL, and `Error::fmt` includes that URL. A log reader could acquire the bot’s API authority. This finding concerns a verified logging path. No production log or bot credential was examined.

```rust
.map_err(|error| TelegramError::Transport(error.to_string()))?;
// Later in the polling loop
tracing::warn!(?error, "telegram polling failed");
```

**Fix.** Strip URLs before formatting errors for all Bot API and file requests. Record the operation and error class. Review access to retained logs and rotate a token that appeared in them.

```rust
fn transport_error(error: reqwest::Error) -> TelegramError {
    TelegramError::Transport(error.without_url().to_string())
}
// At the request boundary
let response = request.send().await.map_err(transport_error)?;
```

<a id="s10"></a>

### S10. HTTP connections have no effective request-header deadline

Every accepted socket receives a spawned task in `src/main.rs:118` and `src/main.rs:131`. The Hyper HTTP/1 builder at line 135 sets only `keep_alive(true)`. With the locked Hyper 1.9.0, the default header deadline is disabled when the builder has no timer. A connection awaiting its first complete header can therefore retain a socket and task until the peer or an upstream component closes it. The default application bind resolves to all IPv4 interfaces (`src/config.rs:34` and line 377). External reachability depends on deployment. [Hyper HTTP/1 builder documentation](https://docs.rs/hyper/1.9.0/hyper/server/conn/http1/struct.Builder.html#method.header_read_timeout)

**Fix.** Install a Tokio timer and an explicit header deadline on every listener. Budget concurrently accepted connections across listeners if the ERP is directly reachable. The header timer still allows long-lived upgraded connections.

```rust
let mut builder = http1::Builder::new();
builder.timer(hyper_util::rt::TokioTimer::new())
    .header_read_timeout(Duration::from_secs(15))
    .keep_alive(true);
builder.serve_connection(io, service).with_upgrades().await?;
```

<a id="s11"></a>

### S11. A stored push token can panic the delivery task

Device registration authenticates the caller at `src/http/handlers/chat/devices.rs:25`. The shared service accepts any nonblank string at `src/core/push/service.rs:39`. On FCM delivery failure, the warning at `src/fcm.rs:148` calls the formatter below before stale-token cleanup. Its byte offsets can fall inside a UTF-8 character. With warning logging enabled and FCM configured, formatting an existing invalid token can abort that delivery task and skip the remaining devices for the recipient.

```rust
format!("{}...{}", &trimmed[..6], &trimmed[trimmed.len() - 6..])
```

**Fix.** Reject malformed inputs at registration. Make logging safe for already persisted tokens as well. A digest also avoids writing the entire value for short tokens (`src/fcm/token_cleanup.rs:13`).

```rust
fn token_log_id(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(token.trim().as_bytes());
    format!("{digest:x}")[..12].to_owned() // hexadecimal ASCII
}
```

<a id="s12"></a>

### S12. Receipt-print failures expose database diagnostics

A PostgreSQL lookup turns a SQLx error into `GscalePortError::StoreWrite(error.to_string())` at `src/db/postgres_gscale_receipt_store.rs:175`. The service preserves it at `crates/mini_rs_gscale_domain/src/service/recording.rs:42`. The authenticated print-confirmation path then copies the message into batch failure state (`src/http/handlers/rps_batch.rs:303`) and the HTTP body (`detail: error.to_string()` at line 458). Depending on the database failure, an operator can see internal schema, constraint or driver information later as well as in the immediate response.

**Fix.** Keep diagnostic details in restricted server logs. Use the same stable public detail in the batch record and HTTP error. Preserve validated business messages and the existing error code/status contract.

```rust
let detail = match &error {
    GscaleServiceError::StoreWrite(_) => "Receipt storage unavailable".to_owned(),
    GscaleServiceError::PrintFailed { .. } => "Printing failed".to_owned(),
    GscaleServiceError::SubmitFailed(_) => "Receipt submission failed".to_owned(),
    GscaleServiceError::InvalidInput(reason) if reason == "client_print_epc_already_used" =>
        "Print code already used".to_owned(),
    _ => error.code().to_owned(),
};
// Both the saved failure and RpsBatchErrorResponse receive this public detail.
```

<a id="s13"></a>

### S13. Image validation occurs after expensive decoding

The protected order-image endpoint dispatches each upload to a blocking task (`src/http/handlers/calculate.rs:169` and line 199). Its helper calls `image::load_from_memory(body)` before checking `ORDER_IMAGE_MAX_PIXELS` at `src/http/handlers/calculate_image.rs:56`. Avatar uploads check compressed bytes at `src/http/handlers/profile.rs:99`, then decode, resize and re-encode synchronously on a Tokio worker (`src/core/profile/service.rs:133`, `src/core/profile/avatar_image.rs:20`). The avatar’s 1,000-pixel longest-side cap applies to output. A small compressed upload can require a much larger working set before either path resizes or rejects it. Concurrent uploads compound the cost.

**Fix.** Check dimensions from the image header before full decode, keep the decoder’s own allocation limit, and share a bounded worker budget across uploads. Set budgets using supported image sizes and available service memory. Run all avatar transforms on the same bounded blocking path.

```rust
let reader = || image::ImageReader::new(std::io::Cursor::new(body)).with_guessed_format();
let (w, h) = reader()?.into_dimensions()?;
if w == 0 || h == 0 || u64::from(w) * u64::from(h) > pixel_limit {
    return Err(ImageUploadError::TooLarge);
}
let mut decoder = reader()?;
let mut limits = image::Limits::default();
limits.max_alloc = Some(decode_budget_bytes);
decoder.limits(limits);
let image = decoder.decode()?;
```

Acquire the permit before queuing the blocking task and move it into the closure. This also bounds work after client cancellation.

```rust
let permit = decode_slots.clone().try_acquire_owned().map_err(|_| busy())?;
let result = tokio::task::spawn_blocking(move || {
    let _permit = permit;
    decode_and_resize(body)
}).await?;
```

<a id="s14"></a>

### S14. Restored sessions can omit an expiry

The compatibility decoder accepts JSON records (`src/core/session/store/lmdb_codec.rs:34`), and missing `expires_at` deserializes as `None` (`src/core/session/models.rs:21`). `is_expired` at line 45 uses `self.expires_at.is_some_and(|expires_at| now > expires_at)`. `SessionManager::get` accepts such a record for an active principal (`src/core/session/manager.rs:100`). The legacy LMDB fixture at `src/core/session/store_tests.rs:52` exercises a record with no expiry. If such a record is restored or retained, its bearer can remain valid across the configured TTL until an explicit revocation or an update assigns an expiry. The number of deployed legacy records is unknown.

**Fix.** In a service configured with a finite TTL, treat missing expiry as expired at both lookup and update, remove the stored record and require reauthentication. Centralize this predicate for cache and persistent records.

```rust
fn expired(record: &SessionRecord, now: OffsetDateTime, ttl: Option<u64>) -> bool {
    match record.expires_at {
        Some(end) => now >= end,
        None => ttl.is_some(),
    }
}
```

<a id="s15"></a>

### S15. A partial gateway TLS configuration silently enables plaintext

The gateway reads certificate and key paths independently (`crates/mini_rs_gateway/src/config.rs:63`). In `crates/mini_rs_gateway/src/main.rs:101` only the pair enables TLS, and the wildcard branch uses `proxy.add_tcp(&bind_addr)`. Supplying just one path starts a plaintext listener without reporting the incomplete TLS configuration. Line 60 also sets the forwarded protocol to HTTPS on the plaintext branch. This affects deployments relying on this gateway for TLS termination.

**Fix.** Validate both paths together during configuration. Keep intentionally plaintext listeners explicit and derive the forwarded scheme from the actual ingress or a trusted TLS terminator.

```rust
match (cert.as_deref(), key.as_deref()) {
    (Some(cert), Some(key)) => add_tls_listener(cert, key)?,
    (None, None) => add_plain_listener(),
    _ => return Err("configure both gateway TLS certificate and key".into()),
}
```

<a id="s16"></a>

### S16. Two public WebSocket library APIs have unenforced safety requirements

The vendored crate exposes `Message::Text(Bytes)` at `third_party/sockudo-ws/src/protocol.rs:37`. Its safe `as_text` and `into_text` accessors call unchecked UTF-8 conversions at lines 178 and 203. A safe caller can construct the public variant with arbitrary bytes. Separately, `SpscQueue` implements `Sync` at `third_party/sockudo-ws/src/queue.rs:84` and exposes safe `try_push(&self, ...)` and `try_pop(&self)` at lines 114 and 146. Slot access assumes exactly one producer and one consumer. Shared references cannot enforce that assumption. These are soundness defects at the vendored library API. The ERP wrapper validates incoming text (`src/http/ws.rs:101`), and no first-party ERP caller of those accessors or that queue was found.

**Fix.** Validate in both text accessors, or use a validated payload type. Keep queue storage private and require unique, non-cloneable producer and consumer handles with mutable operations, or replace it with a reviewed bounded queue. Restrict the primitive’s public API until its concurrency contract is enforced.

```rust
// Within as_text
Message::Text(bytes) => std::str::from_utf8(bytes).ok(),
// Within into_text
Message::Text(bytes) => String::from_utf8(bytes.to_vec()).ok(),
```

## Performance findings

<a id="p01"></a>

### P01. Login loads every account before it checks the phone

After the per-phone throttle admits an attempt, `src/core/auth/service/login.rs:48` tries the built-in admin identity first. When that identity exists, it calls `lookup.list_states().await` at line 178 before checking the phone at line 191. Production requires that identity (`src/db/postgres_auth.rs:36`). Its adapter runs the following unfiltered query at `src/db/postgres_auth.rs:274` and builds a second map at line 370. Consequently, even a nonmatching phone can transfer every account’s verifier and deserialize its access state. Supplier login does another full lookup at `src/core/auth/service/supplier_login.rs:41`. Per-attempt DB and allocation costs grow with the account population before authentication succeeds.

```sql
SELECT principal_ref, credential_hash, access_state FROM mini_auth_accounts
```

**Fix.** Compare the normalized built-in phone first, then use a keyed lookup on the auth port, backed by the existing SQL shape at `src/db/postgres_auth.rs:249`. Fetch states for candidate IDs only in directory-based logins. Preserve checks for missing, blocked and removed accounts, the existing throttle and password verification.

```rust
if !phone_matches_normalized(&identity.phone, phone) {
    return Ok(None);
}
let state = lookup.account_state(ref_).await.map_err(|_| AuthError::Internal)?
    .ok_or(AuthError::Internal)?;
if state.blocked || state.removed || !code_matches(&state.custom_code, code).await? {
    return Ok(None);
}
```

<a id="p02"></a>

### P02. Independent PostgreSQL pools

The server passes its runtime pool to `AppState::from_postgres` at `src/main.rs:63`. Yet the worker, system-user and customer builders at `src/app.rs:287` each reach `config.pool_options().connect_lazy(&config.database_url)` at `src/app/postgres_pool.rs:11`. The chat, inventory and calculation builders use the same helper. The admin catalog opens another pool at `src/app/admin_catalog_overlay.rs:26`. Each gets its own minimum of 2, maximum of 16 and 500 ms acquisition timeout by default (`src/db/postgres_parts/part_01.rs:1`). A busy component can time out while another holds idle capacity, and the application-wide maximum grows with the number of pools.

Pass clones of the runtime pool through production builders. Budget dedicated PostgreSQL LISTEN connections separately. Size the shared cap against PostgreSQL’s available connections and the number of ERP instances.

```rust
let workers = WorkerService::new(Arc::new(PostgresWorkerStore::new(pool.clone())));
let users = SystemUserService::new(Arc::new(PostgresSystemUserStore::new(pool.clone())));
let customers = Arc::new(PostgresCustomerStore::new(pool.clone()));
```

<a id="p03"></a>

### P03. Chat directory performs two credential queries per candidate

The directory asks for up to 500 workers and 500 users per selected system role before paging. Its worker loop calls `state.admin.worker_detail(worker).await` at `src/http/handlers/chat/directory.rs:250`, with the same pattern for system users at line 277. Each nonremoved candidate triggers an account-state and a vault query. When an encrypted code is present it is decrypted. In file-key mode this also calls synchronous filesystem metadata and reads on the async path (`src/core/auth/code_vault.rs:108`). The directory only consumes public fields (`src/core/admin/service/read/users_parts/part_01.rs:328`, `src/db/postgres_auth.rs:315`). A one-entry page still incurs this work. Avatar URLs are also built before the final slice at `src/http/handlers/chat/directory.rs:58`.

Use a directory-specific read. Batch account flags for candidate IDs, apply current visibility and exclusion rules, then sort, page and resolve avatars for returned entries. Preserve default state for missing accounts.

```sql
SELECT principal_ref, access_state
FROM mini_auth_accounts
WHERE principal_ref = ANY($1::text[])
```

<a id="p04"></a>

### P04. Order reconciliation reprocesses unchanged orders

The PostgreSQL sink enables a background loop with a default 30-second delay (`src/app.rs:586`, `src/app/order_sheets.rs:116`). Each pass loads all maps and templates. It compiles each valid map, then discards the program (`src/core/production_map/catalog/service.rs:24`, `src/app/order_sheets.rs:95`). For each eligible map, `templates.iter().find(|template| {` at `src/db/postgres_mini_order.rs:376` scans until the first match. Each match starts a transaction and locks the order at line 466, even when the guarded upserts change zero rows. This adds recurring database work and competes with edits.

Enqueue affected order IDs in the same transaction as map or template changes, including changes to old and new matching keys. Upsert queue rows on each change so a concurrent producer waits for an in-flight claim and requeues work if needed. Process bounded batches, reconciling and acknowledging each claim atomically. Seed existing orders once and retain a bounded repair pass.

```sql
SELECT order_id FROM mini_order_reconcile_queue
ORDER BY order_id LIMIT 100 FOR UPDATE SKIP LOCKED;
-- After reconciling a claimed ID in this transaction
DELETE FROM mini_order_reconcile_queue WHERE order_id = $1;
```

<a id="p05"></a>

### P05. The calculation template list returns the entire visible archive

GET `/v1/mobile/calculate/orders` calls `.list(&key)` at `src/http/handlers/calculate.rs:82`. The PostgreSQL query ends with `ORDER BY saved_at DESC` and `.fetch_all(&self.pool)` at `src/db/postgres_calculate_order.rs:90`. Every matching template is transferred, decoded and serialized, including shared templates visible to this principal. The response grows with the archive. POST also reads the archive at `src/http/handlers/calculate.rs:109` for an existence check.

Introduce a paged response and migrate list callers. Return cursor columns alongside the existing legacy product-name projection. Parenthesize the existing owner visibility predicate, then append this SQL suffix. Bind a capped page size plus one, returning the last emitted timestamp and ID as the next cursor when another row exists. Use a scoped `EXISTS` for the POST check, with the same conflict conditions and exclusions.

```sql
AND ($2::timestamptz IS NULL OR (saved_at, id) < ($2::timestamptz, $3::text))
ORDER BY saved_at DESC, id DESC
LIMIT $4
```

<a id="p06"></a>

### P06. Raw-material intake reads assignments across all orders

The intake-candidate handler loads `.raw_material_assignments()`, filters by order and apparatus in Rust, then awaits `.raw_material_stock_by_barcode(&assignment.barcode)` for each candidate (`src/http/handlers/admin/production_maps/raw_materials.rs:1390`, line 1402). The first query fetches and decodes the full canonical assignment set (`src/db/postgres_production_map/materials/rules.rs:154`). Each remaining barcode adds a separate SQL round trip (`src/db/postgres_gscale_receipt_store.rs:188`). A single order therefore pays for unrelated orders plus sequential inventory reads.

Reuse the existing order-scoped service method. Batch stock retrieval with the current stock-column projection and keep the latest row per folded barcode (`DISTINCT ON (lower(barcode)) ... ORDER BY lower(barcode), updated_at DESC`), using `WHERE lower(barcode) = ANY($1::text[]) AND status <> 'deleted'`, binding normalized candidate barcodes. Preserve apparatus, availability, reservation, rule and execution checks before returning candidates.

```rust
let assignments = state.production_maps.raw_material_assignments_for_order(order_id)
    .await.map_err(production_map_error)?;
let eligible = assignments.into_iter()
    .filter(|a| apparatus_id_matches_text(&a.apparatus_id, apparatus));
```

<a id="p07"></a>

### P07. Receipt confirmation lacks an index for its barcode lookup

RPS client print confirmation calls the receipt store before recording a confirmed print (`src/http/handlers/rps_batch.rs:260`, `crates/mini_rs_gscale_domain/src/service/recording.rs:41`). The query uses `WHERE lower(barcode) = lower($1)` and `ORDER BY updated_at DESC LIMIT 1` at `src/db/postgres_gscale_receipt_store.rs:167`. The shipped receipt schema indexes the original barcode, status, item and preparation owner. None indexes this case-folded receipt barcode (`migrations/postgres/0001_mini_erp_foundation.sql:655` and line 1186, `migrations/postgres/0177_preparation_production_journal_index.sql:6`). The database lacks a matching indexed lookup as receipt history grows.

Add an expression index that supports both the comparison and latest-row selection. A nonunique index preserves existing behavior for historical values differing in case.

```sql
CREATE INDEX idx_gscale_receipts_folded_barcode_updated
ON mini_gscale_receipts (lower(barcode), updated_at DESC)
```

<a id="p08"></a>

### P08. Revoking one account scans all sessions 14 times

Blocking, removal, or credential replacement triggers revocation at `src/core/admin/service/session_security.rs:13–28`. The role loop at `src/app/session_security.rs:36–53` calls `self.delete_for_principal(&role, principal_ref)` 14 times. Each pass holds the global session write guard across the storage await, scans the cache, and performs a synchronous LMDB table scan and commit on a Tokio worker (`src/core/session/manager.rs:127–144`, `src/core/session/store/lmdb.rs:114–137`). Unrelated session requests wait behind each pass.

Add an account-wide store operation. The LMDB implementation should scan once on a blocking worker, removing matching records across roles and their expiry entries in one transaction, including legacy keys. Let a service-owned task finish the revocation even if the requesting client disconnects.

```rust
let _guard = sessions.operations.write().await;
sessions.store.delete_all_roles_for_principal(&id).await?;
sessions.cache.write().await.retain(|_, r| r.principal.ref_.trim() != id);
```

<a id="p09"></a>

### P09. Abandoned sessions stay in the process cache

Every login executes `self.cache.write().await.insert(token.clone(), record)` at `src/core/session/manager.rs:80`. Store hits and updates also insert entries at lines 107 and 169. The cache checks expiry on lookup of that token. The LMDB expiry sweep removes database records only (`src/core/session/store/lmdb.rs:198–221`). Repeated successful logins followed by abandonment accumulate tokens and profiles in memory until that token is looked up after expiry, explicitly removed, or the process exits.

Bound the positive cache at every insertion site. One dependency-free option keeps updates to existing entries and skips caching additional keys at capacity. Sweep expired entries periodically so new sessions can fill the freed slots. A cache miss still consults the store and checks expiry.

```rust
let mut cache = self.cache.write().await;
if cache.contains_key(token) || cache.len() < capacity {
    cache.insert(token.to_owned(), record);
}
```

<a id="p10"></a>

### P10. Mobile requests repeat session queries and clone application state

Both middleware layers resolve the bearer session (`src/http/router/mobile/brigader.rs:26–30`, `src/http/router/mobile/sales.rs:18–20`). Chat authorization then calls `state.sessions.get(&token).await` again (`src/http/handlers/chat/auth.rs:18`). Even a cache hit validates account activity (`src/core/session/manager.rs:85–93`). With PostgreSQL credentials enabled, chat sync therefore issues three serial account queries (`src/db/postgres_auth.rs:249–255`). Each layer also extracts the entire cloneable `AppState`, including services with owned strings (`src/app.rs:69–125`, `src/core/auth/service.rs:29–42`).

Use a single `Arc<AppState>` for handlers and middleware. Authenticate once per distinct credential per request and pass the verified principal to both scope policies and the handler. Keep their capability checks, reject conflicting identities, and retain ongoing session checks for live connections.

```rust
let principal = state.sessions.get(token).await?;
enforce_workspace_scopes(&principal, request.method(), request.uri()).await?;
request.extensions_mut().insert(principal);
```

<a id="p11"></a>

### P11. Image handlers copy entire uploads unnecessarily

The configured Werka image-search endpoint creates a vector, then does `bytes_for_mime: bytes.clone()` at `src/http/handlers/werka/ai_search.rs:174–179`. The MIME detector only checks a prefix of at most 12 bytes at lines 203–230. The order-image endpoint also does `let upload = body.to_vec();` before moving the data into a blocking closure that accepts a byte slice (`src/http/handlers/calculate.rs:198–201`). Each path performs one avoidable allocation and copy proportional to the compressed image size.

Delete the duplicate MIME buffer and borrow the existing vector before moving it. In the order-image closure, move the owned Axum `Bytes` and call the optimizer with `body.as_ref()`.

```rust
let mime_type = detect_image_mime_type(
    upload.filename.as_deref().unwrap_or(""),
    upload.content_type.as_deref().unwrap_or(""),
    &upload.bytes,
);
let image = WerkaAiSearchImage { bytes: upload.bytes, mime_type };
```

<a id="p12"></a>

### P12. The gateway ignores configured upstream timeouts

`crates/mini_rs_gateway/src/config.rs:43` parses a connection timeout (default one second) and an upstream timeout (default 30 seconds). The only production peer constructor at `crates/mini_rs_gateway/src/main.rs:43` immediately returns `Box::new(HttpPeer::new(...))`. It never assigns the settings to the peer. In the locked Pingora 0.8.1 source those peer options default to `None`. Slow or unavailable upstreams can occupy gateway requests beyond the operator’s configured budget.

**Fix.** Apply the configured values when constructing the peer. Read and write timeouts bound individual I/O operations. Define a separate overall request deadline if the deployment needs one, with a distinct policy for streaming requests and WebSockets.

```rust
let mut peer = HttpPeer::new(
    self.config.upstream_addr.as_str(), false, self.config.upstream_host.clone(),
);
peer.options.connection_timeout = Some(self.config.connect_timeout);
peer.options.read_timeout = Some(self.config.upstream_timeout);
peer.options.write_timeout = Some(self.config.upstream_timeout);
Ok(Box::new(peer))
```

## Verification and tooling

The audit followed production startup, router layers, credential extractors, service calls and storage implementations at the stated revision. Performance findings describe work the code performs. No latency, planner cost, deployment exposure or incident rate was inferred from a benchmark. Build artifacts and advisory data were placed outside the checkout. No service was started, database queried or migrated, credentials exercised, or project code changed.

### Clippy

| Command | Result |
|---|---|
| `cargo clippy --locked --workspace --lib --bins` | Exit 0. The workspace’s default-feature library and binary targets emitted 130 warnings. |
| `cargo clippy --locked --workspace --all-targets` | Exit 101. `clippy::unused_io_amount` is denied at `src/core/system_doctor/wifi/probe.rs:95` and line 145. Both are HTTP test fixtures that discard the byte count from `read`. |

Clippy 0.1.98 also identified three temporary strings at `src/http/handlers/admin/production_maps/print_preflight.rs:56`, lines 69 and 85. Each `&apparatus.id.to_string()` can become `apparatus.id.as_str()` using `crates/mini_rs_apparatus_contract/src/identity.rs:22`. The by-value `StageAstatkaReport` at `src/core/production_map/stage_execution.rs:19` occupies at least 696 bytes on this build. The next-largest variant uses at least 272. Boxing the large variant trades stack space for a heap allocation, so these sizes alone do not establish a throughput improvement.

### RustSec dependency scan

Ran official `cargo-audit` 0.23.0 from a temporary directory, verified its archive SHA-256 against the release metadata and scanned the locked versions. The advisory database revision was `52384ab5581a`, dated 23 November 2026. The main scan examined 515 locked packages and returned exit 1. Registry timeouts made its yanked-package check incomplete. A repeat using `--no-fetch --no-yanked` against the same advisory snapshot reproduced all 12 vulnerability matches. These counts are lockfile matches, and a single package version can match multiple advisories. [RustSec releases](https://github.com/rustsec/rustsec/releases) · [Advisory database at the scanned revision](https://github.com/RustSec/advisory-db/tree/52384ab5581a4487b62c549eef95cf7ba7e8bad5)

| Main `Cargo.lock` package | Advisory IDs | First patched release on the indicated line |
|---|---|---|
| `libsqlite3-sys 0.30.1` | RUSTSEC-2026-0459 | 0.39.0. Upgrade the compatible `rusqlite` parent because the project enables bundled SQLite. |
| `pingora-core 0.8.1` / `pingora-proxy 0.8.1` | RUSTSEC-2026-0388 / 0390 | 0.9.0 for both. Update the gateway’s Pingora crates together. |
| `protobuf 2.28.0` | RUSTSEC-2024-0437 | 3.7.2. The resolved path runs through Prometheus and Pingora. |
| `quinn-proto 0.11.18` / `quinn-udp 0.5.14` | RUSTSEC-2026-0523, 0524 / 0538 | 0.11.19 / 0.5.16. Neither appeared in the host workspace’s default normal-dependency tree. |
| `url 2.5.8` | RUSTSEC-2026-0592 | 2.5.9. |
| `zstd 0.13.3` / `zstd-safe 7.2.4` / `zstd-sys 2.0.16+zstd.1.5.7` | RUSTSEC-2026-0391 / 0398, 0403 / 0410 | 0.13.4 / 7.4.0 / 2.1.2. The `zstd-sys` advisory concerns WebAssembly. |

The gateway directly uses Pingora, enables HTTP/2 when terminating TLS, and sets a fixed upstream Host (`crates/mini_rs_gateway/src/main.rs:56` and line 107). Prioritize the coordinated gateway update and regression-test header handling. The SQLite advisory requires opening an affected database/WAL pair. No application-level exploitability was established from the scanner alone. The main scan also returned seven unmaintained and twelve unsound advisory warnings. [Pingora core advisory](https://github.com/RustSec/advisory-db/blob/52384ab5581a4487b62c549eef95cf7ba7e8bad5/crates/pingora-core/RUSTSEC-2026-0388.md) · [Pingora proxy advisory](https://github.com/RustSec/advisory-db/blob/52384ab5581a4487b62c549eef95cf7ba7e8bad5/crates/pingora-proxy/RUSTSEC-2026-0390.md) · [Bundled SQLite advisory](https://github.com/RustSec/advisory-db/blob/52384ab5581a4487b62c549eef95cf7ba7e8bad5/crates/libsqlite3-sys/RUSTSEC-2026-0459.md)

The separately built `tools/iroh_erp_agent/Cargo.lock` returned **36 vulnerability matches across 12 package names** in 400 dependencies, plus two unmaintained and three unsound warnings. Twenty-three matches were in `hickory-net`, `hickory-proto` and `hickory-resolver` 0.26.1. Other affected packages included `rustls` 0.23.41, `rustls-webpki` 0.103.13, `h2` 0.4.15, `url` 2.5.8 and bundled root certificates. Update its lockfile independently and re-run the same advisory check. Suggested minimums from the scan include Hickory 0.26.2, rustls 0.23.46 and rustls-webpki 0.103.16. The deployment script builds this tool with `--locked` at `tools/runtime/up_domain.sh:269`.

### Other requested checks

- **Dynamic SQL and `SELECT *`.** The checkpoint query interpolates identifiers selected by a closed internal enum and binds the supplied key (`src/db/postgres_production_map/cancel_start/checkpoint.rs:41`). The media query uses a fixed projection and a program-selected locking suffix with bound values (`src/db/postgres_chat_media/repository.rs:427`). The chat query’s `SELECT p.*` is inside a one-row lateral subquery with an explicit outer projection (`src/db/postgres_chat/read.rs:48`). No injectable user value or unnecessary whole-row transfer was verified in these paths.
- **Access checks and throttling.** The chat media repository checks active conversation membership, and write paths can require posting rights (`src/db/postgres_chat_media/repository.rs:375`). Login serializes attempts per normalized phone, caps failures at five per 60 seconds per process and bounds tracked phones at 10,000 (`src/core/auth/service/login.rs:9` and `src/core/auth/service/throttle.rs:9`). At capacity, new phones trigger a map sweep while the shared mutex is held. For multi-instance deployments, any aggregate rate limit must operate across processes. Edge-level limits were outside this source review.
- **Secrets and error responses.** Git ignores the local `.env`. The tracked root file is `.env.example`, and the repository history reviewed showed no commit of the root `.env`. The private RSA key under `tests/fixtures/jwt` is a test fixture. S01, S02, S06, S08 and S09 describe separate credential paths. The common `AppError` boundary emits a fixed error body (`src/error.rs:44`). S12 identifies a handler that bypasses that boundary.
- **Unsafe blocks.** First-party runtime sites at `src/core/session/store/lmdb.rs:29`, `src/store/profile_store.rs:173`, `src/store/push_token_store/lmdb.rs:37` and `src/core/rps_batch/store.rs:317` open memory-mapped LMDB environments in application-owned directories. The backup helper at `src/bin/mini_rs_backup_state.rs:21` opens the source read-only for an LMDB snapshot. These calls rely on stable mapped files, exclusive control of directory contents and LMDB’s locking discipline. S16 covers two separate contracts in the vendored transport. This review did not establish memory safety across all third-party code.
- **Release optimization.** The root manifest has no `[profile.release]` override. The shipped build script selects `--release --locked` at `tools/runtime/up_domain.sh:226`. Cargo’s release defaults are `opt-level = 3`, `codegen-units = 16`, and `lto = false`, which permits local thin LTO within a crate. The audit did not measure a benefit from cross-crate LTO or fewer codegen units. [Cargo release profiles](https://doc.rust-lang.org/cargo/reference/profiles.html#release)
