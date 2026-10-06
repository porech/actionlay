use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn sample(dir_var: &str, default: &str, name: &str, required_var: &str) -> Option<PathBuf> {
    let dir = std::env::var_os(dir_var)
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join(default));
    let path = dir.join(name);
    if path.exists() {
        return Some(path);
    }
    if std::env::var_os(required_var).is_some() {
        panic!("{} is missing", path.display());
    }
    eprintln!("sample {name} not found, skipping");
    None
}

fn gopro(name: &str) -> Option<PathBuf> {
    sample(
        "ACTIONLAY_GOPRO_SAMPLES",
        "../../samples/gopro",
        name,
        "ACTIONLAY_REQUIRE_GOPRO_SAMPLES",
    )
}

fn run(args: &[&str], video: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_actionlay-telemetry"))
        .args(args)
        .arg(video)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

/// `dump --points` reproduces gopro-to-csv's first columns (date aside,
/// whose sub-second part follows each tool's clock model).
#[test]
fn points_match_gopro_to_csv() {
    let Some(video) = gopro("hero5.mp4") else {
        return;
    };
    let ours = stdout(&run(&["dump", "--points"], &video));
    let reference = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../telemetry/tests/reference/hero5.gopro-to-csv.csv"),
    )
    .unwrap();
    let cols = |line: &str| -> Vec<String> {
        let f: Vec<&str> = line.trim_end_matches('\r').split(',').collect();
        [0, 1, 2, 4, 5, 6, 7, 8]
            .iter()
            .map(|&i| f[i].to_string())
            .collect()
    };
    let ours: Vec<Vec<String>> = ours.lines().map(cols).collect();
    let theirs: Vec<Vec<String>> = reference.lines().map(cols).collect();
    assert_eq!(ours.len(), 619);
    assert_eq!(ours, theirs);
}

#[test]
fn dump_csv_every_second() {
    let Some(video) = gopro("hero5.mp4") else {
        return;
    };
    let out = stdout(&run(&["dump", "--every", "1"], &video));
    let lines: Vec<&str> = out.lines().collect();
    assert!(
        lines[0].starts_with("t,utc,gps_lock,speed,cspeed,"),
        "{}",
        lines[0]
    );
    assert!(!lines[0].contains("hr"));
    assert_eq!(lines.len(), 1 + 35); // t = 0..=34
    assert!(
        lines[1].starts_with("0,2017-04-17T17:31:02.978Z,Lock3d,0.167,"),
        "{}",
        lines[1]
    );
}

#[test]
fn dump_json_marks_stale_values() {
    let Some(video) = gopro("hero6.mp4") else {
        return;
    };
    let out = stdout(&run(&["dump", "--format", "json", "--every", "5"], &video));
    assert!(out.starts_with("[\n{\"t\":0,"), "{out}");
    assert!(out.trim_end().ends_with(']'));
    // t = 5 is inside hero6's fix gap
    let row = out.lines().find(|l| l.contains("\"t\":5,")).unwrap();
    assert!(row.contains("\"gps_lock\":\"NoLock\""), "{row}");
    assert!(row.contains("\"lat\":{\"stale\":"), "{row}");
}

#[test]
fn info_lists_coverage() {
    let Some(video) = gopro("hero6.mp4") else {
        return;
    };
    let out = stdout(&run(&["info"], &video));
    assert!(out.contains("gps points: 417 (180 locked)"), "{out}");
    assert!(out.contains("duration: 23.023 s"), "{out}");
    let lat = out.lines().find(|l| l.starts_with("lat ")).unwrap();
    assert!(
        lat.contains("43.5%") && lat.contains("1.001-14.014"),
        "{lat}"
    );
}

#[test]
fn video_without_metadata_fails_cleanly() {
    let Some(video) = sample(
        "ACTIONLAY_SAMPLES",
        "../../samples/synthetic",
        "hevc8-1080p30-noaudio.mp4",
        "ACTIONLAY_SAMPLES", // CI sets it wherever synthetic samples exist
    ) else {
        return;
    };
    let out = run(&["dump"], &video);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("no usable camera metadata"), "{err}");
}

fn run_args(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_actionlay-telemetry"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn json_output_parses() {
    let Some(video) = gopro("hero6.mp4") else {
        return;
    };
    let out = stdout(&run(
        &["dump", "--format", "json", "--every", "0.5"],
        &video,
    ));
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    let rows = v.as_array().unwrap();
    assert_eq!(rows.len(), 47); // t = 0, 0.5 ..= 23
    assert_eq!(rows[1]["t"], 0.5);
}

#[test]
fn invalid_every_is_rejected_before_reading() {
    for bad in ["0", "-1", "NaN", "inf", "abc"] {
        let out = run_args(&["dump", "--every", bad, "missing.mp4"]);
        assert!(!out.status.success(), "{bad}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("--every") && !err.contains("missing.mp4:"),
            "{bad}: {err}"
        );
    }
}

#[test]
fn invalid_dop_max_is_rejected_before_reading() {
    for bad in ["0", "-1", "NaN", "inf", "abc"] {
        let out = run_args(&["info", "--dop-max", bad, "missing.mp4"]);
        assert!(!out.status.success(), "{bad}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("--dop-max") && !err.contains("missing.mp4:"),
            "{bad}: {err}"
        );
    }
}

#[test]
fn ffmpeg_open_warnings_are_silenced() {
    let Some(video) = gopro("hero5.mp4") else {
        return;
    };
    let out = run(&["info"], &video);
    assert!(out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("zero duration"), "{err}");
}

#[test]
fn points_conflicts_with_format_and_every() {
    for flag in [["--format", "json"], ["--every", "2"]] {
        let out = run_args(&["dump", "--points", flag[0], flag[1], "x.mp4"]);
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("cannot be used with"));
    }
}

#[test]
fn invalid_format_is_rejected() {
    let out = run_args(&["dump", "--format", "xml", "x.mp4"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid value"));
}

#[test]
fn missing_file_fails_with_message() {
    let out = run_args(&["info", "/nonexistent/video.mp4"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("actionlay-telemetry:"), "{err}");
    assert!(err.contains("/nonexistent/video.mp4"), "{err}");
}
