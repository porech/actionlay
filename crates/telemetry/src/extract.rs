//! Turns demuxed GPMF packets into timestamped samples.
//!
//! Timing: the n samples a stream carries in a packet are spread evenly over
//! the packet's `[pts, pts + duration)`: sample i starts at
//! `pts + duration·i/n` and represents the time until `pts + duration·(i+1)/n`.
//! (gopro-dashboard-overlay uses a rate regressed over the whole file on
//! HERO5–7 and the STMP clock on HERO8+; the two differ by less than 0.1 s
//! except in the final short packet, where its STMP model stretches samples
//! past the end of the file.)
use chrono::{DateTime, NaiveDate, TimeDelta, Utc};

use crate::{
    RawPacket,
    gpmf::{self, FourCc, GpmfError, Klv},
    value::GpsLock,
};

/// Values derived from the GPS track, as displayed (see `derive`).
/// None where the point is not locked or the metric is undefined.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Derived {
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub alt: Option<f64>,
    pub speed: Option<f64>,
    pub cspeed: Option<f64>,
    pub dist: Option<f64>,
    pub codo: Option<f64>,
    pub azi: Option<f64>,
    pub cog: Option<f64>,
    pub cgrad: Option<f64>,
    pub accel: Option<f64>,
}

/// One GPS sample: raw as recorded, plus lock state and derived values.
#[derive(Debug, Clone, PartialEq)]
pub struct GpsPoint {
    /// Index of the gpmd packet (gopro-to-csv's `packet`).
    pub packet: usize,
    /// Index of the sample in its packet (gopro-to-csv's `packet_index`).
    pub index: usize,
    /// File time, seconds.
    pub t: f64,
    /// The sample represents `[t, end)`.
    pub end: f64,
    pub utc: Option<DateTime<Utc>>,
    pub lat: f64,
    pub lon: f64,
    pub alt: f64,
    pub speed2d: f64,
    pub speed3d: f64,
    /// Fix code as recorded (GPSF, or GPS9's per-sample fix).
    pub fix: u32,
    pub dop: f64,
    /// Fix after the lock filter.
    pub lock: GpsLock,
    pub derived: Derived,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Sample<const N: usize> {
    pub t: f64,
    pub end: f64,
    pub v: [f64; N],
}

#[derive(Debug, Default)]
pub(crate) struct Extracted {
    pub gps: Vec<GpsPoint>,
    /// Every 10th accelerometer sample of each packet, camera axes, m/s².
    pub accl: Vec<Sample<3>>,
    /// Gravity unit vector (x, y, z) in the original's axes.
    pub grav: Vec<Sample<3>>,
    /// Orientation (pitch, roll, yaw) in degrees, original parity.
    pub ori: Vec<Sample<3>>,
    /// Camera temperature, °C, one per packet.
    pub temp: Vec<Sample<1>>,
    /// End of the last packet.
    pub duration: f64,
    pub parsed_packets: usize,
    pub first_error: Option<GpmfError>,
    pub warnings: Vec<String>,
}

const DEFAULT_ORIN: &str = "ZXY";
/// gopro-dashboard-overlay keeps 1 accelerometer sample in 10.
const ACCL_DECIMATION: usize = 10;
/// DOP reported when a stream has no GPSP.
const UNKNOWN_DOP: f64 = 99.99;

pub(crate) fn extract(packets: &[RawPacket]) -> Extracted {
    let mut out = Extracted::default();
    let mut gps9 = Vec::new();
    let mut totals = std::collections::BTreeMap::<&'static str, usize>::new();
    let mut tally = Tally::default();
    let mut notes = Vec::new();
    for (k, p) in packets.iter().enumerate() {
        out.duration = out.duration.max(p.pts + p.duration);
        let items = match gpmf::parse(&p.data) {
            Ok(items) => items,
            Err(e) => {
                out.warnings
                    .push(format!("gpmd packet {k} at {:.3} s skipped: {e}", p.pts));
                out.first_error.get_or_insert(e);
                continue;
            }
        };
        out.parsed_packets += 1;
        let mut temp_seen = false;
        for devc in items.iter().filter(|i| i.key == FourCc::new(b"DEVC")) {
            for strm in devc
                .children()
                .iter()
                .filter(|i| i.key == FourCc::new(b"STRM"))
            {
                let s = Stream::new(strm.children());
                if !temp_seen && let Some(t) = s.number(b"TMPC") {
                    out.temp.push(Sample {
                        t: p.pts,
                        end: p.pts + p.duration,
                        v: [t],
                    });
                    temp_seen = true;
                }
                let (kind, r) = if s.has(b"GPS9") {
                    ("GPS9", s.gps9(k, p, &mut gps9))
                } else if s.has(b"GPS5") {
                    ("GPS5", s.gps5(k, p, &mut out.gps))
                } else if s.has(b"ACCL") {
                    ("ACCL", s.accl(p, &mut out.accl, &mut notes))
                } else if s.has(b"GRAV") {
                    ("GRAV", s.grav(p, &mut out.grav))
                } else if s.has(b"CORI") {
                    ("CORI", s.cori(p, &mut out.ori))
                } else {
                    ("", Ok(()))
                };
                // A bad item skips that stream in this payload only; the
                // losses are tallied and reported once per kind below.
                if !kind.is_empty() {
                    *totals.entry(kind).or_default() += 1;
                }
                if let Err(e) = r {
                    tally.note(kind, "skipped", k, e.to_string());
                }
                for (key, action, reason) in notes.drain(..) {
                    tally.note(key, action, k, reason);
                }
            }
        }
    }
    tally.report(&totals, &mut out.warnings);
    // Cameras that write GPS9 (HERO11+) may also write GPS5: prefer GPS9.
    if !gps9.is_empty() {
        out.gps = gps9;
    }
    out.gps.sort_by(|a, b| a.t.total_cmp(&b.t));
    for v in [&mut out.accl, &mut out.grav, &mut out.ori] {
        v.sort_by(|a, b| a.t.total_cmp(&b.t));
    }
    out.temp.sort_by(|a, b| a.t.total_cmp(&b.t));
    out
}

/// (key, what happened, why): a problem found in one payload.
type Note = (&'static str, &'static str, String);

struct Loss {
    action: &'static str,
    count: usize,
    first_packet: usize,
    reason: String,
}

/// Payloads lost or degraded, per kind, reported once after the loop.
#[derive(Default)]
struct Tally(std::collections::BTreeMap<&'static str, Loss>);

impl Tally {
    fn note(&mut self, key: &'static str, action: &'static str, packet: usize, reason: String) {
        self.0
            .entry(key)
            .and_modify(|l| l.count += 1)
            .or_insert(Loss {
                action,
                count: 1,
                first_packet: packet,
                reason,
            });
    }

    fn report(
        &self,
        totals: &std::collections::BTreeMap<&'static str, usize>,
        out: &mut Vec<String>,
    ) {
        for (key, l) in &self.0 {
            let base = key.split(' ').next().unwrap_or(key);
            let total = totals.get(base).copied().unwrap_or(l.count);
            let msg = format!(
                "{key}: {} of {total} payloads {} ({}); first at packet {}",
                l.count, l.action, l.reason, l.first_packet
            );
            log::warn!("{msg}");
            out.push(msg);
        }
    }
}

/// Sample i of n in packet p: `(start, end)`.
fn slot(p: &RawPacket, i: usize, n: usize) -> (f64, f64) {
    let at = |j: usize| p.pts + p.duration * j as f64 / n as f64;
    (at(i), at(i + 1))
}

fn add_seconds(t: DateTime<Utc>, s: f64) -> DateTime<Utc> {
    t + TimeDelta::microseconds((s * 1e6).round() as i64)
}

/// The items of one STRM, with its sticky metadata.
struct Stream<'a> {
    items: &'a [Klv],
}

impl<'a> Stream<'a> {
    fn new(items: &'a [Klv]) -> Self {
        Stream { items }
    }

    fn get(&self, key: &[u8; 4]) -> Option<&'a Klv> {
        self.items.iter().find(|i| i.key == FourCc::new(key))
    }

    fn has(&self, key: &[u8; 4]) -> bool {
        self.get(key).is_some()
    }

    fn number(&self, key: &[u8; 4]) -> Option<f64> {
        let rows = self.get(key)?.numbers(None).ok()?;
        rows.first()?.first().copied()
    }

    fn scal(&self) -> Result<Vec<f64>, GpmfError> {
        match self.get(b"SCAL") {
            None => Ok(vec![1.0]),
            Some(s) => Ok(s.numbers(None)?.into_iter().flatten().collect()),
        }
    }

    /// Rows of `key`, scaled by SCAL, all entries of `key` concatenated.
    fn rows(&self, key: &[u8; 4], width: usize) -> Result<Vec<Vec<f64>>, GpmfError> {
        let scal = self.scal()?;
        let complex = self.get(b"TYPE").and_then(|t| t.data());
        let mut all = Vec::new();
        for item in self.items.iter().filter(|i| i.key == FourCc::new(key)) {
            let mut rows = item.numbers(complex)?;
            gpmf::apply_scale(item.key, &mut rows, &scal)?;
            if let Some(r) = rows.iter().find(|r| r.len() < width) {
                return Err(GpmfError::ScaleMismatch {
                    key: item.key,
                    scal: width,
                    elements: r.len(),
                });
            }
            all.extend(rows);
        }
        Ok(all)
    }

    fn gps5(&self, k: usize, p: &RawPacket, out: &mut Vec<GpsPoint>) -> Result<(), GpmfError> {
        let rows = self.rows(b"GPS5", 5)?;
        let fix = self.number(b"GPSF").unwrap_or(0.0) as u32;
        let dop = self.number(b"GPSP").map_or(UNKNOWN_DOP, |d| d / 100.0);
        let base = self.get(b"GPSU").and_then(Klv::utc);
        let n = rows.len();
        for (i, r) in rows.into_iter().enumerate() {
            let (t, end) = slot(p, i, n);
            out.push(GpsPoint {
                packet: k,
                index: i,
                t,
                end,
                utc: base.map(|b| add_seconds(b, t - p.pts)),
                lat: r[0],
                lon: r[1],
                alt: r[2],
                speed2d: r[3],
                speed3d: r[4],
                fix,
                dop,
                lock: GpsLock::from_fix(fix),
                derived: Derived::default(),
            });
        }
        Ok(())
    }

    /// GPS9 (HERO11+): lat, lon, alt, 2D speed, 3D speed, days since 2000,
    /// seconds of day, DOP, fix — per sample. Not yet checked on a real file.
    fn gps9(&self, k: usize, p: &RawPacket, out: &mut Vec<GpsPoint>) -> Result<(), GpmfError> {
        let rows = self.rows(b"GPS9", 9)?;
        let epoch = NaiveDate::from_ymd_opt(2000, 1, 1)
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .map(|n| n.and_utc());
        let n = rows.len();
        for (i, r) in rows.into_iter().enumerate() {
            let (t, end) = slot(p, i, n);
            let fix = r[8] as u32;
            out.push(GpsPoint {
                packet: k,
                index: i,
                t,
                end,
                utc: epoch.map(|e| add_seconds(e, r[5] * 86_400.0 + r[6])),
                lat: r[0],
                lon: r[1],
                alt: r[2],
                speed2d: r[3],
                speed3d: r[4],
                fix,
                dop: r[7],
                lock: GpsLock::from_fix(fix),
                derived: Derived::default(),
            });
        }
        Ok(())
    }

    fn accl(
        &self,
        p: &RawPacket,
        out: &mut Vec<Sample<3>>,
        notes: &mut Vec<Note>,
    ) -> Result<(), GpmfError> {
        if let Some(unit) = self.get(b"SIUN").and_then(Klv::text)
            && unit != "m/s²"
        {
            notes.push(("ACCL unit", "skipped", format!("unsupported unit {unit:?}")));
            return Ok(());
        }
        let orin = self.get(b"ORIN").and_then(Klv::text);
        let orin = match orin.as_deref() {
            Some(o) if orient(o, &[0.0; 3]).is_some() => o.to_string(),
            Some(o) => {
                notes.push((
                    "ACCL ORIN",
                    "read with the default orientation",
                    format!("unknown ORIN {o:?}, using {DEFAULT_ORIN}"),
                ));
                DEFAULT_ORIN.to_string()
            }
            None => DEFAULT_ORIN.to_string(),
        };
        let rows = self.rows(b"ACCL", 3)?;
        let n = rows.len();
        for (i, r) in rows.iter().enumerate().step_by(ACCL_DECIMATION) {
            let (t, _) = slot(p, i, n);
            let (_, end) = slot(p, (i + ACCL_DECIMATION).min(n) - 1, n);
            if let Some(v) = orient(&orin, r) {
                out.push(Sample { t, end, v });
            }
        }
        Ok(())
    }

    /// GRAV (a, b, c) → (a, −c, −b), as gopro-dashboard-overlay maps it.
    fn grav(&self, p: &RawPacket, out: &mut Vec<Sample<3>>) -> Result<(), GpmfError> {
        let rows = self.rows(b"GRAV", 3)?;
        let n = rows.len();
        for (i, r) in rows.iter().enumerate() {
            let (t, end) = slot(p, i, n);
            out.push(Sample {
                t,
                end,
                v: [r[0], -r[2], -r[1]],
            });
        }
        Ok(())
    }

    fn cori(&self, p: &RawPacket, out: &mut Vec<Sample<3>>) -> Result<(), GpmfError> {
        let rows = self.rows(b"CORI", 4)?;
        let n = rows.len();
        for (i, r) in rows.iter().enumerate() {
            let (t, end) = slot(p, i, n);
            out.push(Sample {
                t,
                end,
                v: cori_to_ori(r),
            });
        }
        Ok(())
    }
}

/// ORIN maps input columns to camera axes: the letter at position i names
/// the axis (X, Y, Z) input column i feeds; lowercase negates it. This
/// reproduces gopro-dashboard-overlay's table (ZXY, YxZ, yXZ, zxY, XzY).
pub(crate) fn orient(orin: &str, v: &[f64]) -> Option<[f64; 3]> {
    let letters = orin.as_bytes();
    if letters.len() != 3 || v.len() < 3 {
        return None;
    }
    let mut out = [0.0; 3];
    let mut seen = [false; 3];
    for (i, &c) in letters.iter().enumerate() {
        let axis = match c.to_ascii_uppercase() {
            b'X' => 0,
            b'Y' => 1,
            b'Z' => 2,
            _ => return None,
        };
        if seen[axis] {
            return None;
        }
        seen[axis] = true;
        out[axis] = if c.is_ascii_lowercase() { -v[i] } else { v[i] };
    }
    Some(out)
}

/// CORI quaternion (GPMF order w, x, y, z) → (ori.pitch, ori.roll, ori.yaw)
/// in degrees, with gopro-dashboard-overlay's quirks kept for parity: its
/// QUATERNION record reads the components as (w, x, z, y), and its Euler
/// conversion returns roll and pitch swapped.
pub(crate) fn cori_to_ori(q: &[f64]) -> [f64; 3] {
    let (w, x, y, z) = (q[0], q[1], q[3], q[2]);
    let roll = (2.0 * (w * x + y * z)).atan2(1.0 - 2.0 * (x * x + y * y));
    let sinp = 2.0 * (w * y - z * x);
    let pitch = if sinp.abs() >= 1.0 {
        std::f64::consts::FRAC_PI_2.copysign(sinp)
    } else {
        sinp.asin()
    };
    let yaw = (2.0 * (w * z + x * y)).atan2(1.0 - 2.0 * (y * y + z * z));
    [roll.to_degrees(), pitch.to_degrees(), yaw.to_degrees()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    fn packet(pts: f64, duration: f64, streams: &[Vec<u8>]) -> RawPacket {
        RawPacket {
            pts,
            duration,
            data: devc(streams),
        }
    }

    #[test]
    fn gps5_points_are_spread_over_the_packet() {
        let pts = [
            [45.0, 7.0, 100.0, 1.0, 1.1],
            [45.000_01, 7.000_01, 100.5, 2.0, 2.1],
        ];
        let p = packet(
            1.001,
            1.001,
            &[gps5_stream("170417173103.500", 3, 606, &pts)],
        );
        let ex = extract(&[p]);
        assert_eq!(ex.gps.len(), 2);
        let g = &ex.gps[1];
        assert_eq!((g.packet, g.index), (0, 1));
        assert!((g.t - 1.5015).abs() < 1e-12);
        assert!((g.end - 2.002).abs() < 1e-12);
        assert_eq!((g.lat, g.lon, g.alt), (45.000_01, 7.000_01, 100.5));
        assert_eq!((g.speed2d, g.speed3d), (2.0, 2.1));
        assert_eq!((g.fix, g.dop, g.lock), (3, 6.06, GpsLock::Lock3d));
        assert_eq!(
            g.utc.unwrap().to_rfc3339(),
            "2017-04-17T17:31:04.000500+00:00"
        );
        assert!((ex.duration - 2.002).abs() < 1e-12);
    }

    #[test]
    fn gps9_carries_time_fix_and_dop_per_sample() {
        // lat, lon, alt, 2D, 3D, days, secs, DOP, fix — TYPE "lllllllSS"
        let scal = [10_000_000, 10_000_000, 1000, 1000, 100, 1, 1000, 100, 1];
        let mut raw = i32s(&[
            450_000_000,
            70_000_000,
            100_000,
            1000,
            110,
            8_871,
            45_000_250,
        ]);
        raw.extend(150u16.to_be_bytes());
        raw.extend(3u16.to_be_bytes());
        let strm = nested(
            b"STRM",
            &[
                item(b"TYPE", b'c', 1, 9, b"lllllllSS"),
                item(b"SCAL", b'l', 4, 9, &i32s(&scal)),
                item(b"GPS9", b'?', 32, 1, &raw),
            ],
        );
        let ex = extract(&[packet(0.0, 1.0, &[strm])]);
        assert!(ex.warnings.is_empty(), "{:?}", ex.warnings);
        let g = &ex.gps[0];
        assert_eq!((g.lat, g.lon, g.alt, g.speed2d), (45.0, 7.0, 100.0, 1.0));
        assert_eq!((g.dop, g.fix, g.lock), (1.5, 3, GpsLock::Lock3d));
        // 8871 days after 2000-01-01 is 2024-04-15; 45000.25 s is 12:30:00.25
        assert_eq!(g.utc.unwrap().to_rfc3339(), "2024-04-15T12:30:00.250+00:00");
    }

    #[test]
    fn gps9_wins_over_gps5_in_the_same_file() {
        let gps5 = gps5_stream("170417173103.500", 3, 100, &[[1.0, 1.0, 1.0, 1.0, 1.0]]);
        let mut raw = i32s(&[450_000_000, 70_000_000, 0, 0, 0, 0, 0]);
        raw.extend([0, 100, 0, 3]);
        let gps9 = nested(
            b"STRM",
            &[
                item(b"TYPE", b'c', 1, 9, b"lllllllSS"),
                item(
                    b"SCAL",
                    b'l',
                    4,
                    9,
                    &i32s(&[10_000_000, 10_000_000, 1, 1, 1, 1, 1, 100, 1]),
                ),
                item(b"GPS9", b'?', 32, 1, &raw),
            ],
        );
        let ex = extract(&[packet(0.0, 1.0, &[gps5, gps9])]);
        assert_eq!(ex.gps.len(), 1);
        assert_eq!(ex.gps[0].lat, 45.0);
    }

    #[test]
    fn accl_is_decimated_reoriented_and_scaled() {
        // 20 samples: rows 0 and 10 are kept. Raw column order is (Z, X, Y)
        // by default, so (100, 200, 300)/10 → x=20, y=30, z=10.
        let mut samples = vec![[0i16; 3]; 20];
        samples[0] = [100, 200, 300];
        samples[10] = [-100, -200, -300];
        let ex = extract(&[packet(0.0, 1.0, &[accl_stream(None, 10, 41.5, &samples)])]);
        assert_eq!(ex.accl.len(), 2);
        assert_eq!(ex.accl[0].v, [20.0, 30.0, 10.0]);
        assert_eq!(ex.accl[1].v, [-20.0, -30.0, -10.0]);
        assert!((ex.accl[1].t - 0.5).abs() < 1e-12);
        assert!((ex.accl[0].end - 0.5).abs() < 1e-12);
        assert!((ex.accl[1].end - 1.0).abs() < 1e-12);
        assert_eq!(ex.temp.len(), 1);
        assert_eq!(ex.temp[0].v, [41.5]);

        let ex = extract(&[packet(
            0.0,
            1.0,
            &[accl_stream(Some("zxY"), 1, 0.0, &[[1, 2, 3]])],
        )]);
        assert_eq!(ex.accl[0].v, [-2.0, 3.0, -1.0]);
    }

    #[test]
    fn orin_rule_reproduces_the_original_table() {
        let v = [1.0, 2.0, 3.0]; // (in0, in1, in2)
        assert_eq!(orient("ZXY", &v), Some([2.0, 3.0, 1.0]));
        assert_eq!(orient("YxZ", &v), Some([-2.0, 1.0, 3.0]));
        assert_eq!(orient("yXZ", &v), Some([2.0, -1.0, 3.0]));
        assert_eq!(orient("zxY", &v), Some([-2.0, 3.0, -1.0]));
        assert_eq!(orient("XzY", &v), Some([1.0, 3.0, -2.0]));
        assert_eq!(orient("XXY", &v), None);
        assert_eq!(orient("XY", &v), None);
    }

    #[test]
    fn grav_and_cori_follow_the_original() {
        let grav = nested(
            b"STRM",
            &[
                item(b"SCAL", b's', 2, 1, &1000i16.to_be_bytes()),
                item(b"GRAV", b's', 6, 1, &i16s(&[100, 200, 300])),
            ],
        );
        // identity, then 30° about GPMF z: (w, x, y, z) = (cos 15°, 0, 0, sin 15°)
        let (c, sn) = (15f64.to_radians().cos(), 15f64.to_radians().sin());
        let q = |v: f64| (v * 32_767.0).round() as i16;
        let cori = nested(
            b"STRM",
            &[
                item(b"SCAL", b's', 2, 1, &32_767i16.to_be_bytes()),
                item(
                    b"CORI",
                    b's',
                    8,
                    2,
                    &i16s(&[q(1.0), 0, 0, 0, q(c), 0, 0, q(sn)]),
                ),
            ],
        );
        let ex = extract(&[packet(0.0, 1.0, &[grav, cori])]);
        assert_eq!(ex.grav[0].v, [0.1, -0.3, -0.2]);
        assert_eq!(ex.ori[0].v, [0.0, 0.0, 0.0]);
        // GPMF z is read as the original's y, so the turn shows up as ori.roll.
        let [pitch, roll, yaw] = ex.ori[1].v;
        assert!(
            pitch.abs() < 0.01 && (roll - 30.0).abs() < 0.01 && yaw.abs() < 0.01,
            "{:?}",
            ex.ori[1].v
        );
    }

    #[test]
    fn malformed_packets_are_skipped_and_reported() {
        let good = packet(
            0.0,
            1.0,
            &[gps5_stream("170417173103.500", 3, 100, &[[1.0; 5]])],
        );
        let mut cut = good.data.clone();
        cut.truncate(cut.len() - 3);
        let bad = RawPacket {
            pts: 1.0,
            duration: 1.0,
            data: cut,
        };
        let ex = extract(&[good, bad]);
        assert_eq!(ex.parsed_packets, 1);
        assert_eq!(ex.gps.len(), 1);
        assert!(matches!(ex.first_error, Some(GpmfError::Truncated { .. })));
        assert_eq!(ex.warnings.len(), 1);
        assert!((ex.duration - 2.0).abs() < 1e-12);
    }

    #[test]
    fn missing_gpsu_keeps_positions_without_utc() {
        let strm = nested(
            b"STRM",
            &[
                item(b"GPSF", b'L', 4, 1, &3u32.to_be_bytes()),
                item(b"GPSU", b'U', 16, 1, b"000000000000.000"),
                item(b"SCAL", b'l', 4, 1, &i32s(&[1])),
                item(b"GPS5", b'l', 20, 1, &i32s(&[45, 7, 100, 1, 1])),
            ],
        );
        let ex = extract(&[packet(0.0, 1.0, &[strm])]);
        assert_eq!(ex.gps.len(), 1);
        assert_eq!(ex.gps[0].utc, None);
        assert_eq!(ex.gps[0].dop, 99.99);
    }

    #[test]
    fn a_bad_scale_skips_only_that_stream() {
        // GPS5 with SCAL 0 in two packets, ACCL valid in both: the file
        // is still processed, GPS skipped, one warning for GPS5.
        let bad_gps = nested(
            b"STRM",
            &[
                item(b"SCAL", b'l', 4, 1, &i32s(&[0])),
                item(b"GPS5", b'l', 20, 1, &i32s(&[45, 7, 100, 1, 1])),
            ],
        );
        let accl = |pts| {
            packet(
                pts,
                1.0,
                &[
                    bad_gps.clone(),
                    accl_stream(None, 10, 30.0, &[[10, 20, 30]]),
                ],
            )
        };
        let ex = extract(&[accl(0.0), accl(1.0)]);
        assert_eq!(ex.parsed_packets, 2);
        assert!(ex.gps.is_empty());
        assert_eq!(ex.accl.len(), 2);
        assert_eq!(ex.temp.len(), 2);
        assert_eq!(ex.warnings.len(), 1, "{:?}", ex.warnings);
        assert!(ex.warnings[0].starts_with("GPS5: 2 of 2 payloads skipped"));
        assert!(ex.warnings[0].contains("first at packet 0"));
        assert!(ex.first_error.is_none());
    }

    #[test]
    fn later_good_payloads_survive_a_bad_one() {
        let bad = nested(
            b"STRM",
            &[
                item(b"SCAL", b'l', 4, 1, &i32s(&[0])),
                item(b"GPS5", b'l', 20, 1, &i32s(&[45, 7, 100, 1, 1])),
            ],
        );
        let good = |pts| {
            packet(
                pts,
                1.0,
                &[gps5_stream("170417173103.500", 3, 100, &[[1.0; 5]])],
            )
        };
        let ex = extract(&[packet(0.0, 1.0, &[bad]), good(1.0), good(2.0)]);
        assert_eq!(ex.gps.len(), 2);
        assert_eq!(ex.gps[0].packet, 1);
        assert_eq!(ex.warnings.len(), 1, "{:?}", ex.warnings);
        assert!(ex.warnings[0].starts_with("GPS5: 1 of 3 payloads skipped"));
    }

    #[test]
    fn accl_unit_and_orin_warnings_are_summarised_once() {
        let strm = |siun: &[u8; 4], orin: &str| {
            nested(
                b"STRM",
                &[
                    item(b"SIUN", b'c', 4, 1, siun),
                    item(b"SCAL", b's', 2, 1, &1i16.to_be_bytes()),
                    item(b"ORIN", b'c', 1, 3, orin.as_bytes()),
                    item(b"ACCL", b's', 6, 1, &i16s(&[1, 2, 3])),
                ],
            )
        };
        let bad_unit = [
            packet(0.0, 1.0, &[strm(b"rpm\0", "ZXY")]),
            packet(1.0, 1.0, &[strm(b"rpm\0", "ZXY")]),
        ];
        let ex = extract(&bad_unit);
        assert!(ex.accl.is_empty());
        assert_eq!(ex.warnings.len(), 1, "{:?}", ex.warnings);
        assert!(ex.warnings[0].starts_with("ACCL unit: 2 of 2 payloads skipped"));

        let bad_orin = [
            packet(0.0, 1.0, &[strm(b"m/s\xb2", "QQQ")]),
            packet(1.0, 1.0, &[strm(b"m/s\xb2", "QQQ")]),
        ];
        let ex = extract(&bad_orin);
        assert_eq!(ex.accl.len(), 2);
        assert_eq!(ex.warnings.len(), 1, "{:?}", ex.warnings);
        assert!(ex.warnings[0].starts_with("ACCL ORIN: 2 of 2 payloads"));
    }
}
