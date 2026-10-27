# Preparation order list caching

The preparation master's sequence uses `/v1/mobile/preparation/orders` and
`/v1/mobile/preparation/orders/live`. Rows contain display labels and canonical
sequence positions. Opening an order still loads and authorizes its detail
through `/v1/mobile/preparation/orders/{order_id}`.

## HTTP reads

The normal page size remains 30. `cursor` advances the same content version.
`length` can request an already loaded prefix in one read (up to 10,000 rows)
without changing the page-size scope or the following cursor.

`if_version` returns HTTP 304 when the authorized list content is unchanged.
The response exposes `x-production-epoch`, `x-production-revision`,
`x-preparation-scope`, and `x-preparation-version` so a retained prefix can
advance its revision without decoding another list. `refresh=true` explicitly
rebuilds the compact catalog.

First-page reads reuse compact rows after validating a PostgreSQL tuple
fingerprint for candidate maps, product materials, calculation templates, and
the selected sequence. This also detects label and material edits that do not
advance the production revision. Responsibilities and apparatus configuration
are rechecked before returning a result. Canonical ordering and execution
visibility continue to use the existing domain rules.

## Live reads

Clients supplying `version` and `length` receive:

- `ready` or `checkpoint` when the existing content remains valid;
- `list_snapshot` with a normal prefix when its baseline cannot be reused;
- `list_delta` with `base_version`, ordered prefix `order_ids`, changed row
  `upserts`, and `removed_ids` when content changes.

Each frame includes the current window metadata. A global revision change
alone does not change the content version. The socket remains open while the
client applies updates. Clients without `version` retain the original
`ready`/`invalidate` protocol.

## Mobile cache

`PreparationOrdersWarmStore` prepares one label prefix after login, shares
pending requests with the visible page, and retains bounded RAM/disk baselines.
Keys include server, account, and permission scope. Disk restoration and HTTP
validation run independently; restored labels never replace a newer result.
Authorization rejection withdraws cached rows. Detail always obtains fresh
server authorization.

The page retains unchanged row identities and thumbnails, applies deltas
without HTTP reloads, and repairs a missing baseline with one shared request.
The existing virtual list and delayed visible thumbnails remain in use.

## Regression coverage

Backend tests cover compact-cache reuse under locked heavy tables, conditional
reads, independent metadata invalidation, canonical cursor positions, current
master permissions, and updates over one real WebSocket. PostgreSQL integration
tests require an isolated `MINI_ERP_TEST_ADMIN_DATABASE_URL`.

Mobile tests are in `preparation_orders_list_test.dart`,
`preparation_orders_warm_store_test.dart`, and
`production_queue_warm_runtime_test.dart`.
