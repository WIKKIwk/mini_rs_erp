# eMudhra TLS dependency update

The backend lockfile now resolves `webpki-roots` to `1.0.11` and
`rustls-webpki` to `0.103.16`. These update the embedded trust anchors and reject
distrusted intermediate public keys. See the
[Render advisory](https://render.com/docs/advisory-emudhra) and
[webpki-roots 1.0.11 release](https://github.com/rustls/webpki-roots/releases/tag/v%2F1.0.11).

`rustls-webpki 0.103.16` requires `aws-lc-rs ^1.18`; the lockfile also updates
`aws-lc-rs` to `1.18.2` and its required `aws-lc-sys` to `0.46.0`.
The `webpki-roots 0.26.11` compatibility package now uses the updated `1.0.11`
trust anchors.

## Release verification

Run the focused TLS check from the deployment environment using the updated
source and lockfile:

```bash
cargo test --locked --test tls_distrust -- --ignored --nocapture
```

The check uses this repository's reqwest/Rustls stack without credentials or a
proxy. It requires direct internet access, accepts a normal trusted HTTPS
connection, and requires both public eMudhra test endpoints to fail with a
certificate verification error. DNS failures and timeouts do not count as a
successful security check. The test is explicitly ignored during ordinary
offline test runs.

Local verification on 2026-10-24 passed: the trusted HTTPS control connected,
and both eMudhra endpoints were rejected with `CertDistrusted`.

Build release artifacts from the same revision and lockfile:

```bash
cargo build --release --locked \
  --bin mini_rs_erp --bin mini_rs_migrate --bin mini_rs_backup_state
```

Deploy the new artifacts and replace the running backend process. Updating the
host CA bundle or restarting the old executable does not apply this fix.
Record the source revision and executable SHA-256, and verify that the deployed
executable matches the new artifact. The handshake's application version alone
does not identify its dependency versions.

`tools/runtime/up_domain.sh` returns early when the backend is already healthy,
before building or restarting it. Use the established deployment procedure to
replace the running process; invoking that script alone does not apply this
update to an already running backend.
