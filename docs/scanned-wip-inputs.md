# Scanned WIP inputs

Apply migration `0184_scanned_wip_inputs` with the backend deployment. Each
successful WIP claim on Start, Merge or a new-input Resume saves an input
receipt in the same production transaction. The receipt retains the original
WIP row and claimed state. Returned receipts remain after an unused session
is removed.

Lamination and cutting workers open **Scan qilingan WIP’lar** with the list
button beside the order sheet's close button. This performs a separate
`GET /v1/mobile/admin/production-maps/scanned-wips` with `apparatus`, `order_id`
and `stage_node_id`. It is not part of the queue snapshot, initial sheet
loading, live feed or polling. The response is `no-store`.

Items expose physical consumption separately from the execution status:
Merge's `processed` marker alone does not establish consumption. Recorded
outputs and processing evidence establish consumption; missing or ambiguous
legacy provenance prevents reversal. Old inputs remain visible, but require
an existing trustworthy Start checkpoint to be returned.

Holding an eligible row opens a confirmation that it has not been physically
processed. `POST /v1/mobile/admin/production-maps/scanned-wips/return` accepts
only the exact scope, session, input batch and server-issued token. The worker
must own the claim and session and retain queue management permission and
apparatus assignment.

The transaction uses the normal order/apparatus locks, validates live work,
ownership, routes, quantities and resource witnesses, restores the original
input, and removes its session provenance. Returning the current last splice
remounts its preceding input; returning an earlier unused input keeps the
current input. Returning the final input of an untouched Start also reverses
fresh material claims and restores the previous queue/schedule state. Output
records are never rewritten. Audit events and idempotent receipts survive
retries and later claims of the same QR.

After a return attempt, including an uncertain response, the history reloads
and the main sheet uses its existing authoritative control refresh. Opening,
reading, retrying or closing history does not refresh the main sheet.
