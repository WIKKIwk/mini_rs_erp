# WIP route continuity

A produced WIP roll keeps its batch ID, printed QR, producing stage, original
destination fields, quantities, and lineage after a production map edit. QR
lookup projects the current input route without changing the stored batch.

An existing explicit destination remains authoritative while it exists. When
that destination was removed, an unclaimed Waiting roll can follow a replacement
only when its exact producing occurrence still exists, there is exactly one
immediate logical successor, and that successor contains one occurrence of the
original canonical next apparatus. An alternative group counts as one logical
successor, including maps drawn with representative edges. Machine names and
operation types never substitute for these identities.

Missing producers, ambiguous successors, missing canonical anchors, and consumed
rolls with deleted explicit destinations fail closed. Legacy records containing
only canonical producer and consumer IDs retain their existing read-only route
when both occurrences are unique and adjacent; this does not recover a supplied
deleted node.

## Lookup contract

The ordinary progress QR response keeps `batch` and adds:

```json
{
  "input_route": {
    "source_stage_node_id": "lamination_1",
    "stage_node_id": "apparatus_6",
    "consumer_apparatus_ids": ["apparatus:example:cut1", "apparatus:example:cut2"],
    "map_fingerprint": "sha256-of-the-current-map",
    "remapped": true
  },
  "input_route_error": null
}
```

Unscoped lookup may return a route error alongside the unchanged batch. Scoped
lookup validates the assigned apparatus and rejects an invalid route. The
mobile client uses server candidates when metadata is present; older servers
retain the previous explicit-node behavior. A limited or stale WIP list does
not override a successful current scoped lookup.

| Error code | Meaning |
| --- | --- |
| `wip_route_source_unresolved` | Producing occurrence cannot be proven. |
| `wip_route_destination_unresolved` | Destination or existing owned occurrence cannot be preserved. |
| `wip_route_ambiguous` | More than one safe occurrence is possible. |
| `wip_route_changed` | Map or input changed before the locked claim; scan again. |

## Claims and future edits

Start and Merge revalidate the current map fingerprint and complete source batch
under the existing order/apparatus and input-row locks. The claim adds
`payload_json.wip_route_binding` naming the exact chosen consumer occurrence;
the original source and destination fields remain history. Established Resume,
handoff, removal, output, and completion preserve that binding and session.

Map saves use the same order lock and reject edits that make a usable outstanding
input unresolved. For active legacy inputs without a binding, the exact source
and uniquely identified owned consumer must stay the same. Removing an unused
alternative is allowed when the remaining evidence still proves the route.
Already unresolved historical records do not block unrelated edits.

Existing queue policy, worker assignment, apparatus occupancy, usage, material,
and quantity checks still apply. A released eligible roll can start downstream
while its producer continues; completing the whole downstream stage retains the
existing outstanding-input and upstream-closure rules.

No migration or replacement label is required for a safely resolvable old QR.
This code does not repair arbitrary production records whose producing history
or current graph is missing or ambiguous.
