# Warehouse QR preview and explicit WIP receipt

The warehouse scanner uses two dedicated, warehouse-authorized routes. Both require `WerkaAccess`, the exact `werka` principal role, and at least one assigned warehouse. No admin or production capability is granted to warehouse users.

## Read-only preview

`GET /v1/mobile/werka/qr/preview?qr_payload=<URL-encoded scanned text>`

Surrounding whitespace is ignored; otherwise send the original scanned payload. Maximum length: 2048 bytes. Scanning does not create a receipt or modify batches, stock, pallet membership, order state, or printing state.

- Pallet: `kind: "paddon"` plus the existing `paddon`, `items`, `snapshot_token`, `warehouses`, `can_receive`, and `receipt` fields
- Individual WIP: `kind: "wip"`, `batch`, `warehouses`, `can_receive`, `receive_blocked_reason`, `snapshot_token`, `receipt`, and nullable `paddon_code`
- Both variants may return `apparatus_names`, a canonical-ID-to-display-name map limited to the returned batches
- A receipt includes `warehouse`, `accepted_by_display_name`, and `accepted_at_unix`

Only HTTP 404 with `error: "qr_not_found"` permits a caller to try its existing non-production stock resolver. Authorization errors, infrastructure errors, unknown response kinds, and HTTP 409 `qr_ambiguous` must fail closed. An ambiguous payload is never resolved by picking a pallet or the first matching WIP.

An already-received WIP is inspectable only by a principal assigned to its receipt warehouse. Its preview has `can_receive: false`. Unreceived WIP on a pallet is inspectable but cannot be individually received.

## Explicit individual receipt

`POST /v1/mobile/werka/wip/receive`

JSON body: `progress_batch_id`, original `qr_payload`, selected `warehouse`, and the unchanged `snapshot_token` from preview.

Success: `ok: true`, committed `batch`, and `receipt`.

The client must ask the user to explicitly receive before calling this endpoint. Eligibility is revalidated at receipt time: waiting/unused final-stage output, unchanged batch and order context, active apparatus, valid location and quantity, no active pallet membership, no conflicting QR identity, and an assigned destination warehouse. HTTP 409 `wip_receipt_conflict` requires refreshing the preview.

Retrying the same confirmed token, batch, QR, and warehouse returns the persisted receipt. A different warehouse, stale token, or a receipt created through another workflow cannot be used to receive again. The write reuses the existing finished-goods stock representation and transaction helper; it does not create a second kind of inventory receipt.

PostgreSQL rechecks the snapshot under the existing order/apparatus locks and the batch row lock. New pallet attachments lock that same batch row and recheck eligibility before inserting membership; same-pallet no-op retries remain unchanged.

## Existing pallet workflow

`GET /v1/mobile/werka/paddons/preview` and `POST /v1/mobile/werka/paddons/receive` retain their existing contracts, snapshot tokens, atomic multi-roll receipt, and retry behavior. The universal scanner continues to call the existing pallet POST when the user explicitly receives a pallet.

## Focused verification

Run `cargo test --locked --features verification --test warehouse_qr` for the no-database domain and real-router checks. The `verification` feature uses isolated in-memory stores and does not enable the legacy library test tree. The existing PostgreSQL pallet-receipt regression additionally checks that received individual WIP cannot be attached to a fresh pallet; it requires an explicitly supplied isolated test database.
