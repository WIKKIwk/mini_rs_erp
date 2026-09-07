# Tayyorlov masteri

System role: `tayyorlov_masteri`; capability: `preparation.access`; numeric login code prefix: `90`.
Create this system user through the existing admin user form, then assign a non-group warehouse from the user card.
No admin privileges, supplier debt, purchase price, or external supplier identity are implied by this role.

## Workflow and invariants

- The master creates material names (homashyo/siryo), not manual serial numbers. These are shared `mini_items` with an owner link.
- Tapping a material receives a positive KG lot into an assigned warehouse. The backend generates its barcode and records an immutable receipt and shared stock event.
- An active production order with `map_json.order_kg` is selected. Each material has its own percentage, greater than zero and at most 100. Percentages are independent; their sum is not required to equal 100.
- Required KG = order KG × percentage / 100, rounded half-up to six decimal places. All arithmetic uses integer micro-KG; incoming decimals are strings.
- Saving immediately consumes FIFO receipt lots. Recipe, order-KG snapshot, receipt allocations, remaining stock, and shared inventory events commit together. Insufficient stock rolls everything back.
- Only the master's own receipts in a currently assigned warehouse may be consumed. Assigned, reserved, consumed, deleted, in-transit, and physically relocated stock is excluded.
- One posted recipe per master/order prevents accidental repeated deductions even with a new request ID. The initial UI exposes immutable history, not editing/deleting posted recipes; corrections require a separately designed reversal workflow.
- Shared `mini_raw_material_stock` remains the physical balance authority. A partially consumed row holds remaining KG. A fully consumed row follows the existing ERP convention of retaining its last quantity with `status=consumed` (available balance zero). Original quantities are preserved in receipt documents.
- A durable `(owner_ref, request_id)` command ledger returns the original result on retry and rejects a different payload under the same key. The mobile client persists unresolved commands per server/account and reuses their IDs after connection loss or restart.

## API

All endpoints require a logged-in Tayyorlov masteri with `preparation.access`.

- `GET /v1/mobile/preparation/snapshot`: assigned warehouses, owned material balances, active KG orders with saved flags, and last 100 receipts/consumptions.
- `POST /v1/mobile/preparation/materials`: `request_id`, `name`.
- `POST /v1/mobile/preparation/receipts`: `request_id`, `item_code`, `warehouse`, `kg`.
- `POST /v1/mobile/preparation/consumptions`: `request_id`, `order_id`, `warehouse`, `expected_order_kg`, `lines: [{item_code, percent}]`.

The order's KG is locked and rechecked on save. A stale KG or previously posted recipe returns 409. Scope violations return 403. Domain rejections include a `preparation_*` code; transport/auth failures do not prove whether an earlier uncertain request committed.

## Delivery

Migration `0093_preparation_master` must be applied using the normal deployment workflow before enabling this role in a running service. No live migration is part of the source implementation.
Tests: backend `preparation` filter (fixed-point rules, isolated PostgreSQL FIFO/rollback/idempotency/concurrency/scope, system-role login and authorization); Flutter `test/preparation_test.dart` (route/role, exact calculation, durable retry, and receipt/recipe UI).
