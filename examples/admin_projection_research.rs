//! Local research benchmark. No production endpoints and no business writes.
//!
//! Run with `cargo run --locked --release --features projection-research
//! --example admin_projection_research`. The feature is disabled by default.
//! Set MINI_ERP_DATABASE_URL or ERP_RESEARCH_ENV_FILE to an isolated clone at
//! 127.0.0.1:5433/mini_rs_erp_new. Each connection enforces read-only statements.
//! ERP_RESEARCH_THREADS selects runtime threads (default 4), and
//! ERP_RESEARCH_MODES can select 0,6,7; mode 0 is required as the baseline.
//! ERP_RESEARCH_APPARATUS selects one canonical apparatus or a unique suffix
//! (default bosma_7). Full snapshots are written only when
//! ERP_RESEARCH_WRITE_SNAPSHOTS=1, under ERP_RESEARCH_OUTPUT_DIR (default data/).
//! Those snapshots contain business data and must stay out of source control.
//! ERP_RESEARCH_HEAP is a build-time switch for allocation instrumentation;
//! rebuild without it for latency measurements. CPU times are Windows-only.
//! This prototype does not implement bounded publication or admin delivery fixes.
use async_trait::async_trait;
use mini_rs_erp::core::apparatus_standard::{ApparatusId, RuntimeApparatusConfiguration};
use mini_rs_erp::core::production_map::{
    CanonicalApparatusResolver, ProductionMapError, ProductionMapService, ProductionMapStorePort,
    optimization_research as memo,
};
use mini_rs_erp::db::postgres_production_map::PostgresProductionMapStore;
use sha2::{Digest, Sha256};
use sqlx::{Row, postgres::PgPoolOptions};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

struct AllocationMeter;
const TRACK_HEAP: bool = option_env!("ERP_RESEARCH_HEAP").is_some();
static LIVE: AtomicU64 = AtomicU64::new(0);
static PEAK: AtomicU64 = AtomicU64::new(0);
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static TRAFFIC: AtomicU64 = AtomicU64::new(0);
fn allocated(size: usize) {
    let live = LIVE.fetch_add(size as u64, Ordering::Relaxed) + size as u64;
    PEAK.fetch_max(live, Ordering::Relaxed);
    ALLOCS.fetch_add(1, Ordering::Relaxed);
    TRAFFIC.fetch_add(size as u64, Ordering::Relaxed);
}
unsafe impl GlobalAlloc for AllocationMeter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if TRACK_HEAP && !ptr.is_null() {
            allocated(layout.size());
        }
        ptr
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if TRACK_HEAP && !ptr.is_null() {
            allocated(layout.size());
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe {
            System.dealloc(ptr, layout);
        }
        if TRACK_HEAP {
            LIVE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
        }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let next = unsafe { System.realloc(ptr, layout, size) };
        if TRACK_HEAP && !next.is_null() {
            LIVE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
            allocated(size);
        }
        next
    }
}
#[global_allocator]
static ALLOCATOR: AllocationMeter = AllocationMeter;

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct FileTime {
    low: u32,
    high: u32,
}
#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut std::ffi::c_void;
    fn GetProcessTimes(
        process: *mut std::ffi::c_void,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
}
#[cfg(windows)]
fn cpu_ms() -> Option<f64> {
    let (mut c, mut e, mut k, mut u) = (
        FileTime::default(),
        FileTime::default(),
        FileTime::default(),
        FileTime::default(),
    );
    assert_ne!(
        unsafe { GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u) },
        0
    );
    Some(
        (((k.high as u64) << 32 | k.low as u64) + ((u.high as u64) << 32 | u.low as u64)) as f64
            / 10_000.,
    )
}
#[cfg(not(windows))]
fn cpu_ms() -> Option<f64> {
    None
}
struct Meter {
    start: Instant,
    cpu: Option<f64>,
    live: u64,
    allocs: u64,
    traffic: u64,
}
impl Meter {
    fn start() -> Self {
        let live = LIVE.load(Ordering::Relaxed);
        PEAK.store(live, Ordering::Relaxed);
        Self {
            start: Instant::now(),
            cpu: cpu_ms(),
            live,
            allocs: ALLOCS.load(Ordering::Relaxed),
            traffic: TRAFFIC.load(Ordering::Relaxed),
        }
    }
    fn finish(&self) -> serde_json::Value {
        serde_json::json!({"heap_instrumentation":TRACK_HEAP,"wall_ms":self.start.elapsed().as_secs_f64()*1000.,
            "process_cpu_ms":cpu_ms().zip(self.cpu).map(|(end,start)|end-start),"rust_allocation_calls":ALLOCS.load(Ordering::Relaxed)-self.allocs,
            "rust_allocation_traffic_bytes":TRAFFIC.load(Ordering::Relaxed)-self.traffic,
            "rust_peak_live_bytes":PEAK.load(Ordering::Relaxed),
            "rust_peak_growth_bytes":PEAK.load(Ordering::Relaxed).saturating_sub(self.live)})
    }
}

#[derive(Clone)]
struct LocalResolver(Vec<Arc<RuntimeApparatusConfiguration>>);
#[async_trait]
impl CanonicalApparatusResolver for LocalResolver {
    async fn resolve(
        &self,
        id: &ApparatusId,
    ) -> Result<Option<Arc<RuntimeApparatusConfiguration>>, ProductionMapError> {
        Ok(self
            .0
            .iter()
            .find(|c| c.runtime.apparatus_id == *id)
            .cloned())
    }
    async fn list(&self) -> Result<Vec<Arc<RuntimeApparatusConfiguration>>, ProductionMapError> {
        Ok(self.0.clone())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(path) = std::env::var_os("ERP_RESEARCH_ENV_FILE") {
        dotenvy::from_path(path)?;
    } else {
        let _ = dotenvy::dotenv();
    }
    let threads = std::env::var("ERP_RESEARCH_THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(threads)
        .enable_all()
        .build()?
        .block_on(run(threads))
}

async fn run(threads: usize) -> Result<(), Box<dyn std::error::Error>> {
    let raw = std::env::var("MINI_ERP_DATABASE_URL")?;
    let url = reqwest::Url::parse(&raw)?;
    if url.host_str() != Some("127.0.0.1")
        || url.port() != Some(5433)
        || url.path() != "/mini_rs_erp_new"
    {
        return Err("research refuses nonlocal database".into());
    }
    let pool = PgPoolOptions::new()
        .min_connections(2)
        .max_connections(8)
        .after_connect(|c, _| {
            Box::pin(async move {
                sqlx::query("SET default_transaction_read_only=on")
                    .execute(&mut *c)
                    .await?;
                sqlx::query("SET statement_timeout='5s'").execute(c).await?;
                Ok(())
            })
        })
        .connect(&raw)
        .await?;
    let rows=sqlx::query("SELECT runtime.payload_json AS runtime_payload,
        queue.payload_json AS queue_payload,material.payload_json AS material_payload,
        capacity.payload_json AS capacity_payload FROM mini_apparatus runtime
        JOIN mini_apparatus_queue_policies queue ON queue.canonical_apparatus_id=runtime.id
        AND (queue.source_revision,queue.source_aasx_sha256)=(runtime.source_revision,runtime.source_aasx_sha256)
        JOIN mini_apparatus_material_rules material ON material.canonical_apparatus_id=runtime.id
        AND (material.source_revision,material.source_aasx_sha256)=(runtime.source_revision,runtime.source_aasx_sha256)
        JOIN mini_apparatus_capacity_profiles capacity ON capacity.canonical_apparatus_id=runtime.id
        AND (capacity.source_revision,capacity.source_aasx_sha256)=(runtime.source_revision,runtime.source_aasx_sha256)
        WHERE runtime.source_revision IS NOT NULL ORDER BY runtime.id").fetch_all(&pool).await?;
    let mut configurations = Vec::new();
    for row in rows {
        let config = RuntimeApparatusConfiguration {
            runtime: serde_json::from_value(row.get("runtime_payload"))?,
            queue: serde_json::from_value(row.get("queue_payload"))?,
            material: serde_json::from_value(row.get("material_payload"))?,
            capacity: serde_json::from_value(row.get("capacity_payload"))?,
        };
        assert!(config.has_coherent_source());
        configurations.push(Arc::new(config));
    }
    let resolver = Arc::new(LocalResolver(configurations));
    let requested_apparatus =
        std::env::var("ERP_RESEARCH_APPARATUS").unwrap_or_else(|_| "bosma_7".into());
    let apparatuses = resolver
        .0
        .iter()
        .map(|c| c.runtime.apparatus_id.to_string())
        .filter(|id| id == &requested_apparatus || id.ends_with(&requested_apparatus))
        .collect::<Vec<_>>();
    if apparatuses.len() != 1 {
        return Err("set ERP_RESEARCH_APPARATUS to one existing canonical apparatus".into());
    }
    let apparatus = apparatuses.into_iter().next().expect("one apparatus");
    let store = Arc::new(PostgresProductionMapStore::new(pool));
    let maps = store.maps().await?;
    let ids = maps.iter().map(|m| m.id.clone()).collect::<Vec<_>>();
    let batches = store.progress_batches_for_orders(&ids).await?;
    let batch_states =
        batches
            .values()
            .flatten()
            .fold(BTreeMap::<String, usize>::new(), |mut counts, b| {
                *counts.entry(format!("{:?}", b.wip_status)).or_default() += 1;
                counts
            });
    println!(
        "{}",
        serde_json::json!({"phase":"dataset","runtime_threads":threads,"maps":maps.len(),
        "heap_instrumentation":TRACK_HEAP,"progress_batches":batches.values().map(Vec::len).sum::<usize>(),"wip_status_counts":batch_states})
    );
    drop(maps);
    drop(batches);
    let mut baseline = BTreeMap::<String, serde_json::Value>::new();
    let write_snapshots = std::env::var("ERP_RESEARCH_WRITE_SNAPSHOTS").as_deref() == Ok("1");
    let output_dir = std::path::PathBuf::from(
        std::env::var_os("ERP_RESEARCH_OUTPUT_DIR")
            .unwrap_or_else(|| "data/diagnostics/admin-projection-research".into()),
    );
    if write_snapshots {
        std::fs::create_dir_all(&output_dir)?;
    }
    let selected_modes = std::env::var("ERP_RESEARCH_MODES")
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(|part| part.trim().parse::<usize>())
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_else(|| (0..=7).collect());
    if !selected_modes.contains(&0) || selected_modes.iter().any(|mode| *mode > 7) {
        return Err("research modes must include original mode 0 and stay in 0..=7".into());
    }
    for pass in 0..3 {
        let order = match pass {
            0 => [0, 1, 2, 3, 4, 5, 6, 7],
            1 => [7, 6, 5, 4, 3, 2, 0, 1],
            _ => [1, 6, 7, 5, 4, 3, 2, 0],
        };
        for mode in order {
            if !selected_modes.contains(&mode) {
                continue;
            }
            memo::set_mode(mode);
            for view in ["admin", "worker"] {
                let service = ProductionMapService::new(store.clone(), resolver.clone());
                let meter = Meter::start();
                let (snapshot, revision) = if view == "admin" {
                    service.live_snapshot_shared_with_revision().await?
                } else {
                    service
                        .worker_snapshot_shared_with_revision(&[apparatus.clone()], &[])
                        .await?
                };
                let metrics = meter.finish();
                let summaries = memo::take_summaries();
                let encode = Instant::now();
                let value = serde_json::to_value(snapshot.as_ref())?;
                let serialization_ms = encode.elapsed().as_secs_f64() * 1000.;
                let same = baseline.get(view).map_or(true, |b| b == &value);
                if !baseline.contains_key(view) {
                    baseline.insert(view.to_string(), value.clone());
                }
                let bytes = serde_json::to_vec(&value)?;
                let sha = format!("{:x}", Sha256::digest(&bytes));
                println!(
                    "{}",
                    serde_json::json!({"phase":"cold_snapshot","runtime_threads":threads,"pass":pass,
                    "mode":mode,"view":view,"metrics":metrics,"memo":summaries,"exact_json_equal":same,
                    "json_sha256":sha,"serialization_ms":serialization_ms,"json_bytes":bytes.len(),"revision":revision,
                    "maps":snapshot.maps.len(),"controls":snapshot.queue_action_controls.values().map(|v|v.len()).sum::<usize>()})
                );
                if !same {
                    return Err("projection output changed; prototype is unsuitable".into());
                }
                if pass == 0 && view == "admin" {
                    if write_snapshots {
                        std::fs::write(
                            output_dir.join(format!("snapshot-mode-{mode}-threads-{threads}.json")),
                            &bytes,
                        )?;
                    }
                    let warm = Meter::start();
                    let (warm_snapshot, _) = service.live_snapshot_shared_with_revision().await?;
                    println!(
                        "{}",
                        serde_json::json!({"phase":"warm_snapshot","mode":mode,"runtime_threads":threads,
                        "metrics":warm.finish(),"same_arc":Arc::ptr_eq(&snapshot,&warm_snapshot)})
                    );
                    service.notify_live();
                    let invalidate = Meter::start();
                    let (fresh, fresh_rev) = service.live_snapshot_shared_with_revision().await?;
                    let fresh_metrics = invalidate.finish();
                    let summaries = memo::take_summaries();
                    let same = serde_json::to_value(fresh.as_ref())? == value;
                    println!(
                        "{}",
                        serde_json::json!({"phase":"after_invalidation","mode":mode,"runtime_threads":threads,
                        "metrics":fresh_metrics,"memo":summaries,"revision":fresh_rev,"exact_json_equal":same})
                    );
                    if !same {
                        return Err("invalidation output changed".into());
                    }
                }
            }
        }
    }
    for mode in [0, 6, 7] {
        if !selected_modes.contains(&mode) {
            continue;
        }
        memo::set_mode(mode);
        let service = Arc::new(ProductionMapService::new(store.clone(), resolver.clone()));
        let meter = Meter::start();
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..8 {
            let service = service.clone();
            tasks.spawn(async move { service.live_snapshot_shared_with_revision().await });
        }
        let mut results = Vec::new();
        while let Some(result) = tasks.join_next().await {
            results.push(result??);
        }
        let metrics = meter.finish();
        let summaries = memo::take_summaries();
        let same_arc = results
            .iter()
            .all(|(s, r)| *r == results[0].1 && Arc::ptr_eq(s, &results[0].0));
        println!(
            "{}",
            serde_json::json!({"phase":"eight_concurrent_readers","mode":mode,"runtime_threads":threads,
            "metrics":metrics,"memo":summaries,"same_arc":same_arc,"readers":results.len()})
        );
        if !same_arc {
            return Err("concurrent reads disagree".into());
        }
        let burst = Arc::new(ProductionMapService::new(store.clone(), resolver.clone()));
        let notifier = burst.clone();
        let notifications = std::thread::spawn(move || {
            for _ in 0..30 {
                std::thread::sleep(Duration::from_millis(30));
                notifier.notify_live();
            }
        });
        let meter = Meter::start();
        let (snapshot, revision) = burst.live_snapshot_shared_with_revision().await?;
        let metrics = meter.finish();
        let summaries = memo::take_summaries();
        notifications.join().unwrap();
        let same = serde_json::to_value(snapshot.as_ref())? == baseline["admin"];
        println!(
            "{}",
            serde_json::json!({"phase":"revision_burst","mode":mode,"runtime_threads":threads,
            "metrics":metrics,"memo":summaries,"revision":revision,"notifications":30,
            "notification_interval_ms":30,"exact_json_equal":same})
        );
        if !same {
            return Err("burst output changed".into());
        }
    }
    Ok(())
}
