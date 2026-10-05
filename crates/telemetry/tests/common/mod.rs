//! Helpers shared by the integration tests that read real GoPro files.
#![allow(dead_code)]
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use actionlay_media::gpmf::read_gpmf_packets;
use actionlay_telemetry::{RawPacket, Telemetry};

/// A public GoPro sample (see scripts/fetch-gopro-samples.sh). None, and the
/// test is skipped, when it was not downloaded — unless
/// ACTIONLAY_REQUIRE_GOPRO_SAMPLES is set, as in CI.
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

pub fn raw_packets(path: &Path) -> Vec<RawPacket> {
    read_gpmf_packets(path)
        .unwrap()
        .into_iter()
        .map(|p| RawPacket {
            pts: p.pts,
            duration: p.duration,
            data: p.data,
        })
        .collect()
}

pub fn load(name: &str) -> Option<Telemetry> {
    let path = gopro_sample(name)?;
    Some(Telemetry::from_gpmf_packets(&raw_packets(&path)).unwrap())
}

/// A reference CSV from tests/reference, as rows of column → text.
pub fn reference(file: &str) -> Vec<HashMap<String, String>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/reference")
        .join(file);
    let text = std::fs::read_to_string(&path).unwrap();
    let mut lines = text.lines().map(|l| l.trim_end_matches('\r'));
    let header: Vec<String> = lines.next().unwrap().split(',').map(String::from).collect();
    lines
        .filter(|l| !l.is_empty())
        .map(|l| {
            let cells: Vec<String> = l.split(',').map(String::from).collect();
            assert_eq!(cells.len(), header.len(), "{file}: malformed row {l}");
            header.iter().cloned().zip(cells).collect()
        })
        .collect()
}

/// A numeric cell; None when empty.
pub fn num(row: &HashMap<String, String>, col: &str) -> Option<f64> {
    let cell = row.get(col).unwrap_or_else(|| panic!("no column {col}"));
    (!cell.is_empty()).then(|| cell.parse().unwrap())
}

pub fn key(row: &HashMap<String, String>) -> (usize, usize) {
    (
        row["packet"].parse().unwrap(),
        row["packet_index"].parse().unwrap(),
    )
}

/// Mean and 95th percentile of absolute errors.
pub fn stats(mut errors: Vec<f64>) -> (f64, f64) {
    assert!(!errors.is_empty());
    errors.sort_by(f64::total_cmp);
    let mean = errors.iter().sum::<f64>() / errors.len() as f64;
    let p95 = errors[(errors.len() as f64 * 0.95) as usize];
    (mean, p95)
}

/// Angle difference in degrees, wrapped to [0, 180].
pub fn angle_diff(a: f64, b: f64) -> f64 {
    let d = (a - b).abs().rem_euclid(360.0);
    d.min(360.0 - d)
}
