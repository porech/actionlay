# Linked activities and video orientation

This guide describes ActionLay 1.4.0. Linked activities, file/folder matching and
alignment are supported in both the desktop and browser applications. Manual
video rotation controls are a desktop feature.

## Link an activity

Open a video first. On desktop, use **File → Video sources…**, then
**Link GPX/FIT/INSGPS…** or **Activity folder…**. You can also drop activity files
or a folder onto an open video. In the browser, expand **Video sources** and use
the corresponding file or folder selector. Both frontends accept GPX, FIT and
Insta360 phone `.insgps` files, including uppercase extensions. Before the
selection controls, both interfaces explain single-file alignment and automatic
matching for multiple files/folders, including the one/many/no-match outcomes.

Linked telemetry is used in playback and export. Available activity values take
precedence; camera telemetry fills the activity's gaps. Linking a GPS activity
does not create IMU data, stitch 360° footage or make unread camera metadata
count as loaded. The original activity and video files are never rewritten.

Each activity file is limited to 64 MB. Desktop scans run off the UI thread,
include nested directories, deduplicate paths and skip directory symlinks.
Browser folder selection uses the files made available by the browser; it does
not grant arbitrary filesystem access. Unrelated file types are ignored.

## Matching multiple files or a folder

Selecting multiple files, or selecting a folder even if it contains just one
activity, uses strict temporal matching:

1. Determine the video start in UTC: an explicit **Video UTC** override takes
   precedence, followed by embedded camera telemetry's start UTC, then video
   creation metadata. Desktop probes stream creation metadata with a container
   fallback; the browser reads creation dates from the MP4 index. Unknown dates,
   including the MP4 epoch placeholder, cannot supply a usable reference.
2. Compare each parsed activity's sample times with the video interval, before
   applying the activity offset. A candidate must have a sample in the interval
   or an interpolatable pair crossing it within the same segment and no more
   than two seconds apart. The outer start/end range alone does not match a
   video that lies inside a long gap.
3. Link the only matching candidate automatically. If several match, list their
   filenames/paths and UTC ranges and require the user to choose. A GPX and an
   INSGPS representation of the same ride remain two candidates.
4. If none match, report that no activity matches the video time and suggest
   selecting a single file to align starts. If the video time is unknown,
   suggest setting **Video UTC** or selecting a single file. Unresolved
   selections also report unreadable files with paths and diagnostic details.

This establishes temporal compatibility, not proof that a track was recorded
with that camera. Simultaneous recordings can produce multiple candidates.
Filenames, file modification dates and directory order do not decide the match.
A wrong camera clock can prevent strict matching even for the correct ride.

## Selecting a single file and adjusting alignment

A single activity file uses the video UTC reference when its samples match the
video interval. Otherwise, including when the video has no usable date, its
first usable timestamp is aligned to video time zero. Both frontends warn:

> Timestamps could not be matched. Starts were aligned. Adjust the activity offset if needed.

No calendar correction is guessed and the activity's timestamps are preserved.
**Video UTC** accepts an RFC 3339 date/time with a timezone, such as
`2026-07-29T14:30:04Z`. Clear the override to use the automatic reference again.

The offset is measured in seconds. A **positive offset moves activity data later
in the video**. In start-aligned mode, `+1` places the first activity sample at
video time 1 second. This can compensate for video and GPS starting separately.
In timestamp-aligned mode the same offset is added to the difference between
activity UTC and video UTC. An offset that leaves no overlapping data reports
an error; it does not silently choose another activity or alignment mode.

Desktop remembers the linked path, UTC override and offset per video. Browser
localStorage remembers the UTC override, offset and selected file identity for
up to 100 videos, but stores no activity contents. After reloading, reselect the
video and activity; reselecting the same activity restores its saved offset.
Selecting a different activity resets the offset to zero. **Unlink** restores
camera-only telemetry.

Telemetry can be longer than a clip: phone GPS may start before recording or
continue after it. Alignment maps its timestamps onto the video timeline and
clips the data used for that video; it does not stretch the activity to match
the clip's duration.

## Supported INSGPS representation

The parser implements the packed representation observed in the supplied
Insta360 phone sample: little-endian records of exactly 53 bytes, with no header.
This is a sample-derived format description, not a guarantee for all Insta360
models, firmware versions or phone apps.

| Byte offset | Size | Value |
|---|---|---|
| 0 | 8 | Unsigned Unix seconds (`u64`) |
| 8 | 2 | Milliseconds within the second (`u16`, 0–999) |
| 10 | 1 | ASCII fix status: `A` valid, `V` void |
| 11 | 8 | Latitude magnitude (`f64`, degrees) |
| 19 | 1 | ASCII hemisphere: `N` or `S` |
| 20 | 8 | Longitude magnitude (`f64`, degrees) |
| 28 | 1 | ASCII hemisphere: `E` or `W` |
| 29 | 8 | Speed (`f64`, metres/second) |
| 37 | 8 | Course over ground (`f64`, degrees) |
| 45 | 8 | Altitude (`f64`, metres) |

Millisecond precision and sample intervals are retained. Hemisphere bytes set
coordinate signs. A valid fix supplies GPS lock, coordinates, speed, course and
altitude; a void fix supplies only an unavailable GPS lock. Stored zero speeds
remain zero. The shared activity validation handles invalid metric values.
Truncated records, invalid status/direction bytes, invalid milliseconds and
unrepresentable timestamps produce explicit errors.

The implementation does not add an official-software speed filter or force
stationary values to zero. A discrepancy with an official overlay requires
separate investigation of the stored values and that software's alignment or
filtering policy.

## Video rotation on desktop

**Video sources → Video rotation** offers **Automatic**, **0° (original)**,
**90° clockwise**, **180°** and **90° counterclockwise**. Automatic is the default
and respects the video's display matrix. A manual choice overrides it and is
remembered per video. Preview, the video shown in the editor, overlay canvas
sizing and exported pixels use the same orientation. Quarter turns swap the
canvas width and height; overlays are rendered onto that oriented canvas.

CLI exports expose the same choice through `--rotation auto|0|90|180|270`.
The browser uses its media playback/decoding orientation handling; the desktop
manual rotation selector has not been added to the browser interface.

## Implementation and verification

- `crates/telemetry/src/external.rs` owns INSGPS parsing, activity matching,
  start fallback, alignment and the common native/WASM byte parser.
- `crates/app/src/activity_sources.rs` scans desktop files and folders;
  `main.rs` handles candidate selection and linked sources. Export receives the
  selected activity, alignment origin and offset.
- `crates/web/src/lib.rs` keeps camera telemetry separate from the merged result.
  Publishing subsequent camera samples remerges the activity, preserving linked
  telemetry during playback and export.
- `web/src/sources.js` plans browser candidate selection; `worker.js` parses and
  links files through WASM, and `main.js` manages controls and per-video settings.
- New controls, warnings and actionable errors use all 34 shared language
  catalogues. Filesystem/parser diagnostic details may retain their own wording.

Regression coverage includes record precision, hemispheres, zero speeds, void
fixes, malformed records, matching and start fallback, offset direction,
recursive/deduplicated directory scanning with symlink loops, ambiguous and
nonmatching selections, browser folder selection using actual video creation
metadata, translated warnings, external telemetry surviving camera updates and
unlinking, and native exported pixels matching preview with a linked activity
and offset. Rotation tests use synthetic media and cover exported orientation.
Private sample videos and GPS tracks are not committed as fixtures.

With native FFmpeg configured as described in [building](building.md):

```sh
cargo test -p actionlay-app -p actionlay-telemetry
bash scripts/build-web.sh
npm --prefix web test
npm --prefix web run test:browser
cargo fmt --all --check
git diff --check
```

Local verification was performed on Apple Silicon macOS and Chromium. Windows,
Linux and other browsers require their platform/CI checks; local success does
not establish compatibility with untested INSGPS variants.
