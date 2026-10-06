//! Measures metadata I/O without printing location/sensor values.
//! Usage: metadata-seek-check FILE [--prefix]
use actionlay_media::gpmf::GpmfReader;
use std::sync::{Arc, atomic::AtomicBool};
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: metadata-seek-check FILE [--prefix]"))?;
    let size = std::fs::metadata(&path)?.len();
    let start = Instant::now();
    let mut reader = GpmfReader::open(path.as_ref(), Arc::new(AtomicBool::new(false)))?;
    println!(
        "open seconds={:.3} bytes={} size={} duration={:.3}",
        start.elapsed().as_secs_f64(),
        reader.bytes_read(),
        size,
        reader.duration()
    );
    let duration = reader.duration();
    anyhow::ensure!(duration > 3.0, "missing/short container duration");
    let mut reference = None;
    for (label, at) in [
        ("near-end", duration * 0.8),
        ("middle", duration * 0.5),
        ("beginning", duration * 0.1),
        ("repeat", duration * 0.8),
    ] {
        let start = Instant::now();
        let before = reader.bytes_read();
        let packets = reader.read_range(at, at + 1.0)?;
        println!(
            "{label} target={at:.3} seconds={:.3} bytes={} packets={} payload={}",
            start.elapsed().as_secs_f64(),
            reader.bytes_read() - before,
            packets.len(),
            packets.iter().map(|p| p.data.len()).sum::<usize>()
        );
        anyhow::ensure!(!packets.is_empty(), "metadata seek returned no packets");
        anyhow::ensure!(
            packets[0].pts <= at + 1e-6 && packets.last().unwrap().pts <= at + 1.0 + 1e-6,
            "metadata seek missed requested interval"
        );
        if label == "near-end" {
            reference = Some(packets);
        } else if label == "repeat" {
            anyhow::ensure!(
                reference.as_ref() == Some(&packets),
                "repeat seek returned different metadata"
            );
        }
    }
    if std::env::args().any(|s| s == "--prefix") {
        let end = duration.min(20.0);
        let start = Instant::now();
        let before = reader.bytes_read();
        let packets = reader.read_range(0.0, end)?;
        println!(
            "prefix end={end:.3} seconds={:.3} bytes={} packets={} payload={}",
            start.elapsed().as_secs_f64(),
            reader.bytes_read() - before,
            packets.len(),
            packets.iter().map(|p| p.data.len()).sum::<usize>()
        );
    }
    println!("total_bytes={}", reader.bytes_read());
    Ok(())
}
