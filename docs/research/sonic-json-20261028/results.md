# JSON codec measurements — 2026-10-28

Synthetic, warm-server measurements on Apple M4, AArch64 NEON, 16 GiB RAM; Rust 1.98.1, release, `-C target-cpu=native`.

Measured source: `241081cd9dc11021955731577b56380185343d3e`. 27 process runs: 20/285/1000 orders, three profiles, three repetitions, 31 measured samples per case. Codec samples batch ten operations and alternate engine order. Router/WS tables compare medians of per-run p50 values; paired codec tables use all nine runs for each order count. Negative percentages mean less elapsed time.

Server domain/read caches are warm. Eight WS connections share one authorized worker identity/scope. HTTP gzip uses the normal level-8 zlib-rs layer; loopback WS compression is disabled. These are elapsed-time comparisons, not production throughput or WAN measurements.

## Paired codec results

| Orders | Case | Bytes | Serde p50 µs | Sonic p50 µs | Time change |
| ---: | --- | ---: | ---: | ---: | ---: |
| 20 | initial_full_snapshot | 98,486 | 64.0542 | 41.7375 | -34.84% |
| 20 | initial_worker_list_20 | 12,770 | 7.2583 | 3.9084 | -46.15% |
| 20 | initial_admin_page_20 | 6,967 | 3.8125 | 2.1375 | -43.93% |
| 20 | typed_domain_snapshot | 96,699 | 61.7541 | 44.4916 | -27.95% |
| 20 | typed_map_list | 36,901 | 30.7584 | 23.2625 | -24.37% |
| 20 | production_map_request | 1,212 | 2.3000 | 2.5334 | +10.15% |
| 20 | integer_map_request | 82 | 0.1917 | 0.2333 | +21.70% |
| 20 | worker_actual_wire_update | 472 | 0.3167 | 0.2583 | -18.44% |
| 20 | page_actual_wire_update | 627 | 0.4459 | 0.3208 | -28.06% |
| 20 | legacy_actual_wire_update | 98,522 | 66.6625 | 43.6458 | -34.53% |
| 285 | initial_full_snapshot | 1,353,262 | 878.3833 | 577.6500 | -34.24% |
| 285 | initial_worker_list_20 | 12,771 | 7.2500 | 3.8666 | -46.67% |
| 285 | initial_admin_page_20 | 7,087 | 3.9250 | 2.2667 | -42.25% |
| 285 | typed_domain_snapshot | 1,330,274 | 847.8708 | 591.8583 | -30.19% |
| 285 | typed_map_list | 525,826 | 438.6042 | 334.6250 | -23.71% |
| 285 | production_map_request | 1,212 | 2.3375 | 2.5708 | +9.98% |
| 285 | integer_map_request | 82 | 0.2042 | 0.2333 | +14.25% |
| 285 | worker_actual_wire_update | 474 | 0.3250 | 0.2750 | -15.38% |
| 285 | page_actual_wire_update | 794 | 0.5708 | 0.3917 | -31.38% |
| 285 | legacy_actual_wire_update | 1,353,297 | 886.4584 | 584.7542 | -34.03% |
| 1000 | initial_full_snapshot | 4,738,788 | 3503.2208 | 2422.4833 | -30.85% |
| 1000 | initial_worker_list_20 | 12,772 | 7.3709 | 3.9542 | -46.35% |
| 1000 | initial_admin_page_20 | 7,089 | 3.9500 | 2.2791 | -42.30% |
| 1000 | typed_domain_snapshot | 4,658,599 | 3261.6792 | 2272.7500 | -30.32% |
| 1000 | typed_map_list | 1,845,001 | 1513.6333 | 1191.5417 | -21.28% |
| 1000 | production_map_request | 1,212 | 2.3542 | 2.5791 | +9.55% |
| 1000 | integer_map_request | 82 | 0.2083 | 0.2375 | +14.02% |
| 1000 | worker_actual_wire_update | 476 | 0.3208 | 0.2667 | -16.86% |
| 1000 | page_actual_wire_update | 796 | 0.5667 | 0.3917 | -30.88% |
| 1000 | legacy_actual_wire_update | 4,738,823 | 3608.3167 | 2544.9917 | -29.47% |

The `production_map_request` has floating point fields and takes the guarded Serde fallback. The integer-only request reaches the checked Sonic parser. Decoder figures include guard overhead. All 216 paired encoder comparisons were byte-identical for these fixtures.

## Actual Router and loopback WebSocket results

| Orders | Case | Profile | Serde p50 µs | Candidate p50 µs | Time change |
| ---: | --- | --- | ---: | ---: | ---: |
| 20 | http_admin_page_20 | sonic_both | 98.79 | 88.42 | -10.50% |
| 20 | http_admin_page_20 | sonic_encode | 98.79 | 95.12 | -3.71% |
| 20 | http_full_snapshot_gzip | sonic_both | 486.58 | 421.12 | -13.45% |
| 20 | http_full_snapshot_gzip | sonic_encode | 486.58 | 426.08 | -12.43% |
| 20 | http_full_snapshot_identity | sonic_both | 317.58 | 239.58 | -24.56% |
| 20 | http_full_snapshot_identity | sonic_encode | 317.58 | 240.50 | -24.27% |
| 20 | http_production_map_put | sonic_both | 54.29 | 58.25 | +7.29% |
| 20 | http_production_map_put | sonic_encode | 54.29 | 57.58 | +6.07% |
| 20 | http_worker_list_20 | sonic_both | 65.71 | 58.21 | -11.41% |
| 20 | http_worker_list_20 | sonic_encode | 65.71 | 57.92 | -11.86% |
| 20 | http_worker_unchanged_304 | sonic_both | 16.33 | 15.21 | -6.89% |
| 20 | http_worker_unchanged_304 | sonic_encode | 16.33 | 16.21 | -0.77% |
| 20 | ws_legacy_initial_snapshot | sonic_both | 722.08 | 706.29 | -2.19% |
| 20 | ws_legacy_initial_snapshot | sonic_encode | 722.08 | 704.50 | -2.44% |
| 20 | ws_legacy_update_1_clients | sonic_both | 707.96 | 683.58 | -3.44% |
| 20 | ws_legacy_update_1_clients | sonic_encode | 707.96 | 680.29 | -3.91% |
| 20 | ws_page_update_1_clients | sonic_both | 620.00 | 619.29 | -0.11% |
| 20 | ws_page_update_1_clients | sonic_encode | 620.00 | 627.92 | +1.28% |
| 20 | ws_worker_update_1_clients | sonic_both | 687.96 | 684.17 | -0.55% |
| 20 | ws_worker_update_1_clients | sonic_encode | 687.96 | 686.12 | -0.27% |
| 20 | ws_worker_update_8_clients | sonic_both | 722.08 | 718.50 | -0.50% |
| 20 | ws_worker_update_8_clients | sonic_encode | 722.08 | 728.12 | +0.84% |
| 285 | http_admin_page_20 | sonic_both | 944.46 | 950.92 | +0.68% |
| 285 | http_admin_page_20 | sonic_encode | 944.46 | 941.08 | -0.36% |
| 285 | http_full_snapshot_gzip | sonic_both | 7217.29 | 6259.38 | -13.27% |
| 285 | http_full_snapshot_gzip | sonic_encode | 7217.29 | 6270.12 | -13.12% |
| 285 | http_full_snapshot_identity | sonic_both | 4372.33 | 3390.00 | -22.47% |
| 285 | http_full_snapshot_identity | sonic_encode | 4372.33 | 3375.83 | -22.79% |
| 285 | http_production_map_put | sonic_both | 552.50 | 553.67 | +0.21% |
| 285 | http_production_map_put | sonic_encode | 552.50 | 547.21 | -0.96% |
| 285 | http_worker_list_20 | sonic_both | 64.96 | 58.17 | -10.46% |
| 285 | http_worker_list_20 | sonic_encode | 64.96 | 58.50 | -9.94% |
| 285 | http_worker_unchanged_304 | sonic_both | 15.75 | 16.21 | +2.91% |
| 285 | http_worker_unchanged_304 | sonic_encode | 15.75 | 16.33 | +3.70% |
| 285 | ws_legacy_initial_snapshot | sonic_both | 5134.50 | 5114.58 | -0.39% |
| 285 | ws_legacy_initial_snapshot | sonic_encode | 5134.50 | 4865.96 | -5.23% |
| 285 | ws_legacy_update_1_clients | sonic_both | 10691.25 | 10467.58 | -2.09% |
| 285 | ws_legacy_update_1_clients | sonic_encode | 10691.25 | 10422.00 | -2.52% |
| 285 | ws_page_update_1_clients | sonic_both | 4113.50 | 4208.12 | +2.30% |
| 285 | ws_page_update_1_clients | sonic_encode | 4113.50 | 4110.42 | -0.07% |
| 285 | ws_worker_update_1_clients | sonic_both | 7378.83 | 7381.75 | +0.04% |
| 285 | ws_worker_update_1_clients | sonic_encode | 7378.83 | 7328.83 | -0.68% |
| 285 | ws_worker_update_8_clients | sonic_both | 7424.42 | 7408.79 | -0.21% |
| 285 | ws_worker_update_8_clients | sonic_encode | 7424.42 | 7418.83 | -0.08% |
| 1000 | http_admin_page_20 | sonic_both | 3315.04 | 3247.17 | -2.05% |
| 1000 | http_admin_page_20 | sonic_encode | 3315.04 | 3303.29 | -0.35% |
| 1000 | http_full_snapshot_gzip | sonic_both | 26807.29 | 23640.83 | -11.81% |
| 1000 | http_full_snapshot_gzip | sonic_encode | 26807.29 | 23657.88 | -11.75% |
| 1000 | http_full_snapshot_identity | sonic_both | 16512.92 | 13108.58 | -20.62% |
| 1000 | http_full_snapshot_identity | sonic_encode | 16512.92 | 13121.17 | -20.54% |
| 1000 | http_production_map_put | sonic_both | 1901.46 | 1893.25 | -0.43% |
| 1000 | http_production_map_put | sonic_encode | 1901.46 | 1930.88 | +1.55% |
| 1000 | http_worker_list_20 | sonic_both | 66.12 | 57.83 | -12.54% |
| 1000 | http_worker_list_20 | sonic_encode | 66.12 | 58.21 | -11.97% |
| 1000 | http_worker_unchanged_304 | sonic_both | 16.46 | 16.33 | -0.77% |
| 1000 | http_worker_unchanged_304 | sonic_encode | 16.46 | 16.75 | +1.77% |
| 1000 | ws_legacy_initial_snapshot | sonic_both | 17558.71 | 16798.25 | -4.33% |
| 1000 | ws_legacy_initial_snapshot | sonic_encode | 17558.71 | 16810.58 | -4.26% |
| 1000 | ws_legacy_update_1_clients | sonic_both | 42050.12 | 40423.88 | -3.87% |
| 1000 | ws_legacy_update_1_clients | sonic_encode | 42050.12 | 41086.42 | -2.29% |
| 1000 | ws_page_update_1_clients | sonic_both | 14198.79 | 14277.33 | +0.55% |
| 1000 | ws_page_update_1_clients | sonic_encode | 14198.79 | 14365.08 | +1.17% |
| 1000 | ws_worker_update_1_clients | sonic_both | 28173.17 | 28067.04 | -0.38% |
| 1000 | ws_worker_update_1_clients | sonic_encode | 28173.17 | 28104.75 | -0.24% |
| 1000 | ws_worker_update_8_clients | sonic_both | 28086.75 | 28526.58 | +1.57% |
| 1000 | ws_worker_update_8_clients | sonic_encode | 28086.75 | 28412.83 | +1.16% |

## Whole-fixture peak resident memory

This includes both codecs, domain state, caches, synthetic payloads and benchmark/client allocations. It is not isolated serializer memory.

| Orders | Profile | Median peak RSS MiB |
| ---: | --- | ---: |
| 20 | serde_json | 33.92 |
| 20 | sonic_encode | 33.95 |
| 20 | sonic_both | 34.14 |
| 285 | serde_json | 103.97 |
| 285 | sonic_encode | 104.41 |
| 285 | sonic_both | 104.70 |
| 1000 | serde_json | 798.72 |
| 1000 | sonic_encode | 806.31 |
| 1000 | sonic_both | 798.25 |

## Compatibility and interpretation

- Raw Sonic and the current Serde settings differ on 3,242 of the 10,000 deterministic float literals. The integrated guarded decoder differs on zero and preserves `-0`. This is a synthetic compatibility probe, not a real-world error rate.
- Large JSON serialization and full HTTP bootstrap improve consistently in this fixture. Small worker/page delta delivery has no convincing improvement: the encoded delta is only a few hundred bytes and view/patch construction dominates.
- Bodyless 304 requests are a control with no payload encoding. Their timing variation is a reminder not to interpret small positive/negative changes as established codec effects.
- Guarded request decoding is slower; mutation endpoints have no consistent gain. Keep the decoder on Serde. Production performance remains unestablished and both default switches remain Serde.

Raw measurements and samples: [summary.json](summary.json), [metadata.json](metadata.json), and the 27 `orders-*.json` files. Test evidence: [verification.json](verification.json). Compile/format evidence: [checks.json](checks.json).
