use std::path::{Path, PathBuf};

/// Finds a sample video; returns None (test is skipped) when it is not available.
pub fn sample(name: &str) -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut dirs: Vec<PathBuf> = std::env::var_os("ACTIONLAY_SAMPLES")
        .map(PathBuf::from)
        .into_iter()
        .collect();
    dirs.push(root.join("samples/synthetic"));
    dirs.push(root.join("samples"));
    let found = dirs.into_iter().map(|d| d.join(name)).find(|p| p.exists());
    if found.is_none() {
        eprintln!("sample {name} not found, skipping");
    }
    found
}

/// A public GoPro sample (scripts/fetch-gopro-samples.sh). None, and the
/// test is skipped, when it was not downloaded — unless
/// ACTIONLAY_REQUIRE_GOPRO_SAMPLES is set, as in CI.
#[allow(dead_code)]
pub fn gopro_sample(name: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("ACTIONLAY_GOPRO_SAMPLES")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/gopro"));
    let path = dir.join(name);
    if path.exists() {
        return Some(path);
    }
    if std::env::var_os("ACTIONLAY_REQUIRE_GOPRO_SAMPLES").is_some() {
        panic!(
            "{} is missing: run scripts/fetch-gopro-samples.sh",
            path.display()
        );
    }
    eprintln!("sample {name} not found, skipping (run scripts/fetch-gopro-samples.sh)");
    None
}
