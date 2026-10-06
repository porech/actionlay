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
    about = "Reads camera telemetry and GPX/FIT activities"
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
        #[arg(long, default_value_t = 1.0, value_parser = positive_finite, allow_negative_numbers = true)]
        every: f64,
        #[arg(long, value_enum, default_value_t = Format::Csv)]
        format: Format,
        /// One CSV row per GPS sample, always CSV; eight of the first nine
        /// columns match gopro-to-csv's (the date is not compared)
        #[arg(long, conflicts_with_all = ["format", "every"])]
        points: bool,
        #[command(flatten)]
        lock: LockArgs,
        #[command(flatten)]
        sources: SourceArgs,
    },
    /// Summarise packets, time span and metric coverage
    Info {
        video: PathBuf,
        #[command(flatten)]
        lock: LockArgs,
        #[command(flatten)]
        sources: SourceArgs,
    },
}

fn positive_finite(s: &str) -> Result<f64, String> {
    let v: f64 = s.parse().map_err(|_| format!("`{s}` is not a number"))?;
    if v > 0.0 && v.is_finite() {
        Ok(v)
    } else {
        Err("must be a positive, finite number".into())
    }
}

/// A float for CSV/JSON output: non-finite values have no representation.
fn num(v: f64) -> Option<String> {
    v.is_finite().then(|| v.to_string())
}

/// `t` rounded to milliseconds, without float noise (0.30000000000000004).
fn t_text(t: f64) -> String {
    let s = format!("{t:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[derive(Args)]
struct LockArgs {
    /// GPS points with a higher DOP count as unlocked
    #[arg(long, default_value_t = 10.0, value_parser = positive_finite, allow_negative_numbers = true)]
    dop_max: f64,
    /// GPS points faster than this (km/h) count as unlocked
    #[arg(long)]
    speed_max_kmh: Option<f64>,
}

#[derive(Args)]
struct SourceArgs {
    /// Link a GPX or FIT activity by UTC
    #[arg(long)]
    activity: Option<PathBuf>,
    /// Positive seconds move activity data later in the video
    #[arg(long, default_value_t = 0.0, allow_negative_numbers = true, value_parser = finite)]
    offset: f64,
    /// UTC of the first frame, e.g. 2026-09-27T12:15:30Z
    #[arg(long)]
    video_utc: Option<String>,
    /// Open this file alone instead of joining GoPro chapters
    #[arg(long)]
    single_file: bool,
    /// Load all GoPro chapters even when an intermediate file was selected
    #[arg(long, conflicts_with = "single_file")]
    all_chapters: bool,
}
fn finite(s: &str) -> Result<f64, String> {
    s.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| "must be finite seconds".into())
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
            sources,
        } => load(video, lock, sources).and_then(|tel| {
            let mut out = BufWriter::new(io::stdout().lock());
            let r = if *points {
                dump_points(&tel, &mut out)
            } else {
                match format {
                    Format::Csv => dump_csv(&tel, *every, &mut out),
                    Format::Json => dump_json(&tel, *every, &mut out),
                }
            };
            r.and_then(|()| out.flush()).map_err(io_error)
        }),
        Cmd::Info {
            video,
            lock,
            sources,
        } => load(video, lock, sources).and_then(|tel| {
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

fn load(video: &Path, lock: &LockArgs, sources: &SourceArgs) -> Result<Telemetry, String> {
    let ext = video
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "gpx" || ext == "fit" {
        if sources.activity.is_some() {
            return Err("--activity requires a video input".into());
        }
        return actionlay_telemetry::external::Activity::read(video)
            .map(|a| a.standalone())
            .map_err(|e| e.to_string());
    }
    let join = !sources.single_file
        && (sources.all_chapters || actionlay_media::chapters::is_first_chapter(video));
    let timeline = actionlay_media::chapters::Timeline::open(video, join)
        .map_err(|e| format!("{}: {e}", video.display()))?;
    let mut raw = Vec::new();
    for chapter in &timeline.chapters {
        for p in read_gpmf_packets(&chapter.path).map_err(|e| e.to_string())? {
            raw.push(RawPacket {
                pts: p.pts + chapter.start,
                duration: p.duration,
                data: p.data,
            });
        }
    }
    let opts = TelemetryOptions {
        lock: LockOptions {
            dop_max: lock.dop_max,
            speed_max: lock.speed_max_kmh.map(|k| k / 3.6),
        },
        video_duration: (timeline.chapters.len() > 1).then_some(timeline.duration),
    };
    let mut tel = if raw.is_empty() {
        actionlay_telemetry::camera::read(
            video,
            timeline.duration,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
        .unwrap_or_else(|_| Telemetry::empty(timeline.duration))
    } else {
        Telemetry::from_gpmf_packets_with(&raw, &opts).map_err(|e| e.to_string())?
    };
    if let Some(path) = &sources.activity {
        let activity =
            actionlay_telemetry::external::Activity::read(path).map_err(|e| e.to_string())?;
        let origin = match &sources.video_utc {
            Some(s) => Some(
                DateTime::parse_from_rfc3339(s)
                    .map_err(|e| e.to_string())?
                    .with_timezone(&Utc),
            ),
            None => tel.start_utc(),
        };
        let external = activity
            .align(origin, timeline.duration, sources.offset)
            .map_err(|e| e.to_string())?;
        tel = tel.merge_external(&external, timeline.duration);
    } else if raw.is_empty()
        && !Metric::ALL
            .iter()
            .any(|&m| tel.availability().is_available(m))
    {
        return Err(format!(
            "{}: no usable camera metadata; link --activity GPX/FIT with --video-utc",
            video.display()
        ));
    }

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
            .map(|&m| s.get(m).present().and_then(num).unwrap_or_default())
            .collect();
        writeln!(
            out,
            "{},{},{:?},{}",
            t_text(t),
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
                Value::Present(v) => Some(format!("\"{}\":{{\"present\":{}}}", m.id(), json(v))),
                Value::Stale { value, age } => Some(format!(
                    "\"{}\":{{\"stale\":{},\"age\":{}}}",
                    m.id(),
                    json(value),
                    json(age)
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
            "{sep}{{\"t\":{},\"utc\":{utc},\"gps_lock\":\"{:?}\",\"values\":{{{}}}}}",
            t_text(t),
            s.gps_lock,
            values.join(",")
        )?;
    }
    writeln!(out, "]")
}

fn json(v: f64) -> String {
    num(v).unwrap_or_else(|| "null".into())
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
    if !v.is_finite() {
        return String::new();
    }
    let s = v.to_string();
    if s.contains('.') { s } else { s + ".0" }
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
