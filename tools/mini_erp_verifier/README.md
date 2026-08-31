# Mini ERP verifier

Run the generic HTTP contract verifier with:

```bash
python3 tools/mini_erp_verifier/verify.py
```

It builds the production crate once with the `verification` feature, then sends
all data-driven cases to the real Axum router in one process. Rust
`#[cfg(test)]` modules are not enabled or compiled. The harness receives a
sanitized environment and uses a disposable directory plus in-memory domain
stores, so it cannot connect to the configured ERP database.

`contracts.json` owns the remaining manual route, authentication, CORS, and
response-shape contracts. `generated_contracts.json` is produced from the
historical Rust tests by the Tree-sitter extractor. Access matrices
automatically generate anonymous and every-role probes for every declared
method. Keep Rust tests only where an independent business invariant,
transaction boundary, database implementation, or typed compiler guarantee is
the actual oracle.

Run `make extract-test-contracts` after intentionally changing the migration
source or extractor. Normal `make verify` runs the extractor in `--check` mode
first and fails when the generated artifact is stale. Generated cases include
their original path and line, and can select the isolated no-provider router
fixture without adding Rust test code.

The migration manifest owns the removed Rust paths and expected source-test
count. The verifier fails if any migrated generic Rust test file is restored,
so `cargo test` cannot silently regain this removed router layer.

Use `--no-build` to rerun cases against an already-built harness, and `--json`
for compact agent-readable output. Route declarations are fingerprinted so a
route addition, removal, or rename is reported before runtime probes begin.
