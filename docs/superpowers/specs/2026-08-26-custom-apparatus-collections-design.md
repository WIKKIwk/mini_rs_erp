# Custom Apparatus Collections Design

## Status

Approved in chat on 2026-08-26. The user asked for a safe implementation of manually managed apparatus groups after confirming that canonical operation groups are intentionally derived and that the legacy apparatus-group authority was removed.

## Goal

Allow an authorized admin to create, rename, reassign, and remove custom named collections of canonical apparatus without changing canonical apparatus identity, operation classification, or any production workflow.

## Non-goals

- Do not restore `/v1/mobile/admin/apparatus-groups` or the dropped `mini_apparatus_groups` authority.
- Do not make a custom collection an execution, scheduling, queue, capacity, WIP, material, tooling, or production-map authority.
- Do not change the existing derived `print`, `laminate`, `cut`, `package`, and `glue` groups.
- Do not delete, retire, or mutate apparatus when a collection or membership is changed.

## Chosen Approach

Add a separate `apparatus collection` aggregate. A collection has an opaque stable ID, a user-visible name, an optimistic-concurrency revision, and an ordered set of exact canonical `ApparatusId` members. The existing operation-derived groups remain visible as a separate read-only section in Accord Mobile V2.

This is preferred over reviving the legacy apparatus-group table because the legacy table was intentionally removed as a runtime authority during the canonical cutover. It is also preferred over storing collections only on the device because collection membership must be shared consistently across admins and devices.

## Backend Domain

Create a focused `apparatus_collections` domain module containing:

- `ApparatusCollection`: `id`, `name`, `apparatus_ids`, and `revision`.
- `ApparatusCollectionStorePort`: list, create, replace, and delete operations.
- `ApparatusCollectionService`: normalization, validation, exact canonical-apparatus validation, duplicate-name handling, optimistic concurrency, and opaque ID generation.
- In-memory and PostgreSQL store implementations.

Validation rules:

- Name is trimmed, non-empty, and at most 80 Unicode scalar values.
- Names are unique case-insensitively after trimming.
- Membership contains no duplicates and at most 500 apparatus IDs.
- Every member is a syntactically valid exact `ApparatusId` and currently resolves to an active canonical apparatus projection.
- Create starts at revision `1`; replace and delete require the exact current revision.
- Collection IDs are generated as `apparatus-collection:<32 lowercase hex characters>` and are never derived from names.

## PostgreSQL Persistence

Add append-only migration `0075_custom_apparatus_collections.sql` with:

- `mini_apparatus_collections(id, name, revision, created_at, updated_at)`.
- A unique index on `lower(btrim(name))`.
- Shape, non-blank name, length, and positive revision constraints.
- `mini_apparatus_collection_members(collection_id, canonical_apparatus_id, position)`.
- `collection_id` cascades on collection deletion.
- `canonical_apparatus_id` references `mini_canonical_apparatus_identities(apparatus_id)` with `ON DELETE RESTRICT`.
- Primary key on `(collection_id, canonical_apparatus_id)` and unique `(collection_id, position)`.

Create, replace, and delete run in transactions. Replace locks the collection row, checks `expected_revision`, replaces members, increments revision, and commits atomically. Database unique/FK conflicts are mapped to stable domain errors.

## HTTP API

Add new routes; do not reuse the deleted legacy route:

- `GET /v1/mobile/admin/apparatus-collections`
- `POST /v1/mobile/admin/apparatus-collections`
- `PUT /v1/mobile/admin/apparatus-collections/{id}`
- `DELETE /v1/mobile/admin/apparatus-collections/{id}?expected_revision=N`

Payloads:

```json
{
  "name": "Bosma A liniyasi",
  "apparatus_ids": [
    "apparatus:default:bosma_7",
    "apparatus:default:bosma_8"
  ]
}
```

Update adds `expected_revision`. Responses return the normalized collection. Delete returns the deleted normalized collection so the client can prove which revision was removed.

Read and mutation access follow the canonical apparatus admin boundary. Mutations require `production.map.manage`; requests without the required capability fail before store access. Stable errors include invalid input, apparatus not found or inactive, duplicate name, collection not found, revision conflict, and persistence failure.

## Mobile Client and UI

Add `AdminApparatusCollection` plus Mobile API methods for list/create/replace/delete. Test mode mirrors the same normalization, duplicate-name, exact-ID, and revision semantics.

Keep the current `Aparat guruhlari` tab and split its content into:

1. `Maxsus guruhlar`: mutable collections from the backend, with add, edit, and delete actions.
2. `Canonical guruhlar`: the current read-only operation-derived groups and explanatory text.

The collection editor contains a required name and a multi-select list of active canonical apparatus. Saving is disabled while a request is in flight. Delete requires confirmation. API failures are shown through the existing admin top-notice pattern. Collection mutation refreshes only collection state; it does not patch apparatus or notify production-map runtime.

## Safety Invariants

- Exact canonical apparatus IDs are the only persisted membership identity.
- Collection mutations never call canonical apparatus create, patch, retire, queue, capacity, production-map, raw-material, or WIP services.
- Apparatus rename preserves collection membership because IDs are stable.
- Apparatus retirement does not silently rewrite collection history; new writes reject inactive apparatus, while reads may retain the exact ID for diagnosis if historical data exists.
- Stale update/delete requests fail closed with a revision conflict.
- Existing derived groups and existing mobile releases keep their current behavior.

## Testing

Backend RED-GREEN coverage:

- Create normalizes name and deduplicates exact members while preserving order.
- Invalid, unknown, or inactive apparatus is rejected without state change.
- Duplicate normalized names are rejected.
- Replace requires the current revision and changes only collection state.
- Delete requires the current revision and leaves canonical apparatus unchanged.
- Legacy `/apparatus-groups` remains `404`.
- PostgreSQL migration contains exact FKs, constraints, and indexes; repository transaction tests run when PostgreSQL is available.

Mobile RED-GREEN coverage:

- API serialization and test-mode CRUD/revision behavior.
- Group tab renders custom and canonical sections independently.
- Add/edit/delete collection UI updates only custom collections.
- Canonical derived groups remain based on `operation` and remain read-only.

Verification uses focused Rust route/domain tests, focused Flutter widget/API tests, formatter checks, static analysis of changed Dart files, and `git diff --check`. No deployment, database migration execution against real data, push, device, simulator, or browser action is part of this task.

## Commit Boundaries

1. Design specification in `mini_rs_erp`.
2. Backend domain, migration, API, and tests in `mini_rs_erp`.
3. Mobile model, API, UI, localization, and tests in `accord_mobile_v2`.

Each commit stages only feature-owned files and excludes the pre-existing RPS/GScale changes in both repositories.
