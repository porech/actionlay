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
