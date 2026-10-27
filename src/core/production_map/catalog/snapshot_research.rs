//! CPU-only snapshot program-reuse benchmark. Fixture creation and initial
//! compilation are outside timing. A run includes cloning incoming maps,
//! building/searching the prior index, map equality, cached program cloning and
//! recompilation on misses. It excludes PostgreSQL, queue controls and wire I/O.

use crate::core::production_map::bbtree_research::Workload;
use sha2::{Digest, Sha256};

use super::{
    ProductionMapDefinition, ProductionMapSaved, compile_saved_maps,
    compile_saved_maps_reusing_programs, snapshot_fixture,
};

struct ProgramReuseWorkload {
    name: &'static str,
    incoming: Vec<ProductionMapDefinition>,
    previous: Vec<ProductionMapSaved>,
    last: Vec<ProductionMapSaved>,
}

#[derive(Clone, Copy)]
enum InputOrder {
    Permuted,
    Ascending,
    Descending,
}

impl ProgramReuseWorkload {
    fn new(size: usize, mixed: bool) -> Self {
        Self::with_order(size, mixed, InputOrder::Permuted)
    }

    fn with_order(size: usize, mixed: bool, order: InputOrder) -> Self {
        let mut incoming = snapshot_fixture::maps(size);
        let mut previous = compile_saved_maps(incoming.clone());
        assert_eq!(previous.len(), size, "all benchmark maps must compile");
        if mixed {
            // 80% unchanged, 10% changed complete definition, 10% newly added.
            for (index, map) in incoming.iter_mut().enumerate() {
                match index % 10 {
                    0 => map.nodes[1].qty_formula = "order_qty * 1.15".into(),
                    1 => map.id.push_str("-new"),
                    _ => {}
                }
            }
        }
        // Change only iteration order, not which definitions are modified or
        // absent from the previous revision. All sorting is fixture setup.
        if !matches!(order, InputOrder::Permuted) {
            incoming.sort_by(|left, right| left.id.cmp(&right.id));
            previous.sort_by(|left, right| left.map.id.cmp(&right.map.id));
            if matches!(order, InputOrder::Descending) {
                incoming.reverse();
                previous.reverse();
            }
        }
        Self {
            name: match (mixed, order) {
                (false, InputOrder::Permuted) => "snapshot_program_reuse_unchanged",
                (true, InputOrder::Permuted) => "snapshot_program_reuse_mixed",
                (false, InputOrder::Ascending) => "snapshot_program_reuse_unchanged_ordered",
                (true, InputOrder::Ascending) => "snapshot_program_reuse_mixed_ordered",
                (false, InputOrder::Descending) => "snapshot_program_reuse_unchanged_reverse",
                (true, InputOrder::Descending) => "snapshot_program_reuse_mixed_reverse",
            },
            incoming,
            previous,
            last: Vec::new(),
        }
    }
}

impl Workload for ProgramReuseWorkload {
    fn name(&self) -> &'static str {
        self.name
    }

    fn size(&self) -> usize {
        self.incoming.len()
    }

    fn run(&mut self) {
        self.last = std::hint::black_box(compile_saved_maps_reusing_programs(
            std::hint::black_box(self.incoming.iter().cloned()),
            Some(std::hint::black_box(self.previous.as_slice())),
        ));
    }

    fn fingerprint(&self) -> String {
        let bytes = serde_json::to_vec(&self.last).expect("saved maps serialize");
        format!("{:x}", Sha256::digest(bytes))
    }
}

pub(super) fn workloads() -> Vec<Box<dyn Workload>> {
    let mut cases: Vec<Box<dyn Workload>> = Vec::new();
    for size in [100, 391, 1_000, 5_000] {
        cases.push(Box::new(ProgramReuseWorkload::new(size, false)));
        cases.push(Box::new(ProgramReuseWorkload::new(size, true)));
    }
    for size in [391, 1_000] {
        for order in [InputOrder::Ascending, InputOrder::Descending] {
            cases.push(Box::new(ProgramReuseWorkload::with_order(
                size, false, order,
            )));
            cases.push(Box::new(ProgramReuseWorkload::with_order(
                size, true, order,
            )));
        }
    }
    cases
}
