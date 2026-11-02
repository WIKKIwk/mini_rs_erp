# Lamination remainder labels

The operator keeps the existing two report fields: printed/input roll remainder
and film remainder. Both contain **net kilograms**, including fractional values.
The report, output and stock workflow is the same for every label setting.
Zero records an exhausted input and creates no remainder label.

Administrators and brigaders independently control automatic labels through
`GET/PUT /v1/mobile/admin/production-maps/lamination-settings`:

```json
{"print_wip_remainder_labels":true,"print_film_remainder_labels":false}
```

A PUT can contain either switch or both. A partial update preserves the other
switch's current value. The switches only select automatic remainder labels;
ordinary output labels and manual reprints of saved remainders remain available.

Apply migration `0185_lamination_remainder_label_settings` and deploy the backend
before the updated mobile app. Migration copies the previous setting to both
switches. The legacy `layer_remainders_required` PUT remains an alias that sets
both switches; mixing legacy and new fields is rejected. Its settings response
is true when both switches are on. Snapshot `layer_remainders_required` remains
true for legacy clients; the updated app uses the two existing fields in all cases.

## Report protocol

`GET /v1/mobile/admin/production-maps/laminatsiya-astatka` supplies the current
session, sources, `label_settings` and `remainder_weights_supported: true`.
The mobile app requires this capability before submitting the two kg fields.
The POST uses the same endpoint and existing report/output fields with:

```json
{
  "layer_remainder_weights": {
    "wip_net_kg": 20.125,
    "film_net_kg": 0,
    "expected_sources": [
      {"source_kind":"progress_batch","source_id":"original-roll","expected_revision":7}
    ]
  },
  "print_transport":"offline"
}
```

`expected_sources` includes every source in the form's snapshot, including sources
answered with zero. Source identity and revision must still match on submission.
A positive kg field must identify exactly one current physical roll of that kind
with valid net kg and length. Missing or ambiguous sources return a conflict;
the server does not guess a roll or fabricate label metadata. An optional-material
execution without sources can submit zero kg, with the existing output policy.
Legacy explicit `layer_remainders` requests remain supported, including pending
request fingerprints. They cannot be mixed with `layer_remainder_weights`.
Historical report-only requests retain their existing audit behavior.

Report summary columns retain their historical `*_leftover_rolls` names. New
two-field requests store kg in those columns; historical count-based records are
not rewritten. The explicit remainder weights identify the new request format.

## Identity and printing

The server derives the returned length from the original net kg/length ratio.
The remainder receives a new saved QR while retaining the source's item, original
producer/supplier, tare, order/customer information and physical stock lineage.
Gross weight is the entered net kg plus the original tare. WIP uses the ordinary
progress label; film uses the raw material label with its width and thickness.

Opening WIP inputs use the same input-roll kg field and WIP label switch.
Migration `0186_opening_wip_layer_remainders` enables their residuals. A positive
residual creates a new opening batch with `parent_batch_id` and the original
intake passport, product and route. Its new QR is resolved and claimed through
the existing opening-WIP scanner. The original batch is retired as processed;
its recorded quantities remain historical and its QR cannot be reactivated.
The child retains the original tare, records the newly measured net kg and marks
its proportional length as estimated. Zero exhausts the input without creating
a child. Pause and Complete commit these changes with the ordinary report/output
transaction, and retries return the same saved child. Bluetooth receives the
opening-WIP label, original owner/product/order data and the new QR and quantities.

The report, queue action, source consumption and remainder creation commit in one
transaction. A stable request ID returns the same immutable receipt on retry.
The app saves that ID before posting, asks the server only to prepare labels,
and selects the printer after a successful receipt. Only positive remainders
enabled by the receipt's `label_settings` print automatically. Printer cancellation
or failure preserves the saved report and QR; history reprints use the saved
operation rather than creating another stock return.

Focused coverage includes all four switch combinations, source metadata, zero
and fractional kg, missing/ambiguous/stale sources, Pause/Complete, optional
materials, printer cancellation/failure, atomic balances and request replay.
Physical printer output still requires a device smoke test.
