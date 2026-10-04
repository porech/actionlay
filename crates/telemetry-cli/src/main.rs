//! `actionlay-telemetry`: prints the telemetry ActionLay reads from a video,
//! for debugging and for comparison with gopro-dashboard-overlay.
use std::{
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use actionlay_media::gpmf::read_gpmf_packets;
use actionlay_telemetry::{
    GpsPoint, LockOptions, Metric, RawPacket, Telemetry, TelemetryOptions, Value,
};
use chrono::{DateTime, SecondsFormat, Utc};
use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "actionlay-telemetry",
    version,
    about = "Reads the telemetry of a GoPro video"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print the metrics at regular intervals, or every GPS sample
    Dump {
        video: PathBuf,
        /// Seconds between rows
        #[arg(long, default_value_t = 1.0)]
        every: f64,
        #[arg(long, value_enum, default_value_t = Format::Csv)]
        format: Format,
        /// One CSV row per GPS sample; the first nine columns match
        /// gopro-to-csv's
        #[arg(long)]
        points: bool,
        #[command(flatten)]
        lock: LockArgs,
    },
    /// Summarise packets, time span and metric coverage
    Info {
        video: PathBuf,
        #[command(flatten)]
        lock: LockArgs,
    },
}

#[derive(Args)]
struct LockArgs {
    /// GPS points with a higher DOP count as unlocked
    #[arg(long, default_value_t = 10.0)]
    dop_max: f64,
    /// GPS points faster than this (km/h) count as unlocked
    #[arg(long)]
    speed_max_kmh: Option<f64>,
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Csv,
    Json,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match &cli.cmd {
        Cmd::Dump {
            video,
            every,
            format,
            points,
            lock,
        } => load(video, lock).and_then(|tel| {
            let mut out = BufWriter::new(io::stdout().lock());
            let r = if *points {
                dump_points(&tel, &mut out)
            } else if *every <= 0.0 {
                return Err("--every must be positive".into());
            } else {
                match format {
                    Format::Csv => dump_csv(&tel, *every, &mut out),
                    Format::Json => dump_json(&tel, *every, &mut out),
                }
            };
            r.and_then(|()| out.flush()).map_err(io_error)
        }),
        Cmd::Info { video, lock } => load(video, lock).and_then(|tel| {
            let mut out = BufWriter::new(io::stdout().lock());
            info(video, &tel, &mut out)
                .and_then(|()| out.flush())
                .map_err(io_error)
        }),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e.is_empty() => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("actionlay-telemetry: {e}");
            ExitCode::FAILURE
        }
    }
}

/// A closed pipe (`| head`) is not an error.
fn io_error(e: io::Error) -> String {
    if e.kind() == io::ErrorKind::BrokenPipe {
        String::new()
    } else {
        e.to_string()
    }
}

fn load(video: &Path, lock: &LockArgs) -> Result<Telemetry, String> {
    let packets = read_gpmf_packets(video).map_err(|e| format!("{}: {e}", video.display()))?;
    if packets.is_empty() {
        return Err(format!(
            "{}: no GoPro metadata (gpmd) stream",
            video.display()
        ));
    }
    let raw: Vec<RawPacket> = packets
        .into_iter()
        .map(|p| RawPacket {
            pts: p.pts,
            duration: p.duration,
            data: p.data,
        })
        .collect();
    let opts = TelemetryOptions {
        lock: LockOptions {
            dop_max: lock.dop_max,
            speed_max: lock.speed_max_kmh.map(|k| k / 3.6),
        },
    };
    let tel = Telemetry::from_gpmf_packets_with(&raw, &opts).map_err(|e| e.to_string())?;
    for w in tel.warnings() {
        eprintln!("warning: {w}");
    }
    Ok(tel)
}

/// Metrics the video has, in registry order.
fn available(tel: &Telemetry) -> Vec<Metric> {
    Metric::ALL
        .into_iter()
        .filter(|&m| tel.availability().is_available(m))
        .collect()
}

fn times(tel: &Telemetry, every: f64) -> impl Iterator<Item = f64> {
    let end = tel.duration() + 1e-9;
    (0..)
        .map(move |k| k as f64 * every)
        .take_while(move |&t| t <= end)
}

fn utc_text(u: Option<DateTime<Utc>>) -> String {
    u.map(|u| u.to_rfc3339_opts(SecondsFormat::Millis, true))
        .unwrap_or_default()
}

fn dump_csv(tel: &Telemetry, every: f64, out: &mut impl Write) -> io::Result<()> {
    let metrics = available(tel);
    let ids: Vec<&str> = metrics.iter().map(|m| m.id()).collect();
    writeln!(out, "t,utc,gps_lock,{}", ids.join(","))?;
    for t in times(tel, every) {
        let s = tel.sample(t);
        let values: Vec<String> = metrics
            .iter()
            .map(|&m| {
                s.get(m)
                    .present()
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            })
            .collect();
        writeln!(
            out,
            "{t},{},{:?},{}",
            utc_text(s.utc),
            s.gps_lock,
            values.join(",")
        )?;
    }
    Ok(())
}

fn dump_json(tel: &Telemetry, every: f64, out: &mut impl Write) -> io::Result<()> {
    let metrics = available(tel);
    writeln!(out, "[")?;
    for (i, t) in times(tel, every).enumerate() {
        let s = tel.sample(t);
        let values: Vec<String> = metrics
            .iter()
            .filter_map(|&m| match s.get(m) {
                Value::Present(v) => Some(format!("\"{}\":{{\"present\":{v}}}", m.id())),
                Value::Stale { value, age } => Some(format!(
                    "\"{}\":{{\"stale\":{value},\"age\":{age}}}",
                    m.id()
                )),
                Value::Absent => None,
            })
            .collect();
        let utc = s
            .utc
            .map(|_| format!("\"{}\"", utc_text(s.utc)))
            .unwrap_or_else(|| "null".into());
        let sep = if i == 0 { "" } else { "," };
        writeln!(
            out,
            "{sep}{{\"t\":{t},\"utc\":{utc},\"gps_lock\":\"{:?}\",\"values\":{{{}}}}}",
            s.gps_lock,
            values.join(",")
        )?;
    }
    writeln!(out, "]")
}

/// Python's `str(datetime)` for a UTC time, as gopro-to-csv prints it.
fn python_date(u: DateTime<Utc>) -> String {
    let micros = u.timestamp_subsec_micros();
    let frac = if micros == 0 {
        String::new()
    } else {
        format!(".{micros:06}")
    };
    format!("{}{frac}+00:00", u.format("%Y-%m-%d %H:%M:%S"))
}

/// A float as Python's `repr` prints it (`-20.0`, not `-20`), so the
/// columns shared with gopro-to-csv compare as text.
fn py(v: f64) -> String {
    let s = v.to_string();
    if v.is_finite() && !s.contains('.') {
        s + ".0"
    } else {
        s
    }
}

fn opt(v: Option<f64>) -> String {
    v.map(py).unwrap_or_default()
}

fn dump_points(tel: &Telemetry, out: &mut impl Write) -> io::Result<()> {
    writeln!(
        out,
        "packet,packet_index,gps_fix,date,lat,lon,dop,alt,speed,\
         t,lat_smoothed,lon_smoothed,speed_smoothed,cspeed,dist,codo,azi,cog,cgrad,accel"
    )?;
    for p in tel.gps_points() {
        let GpsPoint {
            packet,
            index,
            lock,
            derived: d,
            ..
        } = p;
        let locked = lock.is_locked();
        writeln!(
            out,
            "{packet},{index},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            lock.original_name(),
            p.utc.map(python_date).unwrap_or_default(),
            py(p.lat),
            py(p.lon),
            py(p.dop),
            opt(locked.then_some(p.alt)),
            opt(locked.then_some(p.speed2d)),
            p.t,
            opt(d.lat),
            opt(d.lon),
            opt(d.speed),
            opt(d.cspeed),
            opt(d.dist),
            opt(d.codo),
            opt(d.azi),
            opt(d.cog),
            opt(d.cgrad),
            opt(d.accel),
        )?;
    }
    Ok(())
}

fn info(video: &Path, tel: &Telemetry, out: &mut impl Write) -> io::Result<()> {
    let points = tel.gps_points();
    let locked = points.iter().filter(|p| p.lock.is_locked()).count();
    writeln!(out, "file: {}", video.display())?;
    writeln!(out, "duration: {:.3} s", tel.duration())?;
    writeln!(out, "start (UTC): {}", utc_text(tel.start_utc()))?;
    writeln!(out, "gps points: {} ({locked} locked)", points.len())?;
    writeln!(out, "warnings: {}", tel.warnings().len())?;
    writeln!(out, "{:<12} {:>8}  gaps (s)", "metric", "coverage")?;
    for m in available(tel) {
        let gaps: Vec<String> = tel
            .availability()
            .gaps(m)
            .iter()
            .map(|(a, b)| format!("{a:.3}-{b:.3}"))
            .collect();
        writeln!(
            out,
            "{:<12} {:>7.1}%  {}",
            m.id(),
            tel.availability().coverage(m) * 100.0,
            gaps.join(" ")
        )?;
    }
    Ok(())
}
