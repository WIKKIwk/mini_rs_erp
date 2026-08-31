# Rust test migration audit

This Python tool parses Rust source with Tree-sitter and classifies every test
without invoking Cargo or compiling the ERP crate. It is an inventory and
migration-safety tool; it does not delete or rewrite tests.

Run the audit with `uv`; it creates and caches an isolated Python environment
from the dependency metadata embedded in the script:

```bash
make audit-test-migration
uv run --script tools/test_migration_audit/audit.py --json
```

Alternatively, install the same pinned dependencies in a virtual environment:

```bash
python3 -m venv /tmp/mini-erp-audit-env
/tmp/mini-erp-audit-env/bin/python -m pip install \
  -r tools/test_migration_audit/requirements.txt
```

Then audit the current worktree:

```bash
/tmp/mini-erp-audit-env/bin/python tools/test_migration_audit/audit.py
```

Emit a complete agent-readable report, or inspect a historical revision
without restoring deleted test files:

```bash
/tmp/mini-erp-audit-env/bin/python tools/test_migration_audit/audit.py --json
/tmp/mini-erp-audit-env/bin/python tools/test_migration_audit/audit.py \
  --git-ref 975078a^ --path src/http --json
```

Classifications are conservative:

- `automatic_contract`: one router request with one unambiguous static method,
  URI, and asserted status oracle, and no database, custom fixture, fake, or
  domain-state setup;
- `scenario_contract`: HTTP behavior that needs generated fixture or workflow
  support before migration;
- `rust_native`: internal logic, database, transaction, compiler, or other
  non-router behavior that should remain in Rust.

Every decision includes its source line, detected signals, and reasons. A test
is never silently treated as migrated. Run the Python unit tests with:

```bash
cd tools/test_migration_audit
/tmp/mini-erp-audit-env/bin/python -m unittest -v
```

## Historical contract extraction

The extractor turns only `automatic_contract` cases from the pinned migration
manifest into verifier JSON. It reads the old files directly from Git, so it
does not restore or compile the deleted Rust tests:

```bash
make extract-test-contracts
make check-generated-test-contracts
```

Extraction is fail-closed: every source assertion must map to a supported
status, JSON subset, or body oracle. The command aborts on an unknown assertion,
source-test count drift, or automatic-candidate count drift. Fixture-dependent
tests remain listed under `skipped` in the generated artifact until a scenario
generator can represent them safely.

A selected-test manifest can pin individual passing contracts from a mixed Rust
test file. `emit_removal_patch.py` uses Tree-sitter to emit an
`apply_patch`-compatible deletion, and `--check` refuses regenerated output
until every selected function is absent from the worktree.

`extract_all_contracts.py` processes every bundle in `migrations/index.json`.
Selected scenario contracts require an explicit manifest flag and still must
pass the real verifier harness before their Rust source is removed.

Selected multi-request workflows can also opt in explicitly. The extractor
keeps source order, generates one verifier contract per bound response, and
requires every step to have one static request, one status oracle, and only
supported response assertions. `expected_generated_cases` pins the resulting
step count independently from the number of removed Rust test functions.
