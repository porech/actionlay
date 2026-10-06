//! Adapter for normalized accelerometer, camera orientation and timestamped GPS.
//! GoPro GPMF keeps its separate, reference-tested path.
use crate::{
    Metric, Telemetry,
    external::{Activity, ActivityPoint, ExternalError},
};
use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};
use telemetry_parser::{
    Input,
    tags_impl::{GroupId, TagId, TagValue},
};

pub fn read(
    path: &Path,
    duration: f64,
    cancelled: Arc<AtomicBool>,
) -> Result<Telemetry, ExternalError> {
    if !duration.is_finite() || duration <= 0.0 {
        return Err(ExternalError::InvalidSync);
    }
    if cancelled.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(ExternalError::Read("read cancelled".into()));
    }
    std::panic::catch_unwind(|| read_inner(path, duration, cancelled))
        .unwrap_or_else(|_| Err(ExternalError::Read("camera metadata is malformed".into())))
}
fn read_inner(
    path: &Path,
    duration: f64,
    cancelled: Arc<AtomicBool>,
) -> Result<Telemetry, ExternalError> {
    let mut file = std::fs::File::open(path).map_err(|e| ExternalError::Read(e.to_string()))?;
    let size = usize::try_from(
        file.metadata()
            .map_err(|e| ExternalError::Read(e.to_string()))?
            .len(),
    )
    .map_err(|e| ExternalError::Read(e.to_string()))?;
    let input = Input::from_stream(&mut file, size, path, |_| {}, cancelled)
        .map_err(|e| ExternalError::Read(e.to_string()))?;
    if input.samples.is_none() {
        return Err(ExternalError::Read(
            "camera metadata could not be decoded".into(),
        ));
    }
    let imu = telemetry_parser::util::normalized_imu(&input, None)
        .map_err(|e| ExternalError::Read(e.to_string()))?;
    let mut tel = Telemetry::from_camera_imu(&imu, duration);
    let mut gps = Vec::new();
    let mut origin = None;
    let mut orientation = Vec::new();
    for sample in input.samples.as_ref().unwrap() {
        if let Some(tag) = sample
            .tag_map
            .as_ref()
            .and_then(|m| m.get(&GroupId::Quaternion))
            .and_then(|m| m.get(&TagId::Data))
            && let TagValue::Vec_TimeQuaternion_f64(data) = &tag.value
        {
            orientation.extend(data.get().iter().cloned());
        }
        if let Some(map) = sample.tag_map.as_ref().and_then(|m| m.get(&GroupId::GPS))
            && let Some(tag) = map.get(&TagId::Data)
            && let TagValue::Vec_GpsData(data) = &tag.value
        {
            for g in data.get() {
                let micros = g.unix_timestamp * 1e6;
                if !micros.is_finite() || micros.abs() >= i64::MAX as f64 {
                    continue;
                }
                let Some(utc) = chrono::DateTime::from_timestamp_micros(micros.round() as i64)
                else {
                    continue;
                };
                origin.get_or_insert_with(|| {
                    crate::extract::add_seconds(utc, -sample.timestamp_ms / 1000.0)
                });
                if !g.is_acquired {
                    continue;
                }
                let values = [
                    (Metric::Lat, g.lat),
                    (Metric::Lon, g.lon),
                    (Metric::Alt, g.altitude),
                    (Metric::Speed, g.speed / 3.6),
                    (Metric::Cog, g.track),
                ]
                .into_iter()
                .collect();
                gps.push(ActivityPoint {
                    utc,
                    segment: 0,
                    values,
                });
            }
        }
    }
    tel.add_camera_orientation(&orientation, duration);
    if !gps.is_empty() {
        let activity = Activity::validated(gps, Vec::new())?;
        let gps = activity.align(origin.flatten(), duration, 0.0)?;
        tel = tel.merge_external(&gps, duration);
    }
    if !Metric::ALL
        .iter()
        .any(|&m| tel.availability().is_available(m))
    {
        return Err(ExternalError::NoSamples);
    }
    Ok(tel)
}
