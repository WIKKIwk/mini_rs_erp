//! Synthetic research workloads. No environment loading, database or HTTP writes.
//!
//! Fixtures are prepared outside the timed region. Each workload retains its last
//! real domain result so exact fingerprints can be compared between binaries.

pub trait Workload {
    fn name(&self) -> &'static str;
    fn size(&self) -> usize;
    fn run(&mut self);
    fn fingerprint(&self) -> String;
}
pub fn workloads() -> Vec<Box<dyn Workload>> {
    let mut cases = super::service_wip::bbtree_research_workloads();
    cases.extend(super::service_audit::bbtree_research_workloads());
    cases.extend(super::service_maps::bbtree_research_workloads());
    cases
}

pub fn variant() -> &'static str {
    if cfg!(feature = "bbtree-experiment") {
        "bbtree-all"
    } else if cfg!(feature = "bbtree-wip") {
        "bbtree-wip"
    } else if cfg!(feature = "bbtree-audit") {
        "bbtree-audit"
    } else if cfg!(feature = "bbtree-snapshot") {
        "bbtree-snapshot"
    } else {
        "std"
    }
}
