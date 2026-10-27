//! No-DB collection experiment. Run through tools/benchmarks/bbtree_research.py.
//! This executable never loads .env, connects to a database or starts AppState/
//! HTTP. The apparatus domain service uses a synthetic read-only repository.
use mini_rs_erp::core::production_map::bbtree_research::{variant, workloads};
use serde_json::json;
use std::time::{Duration, Instant};

#[cfg(feature = "bbtree-alloc")]
#[path = "bbtree_research/allocation.rs"]
mod allocation;

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let argument = |name: &str| {
        args.windows(2)
            .find(|pair| pair[0] == name)
            .map(|pair| pair[1].as_str())
    };
    let filter = argument("--filter").unwrap_or("");
    let size = argument("--size").map(|value| value.parse::<usize>().unwrap());
    let fixed = argument("--iterations").map(|value| value.parse::<usize>().unwrap());
    let target = Duration::from_millis(
        argument("--target-ms")
            .unwrap_or("250")
            .parse::<u64>()
            .unwrap(),
    );
    let list = args.iter().any(|arg| arg == "--list");
    let mut selected = workloads()
        .into_iter()
        .filter(|workload| {
            (filter.is_empty() || workload.name() == filter)
                && size.is_none_or(|size| size == workload.size())
        })
        .collect::<Vec<_>>();
    assert!(!selected.is_empty(), "no matching research workload");
    if list {
        println!(
            "{}",
            json!({"variant":variant(), "cases":selected.iter().map(|case|
                json!({"name":case.name(), "size":case.size()})).collect::<Vec<_>>()})
        );
        return;
    }
    let mut results = Vec::new();
    for workload in &mut selected {
        workload.run();
        let expected = workload.fingerprint();
        #[cfg(feature = "bbtree-alloc")]
        let allocation = allocation::Sample::start();
        let started = Instant::now();
        let mut iterations = 0_usize;
        loop {
            workload.run();
            iterations += 1;
            if fixed.map_or_else(
                || started.elapsed() >= target,
                |fixed| iterations >= fixed.max(1),
            ) {
                break;
            }
        }
        let elapsed = started.elapsed();
        #[cfg(feature = "bbtree-alloc")]
        let allocations = allocation.finish(iterations);
        #[cfg(not(feature = "bbtree-alloc"))]
        let allocations = serde_json::Value::Null;
        let fingerprint = workload.fingerprint();
        assert_eq!(expected, fingerprint, "non-deterministic workload output");
        results.push(json!({
            "name":workload.name(), "size":workload.size(),
            "iterations":iterations, "elapsed_ns":elapsed.as_nanos(),
            "ns_per_iteration":elapsed.as_secs_f64()*1e9/iterations as f64,
            "fingerprint":fingerprint, "allocations":allocations,
        }));
    }
    println!(
        "{}",
        json!({"schema":1, "variant":variant(),
            "instrumented":cfg!(feature="bbtree-alloc"), "results":results})
    );
}
