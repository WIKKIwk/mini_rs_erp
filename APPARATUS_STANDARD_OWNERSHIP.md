# Canonical Apparatus Ownership

Status: normative ownership and synchronization model.

This document assigns write and read authority. It is not an inventory of the
legacy implementation and does not authorize compatibility authorities.

## Authority matrix

| Concern | Authoritative owner | Derived/read-only consumer |
| --- | --- | --- |
| Apparatus configuration | immutable `CanonicalApparatusRevision` | none |
| Current revision pointer | canonical head repository | runtime readers |
| Portable canonical artifact | deterministic AASX stored with its revision | download/export clients |
| Artifact identity | SHA-256 of exact stored AASX bytes | reconciliation and clients |
| Current runtime catalog | projector-owned `mini_apparatus` row | runtime business logic |
| Queue/material/capacity lookup | projector-owned projection or view | runtime query paths |
| Change notification | transactional outbox | asynchronous integrations |

No row, file, cache, endpoint, or legacy store outside this matrix can write or
override apparatus configuration.

## Sole writer

`CanonicalApparatusService` is the only component allowed to create a new
revision or write a current/derived projection. Public store traits and
production-map services expose reads and operational state commands only.

The canonical service owns:

- server-generated `ApparatusId` creation;
- complete draft/domain validation;
- expected-revision CAS;
- deterministic AASX generation and hashing;
- append-only revision persistence;
- head update;
- current and optional derived projection writes;
- one transactional outbox event;
- post-commit cache invalidation.

Database repository methods that write canonical state are private to this
service transaction boundary. Tests may use an in-memory implementation only
when it obeys the same revision/artifact/projection contract.

## Transaction ownership

Every mutation is one PostgreSQL transaction. The service locks the current
head, validates CAS, creates exactly one successor revision, writes its exact
AASX bytes/hash, projects without DB reads, updates the head/projections, and
adds one outbox event before commit.

The head, revision, AASX, hash, projection, and outbox event are one atomic
change-set. A transaction error or injected fault rolls back all of them.
Cache invalidation occurs only after successful commit and cannot establish
authority.

## Read ownership

Normal runtime requests read PostgreSQL runtime projections. They never read
the AASX BYTEA column and never depend on the AASX parser. Admin AASX download
is the only current-artifact byte read; import parses untrusted bytes before a
canonical service mutation.

Historical/runtime records reference `ApparatusId`. Display names are copied
only for output or historical audit and cannot be used in joins, conflicts,
filters, routing, authorization, reconciliation, or behavior.

## Derived projections

`mini_apparatus` is mandatory. Queue, material, capacity, and other lookup
tables may remain only when justified by runtime query performance. A retained
projection:

- has no public independent write method;
- is written only in the canonical service transaction;
- records source `apparatus_id`, revision, and artifact hash;
- is fully replaceable from one canonical revision;
- participates in zero-drift reconciliation;
- cannot become a fallback authority.

Operational rows such as queue events, WIP, assignments, schedules, and
progress remain independently mutable runtime state, but they cannot mutate or
infer apparatus configuration.

## ISA-95 and AAS ownership boundary

The domain layer owns the project ISA-95 apparatus profile and validation.
The AASX codec owns deterministic representation and untrusted package
validation. IDTA requirements are pinned interoperability inputs; deterministic
bytes, project semantic IDs, and SHA-256 are project invariants.

The projector owns the pure conversion from a revision to all runtime read
models. It performs no DB reads, fallback lookup, random generation, or
apparatus-specific branching.

## Admin ownership

Admin create, patch, replace-from-AASX, and retire commands delegate to
`CanonicalApparatusService`. Capacity/material/queue endpoints, if retained,
are adapters that patch the canonical aggregate and create one revision.

List/detail routes read current projections. AASX GET returns exact stored
bytes. Options expose versioned vocabulary only. Legacy upsert and direct
projection mutation routes are removed or rejected.

## Downstream ownership

Production map, queue, WIP, progress, completion, transfer, materials,
capacity, training, tooling, warehouse, factory-location, worker assignment,
returned-paint, and astatka code consume typed `ApparatusId` plus explicit
capability/execution/policy/lifecycle projections.

Downstream code cannot classify behavior by title, alias, ID literal, role,
family/kind guess, or local default. A rename is invisible to identity and
history. Retired apparatuses remain readable for history and are rejected for
new work.

## Data migration ownership

Migration tooling produces an operator-reviewable mapping:

```text
legacy identity -> ApparatusId -> complete canonical revision
                -> deterministic AASX -> SHA-256
```

Preflight inventories every authority and dependent reference. Conflicts,
ambiguity, missing semantics, and unresolved references abort. No automatic
precedence or `Other` fallback is permitted. Source/target counts and exact
identities reconcile transactionally.

## Clean cutover

The cutover release removes legacy write/read authority in the same release as
consumer migration. It contains no dual read, dual write, optional resolver,
SQLite production authority, default catalog, title lookup, or compatibility
writer.

Production startup requires PostgreSQL canonical repository availability.
Rollback means restore the verified pre-cutover backup and deploy the old
binary. New code never rolls back by consulting legacy configuration.

## Repository boundaries

- Migrations `0001` through `0068` are immutable history.
- New canonical repository/projection changes use append-only migrations after
  `0068`.
- Production migration `0062` remains byte-for-byte unchanged.
- Canonical model, codec, projector, repository, and service are cohesive
  modules with explicit dependency direction.
- Only canonical repository/service modules may contain configuration SQL
  writes.

## Forbidden final state

Final static and executable acceptance must prove absence of:

- `ApparatusUpsert`, `ApparatusMasterData`, and `ApparatusGroupStorePort`;
- SQLite or default-catalog apparatus authority;
- unavailable/optional canonical resolvers;
- independent queue/material/capacity projection writers;
- title/name/alias identity helpers;
- family/kind/role/ID-literal behavior inference;
- runtime AASX reads;
- orphan, duplicate, unresolved, stale, or drifting projections.
