# Read-only MCP local prototype

Base: main 4c9f8b99f285149c875de0830e58e37f723d53fa. Local changes only.
No route, public listener, deployment, real token/grant, mobile changes or database connection was created.
The default Cargo feature set does not include this module. Even when compiled, Server starts disabled.
DenyAll is the only supplied production resolver and rejects every credential.
The synthetic fixture uses no AppState and cannot reach the ERP database.

## What is implemented

Four fixed tools reuse existing services through ErpReadPort:

- erp_summary: warehouse_summaries, bounded per-warehouse product/reserved counts. This is a deliberately narrow operational summary, not full company KPIs or financial totals
- erp_order_status: order_status_detail for one required order ID
- erp_wip: wip_progress_batches for one required order ID; partial is always true because the existing domain service prefetches at most 500 records before filtering
- erp_warehouse: warehouse_stock_items for one required warehouse, bounded first page of available finished-goods stock (not every stock ledger), grouped by order/item/unit; the SQL is case-insensitive; the adapter accepts ASCII case variants and fails closed on other returned-name mismatches, so non-ASCII callers should use canonical names

Every call first resolves an external grant then checks fixed deployment identity, tool scope, native PrincipalRole::Admin AND current AdminAccess AND current CatalogItemRead/ApparatusQueueRead. This conservative first version serves only principals with those native admin role and administrative capabilities. Non-admin warehouse/apparatus/row scoping is not implemented and must not be enabled by removing either native-role or AdminAccess guards without implementing it.

The authorization resolver boundary is trusted server code. Never deserialize VerifiedGrant or Principal from a request, accept mobile session tokens, or turn an admin bearer into an external grant. The native admin role must also be refreshed from the live account, not frozen into a historical token. Live account status/revocation must be resolved on every call, not merely at connection time.

Tool arguments reject unknown fields, controls/empty identifiers, and limits outside 1..100. Read timeouts are five seconds and serialized result size is capped at 128 KiB. Output is field-whitelisted: no workers, phone numbers, supplier names, comments, payload_json, QR payloads, sessions, credentials or raw errors. Quantities carry original units and are not summed across units. retrieved_at_unix is retrieval time, not source transaction time; snapshot_consistent=false makes cross-read atomicity explicit. Domain output strings remain untrusted data.

## Local verification

Reproduce with Rust 1.99.0. This cloud environment needed low-memory build settings (the first debug-heavy test link was killed with SIGKILL):

    export RUSTUP_HOME=/workspace/shared/toolchains/rustup
    export CARGO_HOME=/workspace/shared/toolchains/cargo
    export PATH="$CARGO_HOME/bin:$PATH"
    export CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0
    export CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0

Commands:

    cargo test --locked --features mcp-readonly-prototype --lib mcp_readonly::tests
    cargo build --locked --features mcp-readonly-prototype --example mcp_readonly_fixture
    python3 tools/mcp_readonly_smoke.py
    cargo run --locked --features mcp-readonly-prototype --example mcp_readonly_fixture -- --synthetic-fixture

Then send newline-delimited JSON-RPC initialize, tools/list, tools/call requests on stdin. Example:

    {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"local-check","version":"1"}}}
    {"jsonrpc":"2.0","method":"notifications/initialized"}
    {"jsonrpc":"2.0","id":2,"method":"tools/list"}
    {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"erp_order_status","arguments":{"order_id":"DEMO-ORDER"}}}

The fixture is synthetic and not an OAuth or real ChatGPT connection test. It opens no TCP port and carries no real user credentials. Its output marks synthetic_fixture=true. JSON-RPC notifications are suppressed by the stdio example. Any future network transport must enforce lifecycle negotiation, notification behavior, body limits, request concurrency/rate limiting, Origin allowlisting, HTTPS, and OAuth challenges separately. This core is not yet a complete Streamable HTTP server.

Tests cover default-disabled, unrecognized fixture tokens (not production OAuth or mobile bearer validation), deployment mismatch, live permission revocation, argument bounds, unknown/write tools, allowlisted tool discovery, safe error messages and result metadata. An additional actual ErpReadPort test uses isolated memory-backed domain services (no AppState, live DB or network) to check stock projection and live capability revocation.

## Secure connection design (not provisioned)

1. Pick a genuinely supported distribution route first. An owner-only private demonstration is narrower than cross-user sharing. Do not advertise a universal mobile deep link or shared private install button.
2. For a standalone private MCP endpoint, implement OAuth 2.1 authorization code + PKCE S256. Publish protected-resource and authorization-server metadata; bind resource/audience and issuer. Choose a supported client-registration approach (CIMD, approved predefined clients, or bounded DCR) after checking the target client.
3. A currently authenticated ERP person approves the exact four read scopes. Grants bind that principal to this one configured deployment. The authorization server issues short-lived audience-bound access tokens. Refresh rotation, revocation and current account checks are required. Never use a shared mobile admin token.
4. Resolve each credential securely to the current principal and granted scopes. Intersect with current ERP capability checks on every request. Deny inactive/revoked accounts, stale or wrong-audience tokens and dependency failures. Never trust client-supplied company, principal, capabilities or identity headers.
5. Add POST /mcp using a current MCP SDK or a fully tested Streamable HTTP implementation, protect the entire endpoint, cap body/response/concurrency, restrict origins, and omit credentials and tool payloads from logs. No arbitrary SQL, URLs, generic API proxy or write tools.
6. Mobile connection UI stays disabled until the platform supplies a verified connection flow. Never embed tokens in deep links or URLs. A Sites-hosted server would require its own platform identity validation and a separately authorized ERP bridge; the local adapter does not provide that bridge.
7. Before exposing anything: isolated grant expiry/audience/issuer/revocation tests, current-account deactivation tests, capability/row-scope tests, zero-write database credentials, negative transport tests, and owner approval of the exact hosting and credential changes.

### Distribution limitation checked 2026-10-03

OpenAI's current Sites documentation says personal/Pro Site-hosted plugins cannot be privately shared to other ChatGPT users by invitations/share links; sharing the Site does not share its plugin. Business/Enterprise workspace sharing and reviewed public publication are separate routes. A second person's personal account may require a separately provisioned integration; no one-click support is claimed here.

Source: https://help.openai.com/en/articles/20001547-hosting-a-plugin-with-chatgpt-sites

## Verification status (2026-10-03)

- Official Rust 1.99.0 installed in workspace-local toolchain paths after owner approval; rustup-init checked against the official SHA-256. Shell profiles/security settings were not changed
- Focused Rust unit/domain tests: 9 passed, 0 failed; 1391 unrelated tests filtered out. Full repository test suite was not run
- Default debug-heavy test build was killed with SIGKILL; single-job, debug=0, incremental=0 retry compiled and passed
- Synthetic Rust example build: passed. Compiled-binary stdio smoke: passed (disabled gate, initialize, notification suppression, four-tool discovery/calls, synthetic metadata, malformed JSON and request-size cap)
- rustfmt --check on all new Rust files: passed
- No actual ChatGPT connection, OAuth token verifier, hosted endpoint or production database test has occurred

Tracked source/build prerequisites omitted by snapshot filtering were restored at the exact base SHA: config.rs, Cargo.lock, compile-time taraf.jpg, four migrations, backup-doctor Rust modules, monitor backup Rust handlers, transaction-lock source and qolip-block tests. These are source restorations, not new feature changes or restored user backups. No .env or deployment secrets were read. Demo backups remain excluded.

The scoped patch and SHA-256 file manifest live alongside the source snapshot. They exclude build caches, restored unchanged source, credentials and user backup data.
