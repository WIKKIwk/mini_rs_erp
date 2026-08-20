# Canonical Apparatus Contract

Status: frozen target contract for the one-shot apparatus migration.

Pinned standards: IDTA Release 26-01, AAS Part 1 metamodel v3.2.0, and AASX
Part 5 IDTA-01005 v3.2 using Open Packaging Conventions.

## Scope and source of truth

`src/core/apparatus_standard` is the canonical apparatus configuration domain.
`CanonicalApparatus` is the only live configuration source of truth after
migration. Its `ApparatusId` is immutable, opaque, non-empty, and independent
of display text. The accepted shape is
`apparatus:<namespace>:<opaque-key>`; the old `apparatus:<title>` shape is not
canonical.

The canonical configuration contains only durable apparatus master data:

- identity and display metadata;
- display-only catalog ordering hints; these are non-semantic and never encode
  apparatus topology, routing, or identity;
- current catalog classification and supported capability codes/profiles;
- queue, raw-material, tooling/qolip-scan policies that are currently supported;
- finite capacity, efficiency, setup/cleanup, and working-window configuration;
- factory-map placement and training-enabled references where they are actual
  configuration;
- provenance, revision, and AAS/AASX package metadata.

It does not contain current order assignment, queue entries or states, WIP,
active run sessions, progress, downtime instances, schedule reservations,
material assignments/barcodes, scans, worker actions, or other order-specific
state. Those are runtime or order contracts and must reference the canonical
apparatus by `ApparatusId`.

## Precedence and migration deletion targets

After integration, the canonical contract has precedence over every legacy
apparatus representation. Integrators must remove live reads and writes that
derive identity from titles or maintain a competing apparatus catalog,
including the legacy apparatus identity paths in `src/core/apparatus_groups.rs`,
`src/store/apparatus_group_store.rs`, `src/db/postgres_apparatus_group.rs`,
title matching used as identity in production-map queue/state code, and any
duplicate apparatus master payload. Production-map queue, material, capacity, training,
DB, and HTTP references are canonical-ID-only in this integrated candidate; legacy text
is retained only as an audit/display snapshot and is never a lookup key.

The old implementations are deletion targets for the integrator after all
cross-scope consumers are migrated. No permanent legacy fallback, alternate
source of truth, title-derived lookup, or compatibility identity is permitted.
Historical rows may retain a display-name snapshot for audit/history only.
That snapshot must never be used as live identity, matching, routing, or
configuration lookup.

Where rules conflict, the precedence is:

1. canonical `ApparatusId` and `CanonicalApparatus` configuration;
2. explicit canonical policy and capability configuration;
3. runtime state and order-specific records, which cannot rewrite master data;
4. historical display snapshots, which are informational only.

## Validation invariants

The Rust module rejects blank/control/whitespace IDs, legacy one-segment IDs,
IDs derived from the live display name, invalid family/kind combinations,
duplicate or unsupported capability profiles, invalid capacity windows or
bounds, invalid references, and conflicting queue/material/tooling policies. A
Pechat apparatus (including `ColorPechat` and `Flexo`) must use
`StrictSequence`; `FreePick` is rejected. `ColorPechat` requires an explicit
7–9 station count. A material rule selects either all-state item groups or
requirement groups; it cannot silently mix both modes. Qolip scanning is
represented for the currently supported pechat behavior: 7–9 color
`ColorPechat` and `Flexo`. Non-pechat families cannot carry the required Qolip
policy.

The module is serde-compatible for contract transport and provides bounded
`apparatus_standard::aasx::export_aasx` and
`apparatus_standard::aasx::import_aasx` engineering paths. Export validates
the canonical record before writing an OPC ZIP/AAS 3.2 XML package; import
validates the bounded ZIP relationship graph and parses only that canonical
contract. The package is exchange input/output only; PostgreSQL typed domain
data remains the runtime source of truth.

## AAS/AASX target mapping

The canonical apparatus maps to one project-owned AAS submodel target,
`urn:mini-rs-erp:semantic-id:submodel:apparatus:1`. The submodel contains the
identity, display, classification, capability, policy, capacity, placement,
training, provenance, versioning, and pinned package metadata properties listed
above. `export_aasx` emits these five package entries:

- `[Content_Types].xml` and `_rels/.rels` for OPC package typing and root
  relationship;
- `aasx/aasx-origin` and `aasx/_rels/aasx-origin.rels` for the AASX origin and
  AAS-spec relationship;
- `aasx/data.xml` containing the AAS 3.2 XML environment, one AAS, and one
  apparatus configuration submodel.

The package uses uncompressed ZIP storage, which is valid OPC packaging, and
has the standard `application/asset-administration-shell-package` MIME
metadata. The semantic ID and property model are project-owned and are not
presented as IDTA-issued semantic identifiers. The exporter intentionally does
not include queue positions, current workers, live WIP, assigned order
barcodes, pause/freeze state, or any other order/runtime record.

The authenticated admin HTTP runtime boundary is:

- `GET /v1/mobile/admin/apparatus/{id}/aasx` loads the persisted canonical
  apparatus by `ApparatusId`, exports it as the binary AASX package, and
  returns `Content-Type: application/asset-administration-shell-package` with
  an attachment `.aasx` filename.
- `POST /v1/mobile/admin/apparatus/{id}/aasx` accepts one bounded package up to
  16 MiB, parses it with `import_aasx`, requires the package identity to match
  `{id}` and its revision to match the current canonical revision, then calls
  `ApparatusGroupService::mutate_canonical_apparatus`. The service preserves
  the immutable ID and advances the revision exactly once.

Both routes require the existing authenticated admin/production-map capability
gate. They fail closed for missing canonical records, malformed or oversized
packages, invalid canonical configuration, identity conflicts, and revision
conflicts. They do not promote legacy-only rows, write a competing store, or
bypass `ApparatusGroupService`.

## Integrator requirements

The integrator must register the module if needed, migrate all live apparatus
consumers to `ApparatusId`, remove the legacy identity/source-of-truth paths,
keep runtime/order state out of this configuration, coordinate schema and
migration changes with the assigned owners, and add end-to-end coverage for
the canonical ID across catalog, map, queue, materials, capacity, training,
DB, and HTTP boundaries. The exporter/importer remain one-apparatus, in-memory
byte operations; the authenticated admin routes above provide the runtime file
download/upload boundary and canonical service persistence without turning
AASX into a second runtime source of truth.
