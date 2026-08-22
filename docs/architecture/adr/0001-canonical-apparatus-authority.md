# ADR 0001: Canonical apparatus authority

- Status: accepted
- Date: 2026-08-20
- Scope: apparatus configuration, artifact representation, runtime projection,
  and clean cutover

## Context

The legacy system spreads apparatus configuration across master payloads,
default catalogs, SQLite/PostgreSQL stores, production-map projection tables,
display-name matching, and apparatus-specific behavior branches. Adding AASX
as an export/import endpoint would preserve that split authority.

The target requires one stable identity and one complete configuration model
while keeping normal runtime reads efficient and making configuration portable.

## Decision

The authoritative entity is an immutable, append-only
`CanonicalApparatusRevision`. It implements the project ISA-95 apparatus
profile and is identified by stable `ApparatusId` plus revision.

For every revision, the project deterministically generates and stores one
canonical AASX artifact and SHA-256. Deterministic packaging and hashing are
project invariants, not IDTA normative requirements. Imported packages are
parsed as untrusted candidates and regenerated; uploaded bytes do not become
authority.

`mini_apparatus` and any retained queue/material/capacity lookup tables are
materialized runtime projections. Runtime reads projections and does not parse
AASX.

`CanonicalApparatusService` is the sole writer. Revision append, artifact
storage, head CAS, projection replacement, and one outbox event occur in one
PostgreSQL transaction. Cache invalidation occurs after commit.

The release performs a clean cutover. Legacy stores, default catalogs,
title-based identity, direct projection writers, and optional/fallback
resolvers are removed after exact data migration. Rollback uses a verified
database backup and old binary.

## Consequences

- Configuration writes are serialized by head lock and expected revision.
- Historical revisions and exact artifact bytes are durable and immutable.
- Runtime latency is independent of AASX parsing.
- Display names can collide and can change without identity impact.
- Projection drift is detectable from source revision/hash and repairable from
  the authoritative revision.
- Every apparatus draft must be semantically complete; migration cannot guess
  missing behavior.
- Production cannot start without canonical PostgreSQL persistence.

## Rejected alternatives

- AASX as an optional export/import file: rejected because it leaves parallel
  authority.
- PostgreSQL projection rows as independent writable configuration: rejected
  because synchronization and provenance become ambiguous.
- Title/alias fallback during migration or runtime: rejected because identity
  becomes mutable and non-deterministic.
- Dual-read or dual-write transition release: rejected because conflicts have
  no safe automatic precedence.
- Runtime regeneration of historical AASX: rejected because exact artifact
  identity and audit history would change.

## Verification

Acceptance requires executable deterministic codec tests, append-only/CAS and
fault-injection database tests, exact artifact/payload and projection
equivalence, zero drift diagnostics, canonical-ID-only downstream chains,
forbidden-symbol zero searches, and all final Rust/PostgreSQL gates.
