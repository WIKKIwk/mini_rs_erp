//! Runtime measurements shared by the monitor and the system doctor.
//!
//! A zero memory/disk total means that measurement is unavailable. `cpu_percent`
//! retains the monitor's existing meaning: normalized one-minute load pressure,
//! not a sampled percentage of CPU execution time.

use crate::core::admin::models::AdminServerMonitorRuntime;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(2);
const MIB: u64 = 1024 * 1024;

/// Collect one observation. The caller owns the polling interval.
pub async fn collect() -> AdminServerMonitorRuntime {
    let (memory, disk, load_average) =
        tokio::join!(memory_snapshot(), disk_snapshot(), load_average_snapshot(),);
    AdminServerMonitorRuntime {
        cpu_percent: cpu_pressure_percent(load_average),
        memory_percent: memory.percent,
        memory_used_mb: memory.used_mb,
        memory_total_mb: memory.total_mb,
        disk_path: disk.path,
        disk_percent: disk.percent,
        disk_used_mb: disk.used_mb,
        disk_total_mb: disk.total_mb,
        disk_available_mb: disk.available_mb,
        load_average,
        // These are instantaneous OS observations, not a timed CPU sample.
        sample_seconds: 0,
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct MemorySnapshot {
    used_mb: i64,
    total_mb: i64,
    percent: i64,
}

fn memory_from_bytes(total: u64, available: u64) -> Option<MemorySnapshot> {
    if total < MIB || available > total {
        return None;
    }
    let used = total - available;
    Some(MemorySnapshot {
        used_mb: (used / MIB) as i64,
        total_mb: (total / MIB) as i64,
        percent: ((used as f64 / total as f64) * 100.0)
            .round()
            .clamp(0.0, 100.0) as i64,
    })
}

#[cfg(target_os = "linux")]
async fn memory_snapshot() -> MemorySnapshot {
    tokio::fs::read_to_string("/proc/meminfo")
        .await
        .ok()
        .and_then(|value| parse_linux_memory(&value))
        .unwrap_or_default()
}

#[cfg(any(target_os = "linux", test))]
fn meminfo_kb(value: &str, key: &str) -> Option<u64> {
    let (_, rest) = value.lines().find_map(|line| {
        let (name, rest) = line.split_once(':')?;
        (name == key).then_some((name, rest))
    })?;
    let mut fields = rest.split_whitespace();
    let amount = fields.next()?.parse().ok()?;
    (fields.next()? == "kB").then_some(amount)
}

#[cfg(any(target_os = "linux", test))]
fn parse_linux_memory(value: &str) -> Option<MemorySnapshot> {
    let total = meminfo_kb(value, "MemTotal")?;
    let available = match meminfo_kb(value, "MemAvailable") {
        Some(available) => available,
        None if value.lines().any(|line| line.starts_with("MemAvailable:")) => return None,
        None => {
            // Older kernels do not expose MemAvailable. Exclude reclaimable
            // buffers/file cache, but retain shared memory as occupied memory.
            meminfo_kb(value, "MemFree")?
                .checked_add(meminfo_kb(value, "Buffers")?)?
                .checked_add(meminfo_kb(value, "Cached")?)?
                .checked_add(meminfo_kb(value, "SReclaimable").unwrap_or(0))?
                .checked_sub(meminfo_kb(value, "Shmem").unwrap_or(0))?
        }
    };
    memory_from_bytes(total.checked_mul(1024)?, available.checked_mul(1024)?)
}

#[cfg(target_os = "macos")]
async fn memory_snapshot() -> MemorySnapshot {
    let (total, statistics) = tokio::join!(
        command_output("/usr/sbin/sysctl", &["-n", "hw.memsize"]),
        command_output("/usr/bin/vm_stat", &[]),
    );
    total
        .and_then(|value| value.trim().parse::<u64>().ok())
        .zip(statistics)
        .and_then(|(total, statistics)| parse_macos_memory(total, &statistics))
        .unwrap_or_default()
}

#[cfg(any(target_os = "macos", test))]
fn vm_stat_pages(value: &str, key: &str) -> Option<u64> {
    value.lines().find_map(|line| {
        let (name, rest) = line.split_once(':')?;
        if name.trim() != key {
            return None;
        }
        rest.trim().trim_end_matches('.').parse().ok()
    })
}

#[cfg(any(target_os = "macos", test))]
fn parse_macos_memory(total: u64, value: &str) -> Option<MemorySnapshot> {
    let header = value.lines().next()?;
    let page_size = header
        .split_once("page size of ")?
        .1
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?;
    if page_size == 0 || !page_size.is_power_of_two() {
        return None;
    }
    let free = vm_stat_pages(value, "Pages free")?;
    let reclaimable = match vm_stat_pages(value, "File-backed pages") {
        Some(file_backed) => {
            // File-backed pages already include speculative/inactive file
            // pages. Purgeable anonymous pages are separately reclaimable.
            file_backed.checked_add(vm_stat_pages(value, "Pages purgeable")?)?
        }
        None if value
            .lines()
            .any(|line| line.starts_with("File-backed pages:")) =>
        {
            return None;
        }
        // Older vm_stat versions lack the file-backed total. These queues are
        // an approximation; do not add purgeable pages again because they may
        // already be in the inactive queue.
        None => vm_stat_pages(value, "Pages inactive")?
            .checked_add(vm_stat_pages(value, "Pages speculative")?)?,
    };
    // Compressor storage consumes RAM and stays occupied. In particular, the
    // logical "Pages stored in compressor" is not physical allocated memory.
    let available = free.checked_add(reclaimable)?.checked_mul(page_size)?;
    memory_from_bytes(total, available)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
async fn memory_snapshot() -> MemorySnapshot {
    MemorySnapshot::default()
}

async fn load_average_snapshot() -> f64 {
    #[cfg(target_os = "linux")]
    let value = tokio::fs::read_to_string("/proc/loadavg").await.ok();
    #[cfg(target_os = "macos")]
    let value = command_output("/usr/sbin/sysctl", &["-n", "vm.loadavg"]).await;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let value: Option<String> = None;
    value
        .and_then(|value| parse_load_average(&value))
        .unwrap_or(0.0)
}

fn parse_load_average(value: &str) -> Option<f64> {
    value
        .trim_start_matches(|character: char| character.is_whitespace() || character == '{')
        .split_whitespace()
        .next()?
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value >= 0.0)
}

fn cpu_pressure_percent(load_average: f64) -> i64 {
    let cores = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1) as f64;
    ((load_average / cores) * 100.0).round().clamp(0.0, 100.0) as i64
}

#[cfg(target_os = "macos")]
async fn command_output(program: &str, arguments: &[&str]) -> Option<String> {
    let mut command = Command::new(program);
    command.args(arguments);
    bounded_output(command).await
}

async fn bounded_output(mut command: Command) -> Option<String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("LC_ALL", "C")
        .kill_on_drop(true);
    // Command::output() resets stderr to piped. Spawn explicitly so stderr
    // remains discarded, and dropping the owned child on timeout kills it.
    let child = command.spawn().ok()?;
    let output = timeout(COMMAND_TIMEOUT, child.wait_with_output())
        .await
        .ok()?
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())?
}

#[derive(Debug, Default, PartialEq, Eq)]
struct DiskSnapshot {
    path: String,
    percent: i64,
    used_mb: i64,
    total_mb: i64,
    available_mb: i64,
}

async fn disk_snapshot() -> DiskSnapshot {
    let path = disk_monitor_path();
    let display_path = path.display().to_string();
    let mut command = Command::new("/bin/df");
    command.arg("-Pk").arg(path);
    bounded_output(command)
        .await
        .and_then(|value| {
            value
                .lines()
                .skip(1)
                .find_map(|line| parse_df_line(&display_path, line))
        })
        .unwrap_or_else(|| DiskSnapshot {
            path: display_path,
            ..DiskSnapshot::default()
        })
}

fn disk_monitor_path() -> PathBuf {
    let path = std::env::var_os("MINI_ERP_DISK_MONITOR_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    if path.is_absolute() {
        path
    } else {
        // An absolute operand also prevents a configured leading '-' from
        // becoming a df option, without depending on non-POSIX '--' support.
        std::env::current_dir()
            .map(|current| current.join(&path))
            .unwrap_or_else(|_| PathBuf::from("./").join(path))
    }
}

fn parse_df_line(path: &str, line: &str) -> Option<DiskSnapshot> {
    let mut fields = line.split_whitespace();
    fields.next()?; // Filesystem name; the mount point after capacity may contain spaces.
    let total_kb = fields.next()?.parse::<u64>().ok()?;
    let used_kb = fields.next()?.parse::<u64>().ok()?;
    // A filesystem can report negative available blocks after reserved space
    // is exhausted. That is a known full disk, not a missing measurement.
    let available_kb = fields.next()?.parse::<i128>().ok()?.max(0);
    let percent = fields.next()?.strip_suffix('%')?.parse::<u64>().ok()?;
    if total_kb < 1024 || available_kb > i128::from(total_kb) {
        return None;
    }
    Some(DiskSnapshot {
        path: path.to_string(),
        percent: percent.min(100) as i64,
        used_mb: (used_kb.min(total_kb) / 1024) as i64,
        total_mb: (total_kb / 1024) as i64,
        available_mb: (available_kb / 1024) as i64,
    })
}

#[cfg(test)]
#[path = "system_metrics/tests.rs"]
mod tests;
