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
