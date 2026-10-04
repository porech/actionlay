# M1 – Telemetry: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Read the telemetry of GoPro videos in pure Rust (GPMF KLV parser, GPS5/GPS9, accelerometer, gravity, orientation, temperature), derive the metrics gopro-dashboard-overlay shows (cspeed, dist, codo, azi/cog, cgrad, accel), sample every metric at any file time with Present/Stale/Absent semantics and per-metric coverage, and prove the values against the original on public GoPro samples, with a dump CLI for debugging.

**Architecture:** `actionlay-media` gains `gpmf::read_gpmf_packets` (FFmpeg demux of the `gpmd` stream, all other streams discarded). A new `actionlay-telemetry` crate (no FFmpeg, no other ActionLay crate) parses the packets (`gpmf`), extracts timestamped samples (`extract`), applies the GPS lock filter (`lock`), derives metrics exactly like the original's dashboard pipeline (`derive`, `smoothing`), and serves them through `Telemetry::sample(t) -> Snapshot` with `Availability` (`series`, `telemetry`). A new `actionlay-telemetry-cli` crate builds the `actionlay-telemetry` binary (`dump`, `info`) on top of both. Reference CSVs produced once with gopro-dashboard-overlay 0.134.0 are committed and compared in tests.

**Tech Stack:** Rust 1.98.1 (edition 2024), `chrono` 0.4.45, `geographiclib-rs` 0.2.7 (MIT, Karney's WGS84 geodesics, as the original), `thiserror` 2, `log` 0.4, `clap` 4.6.7 (CLI only), FFmpeg n9.0.2 through `ffmpeg-next` 9.0 (media only). Reference tool: gopro-dashboard-overlay 0.134.0 (Python, development only).

**Spec:** `docs/superpowers/specs/2026-10-04-actionlay-design.md` (§2 `telemetry` crate, §3 Time, §4.4 metrics and availability, §4.4.1 empty states, §8 tests, §10 phase M1). Interface contract with M2: `.superpowers/m1m2/interface-contract.md`.

## Contract with M2: additions and clarifications (no renames, no signature changes)

Every name and signature of the contract is implemented as written. This plan **adds**:

- `Telemetry::from_gpmf_packets_with(&[RawPacket], &TelemetryOptions)`, `TelemetryOptions { lock: LockOptions }`, `LockOptions { dop_max: f64, speed_max: Option<f64> }`.
- `Telemetry::gps_points() -> &[GpsPoint]` (raw + derived values per GPS sample, used by the dump CLI and the reference tests), `Telemetry::warnings() -> &[String]`, `GpsPoint`, `Derived`.
- `TrackPoint { t, lat, lon, alt }` (the contract left its fields open).
- `Availability::is_available(m)`, `Metric::COUNT`, `Metric::ALL`, `Metric::index()`, `Metric::is_external()`, `Value::present()`, `Value::last_known()`, `GpsLock::{from_fix, is_locked, code, original_name}`.
- `units::Unit::None` (id `none`, symbol `""`) for dimensionless metrics (`gps-dop`, `gps-lock`, `grav.*`), `Unit::{id, from_id}`, `units::units_for(Quantity)`, `units::STANDARD_GRAVITY`.
- `actionlay_media::gpmf::fill_missing_durations` (public helper next to `read_gpmf_packets`).

It **clarifies**:

- `Snapshot` stores its values as `[Value; Metric::COUNT]` indexed by `Metric::index()` (= `m as usize`), so the M2 plan's `Snapshot::for_test` ("representation A") compiles as written; every public enum derives `Debug, Clone, Copy, PartialEq` (+ `Eq, Hash` where possible) and `Telemetry` is `Send + Sync`, as M2 requires.

- **Base units** returned by `Snapshot::get`: SI, except angles and coordinates in degrees, temperature in °C, gradient in percent; `grav.*` is a unit vector in g (dimensionless), as in the original.
- **`Value::Absent`** means "no valid sample at or before t": the metric is missing from the file *or* t is before its first valid sample (there is no last value to show). Whether a metric exists in the video at all is `availability().coverage(m) > 0` — M3/M4 must use that, not `Absent`, for "hide when there is no data in this video".
- `odo` = `codo` and `gradient` = `cgrad` for GoPro files (the original falls back the same way); `gps-lock` reports the fix *after* the lock filter (0, 2, 3; 1 = unknown).
- `Telemetry::duration()` is the end of the last GPMF packet; coverage and gaps are measured on `[0, duration()]`, assuming the metadata track spans the video (true on every sample).
- The "haversine" wording of the M1 brief is replaced by the original's actual algorithm: WGS84 geodesic inverse (Karney) via `geographiclib-rs`.
- Spec §4.4 also lists `timestamp`, `gps-packet` and `gps-packet-index`; like the contract, `Metric` leaves them out: the time of day is `Snapshot::utc`, and packet/index are debugging data available through `gps_points()` (M3 can add them if an imported layout uses them).
- `temp` is the camera temperature (TMPC), as the contract says; in the original `temp` is the ambient temperature of a FIT/GPX file, which M6 will merge in.

## Deliberate differences from gopro-dashboard-overlay (each tested against our own definition)

1. **Sample timing**: the n samples of a stream in a packet are spread evenly over `[pts, pts + duration)`. The original regresses one rate over the file (HERO5–7) or uses STMP (HERO8+); the difference is < 0.1 s except in the final short packet, which the original stretches past the end of the file (max-heromode: 0.41 s).
2. **Speed limit off by default**: the original marks points faster than 60 km/h as unlocked by default, which blanks every car or motorbike video. `LockOptions::speed_max` defaults to `None` (the dump CLI exposes `--speed-max-kmh`). The samples never exceed 27 km/h, so this does not affect the comparison.
3. **Collided samples kept**: on HERO8/MAX the original drops ~2 samples per file whose timestamps fall out of order; we keep them.
4. **Short tracks**: with fewer than 108 GPS samples (6 s) the original pairs samples with negative (wrapped-around) indices; we skip those pairs.
5. **Sampling between GPS points**: `sample(t)` interpolates linearly between neighbouring valid samples (the original holds the previous one); azimuth, course, orientation, DOP and lock are held.
6. **Unlocked positions** are not shown: lat/lon become Stale/Absent where the original keeps drawing the raw position.
7. **GPS records without a valid GPSU** keep their positions with `utc: None` (the original drops the whole record).

## Global Constraints

- Project license: GPL-3.0-or-later. New dependencies must be GPL-compatible: `chrono` (MIT/Apache-2.0), `geographiclib-rs` (MIT), `clap` (MIT/Apache-2.0).
- `actionlay-telemetry` has **no normal dependency on FFmpeg or on any other ActionLay crate** (CI checks `cargo tree -e normal`). Only its integration tests use `actionlay-media` (dev-dependency); `cargo test --workspace` needs `FFMPEG_DIR` anyway.
- Rust edition 2024, toolchain 1.98.1; before any cargo command: `source scripts/env.sh`.
- Defaults: DOP limit **10** (`--gps-dop-max` of the original); window **S = 54** samples (18 Hz × 3 s); SES α = **0.45**; Kalman **R = 100, Q = 10, P₀ = 0**; cgrad accepted when distance **> 1 m** and |gradient| **< 45 %**; accelerometer kept **1 sample in 10**; interpolation bridge **≤ 2.0 s**.
- Test samples: only the public GoPro files in `samples/gopro/` (Apache-2.0, fetched by `scripts/fetch-gopro-samples.sh` from gpmf-parser commit **`9a7150632892c7356c91145c889016f07b0ed48d`**, SHA-256 checked) and the synthetic M0 samples. **`samples/GX013370.MP4` and `samples/hero7-GX013370.gpmd.bin` must never be used by tests or CI**, nor any data extracted from them committed (CI greps for the name).
- Reference values come from gopro-dashboard-overlay **0.134.0** (`pip install gopro-overlay==0.134.0`), generated once and committed under `crates/telemetry/tests/reference/`.
- CI: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace -- --test-threads=1`; with `ACTIONLAY_REQUIRE_GOPRO_SAMPLES=1` a missing GoPro sample fails a test instead of skipping it.
- Language: code, comments, commit messages in English.
- Every commit ends with:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01JdSQ8H18o7xo5KZi58mp5c
  ```

## Review Focus

1. **Video without a `gpmd` stream** (any non-GoPro MP4): no error, empty telemetry, CLI fails with a clear message → Task 2 (`file_without_metadata_has_no_packets`), Task 9 (`empty_has_nothing`), Task 11 (`video_without_metadata_fails_cleanly`).
2. **GPS never locked** (hero7/hero8: fix 0, DOP 99.99 throughout): positions and derived metrics Absent everywhere with coverage 0, while `gps-lock`, `gps-dop`, the IMU and the date still work → Task 9 (`never_locked_gps_is_absent_but_reports_lock_and_dop`), Task 10 (`hero7_and_hero8_never_lock`).
3. **Fix lost and regained** (hero6: 3D → none for 13 s → 2D → 3D): Stale with the right age inside the gap, exact gap bounds, coverage 43.5 %, and the lock heuristic that ignores a "fix" repeating the last unlocked reading → Task 6 (`fix_repeating_the_unlocked_reading_is_ignored`), Task 9 (`lost_fix_is_a_stale_gap`), Task 10 (`hero6_fix_gap_is_stale_then_recovers`).
4. **Malformed or truncated KLV**: never a panic; the damaged packet is skipped with a warning and the rest of the file loads; all packets unreadable → `TelemetryError::Unreadable` → Task 4 (`rejects_truncated_input_at_every_length`), Task 5 (`malformed_packets_are_skipped_and_reported`), Task 10 (`damaged_packets_are_skipped`).
5. **Uneven sample counts and the short final packet** (17–20 GPS samples per packet; max-heromode ends with 10 samples in 0.533 s): timestamps strictly increasing and inside the file → Task 2 (`keeps_the_short_final_packet_duration`), Task 5 (`gps5_points_are_spread_over_the_packet`), Task 10 (`max_short_final_packet_stays_inside_the_file`).

Out of scope, by design: **GoPro chapters** (GX01…/GX02…) are M6 — in M1 each file is read on its own and its telemetry starts at its own t = 0. **Timelapse/TimeWarp** file-time ↔ real-time conversion (spec §2) is not done: `Snapshot::utc = start_utc + t` assumes a real-time recording. **GPS9** (HERO11+) is implemented from the GPMF specification and tested on a hand-built packet only — no public HERO11+ sample was available.

---

## File structure

```
Cargo.toml                                   + members telemetry, telemetry-cli; + chrono, geographiclib-rs, clap
.gitignore                                   + carve-out for samples/gopro/README.md
.github/workflows/ci.yml                     + GoPro samples cache/fetch, private-sample guard, FFmpeg-free check, CLI binary
README.md                                    M1 status, tests, credits
scripts/fetch-gopro-samples.sh               pinned download + SHA-256 check
scripts/reference/gpo-dashboard-reference.py dev tool: dashboard values from gopro-dashboard-overlay
samples/gopro/README.md                      attribution, license, what each sample covers
crates/media/
  src/lib.rs                                 + pub mod gpmf
  src/gpmf.rs                                GpmfPacket, read_gpmf_packets, fill_missing_durations
  tests/common/mod.rs                        + gopro_sample()
  tests/gpmf.rs
crates/telemetry/
  Cargo.toml
  src/lib.rs                                 RawPacket, re-exports
  src/units.rs                               Quantity, Unit, UnitSystem, convert, symbol
  src/metric.rs                              Metric registry
  src/value.rs                               Value, GpsLock
  src/gpmf.rs                                KLV parser
  src/test_support.rs                        KLV builders for unit tests
  src/extract.rs                             packets → timestamped samples, GpsPoint
  src/smoothing.rs                           Kalman, SES (original parity)
  src/lock.rs                                GPS lock filter
  src/derive.rs                              derived metrics
  src/series.rs                              sampling, Stale/Absent, coverage
  src/telemetry.rs                           Telemetry, Snapshot, Availability
  tests/common/mod.rs                        sample lookup, reference CSV reader
  tests/reference.rs                         comparison with the original
  tests/gopro_files.rs                       gaps, no lock, short packet, damaged packets
  tests/reference/README.md                  how the CSVs were produced
  tests/reference/{hero5,hero6,max-heromode}.gopro-to-csv.csv
  tests/reference/{hero5,hero6,max-heromode}.dashboard.csv
crates/telemetry-cli/
  Cargo.toml
  src/main.rs                                actionlay-telemetry dump|info
  tests/cli.rs
```

---

### Task 1: Public GoPro samples and CI

**Files:**
- Create: `scripts/fetch-gopro-samples.sh`, `samples/gopro/README.md`
- Modify: `.gitignore`, `.github/workflows/ci.yml`

**Interfaces:**
- Produces: `samples/gopro/{hero5,hero6,hero7,hero8,max-heromode}.mp4` (git-ignored); environment variables honoured by later tests: `ACTIONLAY_GOPRO_SAMPLES` (directory, default `samples/gopro`), `ACTIONLAY_REQUIRE_GOPRO_SAMPLES` (any value: a missing sample panics).

- [ ] **Step 1: Fetch script**

`scripts/fetch-gopro-samples.sh` (executable, `chmod +x`):
```bash
#!/usr/bin/env bash
# Downloads the public GoPro sample videos used by the telemetry tests into
# samples/gopro (or $1). Source: github.com/gopro/gpmf-parser, Apache-2.0,
# pinned to one commit and checked by SHA-256. Files already present with
# the right checksum are kept.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/samples/gopro}"
COMMIT=9a7150632892c7356c91145c889016f07b0ed48d
BASE="https://raw.githubusercontent.com/gopro/gpmf-parser/$COMMIT/samples"
SAMPLES=(
  "hero5.mp4 04e45b2f41dff195b525fa18b7ed5517e5fc2d651127d1b57d1d931de306f915"
  "hero6.mp4 84aebc4e370ef9081f9015bf310d7a858d431258f6b0f2160d731c4308249c67"
  "hero7.mp4 3c593b8f08090e3ec246178c34737d11569330d75df8225e3ee08036c6a04d0b"
  "hero8.mp4 0e068f543ebf59bccb4228e7b5950a4f681753d2bbbf1c838ae60336bc75c1bd"
  "max-heromode.mp4 8e8fa98887f86119be1b886762b1080a92afb6cc146db719a238bdcba908277b"
)

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

mkdir -p "$OUT"
for entry in "${SAMPLES[@]}"; do
  read -r name sum <<<"$entry"
  dest="$OUT/$name"
  if [ -f "$dest" ] && [ "$(sha256 "$dest")" = "$sum" ]; then
    echo "ok   $name (already present)"
    continue
  fi
  curl -fsSL --retry 3 -o "$dest.part" "$BASE/$name"
  got="$(sha256 "$dest.part")"
  if [ "$got" != "$sum" ]; then
    rm -f "$dest.part"
    echo "checksum mismatch for $name: got $got, want $sum" >&2
    exit 1
  fi
  mv "$dest.part" "$dest"
  echo "ok   $name (downloaded)"
done
```

- [ ] **Step 2: Run it twice**

Run: `./scripts/fetch-gopro-samples.sh && ./scripts/fetch-gopro-samples.sh`
Expected: first run prints `ok   <name> (downloaded)` (or `already present`) for the five files; the second run prints `ok   <name> (already present)` five times; exit code 0.

Check that a corrupt file is replaced: `printf x >> samples/gopro/hero8.mp4 && ./scripts/fetch-gopro-samples.sh | grep hero8`
Expected: `ok   hero8.mp4 (downloaded)`.

- [ ] **Step 3: Attribution and ignore rules**

`samples/gopro/README.md`:
```markdown
# Public GoPro samples

Downloaded by `scripts/fetch-gopro-samples.sh`; the videos are not committed.

Source: [gopro/gpmf-parser](https://github.com/gopro/gpmf-parser/tree/9a7150632892c7356c91145c889016f07b0ed48d/samples),
commit `9a7150632892c7356c91145c889016f07b0ed48d`.
Copyright GoPro, Inc., licensed under the
[Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0).
They are used unmodified, as test inputs only.

| File | Camera | Length | GPS | Other streams | Tests |
|---|---|---|---|---|---|
| `hero5.mp4` | HERO5 Black, fw 2.00 | 34.6 s | 3D lock throughout, 618 samples, DOP 4.3–6.1 | ACCL, GYRO, TMPC | reference comparison |
| `hero6.mp4` | HERO6 Black, fw 1.60 | 23.6 s | 3D for 1 s, no lock 1.0–14.0 s, then 2D, then 3D | ACCL, GYRO, TMPC | reference comparison, fix gap |
| `hero7.mp4` | HERO7 Black | 12.7 s | never locks (DOP 99.99) | ACCL, GYRO (no temperature) | no-GPS case |
| `hero8.mp4` | HERO8 Black | 12.7 s | never locks (DOP 99.99) | ACCL, GYRO, TMPC, GRAV, CORI, IORI | no-GPS case, HERO8 streams |
| `max-heromode.mp4` | GoPro MAX, HERO mode | 10.5 s | 3D lock throughout | ACCL, GYRO, TMPC, GRAV, CORI, IORI, MAGN | reference comparison, short final packet |
```

`.gitignore` — replace the line `!/samples/README.md` with:
```
!/samples/README.md
!/samples/gopro/
/samples/gopro/*
!/samples/gopro/README.md
```

Run: `git status --short -uall samples && git check-ignore -v samples/gopro/hero5.mp4`
Expected: `?? samples/gopro/README.md` only (no `.mp4`, no `GX013370`), then `.gitignore:…:/samples/gopro/*	samples/gopro/hero5.mp4`.

- [ ] **Step 4: CI**

In `.github/workflows/ci.yml`, insert after the step `Export ACTIONLAY_SAMPLES (macOS/Linux)`:
```yaml
      # Public GoPro samples (Apache-2.0) for the telemetry tests. Plain
      # downloads, so every OS gets them; fetched only when the pinned list
      # in the script changes.
      - name: Cache GoPro samples
        id: gopro-cache
        uses: actions/cache@v4
        with:
          path: samples/gopro/*.mp4
          key: gopro-samples-${{ hashFiles('scripts/fetch-gopro-samples.sh') }}

      - name: Fetch GoPro samples
        if: steps.gopro-cache.outputs.cache-hit != 'true'
        run: bash ./scripts/fetch-gopro-samples.sh

      - name: Require GoPro samples in tests
        run: echo "ACTIONLAY_REQUIRE_GOPRO_SAMPLES=1" >> "$GITHUB_ENV"

      - name: Private sample stays out of tests and CI
        run: |
          if grep -rn "GX013370" crates scripts .github --exclude=ci.yml; then
            echo "samples/GX013370.MP4 is private: tests and CI must not use it" >&2
            exit 1
          fi
```

Run: `grep -rn "GX013370" crates scripts .github --exclude=ci.yml; echo "exit $?"`
Expected: `exit 1` (no match).

- [ ] **Step 5: Commit**

```bash
git add scripts/fetch-gopro-samples.sh samples/gopro/README.md .gitignore .github/workflows/ci.yml
git commit -m "test: fetch public GoPro samples for the telemetry tests"
```

---

### Task 2: Demux the GoPro metadata stream (media)

**Files:**
- Create: `crates/media/src/gpmf.rs`, `crates/media/tests/gpmf.rs`
- Modify: `crates/media/src/lib.rs`, `crates/media/tests/common/mod.rs`

**Interfaces:**
- Consumes: `ffmpeg_info::init()`, `MediaError` (M0); samples from Task 1.
- Produces (contract): `actionlay_media::gpmf::{GpmfPacket { pts: f64, duration: f64, data: Vec<u8> }, read_gpmf_packets(path: &Path) -> Result<Vec<GpmfPacket>, MediaError>}`; plus `fill_missing_durations(&mut [GpmfPacket])`. Test helper `common::gopro_sample(name) -> Option<PathBuf>`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/media/tests/common/mod.rs`:
```rust
/// A public GoPro sample (scripts/fetch-gopro-samples.sh). None, and the
/// test is skipped, when it was not downloaded — unless
/// ACTIONLAY_REQUIRE_GOPRO_SAMPLES is set, as in CI.
#[allow(dead_code)]
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
```

`crates/media/tests/gpmf.rs`:
```rust
mod common;
use actionlay_media::gpmf::read_gpmf_packets;

#[test]
fn reads_hero5_metadata_packets() {
    let Some(path) = common::gopro_sample("hero5.mp4") else {
        return;
    };
    let packets = read_gpmf_packets(&path).unwrap();
    assert_eq!(packets.len(), 34);
    assert_eq!(packets[0].pts, 0.0);
    assert!((packets[1].pts - 1.001).abs() < 1e-9);
    assert!((packets[33].pts - 33.033).abs() < 1e-9);
    assert!(packets.iter().all(|p| (p.duration - 1.001).abs() < 1e-9));
    assert_eq!(packets[0].data.len(), 4792);
    assert_eq!(&packets[0].data[..4], b"DEVC");
}

#[test]
fn keeps_the_short_final_packet_duration() {
    let Some(path) = common::gopro_sample("max-heromode.mp4") else {
        return;
    };
    let packets = read_gpmf_packets(&path).unwrap();
    assert_eq!(packets.len(), 11);
    let last = packets.last().unwrap();
    assert!((last.pts - 10.01).abs() < 1e-9);
    assert!((last.duration - 0.533).abs() < 1e-9);
}

#[test]
fn file_without_metadata_has_no_packets() {
    let Some(path) = common::sample("hevc8-1080p30-noaudio.mp4") else {
        return;
    };
    assert!(read_gpmf_packets(&path).unwrap().is_empty());
}

#[test]
fn missing_file_is_an_error() {
    assert!(read_gpmf_packets(std::path::Path::new("/nonexistent/video.mp4")).is_err());
}
```

In `crates/media/src/lib.rs` add `pub mod gpmf;` after `pub mod frame;`, and create `crates/media/src/gpmf.rs` with only the unit tests for now:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn p(pts: f64, duration: f64) -> GpmfPacket {
        GpmfPacket {
            pts,
            duration,
            data: Vec::new(),
        }
    }

    #[test]
    fn missing_durations_come_from_neighbours() {
        let mut v = vec![p(0.0, 0.0), p(1.0, 1.0), p(2.0, 0.0)];
        fill_missing_durations(&mut v);
        let d: Vec<f64> = v.iter().map(|x| x.duration).collect();
        assert_eq!(d, vec![1.0, 1.0, 1.0]);
        let mut one = vec![p(0.0, 0.0)];
        fill_missing_durations(&mut one);
        assert_eq!(one[0].duration, 0.0);
    }

    #[test]
    fn gpmd_tag_value() {
        assert_eq!(GPMD_TAG, 0x646d_7067);
    }
}
```

- [ ] **Step 2: Run the tests and verify that they fail**

Run: `source scripts/env.sh && cargo test -p actionlay-media --test gpmf`
Expected: FAIL to compile (`read_gpmf_packets` not found in `actionlay_media::gpmf`).

- [ ] **Step 3: Implementation**

Put above the tests in `crates/media/src/gpmf.rs`:
```rust
//! Demuxing of the GoPro metadata stream (GPMF, codec tag `gpmd`).
use std::path::Path;

use ffmpeg_next as ffmpeg;

use crate::{MediaError, ffmpeg_info};

/// One packet of the metadata stream. Times are seconds of file time.
#[derive(Debug, Clone, PartialEq)]
pub struct GpmfPacket {
    pub pts: f64,
    pub duration: f64,
    pub data: Vec<u8>,
}

const GPMD_TAG: u32 = u32::from_le_bytes(*b"gpmd");

/// Reads every packet of the GoPro metadata stream (codec tag `gpmd`,
/// handler "GoPro MET"). `Ok(vec![])` when the file has no such stream.
pub fn read_gpmf_packets(path: &Path) -> Result<Vec<GpmfPacket>, MediaError> {
    ffmpeg_info::init();
    let mut input = ffmpeg::format::input(path)?;
    let Some(index) = input.streams().find(is_gpmd).map(|s| s.index()) else {
        return Ok(Vec::new());
    };
    let time_base = f64::from(
        input
            .stream(index)
            .map_or(ffmpeg::Rational::new(1, 1000), |s| s.time_base()),
    );
    // Discarded streams are skipped by the demuxer without reading their
    // payload, so a multi-GB video costs only its metadata bytes.
    for i in 0..input.nb_streams() as usize {
        if i != index {
            // SAFETY: i < nb_streams, and the AVStream array lives as long
            // as `input`; only the `discard` field is written.
            unsafe {
                let stream = *(*input.as_mut_ptr()).streams.add(i);
                (*stream).discard = ffmpeg::ffi::AVDiscard::AVDISCARD_ALL;
            }
        }
    }
    let mut packets = Vec::new();
    for (stream, packet) in input.packets() {
        if stream.index() != index {
            continue;
        }
        let (Some(ts), Some(data)) = (packet.pts().or(packet.dts()), packet.data()) else {
            log::warn!("gpmd packet without timestamp or data skipped");
            continue;
        };
        packets.push(GpmfPacket {
            pts: ts as f64 * time_base,
            duration: packet.duration().max(0) as f64 * time_base,
            data: data.to_vec(),
        });
    }
    fill_missing_durations(&mut packets);
    Ok(packets)
}

fn is_gpmd(stream: &ffmpeg::format::stream::Stream) -> bool {
    let params = stream.parameters();
    // SAFETY: the parameters belong to a live stream; codec_tag is plain data.
    let tag = unsafe { (*params.as_ptr()).codec_tag };
    tag == GPMD_TAG
        || stream
            .metadata()
            .get("handler_name")
            .is_some_and(|h| h.trim() == "GoPro MET")
}

/// Packets the muxer stored without a duration get the gap to the next
/// packet; a last packet without one gets the previous packet's duration.
pub fn fill_missing_durations(packets: &mut [GpmfPacket]) {
    for i in 0..packets.len() {
        if packets[i].duration > 0.0 {
            continue;
        }
        packets[i].duration = if i + 1 < packets.len() {
            (packets[i + 1].pts - packets[i].pts).max(0.0)
        } else if i > 0 {
            packets[i - 1].duration
        } else {
            0.0
        };
    }
}
```
Notes: the stream is found by codec tag `gpmd` (`0x646d7067`), falling back to the trimmed handler name (hero5 has a leading tab, others trailing spaces). `ffmpeg-next` 9 has no `set_discard`, hence the raw pointer; discarding the other streams lets the mov demuxer skip their payload (a 1.4 GB file reads its metadata in under 0.1 s instead of streaming all video bytes). FFmpeg prints a harmless warning on stderr about the `fdsc` stream having zero-duration samples.

- [ ] **Step 4: Run the tests and verify that they pass**

Run: `source scripts/env.sh && cargo test -p actionlay-media --lib gpmf && cargo test -p actionlay-media --test gpmf -- --test-threads=1`
Expected: PASS (2 unit tests, 4 integration tests).

- [ ] **Step 5: Commit**

```bash
git add crates/media
git commit -m "feat(media): read the GoPro metadata (gpmd) packets"
```

---

### Task 3: Telemetry crate, units, metrics and values

**Files:**
- Create: `crates/telemetry/Cargo.toml`, `crates/telemetry/src/lib.rs`, `crates/telemetry/src/units.rs`, `crates/telemetry/src/metric.rs`, `crates/telemetry/src/value.rs`
- Modify: `Cargo.toml`, `.github/workflows/ci.yml`

**Interfaces:**
- Produces (contract): `units::{Quantity, Unit, UnitSystem, default_unit(Quantity, UnitSystem) -> Unit, convert(si: f64, unit: Unit) -> f64, symbol(Unit) -> &'static str}`, `Unit::{id, from_id}`, `units::units_for(Quantity) -> &'static [Unit]`; `Metric` with `COUNT`, `ALL`, `index()`, `id()`, `from_id(&str) -> Option<Metric>`, `quantity()`, `is_external()`; `Value { Present(f64), Stale { value, age }, Absent }` with `present()`, `last_known()`; `GpsLock { NoLock, Lock2d, Lock3d, Unknown }` with `from_fix(u32)`, `is_locked()`, `code()`, `original_name()`.

- [ ] **Step 1: Workspace and crate manifest**

Root `Cargo.toml`: set `members = ["crates/media", "crates/app", "crates/telemetry"]` and add to `[workspace.dependencies]`:
```toml
chrono = { version = "0.4.45", default-features = false, features = ["std"] }
geographiclib-rs = "0.2.7"
```

`crates/telemetry/Cargo.toml`:
```toml
[package]
name = "actionlay-telemetry"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true

[dependencies]
chrono.workspace = true
geographiclib-rs.workspace = true
thiserror.workspace = true
log.workspace = true
```

`crates/telemetry/src/lib.rs`:
```rust
//! Telemetry of action-camera videos: GoPro GPMF parsing, derived metrics,
//! and sampling at any file time. Pure Rust; packets come from the caller
//! (the media crate demuxes them), so this crate never links FFmpeg.
pub mod metric;
pub mod units;
mod value;

pub use metric::Metric;
pub use value::{GpsLock, Value};
```

- [ ] **Step 2: Write the failing tests**

Create the three modules with only their test blocks.

`crates/telemetry/src/units.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9 * b.abs().max(1.0)
    }

    #[test]
    fn converts_speed() {
        assert!(close(convert(10.0, Unit::Kmh), 36.0));
        assert!(close(convert(10.0, Unit::Mph), 22.369_362_920_544_02));
        assert!(close(convert(10.0, Unit::Knots), 19.438_444_924_406_05));
        assert!(close(convert(10.0, Unit::Mps), 10.0));
    }

    #[test]
    fn converts_pace_and_standstill() {
        // 10 km/h = 6 min/km
        assert!(close(convert(10.0 / 3.6, Unit::PaceKm), 6.0));
        assert!(close(convert(10.0 / 3.6, Unit::PaceMile), 9.656_064));
        assert!(convert(0.0, Unit::PaceKm).is_infinite());
    }

    #[test]
    fn converts_distance_altitude_temperature_acceleration() {
        assert!(close(convert(1609.344, Unit::Mi), 1.0));
        assert!(close(convert(1852.0, Unit::Nmi), 1.0));
        assert!(close(convert(2500.0, Unit::Km), 2.5));
        assert!(close(convert(100.0, Unit::Ft), 328.083_989_501_312_3));
        assert!(close(convert(100.0, Unit::DegF), 212.0));
        assert!(close(convert(-40.0, Unit::DegF), -40.0));
        assert!(close(convert(9.806_65, Unit::G), 1.0));
    }

    #[test]
    fn ids_round_trip_and_are_unique() {
        for u in ALL_UNITS {
            assert_eq!(Unit::from_id(u.id()), Some(u));
        }
        assert_eq!(Unit::from_id("furlong"), None);
    }

    #[test]
    fn defaults_per_system() {
        assert_eq!(default_unit(Quantity::Speed, UnitSystem::Metric), Unit::Kmh);
        assert_eq!(
            default_unit(Quantity::Speed, UnitSystem::Imperial),
            Unit::Mph
        );
        assert_eq!(
            default_unit(Quantity::Altitude, UnitSystem::Imperial),
            Unit::Ft
        );
        assert_eq!(
            default_unit(Quantity::Distance, UnitSystem::Imperial),
            Unit::Mi
        );
        assert_eq!(
            default_unit(Quantity::Temperature, UnitSystem::Imperial),
            Unit::DegF
        );
        assert_eq!(
            default_unit(Quantity::Acceleration, UnitSystem::Metric),
            Unit::Mps2
        );
        assert_eq!(
            default_unit(Quantity::Ratio, UnitSystem::Metric),
            Unit::Percent
        );
        for q in [
            Quantity::Speed,
            Quantity::Distance,
            Quantity::Altitude,
            Quantity::Acceleration,
            Quantity::Angle,
            Quantity::Temperature,
            Quantity::Ratio,
            Quantity::Dimensionless,
            Quantity::Coordinate,
        ] {
            for s in [UnitSystem::Metric, UnitSystem::Imperial] {
                assert!(units_for(q).contains(&default_unit(q, s)), "{q:?} {s:?}");
            }
        }
    }

    #[test]
    fn symbols() {
        assert_eq!(symbol(Unit::Kmh), "km/h");
        assert_eq!(symbol(Unit::DegC), "°C");
        assert_eq!(symbol(Unit::G), "G");
        assert_eq!(symbol(Unit::Percent), "%");
        assert_eq!(symbol(Unit::PaceKm), "min/km");
    }
}
```

`crates/telemetry/src/metric.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_is_in_index_order_and_ids_round_trip() {
        for (i, m) in Metric::ALL.into_iter().enumerate() {
            assert_eq!(m.index(), i);
            assert_eq!(Metric::from_id(m.id()), Some(m));
        }
        assert_eq!(Metric::from_id("speed"), Some(Metric::Speed));
        assert_eq!(Metric::from_id("gps-lock"), Some(Metric::GpsLock));
        assert_eq!(Metric::from_id("accl.z"), Some(Metric::AcclZ));
        assert_eq!(Metric::from_id("nope"), None);
    }

    #[test]
    fn quantities() {
        assert_eq!(Metric::Speed.quantity(), Quantity::Speed);
        assert_eq!(Metric::CGrad.quantity(), Quantity::Ratio);
        assert_eq!(Metric::Alt.quantity(), Quantity::Altitude);
        assert_eq!(Metric::Odo.quantity(), Quantity::Distance);
        assert_eq!(Metric::Lat.quantity(), Quantity::Coordinate);
        assert_eq!(Metric::AcclX.quantity(), Quantity::Acceleration);
        assert_eq!(Metric::OriYaw.quantity(), Quantity::Angle);
        assert_eq!(Metric::Temp.quantity(), Quantity::Temperature);
        assert_eq!(Metric::GravZ.quantity(), Quantity::Dimensionless);
    }

    #[test]
    fn external_metrics() {
        assert!(Metric::Hr.is_external());
        assert!(!Metric::Speed.is_external());
        assert_eq!(Metric::ALL.iter().filter(|m| m.is_external()).count(), 7);
    }
}
```

`crates/telemetry/src/value.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_accessors() {
        assert_eq!(Value::Present(1.0).present(), Some(1.0));
        assert_eq!(
            Value::Stale {
                value: 2.0,
                age: 1.0
            }
            .present(),
            None
        );
        assert_eq!(
            Value::Stale {
                value: 2.0,
                age: 1.0
            }
            .last_known(),
            Some(2.0)
        );
        assert_eq!(Value::Absent.last_known(), None);
    }

    #[test]
    fn lock_codes_round_trip() {
        for l in [
            GpsLock::NoLock,
            GpsLock::Lock2d,
            GpsLock::Lock3d,
            GpsLock::Unknown,
        ] {
            assert_eq!(GpsLock::from_fix(l.code()), l);
        }
        assert_eq!(GpsLock::from_fix(7), GpsLock::Unknown);
        assert!(GpsLock::Lock2d.is_locked() && !GpsLock::NoLock.is_locked());
        assert_eq!(GpsLock::Lock3d.original_name(), "LOCK_3D");
    }
}
```

- [ ] **Step 3: Run the tests and verify that they fail**

Run: `cargo test -p actionlay-telemetry --lib`
Expected: FAIL to compile (`convert`, `Metric`, `Value` … not found).

- [ ] **Step 4: Implementation**

Above the tests in `crates/telemetry/src/units.rs`:
```rust
//! Physical quantities, display units and conversions.
//!
//! Values inside ActionLay are stored in *base units*: SI, except angles and
//! coordinates in degrees, temperature in °C, gradient in percent, and
//! dimensionless values as they are. [`convert`] turns a base value into a
//! display unit.

/// What a metric measures; decides which units can display it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Quantity {
    Speed,
    Distance,
    Altitude,
    Acceleration,
    Angle,
    Temperature,
    /// Gradient, in percent.
    Ratio,
    Dimensionless,
    Coordinate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitSystem {
    Metric,
    Imperial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unit {
    Kmh,
    Mph,
    Knots,
    Mps,
    PaceKm,
    PaceMile,
    PaceNm,
    Km,
    Mi,
    Nmi,
    M,
    Ft,
    G,
    Mps2,
    DegC,
    DegF,
    Deg,
    Percent,
    /// Dimensionless values (DOP, lock state, gravity in g).
    None,
}

const ALL_UNITS: [Unit; 19] = [
    Unit::Kmh,
    Unit::Mph,
    Unit::Knots,
    Unit::Mps,
    Unit::PaceKm,
    Unit::PaceMile,
    Unit::PaceNm,
    Unit::Km,
    Unit::Mi,
    Unit::Nmi,
    Unit::M,
    Unit::Ft,
    Unit::G,
    Unit::Mps2,
    Unit::DegC,
    Unit::DegF,
    Unit::Deg,
    Unit::Percent,
    Unit::None,
];

/// Standard gravity, m/s².
pub const STANDARD_GRAVITY: f64 = 9.806_65;
const MILE_M: f64 = 1609.344;
const NAUTICAL_MILE_M: f64 = 1852.0;
const FOOT_M: f64 = 0.3048;

impl Unit {
    pub fn id(self) -> &'static str {
        match self {
            Unit::Kmh => "kmh",
            Unit::Mph => "mph",
            Unit::Knots => "knots",
            Unit::Mps => "mps",
            Unit::PaceKm => "pace_km",
            Unit::PaceMile => "pace_mile",
            Unit::PaceNm => "pace_nm",
            Unit::Km => "km",
            Unit::Mi => "mi",
            Unit::Nmi => "nmi",
            Unit::M => "m",
            Unit::Ft => "ft",
            Unit::G => "g",
            Unit::Mps2 => "mps2",
            Unit::DegC => "degc",
            Unit::DegF => "degf",
            Unit::Deg => "deg",
            Unit::Percent => "percent",
            Unit::None => "none",
        }
    }

    pub fn from_id(id: &str) -> Option<Unit> {
        ALL_UNITS.into_iter().find(|u| u.id() == id)
    }
}

/// Units that can display `q`, default units first.
pub fn units_for(q: Quantity) -> &'static [Unit] {
    match q {
        Quantity::Speed => &[
            Unit::Kmh,
            Unit::Mph,
            Unit::Knots,
            Unit::Mps,
            Unit::PaceKm,
            Unit::PaceMile,
            Unit::PaceNm,
        ],
        Quantity::Distance => &[Unit::Km, Unit::Mi, Unit::Nmi, Unit::M],
        Quantity::Altitude => &[Unit::M, Unit::Ft],
        Quantity::Acceleration => &[Unit::Mps2, Unit::G],
        Quantity::Temperature => &[Unit::DegC, Unit::DegF],
        Quantity::Angle | Quantity::Coordinate => &[Unit::Deg],
        Quantity::Ratio => &[Unit::Percent],
        Quantity::Dimensionless => &[Unit::None],
    }
}

pub fn default_unit(q: Quantity, system: UnitSystem) -> Unit {
    let imperial = system == UnitSystem::Imperial;
    match q {
        Quantity::Speed if imperial => Unit::Mph,
        Quantity::Speed => Unit::Kmh,
        Quantity::Distance if imperial => Unit::Mi,
        Quantity::Distance => Unit::Km,
        Quantity::Altitude if imperial => Unit::Ft,
        Quantity::Altitude => Unit::M,
        Quantity::Temperature if imperial => Unit::DegF,
        Quantity::Temperature => Unit::DegC,
        Quantity::Acceleration => Unit::Mps2,
        Quantity::Angle | Quantity::Coordinate => Unit::Deg,
        Quantity::Ratio => Unit::Percent,
        Quantity::Dimensionless => Unit::None,
    }
}

/// Converts a value in base units to `unit`. Pace of a standstill is +∞.
pub fn convert(si: f64, unit: Unit) -> f64 {
    let pace = |metres: f64| {
        if si > 0.0 {
            metres / 60.0 / si
        } else {
            f64::INFINITY
        }
    };
    match unit {
        Unit::Kmh => si * 3.6,
        Unit::Mph => si * 3600.0 / MILE_M,
        Unit::Knots => si * 3600.0 / NAUTICAL_MILE_M,
        Unit::PaceKm => pace(1000.0),
        Unit::PaceMile => pace(MILE_M),
        Unit::PaceNm => pace(NAUTICAL_MILE_M),
        Unit::Km => si / 1000.0,
        Unit::Mi => si / MILE_M,
        Unit::Nmi => si / NAUTICAL_MILE_M,
        Unit::Ft => si / FOOT_M,
        Unit::G => si / STANDARD_GRAVITY,
        Unit::DegF => si * 9.0 / 5.0 + 32.0,
        Unit::Mps | Unit::M | Unit::Mps2 | Unit::DegC | Unit::Deg | Unit::Percent | Unit::None => {
            si
        }
    }
}

pub fn symbol(unit: Unit) -> &'static str {
    match unit {
        Unit::Kmh => "km/h",
        Unit::Mph => "mph",
        Unit::Knots => "kn",
        Unit::Mps => "m/s",
        Unit::PaceKm => "min/km",
        Unit::PaceMile => "min/mi",
        Unit::PaceNm => "min/nmi",
        Unit::Km => "km",
        Unit::Mi => "mi",
        Unit::Nmi => "nmi",
        Unit::M => "m",
        Unit::Ft => "ft",
        Unit::G => "G",
        Unit::Mps2 => "m/s²",
        Unit::DegC => "°C",
        Unit::DegF => "°F",
        Unit::Deg => "°",
        Unit::Percent => "%",
        Unit::None => "",
    }
}
```

Above the tests in `crates/telemetry/src/metric.rs`:
```rust
//! Registry of the metrics a layout can show. String ids are those of
//! gopro-dashboard-overlay wherever it has the metric.
use crate::units::Quantity;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Metric {
    Speed,
    CSpeed,
    Accel,
    Gradient,
    CGrad,
    Alt,
    Odo,
    COdo,
    Dist,
    Azi,
    Cog,
    Lat,
    Lon,
    GpsDop,
    GpsLock,
    AcclX,
    AcclY,
    AcclZ,
    GravX,
    GravY,
    GravZ,
    OriPitch,
    OriRoll,
    OriYaw,
    Temp,
    Hr,
    Cadence,
    Power,
    Respiration,
    GearFront,
    GearRear,
    Sdps,
}

impl Metric {
    /// Number of metrics.
    pub const COUNT: usize = 32;

    /// Every metric, in declaration order (`ALL[m.index()] == m`).
    pub const ALL: [Metric; Metric::COUNT] = [
        Metric::Speed,
        Metric::CSpeed,
        Metric::Accel,
        Metric::Gradient,
        Metric::CGrad,
        Metric::Alt,
        Metric::Odo,
        Metric::COdo,
        Metric::Dist,
        Metric::Azi,
        Metric::Cog,
        Metric::Lat,
        Metric::Lon,
        Metric::GpsDop,
        Metric::GpsLock,
        Metric::AcclX,
        Metric::AcclY,
        Metric::AcclZ,
        Metric::GravX,
        Metric::GravY,
        Metric::GravZ,
        Metric::OriPitch,
        Metric::OriRoll,
        Metric::OriYaw,
        Metric::Temp,
        Metric::Hr,
        Metric::Cadence,
        Metric::Power,
        Metric::Respiration,
        Metric::GearFront,
        Metric::GearRear,
        Metric::Sdps,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn id(self) -> &'static str {
        match self {
            Metric::Speed => "speed",
            Metric::CSpeed => "cspeed",
            Metric::Accel => "accel",
            Metric::Gradient => "gradient",
            Metric::CGrad => "cgrad",
            Metric::Alt => "alt",
            Metric::Odo => "odo",
            Metric::COdo => "codo",
            Metric::Dist => "dist",
            Metric::Azi => "azi",
            Metric::Cog => "cog",
            Metric::Lat => "lat",
            Metric::Lon => "lon",
            Metric::GpsDop => "gps-dop",
            Metric::GpsLock => "gps-lock",
            Metric::AcclX => "accl.x",
            Metric::AcclY => "accl.y",
            Metric::AcclZ => "accl.z",
            Metric::GravX => "grav.x",
            Metric::GravY => "grav.y",
            Metric::GravZ => "grav.z",
            Metric::OriPitch => "ori.pitch",
            Metric::OriRoll => "ori.roll",
            Metric::OriYaw => "ori.yaw",
            Metric::Temp => "temp",
            Metric::Hr => "hr",
            Metric::Cadence => "cadence",
            Metric::Power => "power",
            Metric::Respiration => "respiration",
            Metric::GearFront => "gear.front",
            Metric::GearRear => "gear.rear",
            Metric::Sdps => "sdps",
        }
    }

    pub fn from_id(id: &str) -> Option<Metric> {
        Metric::ALL.into_iter().find(|m| m.id() == id)
    }

    pub fn quantity(self) -> Quantity {
        use Metric::*;
        match self {
            Speed | CSpeed => Quantity::Speed,
            Accel | AcclX | AcclY | AcclZ => Quantity::Acceleration,
            Gradient | CGrad => Quantity::Ratio,
            Alt => Quantity::Altitude,
            Odo | COdo | Dist => Quantity::Distance,
            Azi | Cog | OriPitch | OriRoll | OriYaw => Quantity::Angle,
            Lat | Lon => Quantity::Coordinate,
            Temp => Quantity::Temperature,
            // Gravity is a unit vector in g, as in the original.
            GpsDop | GpsLock | GravX | GravY | GravZ => Quantity::Dimensionless,
            Hr | Cadence | Power | Respiration | GearFront | GearRear | Sdps => {
                Quantity::Dimensionless
            }
        }
    }

    /// True for metrics that only external files (GPX/FIT, M6) can provide.
    pub fn is_external(self) -> bool {
        use Metric::*;
        matches!(
            self,
            Hr | Cadence | Power | Respiration | GearFront | GearRear | Sdps
        )
    }
}
```

Above the tests in `crates/telemetry/src/value.rs`:
```rust
//! What a metric reads at one instant.

/// A metric's reading at time t, in base units (see [`crate::units`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    /// Valid data at t.
    Present(f64),
    /// Temporary gap: the last valid value and the seconds since it stopped
    /// being valid.
    Stale { value: f64, age: f64 },
    /// Nothing to show at t: the file has no valid sample of this metric at
    /// or before t. Whether the metric exists in the file at all is
    /// `Availability::coverage(m) > 0`.
    Absent,
}

impl Value {
    /// The value if Present.
    pub fn present(self) -> Option<f64> {
        match self {
            Value::Present(v) => Some(v),
            _ => None,
        }
    }

    /// The value if Present or Stale.
    pub fn last_known(self) -> Option<f64> {
        match self {
            Value::Present(v) | Value::Stale { value: v, .. } => Some(v),
            Value::Absent => None,
        }
    }
}

/// GPS fix after ActionLay's lock filter (DOP, heuristics).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpsLock {
    NoLock,
    Lock2d,
    Lock3d,
    /// No GPS stream, or a fix code the camera did not document.
    Unknown,
}

impl GpsLock {
    /// GPSF codes: 0 = no lock, 2 = 2D, 3 = 3D.
    pub fn from_fix(fix: u32) -> GpsLock {
        match fix {
            0 => GpsLock::NoLock,
            2 => GpsLock::Lock2d,
            3 => GpsLock::Lock3d,
            _ => GpsLock::Unknown,
        }
    }

    pub fn is_locked(self) -> bool {
        matches!(self, GpsLock::Lock2d | GpsLock::Lock3d)
    }

    /// The GPSF code (Unknown → 1, as gopro-dashboard-overlay's GPSFix).
    pub fn code(self) -> u32 {
        match self {
            GpsLock::NoLock => 0,
            GpsLock::Unknown => 1,
            GpsLock::Lock2d => 2,
            GpsLock::Lock3d => 3,
        }
    }

    /// Name used by gopro-to-csv's `gps_fix` column.
    pub fn original_name(self) -> &'static str {
        match self {
            GpsLock::NoLock => "NO",
            GpsLock::Unknown => "UNKNOWN",
            GpsLock::Lock2d => "LOCK_2D",
            GpsLock::Lock3d => "LOCK_3D",
        }
    }
}
```

- [ ] **Step 5: Run the tests and verify that they pass**

Run: `cargo test -p actionlay-telemetry --lib`
Expected: PASS (`test result: ok. 11 passed`). The crate needs no `FFMPEG_DIR`.

- [ ] **Step 6: CI check that the crate stays FFmpeg-free**

In `.github/workflows/ci.yml`, insert before `- run: cargo fmt --all --check`:
```yaml
      - name: Telemetry crate stays free of FFmpeg
        run: |
          if cargo tree -p actionlay-telemetry -e normal --prefix none | grep -i ffmpeg; then
            echo "actionlay-telemetry must not depend on FFmpeg" >&2
            exit 1
          fi
```

Run: `cargo tree -p actionlay-telemetry -e normal --depth 1`
Expected: `chrono`, `geographiclib-rs`, `log`, `thiserror` only.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/telemetry .github/workflows/ci.yml
git commit -m "feat(telemetry): crate with units, metric registry and values"
```

---

### Task 4: GPMF KLV parser

**Files:**
- Create: `crates/telemetry/src/gpmf.rs`, `crates/telemetry/src/test_support.rs`
- Modify: `crates/telemetry/src/lib.rs`

**Interfaces:**
- Produces: `gpmf::{parse(&[u8]) -> Result<Vec<Klv>, GpmfError>, Klv { key: FourCc, type_char: u8, struct_size: u8, repeat: u16, payload: Payload }, Payload { Nested(Vec<Klv>), Data(Vec<u8>) }, FourCc, GpmfError, type_size(u8) -> Option<usize>, apply_scale(FourCc, &mut [Vec<f64>], &[f64]) -> Result<(), GpmfError>}`; `Klv::{data, children, numbers(Option<&[u8]>) -> Result<Vec<Vec<f64>>, GpmfError>, text() -> Option<String>, utc() -> Option<DateTime<Utc>>}`. Test builders `test_support::{item, nested}`.

GPMF facts used: 8-byte header (FourCC, type, struct size, big-endian u16 repeat), payload `struct_size × repeat` padded to 4 bytes, type 0 = nested. Types: `b B c d f F G j J l L q Q s S U ?`; `q` is Q15.16 and `Q` is Q31.32 fixed point; `U` is a 16-character `yymmddhhmmss.sss` UTC date; `?` is a complex structure described by the stream's `TYPE`. `c` strings are Latin-1 (`SIUN` is `m/s` + 0xB2).

- [ ] **Step 1: Write the failing tests**

`crates/telemetry/src/test_support.rs`:
```rust
//! Builders for hand-made GPMF payloads used by the unit tests.

/// One KLV item with a data payload, padded to 4 bytes.
pub fn item(key: &[u8; 4], type_char: u8, struct_size: u8, repeat: u16, data: &[u8]) -> Vec<u8> {
    assert_eq!(data.len(), usize::from(struct_size) * usize::from(repeat));
    let mut out = key.to_vec();
    out.push(type_char);
    out.push(struct_size);
    out.extend(repeat.to_be_bytes());
    out.extend(data);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    out
}

/// A nested item (type 0) containing `children`.
pub fn nested(key: &[u8; 4], children: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = children.concat();
    let mut out = key.to_vec();
    out.push(0);
    out.push(1);
    out.extend(u16::try_from(body.len()).unwrap().to_be_bytes());
    out.extend(body);
    out
}
```

`crates/telemetry/src/gpmf.rs` (tests only for now):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{item, nested};

    #[test]
    fn parses_nested_items_and_padding() {
        let strm = nested(
            b"STRM",
            &[
                item(b"STNM", b'c', 1, 3, b"abc"), // 3 bytes, padded to 4
                item(b"TSMP", b'L', 4, 1, &7u32.to_be_bytes()),
            ],
        );
        let data = nested(b"DEVC", &[strm]);
        let items = parse(&data).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].key, FourCc::new(b"DEVC"));
        let strm = &items[0].children()[0];
        assert_eq!(strm.key, FourCc::new(b"STRM"));
        assert_eq!(strm.children()[0].text().as_deref(), Some("abc"));
        assert_eq!(strm.children()[1].numbers(None).unwrap(), vec![vec![7.0]]);
    }

    #[test]
    fn decodes_every_numeric_type() {
        let cases: Vec<(u8, Vec<u8>, f64)> = vec![
            (b'b', vec![0xff], -1.0),
            (b'B', vec![0xff], 255.0),
            (b's', (-2i16).to_be_bytes().to_vec(), -2.0),
            (b'S', 65_535u16.to_be_bytes().to_vec(), 65_535.0),
            (b'l', (-3i32).to_be_bytes().to_vec(), -3.0),
            (
                b'L',
                4_000_000_000u32.to_be_bytes().to_vec(),
                4_000_000_000.0,
            ),
            (b'f', 1.5f32.to_be_bytes().to_vec(), 1.5),
            (b'd', 2.25f64.to_be_bytes().to_vec(), 2.25),
            (b'j', (-5i64).to_be_bytes().to_vec(), -5.0),
            (b'J', 6u64.to_be_bytes().to_vec(), 6.0),
            (b'q', (3 * 65_536i32 + 32_768).to_be_bytes().to_vec(), 3.5),
            (b'Q', (-(1i64 << 32) / 4).to_be_bytes().to_vec(), -0.25),
        ];
        for (t, bytes, want) in cases {
            let data = item(b"TEST", t, bytes.len() as u8, 1, &bytes);
            let klv = &parse(&data).unwrap()[0];
            assert_eq!(
                klv.numbers(None).unwrap(),
                vec![vec![want]],
                "type {}",
                t as char
            );
        }
    }

    #[test]
    fn decodes_rows_and_complex_types() {
        // 2 rows of 3 i16
        let mut bytes = Vec::new();
        for v in [1i16, 2, 3, 4, 5, 6] {
            bytes.extend(v.to_be_bytes());
        }
        let klv = &parse(&item(b"ACCL", b's', 6, 2, &bytes)).unwrap()[0];
        assert_eq!(
            klv.numbers(None).unwrap(),
            vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]
        );

        // complex: TYPE "lS" → 6-byte rows
        let mut bytes = (-7i32).to_be_bytes().to_vec();
        bytes.extend(9u16.to_be_bytes());
        let klv = &parse(&item(b"CPLX", b'?', 6, 1, &bytes)).unwrap()[0];
        assert_eq!(klv.numbers(Some(b"lS")).unwrap(), vec![vec![-7.0, 9.0]]);
        assert_eq!(
            klv.numbers(None),
            Err(GpmfError::MissingType {
                key: FourCc::new(b"CPLX")
            })
        );
        assert!(matches!(
            klv.numbers(Some(b"ll")),
            Err(GpmfError::BadStructSize { .. })
        ));
    }

    #[test]
    fn zero_repeat_is_empty() {
        let klv = &parse(&item(b"FACE", b'?', 28, 0, &[])).unwrap()[0];
        assert!(klv.numbers(Some(b"Lffffff")).unwrap().is_empty());
    }

    #[test]
    fn scale_single_and_per_element() {
        let key = FourCc::new(b"GPS5");
        let mut rows = vec![vec![10.0, 20.0], vec![30.0, 40.0]];
        apply_scale(key, &mut rows, &[10.0]).unwrap();
        assert_eq!(rows, vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
        apply_scale(key, &mut rows, &[1.0, 2.0]).unwrap();
        assert_eq!(rows, vec![vec![1.0, 1.0], vec![3.0, 2.0]]);
        assert_eq!(
            apply_scale(key, &mut rows, &[1.0, 2.0, 3.0]),
            Err(GpmfError::ScaleMismatch {
                key,
                scal: 3,
                elements: 2
            })
        );
    }

    #[test]
    fn utc_and_latin1_text() {
        let klv = &parse(&item(b"GPSU", b'U', 16, 1, b"170417173103.985")).unwrap()[0];
        assert_eq!(
            klv.utc().unwrap().to_rfc3339(),
            "2017-04-17T17:31:03.985+00:00"
        );
        let bad = &parse(&item(b"GPSU", b'U', 16, 1, b"000000000000.000")).unwrap()[0];
        assert_eq!(bad.utc(), None);
        let siun = &parse(&item(b"SIUN", b'c', 4, 1, b"m/s\xb2")).unwrap()[0];
        assert_eq!(siun.text().as_deref(), Some("m/s²"));
    }

    #[test]
    fn rejects_truncated_input_at_every_length() {
        let strm = nested(b"STRM", &[item(b"GPS5", b'l', 20, 2, &[1u8; 40])]);
        let data = nested(b"DEVC", &[strm]);
        assert!(parse(&data).is_ok());
        for cut in 1..data.len() {
            let r = parse(&data[..cut]);
            assert!(
                matches!(r, Err(GpmfError::Truncated { .. })),
                "cut at {cut}: {r:?}"
            );
        }
    }

    #[test]
    fn accepts_trailing_zero_bytes_and_rejects_garbage() {
        let mut data = item(b"TSMP", b'L', 4, 1, &1u32.to_be_bytes());
        data.extend([0, 0, 0, 0]);
        assert_eq!(parse(&data).unwrap().len(), 1);
        let mut data = item(b"TSMP", b'L', 4, 1, &1u32.to_be_bytes());
        data.extend([1, 2, 3]);
        assert!(matches!(parse(&data), Err(GpmfError::Truncated { .. })));
    }

    #[test]
    fn rejects_unknown_types_and_deep_nesting() {
        let r = parse(&item(b"WHAT", b'z', 1, 1, &[0]));
        assert!(matches!(
            r,
            Err(GpmfError::UnknownType {
                type_char: b'z',
                ..
            })
        ));

        let mut data = item(b"LEAF", b'B', 1, 1, &[1]);
        for _ in 0..MAX_DEPTH {
            data = nested(b"NEST", &[data]);
        }
        assert!(matches!(parse(&data), Err(GpmfError::TooDeep { .. })));
    }

    #[test]
    fn non_numeric_items_refuse_numbers() {
        let klv = &parse(&item(b"STNM", b'c', 1, 2, b"ab")).unwrap()[0];
        assert_eq!(
            klv.numbers(None),
            Err(GpmfError::NotNumeric {
                key: FourCc::new(b"STNM")
            })
        );
    }
}
```

`crates/telemetry/src/lib.rs`:
```rust
//! Telemetry of action-camera videos: GoPro GPMF parsing, derived metrics,
//! and sampling at any file time. Pure Rust; packets come from the caller
//! (the media crate demuxes them), so this crate never links FFmpeg.
pub mod gpmf;
pub mod metric;
#[cfg(test)]
mod test_support;
pub mod units;
mod value;

pub use metric::Metric;
pub use value::{GpsLock, Value};
```

- [ ] **Step 2: Run the tests and verify that they fail**

Run: `cargo test -p actionlay-telemetry --lib gpmf`
Expected: FAIL to compile (`parse`, `FourCc`, `GpmfError` not found).

- [ ] **Step 3: Implementation**

Above the tests in `crates/telemetry/src/gpmf.rs`:
```rust
//! GoPro Metadata Format (GPMF) KLV parser.
//!
//! Every item has an 8-byte header: a 4-byte key (FourCC), a 1-byte type, a
//! 1-byte structure size and a 2-byte big-endian repeat count. The payload is
//! `struct_size × repeat` bytes, padded to a multiple of 4. Type 0 means the
//! payload is itself a list of items (`DEVC`, `STRM`).
//! Reference: <https://github.com/gopro/gpmf-parser> (Apache-2.0).
use std::fmt;

use chrono::{DateTime, NaiveDateTime, Utc};

/// Deepest nesting accepted (real files use 2: DEVC → STRM).
const MAX_DEPTH: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FourCc(pub [u8; 4]);

impl FourCc {
    pub const fn new(key: &[u8; 4]) -> FourCc {
        FourCc(*key)
    }
}

impl fmt::Display for FourCc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            let c = if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '?'
            };
            write!(f, "{c}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for FourCc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FourCc({self})")
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum GpmfError {
    #[error("{key}: item at offset {offset} needs {need} bytes, only {have} left")]
    Truncated {
        key: FourCc,
        offset: usize,
        need: usize,
        have: usize,
    },
    #[error("{key}: nesting deeper than {MAX_DEPTH} levels")]
    TooDeep { key: FourCc },
    #[error("{key}: unknown type '{}'", *type_char as char)]
    UnknownType { key: FourCc, type_char: u8 },
    #[error("{key}: structure size {struct_size} does not fit type '{}'", *type_char as char)]
    BadStructSize {
        key: FourCc,
        type_char: u8,
        struct_size: u8,
    },
    #[error("{key}: item is not numeric")]
    NotNumeric { key: FourCc },
    #[error("{key}: complex item needs a TYPE")]
    MissingType { key: FourCc },
    #[error("{key}: SCAL has {scal} values for {elements} elements")]
    ScaleMismatch {
        key: FourCc,
        scal: usize,
        elements: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Payload {
    Nested(Vec<Klv>),
    /// Exactly `struct_size × repeat` bytes, padding removed.
    Data(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Klv {
    pub key: FourCc,
    /// GPMF type character (`b'L'`, `b's'`, …); 0 for nested items.
    pub type_char: u8,
    pub struct_size: u8,
    pub repeat: u16,
    pub payload: Payload,
}

/// Parses one GPMF payload (one demuxed `gpmd` packet).
pub fn parse(data: &[u8]) -> Result<Vec<Klv>, GpmfError> {
    parse_level(data, 0, 0)
}

fn parse_level(data: &[u8], base: usize, depth: usize) -> Result<Vec<Klv>, GpmfError> {
    let mut items = Vec::new();
    let mut off = 0;
    while off < data.len() {
        let rest = &data[off..];
        if rest.len() < 8 {
            // Trailing zero padding is harmless; anything else is a cut item.
            if rest.iter().all(|&b| b == 0) {
                break;
            }
            return Err(GpmfError::Truncated {
                key: FourCc(*b"????"),
                offset: base + off,
                need: 8,
                have: rest.len(),
            });
        }
        let key = FourCc([rest[0], rest[1], rest[2], rest[3]]);
        let type_char = rest[4];
        let struct_size = rest[5];
        let repeat = u16::from_be_bytes([rest[6], rest[7]]);
        let len = usize::from(struct_size) * usize::from(repeat);
        let padded = len.div_ceil(4) * 4;
        // The last item may omit its padding.
        if rest.len() - 8 < len {
            return Err(GpmfError::Truncated {
                key,
                offset: base + off,
                need: 8 + len,
                have: rest.len(),
            });
        }
        let body = &rest[8..8 + len];
        let payload = if type_char == 0 {
            if depth + 1 >= MAX_DEPTH {
                return Err(GpmfError::TooDeep { key });
            }
            Payload::Nested(parse_level(body, base + off + 8, depth + 1)?)
        } else {
            if type_size(type_char).is_none() && type_char != b'?' {
                return Err(GpmfError::UnknownType { key, type_char });
            }
            Payload::Data(body.to_vec())
        };
        items.push(Klv {
            key,
            type_char,
            struct_size,
            repeat,
            payload,
        });
        off += (8 + padded).min(rest.len());
    }
    Ok(items)
}

/// Size in bytes of one element of a GPMF type, None for unknown types and
/// for `?` (complex, sized by its TYPE).
pub fn type_size(t: u8) -> Option<usize> {
    Some(match t {
        b'b' | b'B' | b'c' => 1,
        b's' | b'S' => 2,
        b'f' | b'F' | b'l' | b'L' | b'q' => 4,
        b'd' | b'j' | b'J' | b'Q' => 8,
        b'G' | b'U' => 16,
        _ => return None,
    })
}

fn element(t: u8, b: &[u8]) -> Option<f64> {
    Some(match t {
        b'b' => f64::from(b[0] as i8),
        b'B' => f64::from(b[0]),
        b's' => f64::from(i16::from_be_bytes([b[0], b[1]])),
        b'S' => f64::from(u16::from_be_bytes([b[0], b[1]])),
        b'l' => f64::from(i32::from_be_bytes([b[0], b[1], b[2], b[3]])),
        b'L' => f64::from(u32::from_be_bytes([b[0], b[1], b[2], b[3]])),
        b'f' => f64::from(f32::from_be_bytes([b[0], b[1], b[2], b[3]])),
        // Q15.16 signed fixed point
        b'q' => f64::from(i32::from_be_bytes([b[0], b[1], b[2], b[3]])) / 65_536.0,
        b'd' => f64::from_be_bytes(b[..8].try_into().ok()?),
        b'j' => i64::from_be_bytes(b[..8].try_into().ok()?) as f64,
        b'J' => u64::from_be_bytes(b[..8].try_into().ok()?) as f64,
        // Q31.32 signed fixed point
        b'Q' => i64::from_be_bytes(b[..8].try_into().ok()?) as f64 / 4_294_967_296.0,
        _ => return None,
    })
}

impl Klv {
    pub fn data(&self) -> Option<&[u8]> {
        match &self.payload {
            Payload::Data(d) => Some(d),
            Payload::Nested(_) => None,
        }
    }

    pub fn children(&self) -> &[Klv] {
        match &self.payload {
            Payload::Nested(c) => c,
            Payload::Data(_) => &[],
        }
    }

    /// Decodes a numeric item into `repeat` rows of elements. `complex` is
    /// the stream's TYPE string, used only when the type is `?`.
    pub fn numbers(&self, complex: Option<&[u8]>) -> Result<Vec<Vec<f64>>, GpmfError> {
        let key = self.key;
        let data = self.data().ok_or(GpmfError::NotNumeric { key })?;
        let layout: Vec<u8> = if self.type_char == b'?' {
            complex.ok_or(GpmfError::MissingType { key })?.to_vec()
        } else {
            let size = type_size(self.type_char).ok_or(GpmfError::NotNumeric { key })?;
            if size == 0 || usize::from(self.struct_size) % size != 0 {
                return Err(GpmfError::BadStructSize {
                    key,
                    type_char: self.type_char,
                    struct_size: self.struct_size,
                });
            }
            vec![self.type_char; usize::from(self.struct_size) / size]
        };
        let mut row_size = 0;
        for &t in &layout {
            if matches!(t, b'c' | b'U' | b'F' | b'G') {
                return Err(GpmfError::NotNumeric { key });
            }
            row_size += type_size(t).ok_or(GpmfError::UnknownType { key, type_char: t })?;
        }
        if row_size != usize::from(self.struct_size) {
            return Err(GpmfError::BadStructSize {
                key,
                type_char: self.type_char,
                struct_size: self.struct_size,
            });
        }
        let rows = data
            .chunks_exact(row_size.max(1))
            .take(usize::from(self.repeat))
            .map(|row| {
                let mut out = Vec::with_capacity(layout.len());
                let mut at = 0;
                for &t in &layout {
                    let n = type_size(t).unwrap_or(0);
                    out.push(element(t, &row[at..at + n]).unwrap_or(f64::NAN));
                    at += n;
                }
                out
            })
            .collect();
        Ok(rows)
    }

    /// Text of a `c` item (Latin-1, as GoPro writes "m/s²"), trailing NULs
    /// and spaces removed.
    pub fn text(&self) -> Option<String> {
        if self.type_char != b'c' {
            return None;
        }
        let s: String = self.data()?.iter().map(|&b| b as char).collect();
        Some(s.trim_end_matches(['\0', ' ']).to_string())
    }

    /// Parses a `U` item (`yymmddhhmmss.sss`, UTC). None when malformed,
    /// which some firmware does (gpmf-parser issue #162).
    pub fn utc(&self) -> Option<DateTime<Utc>> {
        if self.type_char != b'U' {
            return None;
        }
        let s = std::str::from_utf8(self.data()?).ok()?;
        let s = s.trim_end_matches('\0');
        NaiveDateTime::parse_from_str(s, "%y%m%d%H%M%S%.f")
            .ok()
            .map(|n| n.and_utc())
    }
}

/// Divides decoded rows by SCAL: one value for every element, or one per
/// element of a row.
pub fn apply_scale(key: FourCc, rows: &mut [Vec<f64>], scal: &[f64]) -> Result<(), GpmfError> {
    for row in rows.iter_mut() {
        if scal.len() == 1 {
            row.iter_mut().for_each(|v| *v /= scal[0]);
        } else if scal.len() == row.len() {
            row.iter_mut().zip(scal).for_each(|(v, s)| *v /= s);
        } else {
            return Err(GpmfError::ScaleMismatch {
                key,
                scal: scal.len(),
                elements: row.len(),
            });
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Run the tests and verify that they pass**

Run: `cargo test -p actionlay-telemetry --lib`
Expected: PASS (`test result: ok. 21 passed`).

- [ ] **Step 5: Commit**

```bash
git add crates/telemetry
git commit -m "feat(telemetry): GPMF KLV parser"
```

---

### Task 5: Stream extraction and sample timing

**Files:**
- Create: `crates/telemetry/src/extract.rs`
- Modify: `crates/telemetry/src/lib.rs`, `crates/telemetry/src/test_support.rs`

**Interfaces:**
- Consumes: `gpmf::*` (Task 4), `GpsLock` (Task 3).
- Produces: `RawPacket { pts: f64, duration: f64, data: Vec<u8> }` (contract); `GpsPoint { packet, index, t, end, utc, lat, lon, alt, speed2d, speed3d, fix, dop, lock, derived }`, `Derived { lat, lon, alt, speed, cspeed, dist, codo, azi, cog, cgrad, accel: Option<f64> }`; crate-private `extract::extract(&[RawPacket]) -> Extracted { gps, accl, grav, ori, temp, duration, parsed_packets, first_error, warnings }`, `Sample<const N: usize> { t, end, v: [f64; N] }`, `orient(&str, &[f64]) -> Option<[f64; 3]>`, `cori_to_ori(&[f64]) -> [f64; 3]`.

What is extracted, per `STRM`: **GPS5** (lat, lon, alt, 2D, 3D speed; `GPSF` fix, `GPSP` DOP×100, `GPSU` UTC of the first sample), **GPS9** (per-sample days since 2000, seconds, DOP, fix; preferred over GPS5 when a file has both), **ACCL** (`SIUN` must be m/s²; every 10th sample of each packet; axes remapped by `ORIN`, default `ZXY`: the letter at position i names the camera axis that input column i feeds, lowercase negates — this reproduces the original's five-entry table), **GRAV** (a, b, c) → (a, −c, −b), **CORI** → (ori.pitch, ori.roll, ori.yaw) in degrees with the original's quirks (components read as w, x, z, y; roll and pitch swapped), **TMPC** (first in the packet, °C). A packet that fails to parse is skipped with a warning; a stream that fails to decode is skipped with a warning.

- [ ] **Step 1: Write the failing tests**

Append to `crates/telemetry/src/test_support.rs`:
```rust
pub fn i32s(values: &[i32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

pub fn i16s(values: &[i16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

/// A GPS5 point: lat, lon (deg), alt (m), 2D and 3D speed (m/s).
pub type Gps5 = [f64; 5];

const GPS5_SCAL: [i32; 5] = [10_000_000, 10_000_000, 1000, 1000, 100];

/// A `STRM` with GPSF, GPSU, GPSP and GPS5, as HERO5–10 write it.
pub fn gps5_stream(gpsu: &str, fix: u32, dop_x100: u16, points: &[Gps5]) -> Vec<u8> {
    let mut raw = Vec::new();
    for p in points {
        for (v, s) in p.iter().zip(GPS5_SCAL) {
            raw.push((v * f64::from(s)).round() as i32);
        }
    }
    nested(
        b"STRM",
        &[
            item(b"GPSF", b'L', 4, 1, &fix.to_be_bytes()),
            item(b"GPSU", b'U', 16, 1, gpsu.as_bytes()),
            item(b"GPSP", b'S', 2, 1, &dop_x100.to_be_bytes()),
            item(b"SCAL", b'l', 4, 5, &i32s(&GPS5_SCAL)),
            item(b"GPS5", b'l', 20, points.len() as u16, &i32s(&raw)),
        ],
    )
}

/// An accelerometer `STRM`: raw i16 samples divided by `scal` give m/s².
pub fn accl_stream(orin: Option<&str>, scal: i16, tmpc: f32, samples: &[[i16; 3]]) -> Vec<u8> {
    let mut children = vec![
        item(b"SIUN", b'c', 4, 1, b"m/s\xb2"),
        item(b"SCAL", b's', 2, 1, &scal.to_be_bytes()),
        item(b"TMPC", b'f', 4, 1, &tmpc.to_be_bytes()),
    ];
    if let Some(o) = orin {
        children.push(item(b"ORIN", b'c', 1, 3, o.as_bytes()));
    }
    let flat: Vec<i16> = samples.iter().flatten().copied().collect();
    children.push(item(b"ACCL", b's', 6, samples.len() as u16, &i16s(&flat)));
    nested(b"STRM", &children)
}

/// A `DEVC` holding `streams`: one demuxed packet.
pub fn devc(streams: &[Vec<u8>]) -> Vec<u8> {
    nested(b"DEVC", streams)
}
```

`crates/telemetry/src/extract.rs` (tests only for now):
```rust
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
}
```

`crates/telemetry/src/lib.rs`:
```rust
//! Telemetry of action-camera videos: GoPro GPMF parsing, derived metrics,
//! and sampling at any file time. Pure Rust; packets come from the caller
//! (the media crate demuxes them), so this crate never links FFmpeg.
#[allow(dead_code)] // used by telemetry.rs (Task 9)
mod extract;
pub mod gpmf;
pub mod metric;
#[cfg(test)]
mod test_support;
pub mod units;
mod value;

pub use extract::{Derived, GpsPoint};
pub use metric::Metric;
pub use value::{GpsLock, Value};

/// One demuxed packet of the GoPro metadata stream, times in seconds of
/// file time. The media crate's `GpmfPacket` converts into this.
#[derive(Debug, Clone, PartialEq)]
pub struct RawPacket {
    pub pts: f64,
    pub duration: f64,
    pub data: Vec<u8>,
}
```
(The `allow(dead_code)` goes away in Task 9, when `telemetry.rs` calls `extract`.)

- [ ] **Step 2: Run the tests and verify that they fail**

Run: `cargo test -p actionlay-telemetry --lib extract`
Expected: FAIL to compile (`extract`, `GpsPoint`, `orient`, `cori_to_ori` not found).

- [ ] **Step 3: Implementation**

Above the tests in `crates/telemetry/src/extract.rs`:
```rust
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
                let r = if s.has(b"GPS9") {
                    s.gps9(k, p, &mut gps9)
                } else if s.has(b"GPS5") {
                    s.gps5(k, p, &mut out.gps)
                } else if s.has(b"ACCL") {
                    s.accl(p, &mut out.accl, &mut out.warnings)
                } else if s.has(b"GRAV") {
                    s.grav(p, &mut out.grav)
                } else if s.has(b"CORI") {
                    s.cori(p, &mut out.ori)
                } else {
                    Ok(())
                };
                if let Err(e) = r {
                    out.warnings
                        .push(format!("gpmd packet {k}: stream skipped: {e}"));
                }
            }
        }
    }
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
        warnings: &mut Vec<String>,
    ) -> Result<(), GpmfError> {
        if let Some(unit) = self.get(b"SIUN").and_then(Klv::text)
            && unit != "m/s²"
        {
            warnings.push(format!("ACCL in unsupported unit {unit:?}, ignored"));
            return Ok(());
        }
        let orin = self.get(b"ORIN").and_then(Klv::text);
        let orin = match orin.as_deref() {
            Some(o) if orient(o, &[0.0; 3]).is_some() => o.to_string(),
            Some(o) => {
                warnings.push(format!("unknown ORIN {o:?}, using {DEFAULT_ORIN}"));
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
```

- [ ] **Step 4: Run the tests and verify that they pass**

Run: `cargo test -p actionlay-telemetry --lib && cargo clippy -p actionlay-telemetry --all-targets -- -D warnings`
Expected: PASS (`test result: ok. 29 passed`), no clippy warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/telemetry
git commit -m "feat(telemetry): extract GPS, IMU and temperature samples from GPMF"
```

---

### Task 6: Smoothing filters and GPS lock filter

**Files:**
- Create: `crates/telemetry/src/smoothing.rs`, `crates/telemetry/src/lock.rs`
- Modify: `crates/telemetry/src/lib.rs`

**Interfaces:**
- Consumes: `GpsPoint`, `Derived` (Task 5), `GpsLock` (Task 3).
- Produces: crate-private `smoothing::{Kalman { update(&mut self, f64) -> f64 }, Ses { new(alpha) , update(&mut self, [f64; 2]) -> [f64; 2] }}`, `lock::apply(&mut [GpsPoint], &LockOptions)`; public `LockOptions { dop_max: f64, speed_max: Option<f64> }` (default 10.0, None).

Ported from `gopro_overlay/smoothing.py` and `gpmd_filters.py` (0.134.0):
- Kalman: `est₀ = u₀`; then `K = P/(P+R)`, `est += K·(u − est)`, `P = (1−K)·P + Q` with R = 100, Q = 10, P₀ = 0.
- SES: first output = first input; then `out = α·previous_input + (1−α)·previous_output` (it lags one sample), α = 0.45.
- Lock: DOP > `dop_max` → no lock; speed > `speed_max` (if set) → no lock; and the `GPSLockTracker` heuristic: a point claiming a fix right after an unlocked point, with the same position or the same 2D speed, keeps the unlocked state (the heuristic's memory is only updated by points it accepts).

- [ ] **Step 1: Write the failing tests**

`crates/telemetry/src/smoothing.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kalman_matches_the_original() {
        // Values printed by gopro_overlay.smoothing.Kalman for 1, 2, 3, 3.
        let mut k = Kalman::default();
        let out: Vec<f64> = [1.0, 2.0, 3.0, 3.0].iter().map(|&u| k.update(u)).collect();
        assert_eq!(
            out,
            vec![
                1.0,
                1.090_909_090_909_090_8,
                1.396_946_564_885_496_2,
                1.728_043_609_933_373_8
            ]
        );
        let mut k = Kalman::default();
        assert!((0..100).all(|_| k.update(4.2) == 4.2));
    }

    #[test]
    fn ses_lags_one_sample() {
        let mut s = Ses::new(0.45);
        assert_eq!(s.update([0.0, 0.0]), [0.0, 0.0]);
        assert_eq!(s.update([1.0, 10.0]), [0.0, 0.0]);
        assert_eq!(s.update([2.0, 20.0]), [0.45, 4.5]);
        // 0.45·2 + 0.55·0.45
        let out = s.update([3.0, 30.0]);
        assert!((out[0] - 1.1475).abs() < 1e-12);
    }
}
```

`crates/telemetry/src/lock.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::Derived;

    pub(crate) fn point(fix: u32, dop: f64, lat: f64, speed: f64) -> GpsPoint {
        GpsPoint {
            packet: 0,
            index: 0,
            t: 0.0,
            end: 0.0,
            utc: None,
            lat,
            lon: 7.0,
            alt: 100.0,
            speed2d: speed,
            speed3d: speed,
            fix,
            dop,
            lock: GpsLock::from_fix(fix),
            derived: Derived::default(),
        }
    }

    fn locks(points: &mut [GpsPoint], opts: LockOptions) -> Vec<GpsLock> {
        apply(points, &opts);
        points.iter().map(|p| p.lock).collect()
    }

    #[test]
    fn dop_above_limit_is_not_locked() {
        let mut pts = [
            point(3, 10.0, 45.0, 1.0),
            point(3, 10.01, 45.1, 2.0),
            point(2, 3.0, 45.2, 3.0),
        ];
        assert_eq!(
            locks(&mut pts, LockOptions::default()),
            vec![GpsLock::Lock3d, GpsLock::NoLock, GpsLock::Lock2d]
        );
    }

    #[test]
    fn speed_limit_is_optional() {
        let mut pts = [point(3, 1.0, 45.0, 20.0)];
        assert_eq!(
            locks(&mut pts, LockOptions::default()),
            vec![GpsLock::Lock3d]
        );
        let opts = LockOptions {
            speed_max: Some(60.0 / 3.6),
            ..LockOptions::default()
        };
        assert_eq!(locks(&mut pts, opts), vec![GpsLock::NoLock]);
    }

    #[test]
    fn fix_repeating_the_unlocked_reading_is_ignored() {
        let mut pts = [
            point(0, 99.99, 45.0, 0.0),
            point(3, 2.0, 45.0, 1.0), // same position as the unlocked point
            point(3, 2.0, 45.1, 0.0), // same speed as the unlocked point
            point(3, 2.0, 45.2, 2.0), // genuinely new
            point(3, 2.0, 45.2, 2.0), // repeats, but the previous was locked
        ];
        assert_eq!(
            locks(&mut pts, LockOptions::default()),
            vec![
                GpsLock::NoLock,
                GpsLock::NoLock,
                GpsLock::NoLock,
                GpsLock::Lock3d,
                GpsLock::Lock3d
            ]
        );
    }
}
```

`crates/telemetry/src/lib.rs`:
```rust
//! Telemetry of action-camera videos: GoPro GPMF parsing, derived metrics,
//! and sampling at any file time. Pure Rust; packets come from the caller
//! (the media crate demuxes them), so this crate never links FFmpeg.
#[allow(dead_code)] // used by telemetry.rs (Task 9)
mod extract;
pub mod gpmf;
#[allow(dead_code)] // used by telemetry.rs (Task 9)
mod lock;
pub mod metric;
#[allow(dead_code)] // used by derive.rs (Task 7)
mod smoothing;
#[cfg(test)]
mod test_support;
pub mod units;
mod value;

pub use extract::{Derived, GpsPoint};
pub use lock::LockOptions;
pub use metric::Metric;
pub use value::{GpsLock, Value};

/// One demuxed packet of the GoPro metadata stream, times in seconds of
/// file time. The media crate's `GpmfPacket` converts into this.
#[derive(Debug, Clone, PartialEq)]
pub struct RawPacket {
    pub pts: f64,
    pub duration: f64,
    pub data: Vec<u8>,
}
```

- [ ] **Step 2: Run the tests and verify that they fail**

Run: `cargo test -p actionlay-telemetry --lib`
Expected: FAIL to compile (`Kalman`, `Ses`, `apply`, `LockOptions` not found).

- [ ] **Step 3: Implementation**

Above the tests in `crates/telemetry/src/smoothing.rs`:
```rust
//! The two filters gopro-dashboard-overlay applies, ported for parity
//! (`gopro_overlay/smoothing.py`).

/// Scalar Kalman filter with R = 100, Q = 10, H = 1, P₀ = 0. The first
/// update seeds the estimate, so a constant input passes through unchanged.
#[derive(Debug, Clone, Default)]
pub(crate) struct Kalman {
    p: f64,
    estimate: Option<f64>,
}

impl Kalman {
    const R: f64 = 100.0;
    const Q: f64 = 10.0;

    pub fn update(&mut self, u: f64) -> f64 {
        let est = *self.estimate.get_or_insert(u);
        let k = self.p / (self.p + Self::R);
        let est = est + k * (u - est);
        self.p = (1.0 - k) * self.p + Self::Q;
        self.estimate = Some(est);
        est
    }
}

/// gopro-dashboard-overlay's "simple exponential" smoothing of positions.
/// Its output at step n is `α·x[n−1] + (1−α)·out[n−1]` (it lags one sample),
/// and the first two outputs both equal the first input.
#[derive(Debug, Clone)]
pub(crate) struct Ses {
    alpha: f64,
    previous: Option<[f64; 2]>,
    forecast: Option<[f64; 2]>,
}

impl Ses {
    pub fn new(alpha: f64) -> Self {
        Ses {
            alpha,
            previous: None,
            forecast: None,
        }
    }

    pub fn update(&mut self, x: [f64; 2]) -> [f64; 2] {
        let out = match (self.forecast, self.previous) {
            (Some(f), Some(p)) => {
                let a = self.alpha;
                [a * p[0] + (1.0 - a) * f[0], a * p[1] + (1.0 - a) * f[1]]
            }
            _ => x,
        };
        self.forecast = Some(out);
        self.previous = Some(x);
        out
    }
}
```

Above the tests in `crates/telemetry/src/lock.rs`:
```rust
//! GPS lock filter, after gopro-dashboard-overlay's `gpmd_filters.standard`.
use crate::{extract::GpsPoint, value::GpsLock};

/// When a recorded fix is downgraded to "no lock".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LockOptions {
    /// Points with a DOP above this are not locked (original default: 10).
    pub dop_max: f64,
    /// Points faster than this (m/s) are not locked. The original defaults
    /// to 60 km/h, which would blank every car or motorbike video, so
    /// ActionLay leaves it off by default.
    pub speed_max: Option<f64>,
}

impl Default for LockOptions {
    fn default() -> Self {
        LockOptions {
            dop_max: 10.0,
            speed_max: None,
        }
    }
}

/// Sets `lock` on every point. Besides the DOP and speed limits it keeps the
/// original's heuristic: a point that claims a fix right after a point
/// without one, but repeats that point's position or speed, is a stale
/// reading and keeps the previous (unlocked) state.
pub(crate) fn apply(points: &mut [GpsPoint], opts: &LockOptions) {
    // The last point the heuristic accepted: (lock, lat, lon, speed2d).
    let mut last: Option<(GpsLock, f64, f64, f64)> = None;
    for p in points.iter_mut() {
        let recorded = GpsLock::from_fix(p.fix);
        let mut lock = recorded;
        match last {
            Some((prev, lat, lon, speed))
                if recorded.is_locked()
                    && !prev.is_locked()
                    && ((p.lat == lat && p.lon == lon) || p.speed2d == speed) =>
            {
                lock = prev;
            }
            _ => last = Some((recorded, p.lat, p.lon, p.speed2d)),
        }
        if p.dop > opts.dop_max || opts.speed_max.is_some_and(|max| p.speed2d > max) {
            lock = GpsLock::NoLock;
        }
        p.lock = lock;
    }
}
```

- [ ] **Step 4: Run the tests and verify that they pass**

Run: `cargo test -p actionlay-telemetry --lib`
Expected: PASS (`test result: ok. 34 passed`). The Kalman test asserts the exact values the Python original prints for inputs 1, 2, 3, 3.

- [ ] **Step 5: Commit**

```bash
git add crates/telemetry
git commit -m "feat(telemetry): Kalman/SES filters and GPS lock filter"
```

---

### Task 7: Derived metrics

**Files:**
- Create: `crates/telemetry/src/derive.rs`
- Modify: `crates/telemetry/src/lib.rs`

**Interfaces:**
- Consumes: `GpsPoint`, `Derived` (Task 5), `Kalman`, `Ses` (Task 6), `geographiclib_rs::{Geodesic, InverseGeodesic}`.
- Produces: crate-private `derive::derive(&mut [GpsPoint])` (fills `GpsPoint::derived`), `derive::PAIR_SKIP = 54`.

Definitions (S = 54, "locked" = 2D or 3D after the lock filter, points in time order; positions in steps 2 and 5 are the SES-smoothed ones):

| Metric | Definition | Stored on |
|---|---|---|
| `lat`, `lon` | SES(α = 0.45) of the positions of locked points | each locked point |
| `alt` | recorded altitude | each locked point |
| `speed` | Kalman over the recorded 2D speed of *every* point | each locked point |
| `cspeed` | Kalman(d / dt) for pairs (i, i+S) with both locked; 0 when dt ≤ 0. d = WGS84 geodesic distance, dt = UTC difference (file time if a UTC is missing). One Kalman state runs over the forward pairs, then over the last S points paired backwards | i (forward), j (last S, pair (j−S, j)) |
| `dist` | d / S of the same pair (the original's per-point distance) | as cspeed |
| `azi` | initial geodesic bearing of the pair, −180…180° | as cspeed |
| `cog` | `azi` if ≥ 0 else `azi + 360` | as cspeed |
| `codo` (= `odo`) | running sum of `dist` over locked points, in order | each locked point |
| `accel` | `(v[i+S] − v[i]) / dt` on the recorded 2D speed for every forward pair, locked or not; 0 when either speed or dt is 0 | i+S |
| `cgrad` (= `gradient`) | `100·(alt[b] − alt[a]) / d` for pairs with both points 3D-locked and both altitudes ≠ 0, kept when d > 1 m and the result is within ±45 % | as cspeed |

Finally every derived value of a point that is not locked is cleared. With n ≤ S there are no pairs; with S < n < 2S the backward pass covers only j ≥ S (deliberate difference 4).

- [ ] **Step 1: Write the failing tests**

`crates/telemetry/src/derive.rs` (tests only for now):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, TimeDelta, Utc};

    /// 1 m in latitude at 45° N (WGS84).
    const DEG_PER_M: f64 = 1.0 / 111_131.745;
    const RATE: f64 = 18.0;

    fn utc0() -> DateTime<Utc> {
        "2024-05-01T10:00:00Z".parse().unwrap()
    }

    /// n points heading north at `speed` m/s, climbing `climb` m per second.
    fn track(n: usize, speed: f64, climb: f64) -> Vec<GpsPoint> {
        (0..n)
            .map(|i| {
                let t = i as f64 / RATE;
                GpsPoint {
                    packet: i / 18,
                    index: i % 18,
                    t,
                    end: t + 1.0 / RATE,
                    utc: Some(utc0() + TimeDelta::microseconds((t * 1e6).round() as i64)),
                    lat: 45.0 + speed * t * DEG_PER_M,
                    lon: 7.0,
                    alt: 100.0 + climb * t,
                    speed2d: speed,
                    speed3d: speed,
                    fix: 3,
                    dop: 1.0,
                    lock: GpsLock::Lock3d,
                    derived: Derived::default(),
                }
            })
            .collect()
    }

    #[test]
    fn steady_northbound_track() {
        let mut pts = track(200, 1.0, 0.1);
        derive(&mut pts);
        let d = pts[100].derived;
        assert!((d.cspeed.unwrap() - 1.0).abs() < 0.01, "{d:?}");
        assert!((d.dist.unwrap() - 1.0 / RATE).abs() < 0.001, "{d:?}");
        assert!(d.azi.unwrap().abs() < 1e-6, "{d:?}");
        let cog = d.cog.unwrap();
        assert!(cog < 1e-6 || cog > 360.0 - 1e-6, "{d:?}");
        // climb 0.1 m/s at 1 m/s → 10 %
        assert!((d.cgrad.unwrap() - 10.0).abs() < 0.1, "{d:?}");
        assert_eq!(d.accel, Some(0.0));
        assert_eq!(d.speed, Some(1.0));
        assert_eq!(d.alt, Some(pts[100].alt));
        // smoothing lags the raw position by about 2.2 samples
        let lag_m = (pts[100].lat - d.lat.unwrap()) / DEG_PER_M;
        assert!((lag_m - 2.22 / RATE).abs() < 0.01, "lag {lag_m}");
    }

    #[test]
    fn every_point_of_a_long_locked_track_gets_values() {
        let mut pts = track(200, 1.0, 0.1);
        derive(&mut pts);
        for (i, p) in pts.iter().enumerate() {
            let d = p.derived;
            assert!(
                d.cspeed.is_some() && d.dist.is_some() && d.cgrad.is_some(),
                "point {i}"
            );
            assert_eq!(d.accel.is_some(), i >= PAIR_SKIP, "point {i}");
        }
    }

    #[test]
    fn odometer_sums_dist() {
        let mut pts = track(200, 2.0, 0.0);
        derive(&mut pts);
        let mut sum = 0.0;
        for p in &pts {
            sum += p.derived.dist.unwrap();
            assert_eq!(p.derived.codo, Some(sum));
        }
        // ~ 2 m/s × 199/18 s, a little less while the smoothing settles
        let codo = pts.last().unwrap().derived.codo.unwrap();
        assert!((codo - 2.0 * 199.0 / RATE).abs() < 0.3, "codo {codo}");
    }

    #[test]
    fn accel_uses_recorded_speed() {
        let mut pts = track(120, 1.0, 0.0);
        for (i, p) in pts.iter_mut().enumerate() {
            p.speed2d = 1.0 + i as f64 / RATE; // +1 m/s per second
        }
        derive(&mut pts);
        assert_eq!(pts[10].derived.accel, None);
        assert!((pts[60].derived.accel.unwrap() - 1.0).abs() < 1e-6);
        // a standstill at either end gives 0, as in the original
        pts[0].speed2d = 0.0;
        derive(&mut pts);
        assert_eq!(pts[PAIR_SKIP].derived.accel, Some(0.0));
    }

    #[test]
    fn steep_or_short_gradients_are_dropped() {
        let mut pts = track(120, 1.0, 0.5); // 50 %: rejected
        derive(&mut pts);
        assert!(pts.iter().all(|p| p.derived.cgrad.is_none()));
        let mut pts = track(120, 0.2, 0.01); // 0.6 m per pair: too short
        derive(&mut pts);
        assert!(pts.iter().all(|p| p.derived.cgrad.is_none()));
    }

    #[test]
    fn unlocked_points_have_no_derived_values_and_odo_continues() {
        let mut pts = track(200, 1.0, 0.0);
        for p in &mut pts[60..80] {
            p.lock = GpsLock::NoLock;
        }
        derive(&mut pts);
        for p in &pts[60..80] {
            assert_eq!(p.derived, Derived::default());
        }
        // pairs that reach into the gap are skipped …
        assert_eq!(pts[10].derived.cspeed, None);
        // … pairs that jump over it are kept, as in the original
        assert!(pts[40].derived.cspeed.is_some());
        let before = pts[59].derived.codo.unwrap();
        let after = pts[80].derived.codo.unwrap();
        assert!(after >= before);
    }

    #[test]
    fn two_d_points_get_speed_but_no_gradient() {
        let mut pts = track(200, 1.0, 0.1);
        for p in &mut pts {
            p.lock = GpsLock::Lock2d;
        }
        derive(&mut pts);
        assert!(pts[100].derived.cspeed.is_some());
        assert!(pts.iter().all(|p| p.derived.cgrad.is_none()));
    }

    #[test]
    fn tracks_shorter_than_two_windows_do_not_wrap_around() {
        let mut pts = track(80, 1.0, 0.0);
        derive(&mut pts);
        // forward pairs for i < 26, backward pairs for j ≥ 54
        assert!(pts[25].derived.cspeed.is_some());
        assert!(pts[26..54].iter().all(|p| p.derived.cspeed.is_none()));
        assert!(pts[54].derived.cspeed.is_some());
    }

    #[test]
    fn short_tracks_have_no_pairs() {
        let mut pts = track(PAIR_SKIP, 1.0, 0.0);
        derive(&mut pts);
        for p in &pts {
            let d = p.derived;
            assert!(d.lat.is_some() && d.speed.is_some() && d.codo == Some(0.0));
            assert!(d.cspeed.is_none() && d.dist.is_none() && d.accel.is_none());
        }
        derive(&mut []);
    }
}
```

`crates/telemetry/src/lib.rs`:
```rust
//! Telemetry of action-camera videos: GoPro GPMF parsing, derived metrics,
//! and sampling at any file time. Pure Rust; packets come from the caller
//! (the media crate demuxes them), so this crate never links FFmpeg.
#[allow(dead_code)] // used by telemetry.rs (Task 9)
mod derive;
#[allow(dead_code)] // used by telemetry.rs (Task 9)
mod extract;
pub mod gpmf;
#[allow(dead_code)] // used by telemetry.rs (Task 9)
mod lock;
pub mod metric;
#[allow(dead_code)] // used by telemetry.rs (Task 9)
mod smoothing;
#[cfg(test)]
mod test_support;
pub mod units;
mod value;

pub use extract::{Derived, GpsPoint};
pub use lock::LockOptions;
pub use metric::Metric;
pub use value::{GpsLock, Value};

/// One demuxed packet of the GoPro metadata stream, times in seconds of
/// file time. The media crate's `GpmfPacket` converts into this.
#[derive(Debug, Clone, PartialEq)]
pub struct RawPacket {
    pub pts: f64,
    pub duration: f64,
    pub data: Vec<u8>,
}
```

- [ ] **Step 2: Run the tests and verify that they fail**

Run: `cargo test -p actionlay-telemetry --lib derive`
Expected: FAIL to compile (`derive`, `PAIR_SKIP` not found).

- [ ] **Step 3: Implementation**

Above the tests in `crates/telemetry/src/derive.rs`:
```rust
//! Metrics derived from the GPS track, computed as gopro-dashboard-overlay
//! 0.134.0 computes them for a dashboard (`gopro-dashboard.py`, the
//! "processing" block; `timeseries_process.py`; `framemeta.py`).
//!
//! Points are visited in time order; "locked" means a 2D or 3D fix after the
//! lock filter. With S = 54 (3 s at 18 Hz):
//!
//! 1. **lat/lon** of locked points are smoothed with [`Ses`] (α = 0.45).
//! 2. For each pair (i, i+S) with both points locked, on the smoothed
//!    positions: `d` = WGS84 geodesic distance, `azi` = initial bearing
//!    (−180…180°), `dt` = UTC difference (file time if a UTC is missing),
//!    `cspeed` = Kalman(d / dt, or 0 when dt ≤ 0), `dist` = d / S,
//!    `cog` = azi mod 360. Pairs are stored on point i for i < n−S; then the
//!    last S points get the pair (j−S, j) stored on j, continuing the same
//!    Kalman state.
//! 3. **codo** = running sum of `dist` over locked points.
//! 4. **accel** on point i+S for every pair (i, i+S), locked or not:
//!    `(v[i+S] − v[i]) / dt` on the recorded 2D speed, or 0 when either
//!    speed or dt is 0.
//! 5. **cgrad** for pairs with both points 3D-locked and both altitudes
//!    non-zero: `100·Δalt / d` when d > 1 m and |result| < 45, stored like
//!    step 2.
//! 6. **speed** = Kalman over the recorded 2D speed of every point.
//! 7. Every derived value of a point that is not locked is cleared.
//!
//! With n ≤ S points there are no pairs and nothing but lat/lon/alt/speed/codo
//! is derived. With S < n < 2S the backward pass only covers points j ≥ S.
//! (The original wraps around to the end of the track with negative indices
//! in both cases; recordings shorter than ~6 s of GPS are affected.)
use geographiclib_rs::{Geodesic, InverseGeodesic};

use crate::{
    extract::{Derived, GpsPoint},
    smoothing::{Kalman, Ses},
    value::GpsLock,
};

pub(crate) const PAIR_SKIP: usize = 54;
const SES_ALPHA: f64 = 0.45;
const CGRAD_MIN_DIST: f64 = 1.0;
const CGRAD_MAX_ABS: f64 = 45.0;

fn locked(p: &GpsPoint) -> bool {
    p.lock.is_locked()
}

fn locked_3d(p: &GpsPoint) -> bool {
    p.lock == GpsLock::Lock3d
}

fn dt(a: &GpsPoint, b: &GpsPoint) -> f64 {
    match (a.utc, b.utc) {
        (Some(ua), Some(ub)) => (ub - ua).as_seconds_f64(),
        _ => b.t - a.t,
    }
}

/// Distance (m) and initial bearing (°) between two smoothed positions.
fn inverse(a: &[f64; 2], b: &[f64; 2]) -> (f64, f64) {
    let (s12, azi1, _azi2, _a12): (f64, f64, f64, f64) =
        Geodesic::wgs84().inverse(a[0], a[1], b[0], b[1]);
    (s12, azi1)
}

pub(crate) fn derive(points: &mut [GpsPoint]) {
    let n = points.len();
    for p in points.iter_mut() {
        p.derived = Derived::default();
    }

    // 1. smoothed positions (raw for points that are not locked)
    let mut ses = Ses::new(SES_ALPHA);
    let pos: Vec<[f64; 2]> = points
        .iter()
        .map(|p| {
            if locked(p) {
                ses.update([p.lat, p.lon])
            } else {
                [p.lat, p.lon]
            }
        })
        .collect();

    // the forward pairs, then the last S points paired backwards
    let pairs: Vec<(usize, usize, usize)> = if n > PAIR_SKIP {
        (0..n - PAIR_SKIP)
            .map(|i| (i, i + PAIR_SKIP, i))
            .chain(((n - PAIR_SKIP).max(PAIR_SKIP)..n).map(|j| (j - PAIR_SKIP, j, j)))
            .collect()
    } else {
        Vec::new()
    };

    // 2. speeds, distance, bearing
    let mut kalman = Kalman::default();
    for &(a, b, store) in &pairs {
        if !(locked(&points[a]) && locked(&points[b])) {
            continue;
        }
        let (d, azi) = inverse(&pos[a], &pos[b]);
        let dt = dt(&points[a], &points[b]);
        let raw = if dt > 0.0 { d / dt } else { 0.0 };
        let out = &mut points[store].derived;
        out.cspeed = Some(kalman.update(raw));
        out.dist = Some(d / PAIR_SKIP as f64);
        out.azi = Some(azi);
        out.cog = Some(if azi >= 0.0 { azi } else { azi + 360.0 });
    }

    // 3. odometer
    let mut total = 0.0;
    for p in points.iter_mut().filter(|p| p.lock.is_locked()) {
        total += p.derived.dist.unwrap_or(0.0);
        p.derived.codo = Some(total);
    }

    // 4. acceleration from the recorded speed (forward pairs only)
    for i in 0..n.saturating_sub(PAIR_SKIP) {
        let (a, b) = (&points[i], &points[i + PAIR_SKIP]);
        let dt = dt(a, b);
        let accel = if a.speed2d != 0.0 && b.speed2d != 0.0 && dt != 0.0 {
            (b.speed2d - a.speed2d) / dt
        } else {
            0.0
        };
        points[i + PAIR_SKIP].derived.accel = Some(accel);
    }

    // 5. gradient
    for &(a, b, store) in &pairs {
        let (pa, pb) = (&points[a], &points[b]);
        if !(locked_3d(pa) && locked_3d(pb)) || pa.alt == 0.0 || pb.alt == 0.0 {
            continue;
        }
        let gain = pb.alt - pa.alt;
        let (d, _) = inverse(&pos[a], &pos[b]);
        if d > CGRAD_MIN_DIST {
            let grad = gain / d * 100.0;
            if grad.abs() < CGRAD_MAX_ABS {
                points[store].derived.cgrad = Some(grad);
            }
        }
    }

    // 6. smoothed speed, 7. clear what is not locked
    let mut kalman = Kalman::default();
    for (p, smoothed) in points.iter_mut().zip(&pos) {
        let speed = kalman.update(p.speed2d);
        if locked(p) {
            let d = &mut p.derived;
            d.lat = Some(smoothed[0]);
            d.lon = Some(smoothed[1]);
            d.alt = Some(p.alt);
            d.speed = Some(speed);
        } else {
            p.derived = Derived::default();
        }
    }
}
```

- [ ] **Step 4: Run the tests and verify that they pass**

Run: `cargo test -p actionlay-telemetry --lib`
Expected: PASS (`test result: ok. 43 passed`).

- [ ] **Step 5: Commit**

```bash
git add crates/telemetry
git commit -m "feat(telemetry): derived metrics as in gopro-dashboard-overlay"
```

---

### Task 8: Series: sampling, Stale/Absent, coverage

**Files:**
- Create: `crates/telemetry/src/series.rs`
- Modify: `crates/telemetry/src/lib.rs`

**Interfaces:**
- Consumes: `Value` (Task 3).
- Produces: crate-private `series::{Series::new(Interp, impl IntoIterator<Item = (f64, f64, Option<f64>)>) -> Series, Series::sample(&self, t) -> Value, Series::covered(&self) -> Vec<(f64, f64)>, Interp { Linear, Step }, gaps_and_coverage(&[(f64, f64)], duration) -> (Vec<(f64, f64)>, f64), MAX_BRIDGE = 2.0}`.

Rules of `sample(t)`, with k the last sample starting at or before t:
- no such k → **Absent**;
- sample k valid, sample k+1 valid, `t[k+1] − t[k] ≤ 2 s` and `t < t[k+1]` → **Present** (linear interpolation, or k's value for `Step`);
- sample k valid and `t < end[k]` → **Present**(v[k]);
- otherwise, with j the last valid sample at or before k → **Stale** { v[j], age = t − end[j] }; no valid j → **Absent**.
Non-finite values count as invalid. `covered()` returns the merged intervals where `sample` is Present; `gaps_and_coverage` clips them to `[0, duration]`.

- [ ] **Step 1: Write the failing tests**

`crates/telemetry/src/series.rs` (tests only for now):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn s(interp: Interp, samples: &[(f64, f64, Option<f64>)]) -> Series {
        Series::new(interp, samples.iter().copied())
    }

    #[test]
    fn interpolates_between_valid_neighbours() {
        let ser = s(
            Interp::Linear,
            &[(0.0, 1.0, Some(0.0)), (1.0, 2.0, Some(10.0))],
        );
        assert_eq!(ser.sample(0.5), Value::Present(5.0));
        assert_eq!(ser.sample(1.5), Value::Present(10.0));
        assert_eq!(
            ser.sample(2.5),
            Value::Stale {
                value: 10.0,
                age: 0.5
            }
        );
        assert_eq!(ser.sample(-0.1), Value::Absent);
        let step = s(
            Interp::Step,
            &[(0.0, 1.0, Some(0.0)), (1.0, 2.0, Some(10.0))],
        );
        assert_eq!(step.sample(0.9), Value::Present(0.0));
    }

    #[test]
    fn invalid_samples_make_the_last_value_stale() {
        let ser = s(
            Interp::Linear,
            &[
                (0.0, 1.0, Some(1.0)),
                (1.0, 2.0, None),
                (2.0, 3.0, Some(3.0)),
            ],
        );
        assert_eq!(ser.sample(0.5), Value::Present(1.0));
        assert_eq!(
            ser.sample(1.25),
            Value::Stale {
                value: 1.0,
                age: 0.25
            }
        );
        assert_eq!(ser.sample(2.5), Value::Present(3.0));
        assert_eq!(ser.covered(), vec![(0.0, 1.0), (2.0, 3.0)]);
    }

    #[test]
    fn leading_invalid_samples_are_absent() {
        let ser = s(Interp::Linear, &[(0.0, 1.0, None), (1.0, 2.0, Some(5.0))]);
        assert_eq!(ser.sample(0.5), Value::Absent);
        assert_eq!(ser.sample(1.5), Value::Present(5.0));
        assert_eq!(ser.covered(), vec![(1.0, 2.0)]);
        assert!(s(Interp::Linear, &[(0.0, 1.0, None)]).covered().is_empty());
        assert_eq!(s(Interp::Linear, &[]).sample(1.0), Value::Absent);
    }

    #[test]
    fn long_holes_are_not_bridged() {
        let ser = s(
            Interp::Linear,
            &[(0.0, 0.1, Some(0.0)), (5.0, 5.1, Some(10.0))],
        );
        assert_eq!(
            ser.sample(1.0),
            Value::Stale {
                value: 0.0,
                age: 0.9
            }
        );
        assert_eq!(ser.covered(), vec![(0.0, 0.1), (5.0, 5.1)]);
        // a missing packet (1 s) is bridged
        let ser = s(
            Interp::Linear,
            &[(0.0, 0.1, Some(0.0)), (1.0, 1.1, Some(10.0))],
        );
        assert_eq!(ser.sample(0.5), Value::Present(5.0));
        assert_eq!(ser.covered(), vec![(0.0, 1.1)]);
    }

    #[test]
    fn non_finite_values_are_invalid() {
        let ser = s(
            Interp::Linear,
            &[(0.0, 1.0, Some(f64::NAN)), (1.0, 2.0, Some(1.0))],
        );
        assert_eq!(ser.sample(0.5), Value::Absent);
    }

    #[test]
    fn coverage_and_gaps() {
        let (gaps, cov) = gaps_and_coverage(&[(0.0, 1.0), (2.0, 3.0)], 4.0);
        assert_eq!(gaps, vec![(1.0, 2.0), (3.0, 4.0)]);
        assert!((cov - 0.5).abs() < 1e-12);
        let (gaps, cov) = gaps_and_coverage(&[], 4.0);
        assert_eq!((gaps, cov), (vec![(0.0, 4.0)], 0.0));
        let (gaps, cov) = gaps_and_coverage(&[(-1.0, 9.0)], 4.0);
        assert_eq!((gaps, cov), (vec![], 1.0));
        assert_eq!(gaps_and_coverage(&[(0.0, 1.0)], 0.0), (vec![], 0.0));
    }
}
```

`crates/telemetry/src/lib.rs`:
```rust
//! Telemetry of action-camera videos: GoPro GPMF parsing, derived metrics,
//! and sampling at any file time. Pure Rust; packets come from the caller
//! (the media crate demuxes them), so this crate never links FFmpeg.
#[allow(dead_code)] // used by telemetry.rs (Task 9)
mod derive;
#[allow(dead_code)] // used by telemetry.rs (Task 9)
mod extract;
pub mod gpmf;
#[allow(dead_code)] // used by telemetry.rs (Task 9)
mod lock;
pub mod metric;
#[allow(dead_code)] // used by telemetry.rs (Task 9)
mod series;
#[allow(dead_code)] // used by telemetry.rs (Task 9)
mod smoothing;
#[cfg(test)]
mod test_support;
pub mod units;
mod value;

pub use extract::{Derived, GpsPoint};
pub use lock::LockOptions;
pub use metric::Metric;
pub use value::{GpsLock, Value};

/// One demuxed packet of the GoPro metadata stream, times in seconds of
/// file time. The media crate's `GpmfPacket` converts into this.
#[derive(Debug, Clone, PartialEq)]
pub struct RawPacket {
    pub pts: f64,
    pub duration: f64,
    pub data: Vec<u8>,
}
```

- [ ] **Step 2: Run the tests and verify that they fail**

Run: `cargo test -p actionlay-telemetry --lib series`
Expected: FAIL to compile (`Series`, `Interp`, `gaps_and_coverage` not found).

- [ ] **Step 3: Implementation**

Above the tests in `crates/telemetry/src/series.rs`:
```rust
//! One metric over time: sampling at any t, and where the data is valid.
use crate::value::Value;

/// Neighbouring valid samples further apart than this are not bridged.
pub(crate) const MAX_BRIDGE: f64 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Interp {
    /// Linear between neighbouring valid samples.
    Linear,
    /// Hold the earlier sample (angles, discrete values).
    Step,
}

/// Samples of one metric. Sample k starts at `t[k]`, represents the time
/// until `end[k]` and is valid when `v[k]` is Some.
#[derive(Debug, Clone)]
pub(crate) struct Series {
    t: Vec<f64>,
    end: Vec<f64>,
    v: Vec<Option<f64>>,
    /// Index of the last valid sample at or before k.
    last_valid: Vec<Option<usize>>,
    interp: Interp,
}

impl Series {
    /// `samples` must be sorted by start time.
    pub fn new(interp: Interp, samples: impl IntoIterator<Item = (f64, f64, Option<f64>)>) -> Self {
        let mut s = Series {
            t: Vec::new(),
            end: Vec::new(),
            v: Vec::new(),
            last_valid: Vec::new(),
            interp,
        };
        let mut last = None;
        for (t, end, v) in samples {
            if v.is_some_and(f64::is_finite) {
                last = Some(s.t.len());
            }
            s.t.push(t);
            s.end.push(end);
            s.v.push(v.filter(|x| x.is_finite()));
            s.last_valid.push(last);
        }
        s
    }

    /// True when valid samples k and k+1 are close enough to be joined.
    fn bridged(&self, k: usize) -> bool {
        k + 1 < self.t.len()
            && self.v[k].is_some()
            && self.v[k + 1].is_some()
            && self.t[k + 1] - self.t[k] <= MAX_BRIDGE
    }

    pub fn sample(&self, t: f64) -> Value {
        // last sample starting at or before t
        let k = self.t.partition_point(|&s| s <= t);
        if k == 0 {
            return Value::Absent;
        }
        let k = k - 1;
        if let Some(v) = self.v[k] {
            if self.bridged(k) && t < self.t[k + 1] {
                let v1 = self.v[k + 1].unwrap_or(v);
                return Value::Present(match self.interp {
                    Interp::Step => v,
                    Interp::Linear => {
                        let f = (t - self.t[k]) / (self.t[k + 1] - self.t[k]);
                        v + (v1 - v) * f
                    }
                });
            }
            if t < self.end[k] {
                return Value::Present(v);
            }
        }
        match self.last_valid[k] {
            Some(j) => Value::Stale {
                value: self.v[j].unwrap_or(f64::NAN),
                age: (t - self.end[j]).max(0.0),
            },
            None => Value::Absent,
        }
    }

    /// Merged intervals where `sample` is Present.
    pub fn covered(&self) -> Vec<(f64, f64)> {
        let mut out: Vec<(f64, f64)> = Vec::new();
        for k in 0..self.t.len() {
            if self.v[k].is_none() {
                continue;
            }
            let mut end = self.end[k];
            if self.bridged(k) {
                end = end.max(self.t[k + 1]);
            }
            match out.last_mut() {
                Some(last) if self.t[k] <= last.1 => last.1 = last.1.max(end),
                _ => out.push((self.t[k], end)),
            }
        }
        out
    }
}

/// Gaps of `covered` within `[0, duration]` and the covered fraction.
pub(crate) fn gaps_and_coverage(covered: &[(f64, f64)], duration: f64) -> (Vec<(f64, f64)>, f64) {
    if duration <= 0.0 {
        return (Vec::new(), 0.0);
    }
    let mut gaps = Vec::new();
    let mut cursor = 0.0;
    let mut total = 0.0;
    for &(a, b) in covered {
        let (a, b) = (a.clamp(0.0, duration), b.clamp(0.0, duration));
        if a > cursor {
            gaps.push((cursor, a));
        }
        total += (b - a.max(cursor)).max(0.0);
        cursor = cursor.max(b);
    }
    if cursor < duration {
        gaps.push((cursor, duration));
    }
    (gaps, (total / duration).clamp(0.0, 1.0))
}
```

- [ ] **Step 4: Run the tests and verify that they pass**

Run: `cargo test -p actionlay-telemetry --lib`
Expected: PASS (`test result: ok. 49 passed`).

- [ ] **Step 5: Commit**

```bash
git add crates/telemetry
git commit -m "feat(telemetry): time series with stale gaps and coverage"
```

---

### Task 9: Telemetry, Snapshot and Availability

**Files:**
- Create: `crates/telemetry/src/telemetry.rs`
- Modify: `crates/telemetry/src/lib.rs`

**Interfaces:**
- Consumes: everything of Tasks 3–8.
- Produces (contract): `Telemetry::{from_gpmf_packets(&[RawPacket]) -> Result<Telemetry, TelemetryError>, empty(duration: f64) -> Telemetry, duration() -> f64, start_utc() -> Option<DateTime<Utc>>, availability() -> &Availability, sample(t: f64) -> Snapshot, track() -> &[TrackPoint]}`; `Snapshot { pub t, pub utc, pub gps_lock, values (private) }` with `get(Metric) -> Value`; `Availability::{coverage(Metric) -> f64, gaps(Metric) -> &[(f64, f64)]}`. Additions: `from_gpmf_packets_with`, `TelemetryOptions`, `gps_points()`, `warnings()`, `Availability::is_available`, `TrackPoint { t, lat, lon, alt }`, `TelemetryError::Unreadable { packets, first }`.

Assembly: extract → lock filter → derive → accelerometer through a per-axis Kalman (as the original displays it) → one `Series` per metric (`Linear` for continuous values; `Step` for `azi`, `cog`, `gps-dop`, `gps-lock`, `ori.*`); external metrics (`hr`, `cadence`, …) have no series. `start_utc` = UTC of the first locked GPS sample minus its file time, else of the first sample with a GPSU (the receiver's clock is usually right before a fix: hero7/hero8). `Snapshot::utc = start_utc + t`; `Snapshot::gps_lock` = last known `gps-lock`, `Unknown` without GPS. No packets → `empty(0.0)`; packets but none parseable → `Err(Unreadable)`.

- [ ] **Step 1: Write the failing tests**

`crates/telemetry/src/telemetry.rs` (tests only for now):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    /// `fixes.len()` one-second packets of 18 GPS points heading north at
    /// about 1 m/s, packet k with GPSF `fixes[k]` (DOP 99.99 when unlocked).
    /// The recorded speed differs from point to point (by more than GPS5's
    /// 1 mm/s resolution): a fix that repeats the previous unlocked speed
    /// would be ignored (see `lock`).
    fn gps_packets(fixes: &[u32]) -> Vec<RawPacket> {
        fixes
            .iter()
            .enumerate()
            .map(|(k, &fix)| {
                let points: Vec<Gps5> = (0..18)
                    .map(|i| {
                        let t = k as f64 + i as f64 / 18.0;
                        let v = 1.0 + 0.01 * i as f64;
                        [45.0 + t / 111_131.745, 7.0, 100.0 + t, v, v]
                    })
                    .collect();
                let dop = if fix >= 2 { 150 } else { 9999 };
                let gpsu = format!("2405011000{k:02}.000");
                RawPacket {
                    pts: k as f64,
                    duration: 1.0,
                    data: devc(&[gps5_stream(&gpsu, fix, dop, &points)]),
                }
            })
            .collect()
    }

    #[test]
    fn telemetry_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Telemetry>();
        assert_send_sync::<Snapshot>();
    }

    #[test]
    fn empty_has_nothing() {
        let tel = Telemetry::empty(10.0);
        assert_eq!(tel.duration(), 10.0);
        let snap = tel.sample(1.0);
        assert_eq!(snap.get(Metric::Speed), Value::Absent);
        assert_eq!(snap.gps_lock, GpsLock::Unknown);
        assert_eq!(snap.utc, None);
        assert_eq!(tel.availability().coverage(Metric::Lat), 0.0);
        assert_eq!(tel.availability().gaps(Metric::Lat), &[(0.0, 10.0)]);
        assert!(
            Telemetry::from_gpmf_packets(&[])
                .unwrap()
                .track()
                .is_empty()
        );
    }

    #[test]
    fn locked_track_is_present_everywhere() {
        let tel = Telemetry::from_gpmf_packets(&gps_packets(&[3; 8])).unwrap();
        assert_eq!(tel.duration(), 8.0);
        assert_eq!(tel.gps_points().len(), 144);
        assert_eq!(tel.track().len(), 144);
        let snap = tel.sample(2.5);
        assert_eq!(snap.gps_lock, GpsLock::Lock3d);
        assert!(snap.get(Metric::Lat).present().is_some());
        let speed = snap.get(Metric::Speed).present().unwrap();
        assert!(speed > 1.0 && speed < 1.17, "{speed}");
        assert!((snap.get(Metric::CSpeed).present().unwrap() - 1.0).abs() < 0.02);
        assert_eq!(snap.get(Metric::GpsDop), Value::Present(1.5));
        assert_eq!(
            snap.utc.unwrap().to_rfc3339(),
            "2024-05-01T10:00:02.500+00:00"
        );
        assert_eq!(
            tel.start_utc().unwrap().to_rfc3339(),
            "2024-05-01T10:00:00+00:00"
        );
        assert_eq!(tel.availability().coverage(Metric::Lat), 1.0);
        assert!(tel.availability().gaps(Metric::Lat).is_empty());
        assert!(!tel.availability().is_available(Metric::AcclX));
        assert!(!tel.availability().is_available(Metric::Hr));
        assert_eq!(snap.get(Metric::Hr), Value::Absent);
    }

    #[test]
    fn lost_fix_is_a_stale_gap() {
        let tel = Telemetry::from_gpmf_packets(&gps_packets(&[3, 3, 0, 0, 3])).unwrap();
        let lat = tel.sample(2.5).get(Metric::Lat);
        let Value::Stale { value, age } = lat else {
            panic!("{lat:?}")
        };
        assert!((age - 0.5).abs() < 1e-9);
        assert_eq!(Some(value), tel.sample(1.99).get(Metric::Lat).present());
        assert_eq!(tel.sample(2.5).gps_lock, GpsLock::NoLock);
        assert!(tel.sample(4.5).get(Metric::Lat).present().is_some());
        assert!((tel.availability().coverage(Metric::Lat) - 0.6).abs() < 1e-9);
        assert_eq!(tel.availability().gaps(Metric::Lat), &[(2.0, 4.0)]);
        // the DOP itself is known throughout
        assert_eq!(tel.availability().coverage(Metric::GpsDop), 1.0);
        assert_eq!(tel.sample(2.5).get(Metric::GpsDop), Value::Present(99.99));
    }

    #[test]
    fn never_locked_gps_is_absent_but_reports_lock_and_dop() {
        let tel = Telemetry::from_gpmf_packets(&gps_packets(&[0, 0, 0])).unwrap();
        for t in [0.0, 1.5, 2.9, 10.0] {
            let snap = tel.sample(t);
            assert_eq!(snap.get(Metric::Lat), Value::Absent, "t={t}");
            assert_eq!(snap.get(Metric::Speed), Value::Absent, "t={t}");
            assert_eq!(snap.gps_lock, GpsLock::NoLock, "t={t}");
        }
        assert_eq!(tel.sample(1.0).get(Metric::GpsLock), Value::Present(0.0));
        assert_eq!(tel.availability().coverage(Metric::Lat), 0.0);
        assert!(tel.track().is_empty());
        // the receiver's clock is still used for the date
        assert_eq!(
            tel.start_utc().unwrap().to_rfc3339(),
            "2024-05-01T10:00:00+00:00"
        );
    }

    #[test]
    fn unreadable_packets() {
        let mut packets = gps_packets(&[3, 3]);
        packets[1].data.truncate(10);
        let tel = Telemetry::from_gpmf_packets(&packets).unwrap();
        assert_eq!(tel.gps_points().len(), 18);
        assert_eq!(tel.warnings().len(), 1);
        assert_eq!(tel.duration(), 2.0);

        packets[0].data.truncate(10);
        let err = Telemetry::from_gpmf_packets(&packets).unwrap_err();
        assert!(
            matches!(err, TelemetryError::Unreadable { packets: 2, .. }),
            "{err}"
        );
    }

    #[test]
    fn accelerometer_and_temperature() {
        let samples = vec![[100i16, 200, 300]; 200];
        let packets: Vec<RawPacket> = (0..2)
            .map(|k| RawPacket {
                pts: f64::from(k),
                duration: 1.0,
                data: devc(&[accl_stream(None, 10, 40.0 + k as f32, &samples)]),
            })
            .collect();
        let tel = Telemetry::from_gpmf_packets(&packets).unwrap();
        let snap = tel.sample(1.5);
        assert_eq!(snap.get(Metric::AcclX), Value::Present(20.0));
        assert_eq!(snap.get(Metric::AcclZ), Value::Present(10.0));
        assert_eq!(snap.get(Metric::Temp), Value::Present(41.0));
        assert_eq!(tel.sample(0.5).get(Metric::Temp), Value::Present(40.5));
        assert_eq!(snap.get(Metric::Lat), Value::Absent);
        assert_eq!(snap.gps_lock, GpsLock::Unknown);
        assert_eq!(tel.availability().coverage(Metric::AcclY), 1.0);
    }

    #[test]
    fn lock_options_are_applied() {
        let opts = TelemetryOptions {
            lock: LockOptions {
                dop_max: 1.0,
                speed_max: None,
            },
        };
        let tel = Telemetry::from_gpmf_packets_with(&gps_packets(&[3, 3]), &opts).unwrap();
        assert_eq!(tel.sample(0.5).gps_lock, GpsLock::NoLock);
    }
}
```

`crates/telemetry/src/lib.rs` (final; the `allow(dead_code)` attributes are gone):
```rust
//! Telemetry of action-camera videos: GoPro GPMF parsing, derived metrics,
//! and sampling at any file time. Pure Rust; packets come from the caller
//! (the media crate demuxes them), so this crate never links FFmpeg.
mod derive;
mod extract;
pub mod gpmf;
mod lock;
pub mod metric;
mod series;
mod smoothing;
mod telemetry;
#[cfg(test)]
mod test_support;
pub mod units;
mod value;

pub use extract::{Derived, GpsPoint};
pub use lock::LockOptions;
pub use metric::Metric;
pub use telemetry::{
    Availability, Snapshot, Telemetry, TelemetryError, TelemetryOptions, TrackPoint,
};
pub use value::{GpsLock, Value};

/// One demuxed packet of the GoPro metadata stream, times in seconds of
/// file time. The media crate's `GpmfPacket` converts into this.
#[derive(Debug, Clone, PartialEq)]
pub struct RawPacket {
    pub pts: f64,
    pub duration: f64,
    pub data: Vec<u8>,
}
```

- [ ] **Step 2: Run the tests and verify that they fail**

Run: `cargo test -p actionlay-telemetry --lib telemetry`
Expected: FAIL to compile (`Telemetry`, `Snapshot`, `TelemetryOptions` not found).

- [ ] **Step 3: Implementation**

Above the tests in `crates/telemetry/src/telemetry.rs`:
```rust
//! The telemetry of one video: series per metric, sampled at any file time.
use chrono::{DateTime, TimeDelta, Utc};

use crate::{
    RawPacket,
    derive::derive,
    extract::{GpsPoint, Sample, extract},
    gpmf::GpmfError,
    lock::{self, LockOptions},
    metric::Metric,
    series::{Interp, Series, gaps_and_coverage},
    smoothing::Kalman,
    value::{GpsLock, Value},
};

#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("none of the {packets} GPMF packets could be read: {first}")]
    Unreadable { packets: usize, first: GpmfError },
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TelemetryOptions {
    pub lock: LockOptions,
}

/// A locked GPS point as displayed (smoothed position), for maps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackPoint {
    pub t: f64,
    pub lat: f64,
    pub lon: f64,
    pub alt: f64,
}

/// Which metrics a video has, and where.
#[derive(Debug, Clone)]
pub struct Availability {
    /// Indexed by `Metric::index()`: (coverage, gaps).
    per_metric: Vec<(f64, Vec<(f64, f64)>)>,
}

impl Availability {
    /// Share of `[0, duration]` where the metric is Present, 0.0..=1.0.
    pub fn coverage(&self, m: Metric) -> f64 {
        self.per_metric[m.index()].0
    }

    /// Intervals of `[0, duration]` where the metric is not Present.
    pub fn gaps(&self, m: Metric) -> &[(f64, f64)] {
        &self.per_metric[m.index()].1
    }

    /// True when the video has the metric anywhere.
    pub fn is_available(&self, m: Metric) -> bool {
        self.coverage(m) > 0.0
    }
}

/// Every metric at one instant.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub t: f64,
    pub utc: Option<DateTime<Utc>>,
    pub gps_lock: GpsLock,
    /// Indexed by `Metric::index()`.
    values: [Value; Metric::COUNT],
}

impl Snapshot {
    pub fn get(&self, m: Metric) -> Value {
        self.values[m.index()]
    }
}

#[derive(Debug, Clone)]
pub struct Telemetry {
    duration: f64,
    start_utc: Option<DateTime<Utc>>,
    /// Indexed by `Metric::index()`; None when the source has no such data.
    series: Vec<Option<Series>>,
    availability: Availability,
    gps: Vec<GpsPoint>,
    track: Vec<TrackPoint>,
    warnings: Vec<String>,
}

fn shift(t: DateTime<Utc>, seconds: f64) -> DateTime<Utc> {
    t + TimeDelta::microseconds((seconds * 1e6).round() as i64)
}

fn gps_series(gps: &[GpsPoint], interp: Interp, f: impl Fn(&GpsPoint) -> Option<f64>) -> Series {
    Series::new(interp, gps.iter().map(|p| (p.t, p.end, f(p))))
}

fn axis_series(samples: &[Sample<3>], axis: usize, interp: Interp) -> Series {
    Series::new(
        interp,
        samples.iter().map(|s| (s.t, s.end, Some(s.v[axis]))),
    )
}

impl Telemetry {
    pub fn from_gpmf_packets(packets: &[RawPacket]) -> Result<Telemetry, TelemetryError> {
        Self::from_gpmf_packets_with(packets, &TelemetryOptions::default())
    }

    pub fn from_gpmf_packets_with(
        packets: &[RawPacket],
        opts: &TelemetryOptions,
    ) -> Result<Telemetry, TelemetryError> {
        let mut ex = extract(packets);
        if ex.parsed_packets == 0 {
            if let Some(first) = ex.first_error.take() {
                return Err(TelemetryError::Unreadable {
                    packets: packets.len(),
                    first,
                });
            }
            return Ok(Telemetry::empty(ex.duration));
        }
        lock::apply(&mut ex.gps, &opts.lock);
        derive(&mut ex.gps);

        // The original shows the accelerometer through a per-axis Kalman.
        let mut k = [Kalman::default(), Kalman::default(), Kalman::default()];
        for s in &mut ex.accl {
            for (axis, v) in s.v.iter_mut().enumerate() {
                *v = k[axis].update(*v);
            }
        }

        let mut series: Vec<Option<Series>> = vec![None; Metric::COUNT];
        let mut set = |m: Metric, s: Series| series[m.index()] = Some(s);
        let g = &ex.gps;
        if !g.is_empty() {
            use Interp::{Linear, Step};
            set(Metric::Lat, gps_series(g, Linear, |p| p.derived.lat));
            set(Metric::Lon, gps_series(g, Linear, |p| p.derived.lon));
            set(Metric::Alt, gps_series(g, Linear, |p| p.derived.alt));
            set(Metric::Speed, gps_series(g, Linear, |p| p.derived.speed));
            set(Metric::CSpeed, gps_series(g, Linear, |p| p.derived.cspeed));
            set(Metric::Accel, gps_series(g, Linear, |p| p.derived.accel));
            set(Metric::Dist, gps_series(g, Linear, |p| p.derived.dist));
            set(Metric::COdo, gps_series(g, Linear, |p| p.derived.codo));
            // GoPro has no odometer of its own: the original falls back to codo.
            set(Metric::Odo, gps_series(g, Linear, |p| p.derived.codo));
            set(Metric::CGrad, gps_series(g, Linear, |p| p.derived.cgrad));
            // ... and to cgrad for gradient.
            set(Metric::Gradient, gps_series(g, Linear, |p| p.derived.cgrad));
            set(Metric::Azi, gps_series(g, Step, |p| p.derived.azi));
            set(Metric::Cog, gps_series(g, Step, |p| p.derived.cog));
            set(Metric::GpsDop, gps_series(g, Step, |p| Some(p.dop)));
            set(
                Metric::GpsLock,
                gps_series(g, Step, |p| Some(f64::from(p.lock.code()))),
            );
        }
        if !ex.accl.is_empty() {
            set(Metric::AcclX, axis_series(&ex.accl, 0, Interp::Linear));
            set(Metric::AcclY, axis_series(&ex.accl, 1, Interp::Linear));
            set(Metric::AcclZ, axis_series(&ex.accl, 2, Interp::Linear));
        }
        if !ex.grav.is_empty() {
            set(Metric::GravX, axis_series(&ex.grav, 0, Interp::Linear));
            set(Metric::GravY, axis_series(&ex.grav, 1, Interp::Linear));
            set(Metric::GravZ, axis_series(&ex.grav, 2, Interp::Linear));
        }
        if !ex.ori.is_empty() {
            set(Metric::OriPitch, axis_series(&ex.ori, 0, Interp::Step));
            set(Metric::OriRoll, axis_series(&ex.ori, 1, Interp::Step));
            set(Metric::OriYaw, axis_series(&ex.ori, 2, Interp::Step));
        }
        if !ex.temp.is_empty() {
            let temp = ex.temp.iter().map(|s| (s.t, s.end, Some(s.v[0])));
            set(Metric::Temp, Series::new(Interp::Linear, temp));
        }

        // UTC at file time 0: from the first locked point, else from any
        // GPSU (receivers usually know the time before they get a fix).
        let start_utc = g
            .iter()
            .filter(|p| p.lock.is_locked())
            .chain(g.iter())
            .find_map(|p| p.utc.map(|u| shift(u, -p.t)));
        let track = g
            .iter()
            .filter_map(|p| {
                Some(TrackPoint {
                    t: p.t,
                    lat: p.derived.lat?,
                    lon: p.derived.lon?,
                    alt: p.derived.alt?,
                })
            })
            .collect();
        let availability = availability(&series, ex.duration);
        Ok(Telemetry {
            duration: ex.duration,
            start_utc,
            series,
            availability,
            gps: ex.gps,
            track,
            warnings: ex.warnings,
        })
    }

    /// Telemetry of a video without any: every metric is Absent.
    pub fn empty(duration: f64) -> Telemetry {
        let series = vec![None; Metric::COUNT];
        Telemetry {
            duration,
            start_utc: None,
            availability: availability(&series, duration),
            series,
            gps: Vec::new(),
            track: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// End of the last GPMF packet (the video duration when the track
    /// spans the whole video), or the duration given to `empty`.
    pub fn duration(&self) -> f64 {
        self.duration
    }

    pub fn start_utc(&self) -> Option<DateTime<Utc>> {
        self.start_utc
    }

    pub fn availability(&self) -> &Availability {
        &self.availability
    }

    pub fn sample(&self, t: f64) -> Snapshot {
        let values: [Value; Metric::COUNT] = std::array::from_fn(|i| {
            self.series[i]
                .as_ref()
                .map_or(Value::Absent, |s| s.sample(t))
        });
        let gps_lock = values[Metric::GpsLock.index()]
            .last_known()
            .map_or(GpsLock::Unknown, |code| GpsLock::from_fix(code as u32));
        Snapshot {
            t,
            utc: self.start_utc.map(|u| shift(u, t)),
            gps_lock,
            values,
        }
    }

    /// Locked GPS points, smoothed as displayed.
    pub fn track(&self) -> &[TrackPoint] {
        &self.track
    }

    /// Every GPS sample, raw and derived (for the dump CLI and tests).
    pub fn gps_points(&self) -> &[GpsPoint] {
        &self.gps
    }

    /// Non-fatal problems met while reading (skipped packets, streams).
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
}

fn availability(series: &[Option<Series>], duration: f64) -> Availability {
    let per_metric = series
        .iter()
        .map(|s| {
            let covered = s.as_ref().map(Series::covered).unwrap_or_default();
            let (gaps, coverage) = gaps_and_coverage(&covered, duration);
            (coverage, gaps)
        })
        .collect();
    Availability { per_metric }
}
```

- [ ] **Step 4: Run the tests and verify that they pass**

Run: `cargo test -p actionlay-telemetry --lib && cargo clippy -p actionlay-telemetry --all-targets -- -D warnings`
Expected: PASS (`test result: ok. 57 passed`), no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/telemetry
git commit -m "feat(telemetry): Telemetry, Snapshot and Availability"
```

---

### Task 10: Comparison with gopro-dashboard-overlay on real files

**Files:**
- Create: `scripts/reference/gpo-dashboard-reference.py`, `crates/telemetry/tests/reference/README.md`, `crates/telemetry/tests/reference/{hero5,hero6,max-heromode}.{gopro-to-csv,dashboard}.csv` (generated), `crates/telemetry/tests/common/mod.rs`, `crates/telemetry/tests/reference.rs`, `crates/telemetry/tests/gopro_files.rs`
- Modify: `crates/telemetry/Cargo.toml`

**Interfaces:**
- Consumes: `actionlay_media::gpmf::read_gpmf_packets` (Task 2, dev-dependency only), `Telemetry::{gps_points, sample, availability, …}` (Task 9), samples (Task 1).
- Produces: committed reference CSVs; test helpers `common::{gopro_sample, raw_packets, load, reference, num, key, stats}`.

Two references per sample, both from gopro-dashboard-overlay 0.134.0:
- `*.gopro-to-csv.csv`: the original's own `gopro-to-csv.py` — one row per GPS sample with the **raw** parsed values (`lat`, `lon`, `alt`, `speed`, `dop`, `gps_fix`, `date`) and the accelerometer as displayed. Its derived columns come from a different pipeline than the dashboard's (no position smoothing, no speed Kalman), so they are not used.
- `*.dashboard.csv`: `scripts/reference/gpo-dashboard-reference.py`, which loads the file exactly like `gopro-dashboard.py` and runs its processing block verbatim, then dumps the derived values the dashboard shows (smoothed lat/lon, Kalman speed, cspeed, dist, codo, azi, cog, cgrad, accel) plus gravity and orientation.

Rows are joined on `(packet, packet_index)`, never on time. Tolerances (measured on these samples, then a margin): raw lat/lon 1e-7°, alt 1 mm, speed 1 mm/s, DOP 1e-6, fix exact, date 1 ms at index 0 and 0.1 s elsewhere except in the final packet; dashboard lat/lon 2e-6°, speed 0.02 m/s, cspeed 0.15 m/s, dist 1 mm, codo 0.2 m, azi/cog 1°, cgrad 1 %, accel 0.05 m/s² (justified in the test's doc comment: the timing model of deliberate difference 1, and the original's reordering on MAX); accelerometer mean |Δ| < 0.25 m/s² and p95 < 0.8 m/s² (same decimation and Kalman, but sampled on our clock: measured mean 0.11–0.16, p95 0.41–0.59); gravity mean < 0.01, p95 < 0.03; orientation mean < 0.5°, p95 < 2° (measured 0.003/0.014 and 0.25°/0.87°).

- [ ] **Step 1: Reference generator**

`scripts/reference/gpo-dashboard-reference.py`:
```python
#!/usr/bin/env python3
"""Per-GPS-sample values as gopro-dashboard-overlay shows them on a dashboard.

Development tool only (not distributed): it imports gopro-dashboard-overlay
(GPL-3.0) and replays the processing block of its gopro-dashboard.py, so
ActionLay's derived metrics can be compared with the original's.

Usage: python gpo-dashboard-reference.py VIDEO.mp4 OUT.csv
Needs gopro-dashboard-overlay 0.134.0 and ffmpeg/ffprobe on PATH.
"""
import csv
import sys
from pathlib import Path

from gopro_overlay import gpmd_filters, timeseries_process
from gopro_overlay.ffmpeg import FFMPEG
from gopro_overlay.ffmpeg_gopro import FFMPEGGoPro
from gopro_overlay.framemeta_gpmd import LoadFlag
from gopro_overlay.gpmf import GPS_FIXED_VALUES, GPSFix
from gopro_overlay.loading import GoproLoader
from gopro_overlay.units import units

src, dst = Path(sys.argv[1]), sys.argv[2]
loader = GoproLoader(
    ffmpeg_gopro=FFMPEGGoPro(FFMPEG()),
    units=units,
    flags={LoadFlag.ACCL, LoadFlag.GRAV, LoadFlag.CORI},
    # gopro-dashboard.py defaults: --gps-dop-max 10 --gps-speed-max 60 kph
    gps_lock_filter=gpmd_filters.standard(dop_max=10, speed_max=units.Quantity(60, "kph")),
)
fm = loader.load(src).framemeta
for e in fm.items():
    e.update(raw_lat=e.point.lat, raw_lon=e.point.lon, raw_speed=e.speed)

# Verbatim from gopro-dashboard.py 0.134.0, "processing" block.
packets_per_second = 18
locked_2d = lambda e: e.gpsfix in GPS_FIXED_VALUES
locked_3d = lambda e: e.gpsfix == GPSFix.LOCK_3D.value
fm.process(timeseries_process.process_ses("point", lambda i: i.point, alpha=0.45), filter_fn=locked_2d)
fm.process_deltas(timeseries_process.calculate_speeds(), skip=packets_per_second * 3, filter_fn=locked_2d)
fm.process(timeseries_process.calculate_odo(), filter_fn=locked_2d)
fm.process_accel(timeseries_process.calculate_accel(), skip=18 * 3)
fm.process_deltas(timeseries_process.calculate_gradient(), skip=packets_per_second * 3, filter_fn=locked_3d)
fm.process(timeseries_process.process_kalman("speed", lambda e: e.speed))
fm.process(timeseries_process.filter_locked())


def m(v):
    return "" if v is None else getattr(v, "magnitude", v)


columns = ["packet", "packet_index", "timestamp_ms", "date", "gps_fix", "dop",
           "raw_lat", "raw_lon", "raw_speed", "lat", "lon", "alt", "speed",
           "cspeed", "dist", "codo", "azi", "cog", "cgrad", "accel",
           "accl_x", "accl_y", "accl_z", "grav_x", "grav_y", "grav_z",
           "ori_pitch", "ori_roll", "ori_yaw"]
with open(dst, "w", newline="") as f:
    w = csv.writer(f, lineterminator="\n")
    w.writerow(columns)
    for e in fm.items():
        a, g, o = e.accl, e.grav, e.ori
        w.writerow([
            m(e.packet), m(e.packet_index), m(e.timestamp), e.dt.isoformat(),
            GPSFix(e.gpsfix).name, m(e.dop), e.raw_lat, e.raw_lon, m(e.raw_speed),
            e.point.lat, e.point.lon, m(e.alt), m(e.speed), m(e.cspeed), m(e.dist),
            m(e.codo), m(e.azi), m(e.cog), m(e.cgrad), m(e.accel),
            m(a.x) if a else "", m(a.y) if a else "", m(a.z) if a else "",
            m(g.x) if g else "", m(g.y) if g else "", m(g.z) if g else "",
            m(o.pitch) if o else "", m(o.roll) if o else "", m(o.yaw) if o else "",
        ])
```

- [ ] **Step 2: Generate the reference CSVs (once)**

If `/tmp/gpo-venv` does not exist: `python3 -m venv /tmp/gpo-venv && /tmp/gpo-venv/bin/pip install gopro-overlay==0.134.0`. The tool calls `ffprobe`/`ffmpeg` from `PATH` (Homebrew's is fine; it only demuxes).

Run:
```bash
mkdir -p crates/telemetry/tests/reference
for s in hero5 hero6 max-heromode; do
  /tmp/gpo-venv/bin/gopro-to-csv.py samples/gopro/$s.mp4 crates/telemetry/tests/reference/$s.gopro-to-csv.csv
  /tmp/gpo-venv/bin/python scripts/reference/gpo-dashboard-reference.py samples/gopro/$s.mp4 crates/telemetry/tests/reference/$s.dashboard.csv
done
wc -l crates/telemetry/tests/reference/*.csv
```
Expected line counts (header included): hero5 619 and 619, hero6 418 and 418, max-heromode 190 and 190. The first data row of `hero5.gopro-to-csv.csv` is `0,0,LOCK_3D,2017-04-17 17:31:03+00:00,33.1264969,-117.3273542,6.06,-20.184,0.167,,0.05136178522457279,2.965,-33.72215971181636,,-7.1028453006395225,-0.18181818181818182,0.9258373205741627,9.770334928229666` (gopro-to-csv writes CRLF line ends; the tests accept both).

`crates/telemetry/tests/reference/README.md`:
```markdown
# Reference values from gopro-dashboard-overlay

Generated once, on 2026-10-05, with gopro-dashboard-overlay **0.134.0**
(`pip install gopro-overlay==0.134.0`, Python 3.14) from the public samples
fetched by `scripts/fetch-gopro-samples.sh`:

    gopro-to-csv.py samples/gopro/<s>.mp4 <s>.gopro-to-csv.csv
    python scripts/reference/gpo-dashboard-reference.py samples/gopro/<s>.mp4 <s>.dashboard.csv

for `<s>` in `hero5`, `hero6`, `max-heromode`.

- `*.gopro-to-csv.csv`: the original's CSV export with its defaults
  (`--gps-dop-max 10`, `--gps-speed-max 60` km/h): one row per GPS sample.
  Used for the raw values (lat, lon, alt, speed, dop, gps_fix, date) and
  the accelerometer.
- `*.dashboard.csv`: the values a dashboard shows, from the processing
  block of `gopro-dashboard.py` replayed verbatim (same defaults). Used for
  the derived metrics, gravity and orientation (orientation in radians).

Known behaviour of the original visible in these files: on max-heromode two
samples, (0, 16) and (4, 0), are missing — their timestamps fall out of
order and the original's `items()` skips them. ActionLay keeps them.

The values are derived from public sample videos (Apache-2.0, GoPro, Inc.).
Never add files produced from private footage here.
```

- [ ] **Step 3: Write the tests**

`crates/telemetry/Cargo.toml` — add:
```toml
[dev-dependencies]
# Only the integration tests read real MP4 files; the library itself never
# links FFmpeg (checked by `cargo tree -e normal` in CI).
actionlay-media = { path = "../media" }
```

`crates/telemetry/tests/common/mod.rs`:
```rust
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
            header
                .iter()
                .cloned()
                .zip(l.split(',').map(String::from))
                .collect()
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
```

`crates/telemetry/tests/reference.rs`:
```rust
//! Comparison with gopro-dashboard-overlay 0.134.0 on public GoPro samples.
//! How the reference files were made: tests/reference/README.md.
mod common;

use std::collections::HashMap;

use actionlay_telemetry::{Derived, GpsPoint, Metric, Telemetry};
use chrono::DateTime;
use common::{key, num, reference, stats};

const SAMPLES: [&str; 3] = ["hero5", "hero6", "max-heromode"];

fn by_key(tel: &Telemetry) -> HashMap<(usize, usize), &GpsPoint> {
    tel.gps_points()
        .iter()
        .map(|p| ((p.packet, p.index), p))
        .collect()
}

fn assert_close(what: &str, ours: f64, theirs: f64, tol: f64) {
    assert!(
        (ours - theirs).abs() <= tol,
        "{what}: ours {ours}, original {theirs}, tolerance {tol}"
    );
}

/// Raw values parsed from GPMF must equal gopro-to-csv's.
#[test]
fn raw_gps_matches_gopro_to_csv() {
    for s in SAMPLES {
        let Some(tel) = common::load(&format!("{s}.mp4")) else {
            continue;
        };
        let ours = by_key(&tel);
        let rows = reference(&format!("{s}.gopro-to-csv.csv"));
        let last_packet = rows.iter().map(|r| key(r).0).max().unwrap();
        for r in &rows {
            let at = format!("{s} {:?}", key(r));
            // every reference row has a match; we may have more (the
            // original drops samples whose timestamps collide)
            let p = ours.get(&key(r)).unwrap_or_else(|| panic!("{at}: missing"));
            assert_eq!(p.lock.original_name(), r["gps_fix"], "{at}");
            assert_close(&format!("{at} lat"), p.lat, num(r, "lat").unwrap(), 1e-7);
            assert_close(&format!("{at} lon"), p.lon, num(r, "lon").unwrap(), 1e-7);
            assert_close(&format!("{at} dop"), p.dop, num(r, "dop").unwrap(), 1e-6);
            if p.lock.is_locked() {
                assert_close(&format!("{at} alt"), p.alt, num(r, "alt").unwrap(), 1e-3);
                assert_close(
                    &format!("{at} speed"),
                    p.speed2d,
                    num(r, "speed").unwrap(),
                    1e-3,
                );
            } else {
                assert_eq!(num(r, "alt"), None, "{at}");
                assert_eq!(num(r, "speed"), None, "{at}");
            }
            // UTC: equal to GPSU at the first sample of a packet; within
            // 0.1 s elsewhere (the original spaces samples by its own clock
            // model), except in the final short packet, which the original
            // stretches past the end of the file.
            let theirs = DateTime::parse_from_rfc3339(&r["date"].replacen(' ', "T", 1)).unwrap();
            let diff = (p.utc.unwrap() - theirs.to_utc()).as_seconds_f64().abs();
            if p.index == 0 {
                assert!(diff < 1e-3, "{at} date off by {diff}");
            } else if p.packet != last_packet {
                assert!(diff < 0.1, "{at} date off by {diff}");
            }
        }
        assert!(tel.gps_points().len() >= rows.len());
    }
}

type Getter = fn(&Derived) -> Option<f64>;

/// (column, tolerance, value). Tolerances, measured on these samples plus a
/// margin:
/// - lat/lon 2e-6°: identical on HERO5/6; on MAX the original sorts samples
///   by its STMP clock, which moves one sample per packet boundary across
///   the next packet's first sample and shifts its smoothing slightly.
/// - speed 0.02 m/s, cspeed 0.15 m/s, accel 0.05 m/s²: our UTC spacing of
///   samples (packet duration / n) differs from the original's by up to
///   ~2 % over a 3 s window; the Kalman filters carry that along.
/// - dist 1 mm, codo 0.2 m, azi/cog 1°, cgrad 1 %: same causes.
const DERIVED: [(&str, f64, Getter); 11] = [
    ("lat", 2e-6, |d| d.lat),
    ("lon", 2e-6, |d| d.lon),
    ("alt", 1e-3, |d| d.alt),
    ("speed", 0.02, |d| d.speed),
    ("cspeed", 0.15, |d| d.cspeed),
    ("dist", 1e-3, |d| d.dist),
    ("codo", 0.2, |d| d.codo),
    ("azi", 1.0, |d| d.azi),
    ("cog", 1.0, |d| d.cog),
    ("cgrad", 1.0, |d| d.cgrad),
    ("accel", 0.05, |d| d.accel),
];

/// Derived values must match what the original's dashboard pipeline shows.
#[test]
fn derived_metrics_match_the_dashboard() {
    for s in SAMPLES {
        let Some(tel) = common::load(&format!("{s}.mp4")) else {
            continue;
        };
        let ours = by_key(&tel);
        for r in &reference(&format!("{s}.dashboard.csv")) {
            let at = format!("{s} {:?}", key(r));
            let p = ours.get(&key(r)).unwrap_or_else(|| panic!("{at}: missing"));
            if !p.lock.is_locked() {
                assert_eq!(p.derived, Derived::default(), "{at}");
                continue;
            }
            for (col, tol, get) in DERIVED {
                match (get(&p.derived), num(r, col)) {
                    (None, None) => {}
                    (Some(a), Some(b)) => {
                        let mut d = (a - b).abs();
                        if col == "azi" || col == "cog" {
                            d = d.min(360.0 - d);
                        }
                        assert!(d <= tol, "{at} {col}: ours {a}, original {b}, tol {tol}");
                    }
                    (a, b) => panic!("{at} {col}: ours {a:?}, original {b:?}"),
                }
            }
        }
    }
}

/// The accelerometer as displayed: same decimation and Kalman filter as the
/// original, but sampled on our clock, so compared statistically.
#[test]
fn accelerometer_is_close_to_the_original() {
    for s in SAMPLES {
        let Some(tel) = common::load(&format!("{s}.mp4")) else {
            continue;
        };
        let ours = by_key(&tel);
        let mut errors = Vec::new();
        for r in &reference(&format!("{s}.gopro-to-csv.csv")) {
            let p = ours[&key(r)];
            let snap = tel.sample(p.t);
            for (m, col) in [
                (Metric::AcclX, "accl_x"),
                (Metric::AcclY, "accl_y"),
                (Metric::AcclZ, "accl_z"),
            ] {
                let a = snap.get(m).present().unwrap();
                errors.push((a - num(r, col).unwrap()).abs());
            }
        }
        let (mean, p95) = stats(errors);
        assert!(mean < 0.25 && p95 < 0.8, "{s}: mean {mean}, p95 {p95}");
    }
}

#[test]
fn gravity_and_orientation_are_close_to_the_original() {
    let Some(tel) = common::load("max-heromode.mp4") else {
        return;
    };
    let ours = by_key(&tel);
    let (mut grav, mut ori) = (Vec::new(), Vec::new());
    for r in &reference("max-heromode.dashboard.csv") {
        let snap = tel.sample(ours[&key(r)].t);
        for (m, col) in [
            (Metric::GravX, "grav_x"),
            (Metric::GravY, "grav_y"),
            (Metric::GravZ, "grav_z"),
        ] {
            grav.push((snap.get(m).present().unwrap() - num(r, col).unwrap()).abs());
        }
        for (m, col) in [
            (Metric::OriPitch, "ori_pitch"),
            (Metric::OriRoll, "ori_roll"),
            (Metric::OriYaw, "ori_yaw"),
        ] {
            let d = (snap.get(m).present().unwrap() - num(r, col).unwrap().to_degrees()).abs();
            ori.push(d.min(360.0 - d));
        }
    }
    let (gm, gp) = stats(grav);
    assert!(gm < 0.01 && gp < 0.03, "grav mean {gm}, p95 {gp}");
    let (om, op) = stats(ori);
    assert!(om < 0.5 && op < 2.0, "ori mean {om}°, p95 {op}°");
}
```

`crates/telemetry/tests/gopro_files.rs`:
```rust
//! Behaviour on the public GoPro samples: gaps, no lock, short packets,
//! damaged packets.
mod common;

use actionlay_telemetry::{GpsLock, Metric, Telemetry, Value};

#[test]
fn hero5_locked_throughout() {
    let Some(tel) = common::load("hero5.mp4") else {
        return;
    };
    assert!((tel.duration() - 34.034).abs() < 1e-9);
    assert_eq!(tel.gps_points().len(), 618);
    assert_eq!(tel.track().len(), 618);
    assert_eq!(
        tel.start_utc().unwrap().to_rfc3339(),
        "2017-04-17T17:31:03+00:00"
    );
    let a = tel.availability();
    for m in [
        Metric::Lat,
        Metric::Speed,
        Metric::CSpeed,
        Metric::AcclZ,
        Metric::Temp,
    ] {
        assert_eq!(a.coverage(m), 1.0, "{}", m.id());
    }
    // accel needs a 3 s window
    assert_eq!(a.gaps(Metric::Accel).len(), 1);
    assert!((a.gaps(Metric::Accel)[0].1 - 3.003).abs() < 1e-9);
    assert!(!a.is_available(Metric::GravX));
    let snap = tel.sample(10.0);
    assert_eq!(snap.gps_lock, GpsLock::Lock3d);
    assert!((snap.get(Metric::AcclZ).present().unwrap() - 9.8).abs() < 1.5);
    assert!(tel.warnings().is_empty());
}

#[test]
fn hero6_fix_gap_is_stale_then_recovers() {
    let Some(tel) = common::load("hero6.mp4") else {
        return;
    };
    // packet 0 is locked, packets 1–13 have no fix, 2D from packet 14
    let lat = tel.sample(5.0).get(Metric::Lat);
    let Value::Stale { age, .. } = lat else {
        panic!("{lat:?}")
    };
    assert!((age - 3.999).abs() < 1e-6, "age {age}");
    assert_eq!(tel.sample(5.0).gps_lock, GpsLock::NoLock);
    assert_eq!(tel.sample(14.5).gps_lock, GpsLock::Lock2d);
    assert_eq!(tel.sample(20.0).gps_lock, GpsLock::Lock3d);
    assert!(tel.sample(20.0).get(Metric::Lat).present().is_some());
    let a = tel.availability();
    let gaps = a.gaps(Metric::Lat);
    assert_eq!(gaps.len(), 1);
    assert!((gaps[0].0 - 1.001).abs() < 1e-9 && (gaps[0].1 - 14.014).abs() < 1e-9);
    assert!((a.coverage(Metric::Lat) - 10.01 / 23.023).abs() < 1e-9);
    assert_eq!(a.coverage(Metric::GpsDop), 1.0);
}

#[test]
fn hero7_and_hero8_never_lock() {
    for (name, start) in [
        ("hero7.mp4", "2019-11-18T23:42:08.755+00:00"),
        ("hero8.mp4", "2019-11-18T23:42:08.645+00:00"),
    ] {
        let Some(tel) = common::load(name) else {
            continue;
        };
        for t in [0.0, 3.3, 9.9] {
            let snap = tel.sample(t);
            assert_eq!(snap.get(Metric::Lat), Value::Absent, "{name} {t}");
            assert_eq!(snap.get(Metric::Speed), Value::Absent, "{name} {t}");
            assert_eq!(
                snap.get(Metric::GpsDop),
                Value::Present(99.99),
                "{name} {t}"
            );
            assert_eq!(snap.get(Metric::GpsLock), Value::Present(0.0), "{name} {t}");
            assert_eq!(snap.gps_lock, GpsLock::NoLock, "{name} {t}");
            assert!(snap.get(Metric::AcclZ).present().is_some(), "{name} {t}");
        }
        let a = tel.availability();
        assert_eq!(a.coverage(Metric::Lat), 0.0, "{name}");
        assert_eq!(a.coverage(Metric::CSpeed), 0.0, "{name}");
        assert!(tel.track().is_empty(), "{name}");
        // the date still comes from the receiver's clock
        assert_eq!(tel.start_utc().unwrap().to_rfc3339(), start, "{name}");
    }
    if let Some(tel) = common::load("hero8.mp4") {
        let a = tel.availability();
        assert!(a.is_available(Metric::GravZ) && a.is_available(Metric::OriYaw));
    }
}

#[test]
fn max_short_final_packet_stays_inside_the_file() {
    let Some(tel) = common::load("max-heromode.mp4") else {
        return;
    };
    let pts = tel.gps_points();
    assert!(pts.windows(2).all(|w| w[0].t < w[1].t));
    assert!(pts.iter().all(|p| p.end <= tel.duration() + 1e-9));
    let last: Vec<_> = pts.iter().filter(|p| p.packet == 10).collect();
    assert_eq!(last.len(), 10);
    assert!((last[0].t - 10.01).abs() < 1e-9);
    assert!((last[9].end - 10.543).abs() < 1e-9);
    let a = tel.availability();
    for m in [Metric::GravX, Metric::OriPitch, Metric::Lat] {
        assert_eq!(a.coverage(m), 1.0, "{}", m.id());
    }
}

#[test]
fn damaged_packets_are_skipped() {
    let Some(path) = common::gopro_sample("hero5.mp4") else {
        return;
    };
    let packets = common::raw_packets(&path);
    let full = Telemetry::from_gpmf_packets(&packets).unwrap();
    let in_packet_3 = full.gps_points().iter().filter(|p| p.packet == 3).count();
    for cut in (1..packets[3].data.len()).step_by(97) {
        let mut damaged = packets.clone();
        damaged[3].data.truncate(cut);
        let tel = Telemetry::from_gpmf_packets(&damaged).unwrap();
        assert_eq!(tel.warnings().len(), 1, "cut {cut}");
        assert_eq!(tel.gps_points().len(), 618 - in_packet_3, "cut {cut}");
        assert!(
            tel.sample(3.5).get(Metric::Lat).present().is_some(),
            "cut {cut}"
        );
    }
    // flipped bytes: never a panic
    for at in (0..packets[3].data.len()).step_by(13) {
        let mut damaged = packets.clone();
        damaged[3].data[at] ^= 0xff;
        let _ = Telemetry::from_gpmf_packets(&damaged).unwrap();
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `source scripts/env.sh && ACTIONLAY_REQUIRE_GOPRO_SAMPLES=1 cargo test -p actionlay-telemetry --test reference --test gopro_files -- --test-threads=1`
Expected: PASS (`reference`: 4 passed; `gopro_files`: 5 passed). These tests exercise code already written in Tasks 2–9; if one fails, the failure message names the sample, the `(packet, index)` and both values — fix the implementation, never widen a tolerance without re-measuring and updating the justification.

Run: `cargo tree -p actionlay-telemetry -e normal --prefix none | grep -ci ffmpeg`
Expected: `0`.

- [ ] **Step 5: Commit**

```bash
git add scripts/reference crates/telemetry
git commit -m "test(telemetry): compare with gopro-dashboard-overlay on public GoPro samples"
```

---

### Task 11: Dump CLI, CI binary, README

**Files:**
- Create: `crates/telemetry-cli/Cargo.toml`, `crates/telemetry-cli/src/main.rs`, `crates/telemetry-cli/tests/cli.rs`
- Modify: `Cargo.toml`, `.github/workflows/ci.yml`, `README.md`

**Interfaces:**
- Consumes: `read_gpmf_packets` (Task 2), `Telemetry`, `TelemetryOptions`, `LockOptions`, `Metric`, `Value`, `GpsPoint`, `GpsLock::original_name` (Tasks 3–9), `tests/reference/hero5.gopro-to-csv.csv` (Task 10).
- Produces: binary `actionlay-telemetry`:
  - `dump <VIDEO> [--every <S>=1.0] [--format csv|json] [--points] [--dop-max <DOP>=10] [--speed-max-kmh <KMH>]`
  - `info <VIDEO> [--dop-max …] [--speed-max-kmh …]`

`dump` (CSV) prints `t,utc,gps_lock,<every available metric id>` at t = 0, S, 2S … ≤ duration, Present values only (Stale/Absent cells are empty); JSON prints the same as an array, with `{"present":v}` or `{"stale":v,"age":a}` per metric and Absent metrics omitted. `dump --points` prints one row per GPS sample whose first nine columns are gopro-to-csv's (`packet,packet_index,gps_fix,date,lat,lon,dop,alt,speed`, floats printed like Python), followed by `t` and our derived values — `diff` against the original works directly. `info` prints duration, start UTC, GPS sample counts, warnings and per-metric coverage with gaps. A file without metadata exits with status 1 and `no GoPro metadata (gpmd) stream`.

- [ ] **Step 1: Crate and failing tests**

Root `Cargo.toml`: `members = ["crates/media", "crates/app", "crates/telemetry", "crates/telemetry-cli"]` and in `[workspace.dependencies]`:
```toml
clap = { version = "4.6.7", features = ["derive"] }
```

`crates/telemetry-cli/Cargo.toml`:
```toml
[package]
name = "actionlay-telemetry-cli"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true

[[bin]]
name = "actionlay-telemetry"
path = "src/main.rs"

[dependencies]
actionlay-media = { path = "../media" }
actionlay-telemetry = { path = "../telemetry" }
chrono.workspace = true
clap.workspace = true
```

`crates/telemetry-cli/tests/cli.rs`:
```rust
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
        lines[1].starts_with("0,2017-04-17T17:31:03.000Z,Lock3d,0.167,"),
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
        "ACTIONLAY_REQUIRE_SYNTHETIC_SAMPLES",
    ) else {
        return;
    };
    let out = run(&["dump"], &video);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("no GoPro metadata (gpmd) stream"), "{err}");
}
```

`crates/telemetry-cli/src/main.rs` with a stub so the tests compile and fail:
```rust
fn main() {}
```

- [ ] **Step 2: Run the tests and verify that they fail**

Run: `source scripts/env.sh && cargo test -p actionlay-telemetry-cli -- --test-threads=1`
Expected: FAIL (`points_match_gopro_to_csv`, `dump_csv_every_second`, `dump_json_marks_stale_values`, `info_lists_coverage` fail on empty output; `video_without_metadata_fails_cleanly` fails because the stub exits 0).

- [ ] **Step 3: Implementation**

`crates/telemetry-cli/src/main.rs`:
```rust
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
```

- [ ] **Step 4: Run the tests and verify that they pass**

Run: `source scripts/env.sh && cargo test -p actionlay-telemetry-cli -- --test-threads=1`
Expected: PASS (5 passed).

Manual comparison with the original:
```bash
cargo run -q -p actionlay-telemetry-cli -- dump --points samples/gopro/hero6.mp4 2>/dev/null | cut -d, -f1-3,5-9 > /tmp/ours.csv
tr -d '\r' < crates/telemetry/tests/reference/hero6.gopro-to-csv.csv | cut -d, -f1-3,5-9 | diff /tmp/ours.csv - && echo identical
cargo run -q -p actionlay-telemetry-cli -- info samples/gopro/hero6.mp4 2>/dev/null | grep -E '^(gps points|lat )'
```
Expected: `identical`, then `gps points: 417 (180 locked)` and a `lat` line with `43.5%  1.001-14.014`.

- [ ] **Step 5: CI binary and README**

`.github/workflows/ci.yml` — in `Build release binaries` use
`run: cargo build --release -p actionlay-app -p actionlay-media -p actionlay-telemetry-cli --bins`
and add to the `Upload binaries` paths:
```yaml
            target/release/actionlay-telemetry
            target/release/actionlay-telemetry.exe
```

`README.md`:
- Status note: replace `**Status: early prototype (milestone M0).** Today ActionLay is a fast,` … `Expect rough edges.` with:
  ```markdown
  > **Status: early prototype (milestone M1).** Today ActionLay is a fast,
  > hardware-accelerated video player for GoPro footage, and it reads the
  > telemetry GoPro cameras record. The overlay, the layout editor and the
  > export are being built next; see the [roadmap](#roadmap). Expect rough
  > edges.
  ```
- `## What works today`: add the bullet
  ```markdown
  - Reads GoPro telemetry (GPS, speed, altitude, accelerometer, gravity,
    orientation, camera temperature) and computes the same derived values as
    gopro-dashboard-overlay. `actionlay-telemetry dump VIDEO` prints them as
    CSV or JSON; `actionlay-telemetry info VIDEO` shows where data is missing.
  ```
- Roadmap: `| M1 |` → `| **M1** ✅ |`.
- Testing paragraph ("To run the tests, first generate the synthetic sample videos…"): replace the code block with
  ```bash
  ./scripts/make-synthetic-samples.sh
  ./scripts/fetch-gopro-samples.sh     # public GoPro samples, ~33 MB
  cargo test --workspace -- --test-threads=1
  ```
- Credits: replace the telemetry-parser bullet with
  ```markdown
  - **[gpmf-parser](https://github.com/gopro/gpmf-parser)** by GoPro documents
    the GPMF telemetry format; its sample videos (Apache-2.0) are ActionLay's
    telemetry test files.
  - **[telemetry-parser](https://github.com/AdrianEddy/telemetry-parser)** by
    AdrianEddy is the planned telemetry reader for DJI, Insta360 and other
    cameras.
  - **[GeographicLib](https://geographiclib.sourceforge.io)** (Charles Karney),
    through [geographiclib-rs](https://github.com/georust/geographiclib-rs),
    computes distances and bearings exactly as gopro-dashboard-overlay does.
  ```

- [ ] **Step 6: Full verification (same as CI)**

Run:
```bash
source scripts/env.sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
ACTIONLAY_REQUIRE_GOPRO_SAMPLES=1 cargo test --workspace -- --test-threads=1
cargo build --release -p actionlay-telemetry-cli
time ./target/release/actionlay-telemetry info samples/gopro/hero5.mp4 > /dev/null
```
Expected: no formatting diff, no clippy warnings, every test passes (telemetry: 57 unit + 4 reference + 5 files; media: +2 unit, +4 gpmf; CLI: 5), `info` well under a second.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/telemetry-cli .github/workflows/ci.yml README.md
git commit -m "feat(telemetry-cli): actionlay-telemetry dump and info"
```
