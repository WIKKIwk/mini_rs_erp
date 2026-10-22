# Accord transport patches

Upstream: sockudo-ws 2.0.1, crates.io package.
Archive SHA-256: `d332a6a163a2386c9785dccb61e69c4addd69e591de1d7c8ad32680c96efd217`.

Upstream repository: https://github.com/sockudo/sockudo-ws
Upstream VCS commit recorded in the package: `5711f8580ececbe7cf93d9cc029d5eadc63dab03`.
Upstream license and package metadata are retained. The dependency remains pinned
to version `2.0.1`; this directory is an auditable local patch, not an upgrade.

## Local patches

- **RFC 7692 negotiation:** Axum negotiates the actual client constraints instead
  of discarding them. Comma-separated alternatives, repeated header fields,
  quoted parameters, duplicate/unknown/malformed parameters, the independent
  takeover directions, and explicitly offered 15-bit limits are handled. The
  response only includes `client_max_window_bits` when offered. Unsupported
  8-bit encoder offers are declined without panicking; flate2 requires at least
  9 encoder bits. An 8-bit peer's input is safely read by a 9-bit decoder.
- **Dictionary synchronization:** when context takeover is enabled, a compression
  attempt always emits its compressed result, even if it expanded. Returning an
  uncompressed frame after mutating the dictionary desynchronized subsequent
  messages. Messages below the configured threshold still skip compression
  before any dictionary mutation. Independent encodings may skip an expanded
  result. Large incompressible messages finish flushing all DEFLATE output.
- **Real shared encoders:** Axum/Tokio `Compression::Shared` selects actual
  process pools keyed by the exact negotiated configuration and endpoint role.
  Weak registry entries release unused pools. Decoders remain per connection;
  encoder state is reset between uses. Shared writer ownership is retained on
  `split()`. Axum advertises the necessary server no-takeover flag. A direct
  stream caller supplying a takeover agreement retains a dedicated encoder
  rather than silently changing that agreement.
- **Bounded decompression:** output is checked before returning, including when
  a single inflate call expands past the limit. Writable inflate output is
  bounded by the accepted payload size plus one detection byte. Compression
  headers on control/continuation frames and oversized uncompressed messages
  in a compression-enabled connection are rejected in unified and split paths.
- **Uninitialized output:** both codec directions pass `BytesMut` spare capacity
  directly to flate2's safe `compress_uninit` / `decompress_uninit` APIs. The
  upstream raw casts to initialized-byte slices are removed; only extending the
  output length by the codec's actual initialized byte count remains unsafe.
- **Handshake validation:** Axum validates a base64 16-byte key and exact
  Upgrade/Connection tokens; substring matches are rejected. No WebSocket
  subprotocol is selected by default, matching the application's previous Axum
  behavior. The explicit `.protocols([...])` builder selects only a supported
  token actually offered by the client.
- **Backpressure:** `Config.max_backpressure` now limits actual encoded pending
  bytes in unified and split Tokio writers. Exceeding it is terminal after
  encoding so a rejected frame cannot leave an otherwise reusable dictionary
  out of sync. `poll_ready` flushes at the high-water mark. Split requests also
  reserve raw queue bytes against one `max_message_size` frame budget (plus
  frame overhead), with cancellation-safe release. This raw queue budget is
  distinct from the encoded backpressure limit, so highly compressible
  snapshots are not rejected solely because they exceed the encoded limit
  before compression.

## Verification

`ACCORD_UPSTREAM.patch` contains the source-only diff against the checksum-verified
crates.io archive; only five upstream source files are patched.

The library suite includes regression cases for the above changes, real Axum
plain/compressed split heartbeats, matching pool reuse, independent clients,
split writer round trips, a two MiB incompressible message, and post-overflow
terminal state. The real-TCP heartbeat regression uses bounded real-time waits; virtual time
remains limited to in-memory transport tests to avoid socket-readiness races.

The exact application feature selection is tested with:

```sh
CARGO_TARGET_DIR=/tmp/accord-sockudo-core-target cargo test \
  --manifest-path third_party/sockudo-ws/Cargo.toml --lib \
  --no-default-features \
  --features simd,fastrand,tokio-runtime,axum-integration,permessage-deflate
```

These checks cover the HTTP/1 Tokio/Axum path used by Accord. The optional
Compio, HTTP/2, HTTP/3 and io_uring integrations are not qualified by this patch.
The external Autobahn conformance suite is not claimed as executed.
