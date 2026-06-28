use std::fs;
use std::path::{Path, PathBuf};

const MAX_SOURCE_LINES: usize = 500;

#[test]
fn rust_source_files_do_not_exceed_500_lines() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut oversized = Vec::new();
    collect_oversized_rust_files(&root, &mut oversized);
    assert!(
        oversized.is_empty(),
        "500 qatordan oshgan Rust fayllar:\n{}",
        oversized
            .iter()
            .map(|(path, lines)| format!("{}: {lines}", path.display()))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn collect_oversized_rust_files(dir: &Path, oversized: &mut Vec<(PathBuf, usize)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_oversized_rust_files(&path, oversized);
            continue;
        }
        if should_skip_file_size_check(&path) {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let lines = content.lines().count();
        if lines > MAX_SOURCE_LINES {
            oversized.push((path, lines));
        }
    }
}

fn should_skip_file_size_check(path: &Path) -> bool {
    if path.extension().and_then(|value| value.to_str()) != Some("rs") {
        return true;
    }
    let relative = path
        .strip_prefix(Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
        .unwrap_or(path);
    let path_text = relative.to_string_lossy();
    path_text.contains("/tests/")
        || path_text.contains("_tests/")
        || path_text.ends_with("_tests.rs")
        || path_text.ends_with("_test.rs")
        || path_text.ends_with("_bench.rs")
}
