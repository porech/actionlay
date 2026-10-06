//! Inspect camera parser capabilities without dumping GPS/sensor samples.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::{Arc, atomic::AtomicBool};
    let path = std::env::args().nth(1).ok_or("usage: camera-check FILE")?;
    let mut file = std::fs::File::open(&path)?;
    let size = file.metadata()?.len() as usize;
    let input = telemetry_parser::Input::from_stream(
        &mut file,
        size,
        &path,
        |_| {},
        Arc::new(AtomicBool::new(false)),
    )?;
    let imu = telemetry_parser::util::normalized_imu(&input, None)?;
    let mut groups = std::collections::BTreeMap::new();
    for sample in input.samples.as_deref().unwrap_or_default() {
        for (g, tags) in sample.tag_map.as_ref().into_iter().flatten() {
            *groups.entry(format!("{g:?}")).or_insert(0) += tags.len();
        }
    }
    println!(
        "camera={} model={:?} samples={} imu={} accl={} gyro={} groups={groups:?}",
        input.camera_type(),
        input.camera_model(),
        input.samples.as_ref().map_or(0, Vec::len),
        imu.len(),
        imu.iter().filter(|p| p.accl.is_some()).count(),
        imu.iter().filter(|p| p.gyro.is_some()).count()
    );
    Ok(())
}
