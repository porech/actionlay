//! Timestamped activity files, independent of the camera and its frame rate.
use crate::{Metric, Telemetry};
use chrono::{DateTime, Utc};
use quick_xml::{Reader, events::Event};
use std::{collections::BTreeMap, io::BufRead, path::Path};

#[derive(Debug, thiserror::Error)]
pub enum ExternalError {
    #[error("activity file: {0}")]
    Read(String),
    #[error("the activity has no usable timestamped samples")]
    NoSamples,
    #[error("video UTC is unknown: enter the UTC time of its first frame")]
    MissingVideoTime,
    #[error("sync offset and video duration must be finite; duration must be positive")]
    InvalidSync,
    #[error("the activity does not overlap the video at this sync offset")]
    NoOverlap,
}

#[derive(Debug, Clone)]
pub struct ActivityPoint {
    pub utc: DateTime<Utc>,
    /// Distinct track segments are never interpolated together.
    pub segment: usize,
    pub values: BTreeMap<Metric, f64>,
}

#[derive(Debug, Clone)]
pub struct Activity {
    pub points: Vec<ActivityPoint>,
    pub warnings: Vec<String>,
}
impl Activity {
    pub fn read(path: &Path) -> Result<Self, ExternalError> {
        let file = std::fs::File::open(path).map_err(|e| ExternalError::Read(e.to_string()))?;
        match path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "gpx" => Self::from_gpx(std::io::BufReader::new(file)),
            "fit" => Self::from_fit(file),
            _ => Err(ExternalError::Read("expected a .gpx or .fit file".into())),
        }
    }

    pub fn from_gpx(reader: impl BufRead) -> Result<Self, ExternalError> {
        let mut xml = Reader::from_reader(reader);
        xml.config_mut().trim_text(true);
        let mut buf = Vec::new();
        let mut points = Vec::new();
        let mut warnings = Vec::new();
        let mut segment = 0;
        let mut values = None::<BTreeMap<Metric, f64>>;
        let mut utc = None;
        let mut field = String::new();
        let mut root = false;
        let mut root_closed = false;
        loop {
            match xml
                .read_event_into(&mut buf)
                .map_err(|e| ExternalError::Read(e.to_string()))?
            {
                Event::Start(e) => {
                    let name = e.local_name();
                    let name = std::str::from_utf8(name.as_ref()).unwrap_or("");
                    if name == "gpx" {
                        root = true;
                    }
                    if name == "trkseg" {
                        segment += 1;
                    }
                    if name == "trkpt" {
                        let mut v = BTreeMap::new();
                        for attr in e.attributes() {
                            let attr = attr.map_err(|e| ExternalError::Read(e.to_string()))?;
                            let metric = match attr.key.as_ref() {
                                b"lat" => Some(Metric::Lat),
                                b"lon" => Some(Metric::Lon),
                                _ => None,
                            };
                            if let Some(m) = metric {
                                let text = attr
                                    .decode_and_unescape_value(xml.decoder())
                                    .map_err(|e| ExternalError::Read(e.to_string()))?;
                                if let Ok(n) = text.parse::<f64>() {
                                    v.insert(m, n);
                                }
                            }
                        }
                        values = Some(v);
                        utc = None;
                    }
                    field = name.to_owned();
                }
                Event::Text(e) if values.is_some() => {
                    let text = e
                        .xml_content()
                        .map_err(|e| ExternalError::Read(e.to_string()))?;
                    if field == "time" {
                        utc = DateTime::parse_from_rfc3339(&text)
                            .ok()
                            .map(|u| u.with_timezone(&Utc));
                    } else if field == "fix" {
                        let fix = match text.as_ref() {
                            "none" => Some(0.0),
                            "2d" => Some(2.0),
                            "3d" | "dgps" | "pps" => Some(3.0),
                            _ => None,
                        };
                        if let Some(fix) = fix {
                            values.as_mut().unwrap().insert(Metric::GpsLock, fix);
                        }
                    } else if let Some(m) = gpx_metric(&field)
                        && let Ok(n) = text.parse::<f64>()
                    {
                        values.as_mut().unwrap().insert(m, n);
                    }
                }
                Event::End(e) if e.local_name().as_ref() == b"trkpt" => {
                    if let Some(v) = values.take() {
                        if let Some(utc) = utc.take() {
                            points.push(ActivityPoint {
                                utc,
                                segment,
                                values: v,
                            });
                        } else {
                            warnings.push("GPX point without a valid UTC timestamp skipped".into());
                        }
                    }
                    field.clear();
                }
                Event::End(e) => {
                    if e.local_name().as_ref() == b"gpx" {
                        root_closed = true;
                    }
                    field.clear();
                }
                Event::DocType(_) => {
                    return Err(ExternalError::Read(
                        "GPX document types are not supported".into(),
                    ));
                }
                Event::Eof => break,
                _ => {}
            }
            buf.clear();
        }
        if !root || !root_closed {
            return Err(ExternalError::Read("missing GPX root".into()));
        }
        Self::validated(points, warnings)
    }

    pub fn from_fit(mut reader: impl std::io::Read) -> Result<Self, ExternalError> {
        let mut records =
            fitparser::from_reader(&mut reader).map_err(|e| ExternalError::Read(e.to_string()))?;
        // FIT messages may be grouped by type rather than chronological order.
        // Apply state-changing events before sensor records at the same time.
        records.sort_by_key(|record| {
            let utc = record
                .fields()
                .iter()
                .find_map(|field| match field.value() {
                    fitparser::Value::Timestamp(t) if field.name() == "timestamp" => Some(*t),
                    _ => None,
                });
            (utc, record.kind() != fitparser::profile::MesgNum::Event)
        });
        let mut points = Vec::new();
        let mut warnings = Vec::new();
        let mut segment = 0;
        let mut gears = BTreeMap::new();
        for record in records {
            // Timer stop/start boundaries must not interpolate across a pause.
            if record.kind() == fitparser::profile::MesgNum::Event {
                for field in record.fields() {
                    if field.name() == "gear_change_data"
                        && let Ok(data) = TryInto::<f64>::try_into(field.value().clone())
                        && data.is_finite()
                        && data >= 0.0
                        && data < u32::MAX as f64
                        && data.fract() == 0.0
                    {
                        let data = data as u32;
                        // FIT Event component order: rear number/teeth, front number/teeth.
                        for (metric, teeth) in [
                            (Metric::GearRear, (data >> 8) & 255),
                            (Metric::GearFront, (data >> 24) & 255),
                        ] {
                            if teeth > 0 && teeth < 255 {
                                gears.insert(metric, f64::from(teeth));
                            }
                        }
                    }
                    if let Some(m @ (Metric::GearFront | Metric::GearRear)) =
                        fit_metric(field.name())
                        && let Ok(v) = TryInto::<f64>::try_into(field.value().clone())
                    {
                        gears.insert(m, v);
                    }
                }
                if record
                    .fields()
                    .iter()
                    .any(|f| f.name() == "event" && f.value().to_string() == "timer")
                {
                    segment += 1;
                }
                continue;
            }
            if record.kind() != fitparser::profile::MesgNum::Record {
                continue;
            }
            let mut values = gears.clone();
            let mut utc = None;
            for field in record.fields() {
                if field.name() == "timestamp"
                    && let fitparser::Value::Timestamp(t) = field.value()
                {
                    utc = Some(t.with_timezone(&Utc));
                }
                if let Some(m) = fit_metric(field.name())
                    && let Ok(mut v) = TryInto::<f64>::try_into(field.value().clone())
                {
                    if field.units() == "semicircles" {
                        v *= 180.0 / 2_f64.powi(31);
                    }
                    // Prefer enhanced FIT fields when both are present.
                    if !values.contains_key(&m) || field.name().starts_with("enhanced_") {
                        values.insert(m, v);
                    }
                }
            }
            if let Some(utc) = utc {
                points.push(ActivityPoint {
                    utc,
                    segment,
                    values,
                });
            } else {
                warnings.push("FIT record without a UTC timestamp skipped".into());
            }
        }
        Self::validated(points, warnings)
    }

    pub(crate) fn validated(
        mut points: Vec<ActivityPoint>,
        mut warnings: Vec<String>,
    ) -> Result<Self, ExternalError> {
        for p in &mut points {
            p.values.retain(|m, v| {
                v.is_finite()
                    && match m {
                        Metric::Lat => v.abs() <= 90.0,
                        Metric::Lon => v.abs() <= 180.0,
                        Metric::Speed
                        | Metric::Hr
                        | Metric::Cadence
                        | Metric::Power
                        | Metric::Odo
                        | Metric::Respiration => *v >= 0.0,
                        _ => true,
                    }
            });
            if !p.values.contains_key(&Metric::Lat) || !p.values.contains_key(&Metric::Lon) {
                p.values.remove(&Metric::Lat);
                p.values.remove(&Metric::Lon);
            }
            if p.values.get(&Metric::GpsLock) == Some(&0.0) {
                p.values.remove(&Metric::Lat);
                p.values.remove(&Metric::Lon);
                p.values.remove(&Metric::Alt);
            }
        }
        points.retain(|p| !p.values.is_empty());
        points.sort_by_key(|p| p.utc);
        let before = points.len();
        // Combine sensor-only and GPS messages at the same timestamp.
        let mut merged: Vec<ActivityPoint> = Vec::new();
        for p in points {
            if let Some(last) = merged.last_mut().filter(|q| q.utc == p.utc) {
                last.values.extend(p.values);
            } else {
                merged.push(p);
            }
        }
        if merged.len() != before {
            warnings.push("duplicate timestamps combined".into());
        }
        if merged.is_empty() {
            return Err(ExternalError::NoSamples);
        }
        Ok(Self {
            points: merged,
            warnings,
        })
    }

    /// Positive offset moves the activity later on the video timeline.
    pub fn align(
        &self,
        video_utc: Option<DateTime<Utc>>,
        duration: f64,
        offset: f64,
    ) -> Result<Telemetry, ExternalError> {
        if !duration.is_finite() || duration <= 0.0 || !offset.is_finite() {
            return Err(ExternalError::InvalidSync);
        }
        let origin = video_utc.ok_or(ExternalError::MissingVideoTime)?;
        Telemetry::from_activity(self, origin, duration, offset)
    }
    pub fn standalone(&self) -> Telemetry {
        let start = self.points[0].utc;
        let duration = (self.points.last().unwrap().utc - start).as_seconds_f64() + 1.0;
        self.align(Some(start), duration, 0.0)
            .expect("validated activity")
    }
}
fn gpx_metric(name: &str) -> Option<Metric> {
    Some(match name {
        "ele" => Metric::Alt,
        "speed" => Metric::Speed,
        "hr" => Metric::Hr,
        "cad" | "cadence" => Metric::Cadence,
        "power" | "watts" => Metric::Power,
        "atemp" | "temp" => Metric::Temp,
        "hdop" => Metric::GpsDop,
        _ => return None,
    })
}
fn fit_metric(name: &str) -> Option<Metric> {
    Some(match name {
        "position_lat" => Metric::Lat,
        "position_long" => Metric::Lon,
        "altitude" | "enhanced_altitude" => Metric::Alt,
        "speed" | "enhanced_speed" => Metric::Speed,
        "distance" => Metric::Odo,
        "heart_rate" => Metric::Hr,
        "cadence" => Metric::Cadence,
        "power" => Metric::Power,
        "temperature" => Metric::Temp,
        "front_gear" => Metric::GearFront,
        "rear_gear" => Metric::GearRear,
        "respiration_rate" => Metric::Respiration,
        _ => return None,
    })
}
