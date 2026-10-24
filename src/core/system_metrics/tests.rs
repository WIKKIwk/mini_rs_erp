use super::*;

#[test]
fn linux_memory_uses_available_instead_of_free() {
    let value = "MemTotal: 8192000 kB\nMemFree: 102400 kB\nMemAvailable: 4096000 kB\n";
    assert_eq!(
        parse_linux_memory(value),
        Some(MemorySnapshot {
            used_mb: 4000,
            total_mb: 8000,
            percent: 50,
        })
    );
}

#[test]
fn linux_old_kernel_excludes_reclaimable_file_cache() {
    let value = "MemTotal: 8192000 kB\nMemFree: 1024000 kB\nBuffers: 512000 kB\n\
                 Cached: 2048000 kB\nSReclaimable: 1024000 kB\nShmem: 512000 kB\n";
    assert_eq!(parse_linux_memory(value).unwrap().used_mb, 4000);
}

#[test]
fn linux_missing_negative_or_impossible_values_are_unknown() {
    for value in [
        "",
        "MemTotal: 8192000 kB\n",
        "MemTotal: -8192000 kB\nMemAvailable: 4096000 kB\n",
        "MemTotal: 8192000 kB\nMemAvailable: -1 kB\n",
        "MemTotal: 8192000 kB\nMemAvailable: 8192001 kB\n",
        "MemTotal: 8192000 bytes\nMemAvailable: 4096000 kB\n",
    ] {
        assert_eq!(parse_linux_memory(value), None, "{value}");
    }
}

#[test]
fn df_accepts_spaced_mount_points_and_bsd_capacity() {
    let disk = parse_df_line(
        "/Volumes/ERP Data",
        "/dev/disk3s5 8192000 4096000 2048000 67% /Volumes/ERP Data",
    )
    .unwrap();
    assert_eq!(disk.path, "/Volumes/ERP Data");
    assert_eq!(disk.total_mb, 8000);
    assert_eq!(disk.used_mb, 4000);
    assert_eq!(disk.available_mb, 2000);
    // df capacity reflects reserved/APFS space and need not equal used / total.
    assert_eq!(disk.percent, 67);
}

#[test]
fn df_invalid_totals_and_unavailable_measurements_are_unknown() {
    for value in [
        "",
        "Filesystem 1024-blocks Used Available Capacity Mounted on",
        "/dev/disk 0 0 0 0% /",
        "/dev/disk 4096 100 5000 1% /",
        "/dev/disk 4096 -100 100 1% /",
        "/dev/disk 4096 100 100 NaN% /",
    ] {
        assert_eq!(parse_df_line("/", value), None, "{value}");
    }
}

#[test]
fn df_negative_available_is_known_exhausted_space() {
    let disk = parse_df_line("/", "/dev/disk 8192000 8192000 -1024 101% /").unwrap();
    assert_eq!(disk.total_mb, 8000);
    assert_eq!(disk.used_mb, 8000);
    assert_eq!(disk.available_mb, 0);
    assert_eq!(disk.percent, 100);
}

#[test]
fn macos_uses_reported_page_size_and_keeps_compressor_occupied() {
    for page_size in [4096, 16384] {
        let statistics = format!(
            "Mach Virtual Memory Statistics: (page size of {page_size} bytes)\n\
             Pages free: 32.\n\
             File-backed pages: 24.\n\
             Pages purgeable: 8.\n\
             Pages inactive: 40.\n\
             Pages speculative: 16.\n\
             Pages occupied by compressor: 32.\n\
             Pages stored in compressor: 256.\n"
        );
        let total = page_size * 512;
        let memory = parse_macos_memory(total, &statistics).unwrap();
        // Only free + file cache + purgeable (64 pages) are available. Do not
        // add inactive/speculative again or count logical compressor pages.
        assert_eq!(memory.percent, 88);
        assert_eq!(memory.used_mb, ((total - page_size * 64) / MIB) as i64);
    }
}

#[test]
fn macos_old_format_does_not_double_count_purgeable_pages() {
    let value = "Mach Virtual Memory Statistics: (page size of 4096 bytes)\n\
                 Pages free: 32.\nPages inactive: 24.\nPages speculative: 8.\n\
                 Pages purgeable: 16.\n";
    assert_eq!(parse_macos_memory(MIB, value).unwrap().percent, 75);
}

#[test]
fn macos_missing_malformed_and_inconsistent_values_are_unknown() {
    for value in [
        "",
        "Mach Virtual Memory Statistics: (page size of 0 bytes)\n",
        "Mach Virtual Memory Statistics: (page size of 3 bytes)\n",
        "Mach Virtual Memory Statistics: (page size of 4096 bytes)\nPages free: -1.\n",
        "Mach Virtual Memory Statistics: (page size of 4096 bytes)\nPages free: 1.\n",
        "Mach Virtual Memory Statistics: (page size of 4096 bytes)\nPages free: 512.\n\
         File-backed pages: 1.\nPages purgeable: 1.\n",
    ] {
        assert_eq!(parse_macos_memory(MIB, value), None, "{value}");
    }
    let valid = "Mach Virtual Memory Statistics: (page size of 4096 bytes)\n\
                 Pages free: 1.\nFile-backed pages: 1.\nPages purgeable: 1.\n";
    assert_eq!(parse_macos_memory(0, valid), None);
}

#[test]
fn load_parses_both_platforms_and_rejects_non_finite_values() {
    assert_eq!(parse_load_average("0.50 0.10 0.05 1/50 123"), Some(0.5));
    assert_eq!(parse_load_average("{ 1.25 2.0 3.0 }"), Some(1.25));
    for value in ["", "NaN", "inf", "-0.1", "not available"] {
        assert_eq!(parse_load_average(value), None, "{value}");
    }
}
