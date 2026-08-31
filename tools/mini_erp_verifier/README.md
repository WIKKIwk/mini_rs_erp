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

The default command does not blindly invoke Cargo. It reuses the harness while
Cargo's generated dependency list and workspace build configuration are older
than the binary. A changed production Rust input rebuilds once; Python/JSON
contract-only changes run without Cargo.

`contracts.json` owns the remaining manual route, authentication, CORS, and
response-shape contracts. `generated_contracts.json` is produced from the first
historical Rust migration, while `generated_automatic_contracts.json` contains
individually selected current-worktree migrations. Both are produced from
pinned Git snapshots by the Tree-sitter extractor. Additional generated
automatic/scenario bundles are discovered through the migration index rather
than hardcoded verifier wiring. Access matrices
automatically generate anonymous and every-role probes for every declared
method. Keep Rust tests only where an independent business invariant,
transaction boundary, database implementation, or typed compiler guarantee is
the actual oracle.

Run `make extract-test-contracts` after intentionally changing the migration
source or extractor. Normal `make verify` runs the extractor in `--check` mode
first and fails when the generated artifact is stale. Generated cases include
their original path and line, and can select the isolated no-provider router
fixture without adding Rust test code.

Migration manifests own removed Rust paths/functions and expected source-test
counts. The verifier fails if any migrated generic Rust test is restored, so
`cargo test` cannot silently regain this removed router layer.

Use `--no-build` to rerun cases against an already-built harness, and `--json`
for compact agent-readable output. Route declarations are fingerprinted so a
route addition, removal, or rename is reported before runtime probes begin.
