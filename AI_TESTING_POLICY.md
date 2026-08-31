# AI Testing Policy

This policy applies to every AI agent changing Mini ERP or its mobile contract.
Its goal is to verify behavior with the least compilation, context, token, and
maintenance cost without weakening the test oracle.

## Required decision order

Use the first layer that can correctly verify the requested behavior:

1. **Existing verifier/tool**
   - Run `make verify` first for HTTP route, authentication, method, status,
     header, JSON shape, and multi-request workflow behavior.
   - Prefer declarative contracts generated through
     `tools/test_migration_audit/` over writing a new test file.
   - Do not hand-edit generated contract bundles. Update the pinned migration
     manifest or extractor and run `make extract-test-contracts`.

2. **Extend the generic verifier capability**
   - If the verifier cannot express a reusable assertion, request shape, role,
     or workflow, add one general extractor/verifier capability.
   - Add focused Python unit coverage for that tool capability.
   - Do not create a feature-specific fake or one-off runner when a reusable
     operator can represent the behavior.
   - Extraction must fail closed: an unknown request, fixture, or assertion
     must reject migration instead of silently dropping coverage.

3. **Python with `pytest`**
   - Write Python only when the behavior cannot be represented safely by the
     existing data-driven verifier.
   - Use it for running-server API integration, external workflows, E2E,
     approved test-database integration, property-based testing, or failure
     injection outside private Rust internals.
   - Put backend Python tests under `tests_python/`; create that directory only
     when the first justified Python test is added.
   - Never point a test at production data or an unapproved database.

4. **Rust test**
   - Write Rust only when the oracle requires private functions, traits,
     typed domain objects, store implementation details, transactions,
     database invariants, concurrency, memory safety, or compile-time checks.
   - A route method, status code, authorization matrix, header, JSON field, or
     ordinary API workflow is not by itself a reason to add a Rust test.
   - Run the narrowest relevant Rust test target. Do not run every Rust test or
     compile all `#[cfg(test)]` modules unless the change genuinely crosses
     those boundaries.

5. **Dart/Flutter test**
   - Use Dart for mobile DTO parsing, session/capability state, navigation,
     widgets, UI behavior, and client-side error handling.
   - Do not duplicate a backend contract test in Dart unless the mobile parser
     or consumer behavior is the actual oracle.

## Rules for AI agents

- Do not create a new test file until the existing verifier and closest tests
  have been checked for reusable coverage.
- Do not duplicate the same behavior in JSON, Python, Rust, and Dart.
- Do not read every test file by default. Inspect the changed production path,
  its direct consumers, the closest existing contract, and the audit output.
- Do not run the full Rust suite merely because Rust test files exist.
- Never delete a Rust test only because extraction succeeded. The generated
  contract must pass the real verifier harness in a fresh isolated workflow.
- Keep Rust-native tests in Rust. Tool migration is for externally observable
  contracts, not for weakening transaction or business-invariant coverage.
- A passing compiler is not a behavioral oracle. A generated test is not valid
  unless its expected result independently represents the requested behavior.

## Standard verification commands

```bash
make verify
make audit-test-migration
make test-python-tools
cargo fmt --check
```

Run focused `cargo test` or database integration only when the selected layer
requires it. The normal verifier path must reuse its cached harness when Rust
production inputs are unchanged and must not compile Rust test modules.

## Short rule

**Tool first. Reusable verifier extension second. Python third. Rust only for
Rust-native guarantees. Dart only for mobile behavior.**
