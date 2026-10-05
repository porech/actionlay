# M2 – Layout and basic render: results

Date: 2026-10-05 · Code commit: b312150, branch `m1-m2` (the report is a later commit) · Machine: Apple M4 Max, macOS (Darwin 27.0.0)

Gate status: **partial**. All automatic measurements are done. The visual and behavioural checks are **PENDING — user check**: whoever wrote this report cannot see the window or hear audio. No visual result has been made up.

## Automatic checks (measured)
- `cargo fmt --all --check`: ok.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean (0 warnings).
- `cargo test --workspace -- --test-threads=1`: all pass, 0 failed (app 46, layout 51 + schema 3, media 23 + audio_decode 2 + ffmpeg_link 2 + gpmf 6 + player 8 + probe 4 + video_decode 5, render 15 + golden 7 + render 19, telemetry 80 + contract_additions 2 + gopro_files 7 + reference 5, telemetry CLI 12).

## Render time (default layout, release, render-bench, two runs)
| Size | mean | p50 | p95 | max | clear | text | icons | shapes |
|---|---|---|---|---|---|---|---|---|
| 1920x1440 | 3.28 ms | 3.27 | 3.57 | 3.86 | 0.08 | 2.49 | 0.08 | 0.63 |
| 3840x2160 | 5.21 ms | 5.21 | 5.52 | 6.24 | 0.24 | 3.50 | 0.17 | 1.30 |

Second run: 1920x1440 mean 3.26 / p95 3.45 / max 3.79; 3840x2160 mean 5.22 / p95 5.49 / max 5.79 (stage times within 0.01 ms).

Target 1920x1440 p95 < 15 ms: **met** (3.57 ms, 4.2x margin). No action needed. At 25 Hz the budget per frame is 40 ms; 4K uses ~14%.

## Timed release launches (measured)
`RUST_LOG=actionlay=debug ./target/release/actionlay <file>`, killed after 12 s. The app opens paused, so only the first overlay frame (t=0) is logged; the figures below are that frame, rendered at the physical window size (the window size is the app default, not chosen by me). Playback timings are part of the user check.

| File | Physical size | First overlay frame | Log |
|---|---|---|---|
| samples/gopro/hero5.mp4 | 1568x881 | 3.21 ms | `video target format: Bgra8Unorm (srgb: false)`, shader `fs_main` |
| samples/gopro/hero7.mp4 (no GPS) | 1568x888 | 2.26 ms | same |
| samples/synthetic/hevc8-1080p30-noaudio.mp4 (no telemetry) | 1568x882 | 1.65 ms | same |

No error or warning from the app on any of the three (FFmpeg prints its usual "zero duration" note for the GoPro data stream). On this machine the srgb path is not used, so the `fs_main_srgb` shader (check 7) is not exercised here; it matters on other GPUs/platforms.

## Checks (Task 13 Step 2) — PENDING — user check
Launch: `source scripts/env.sh && cargo run --release -p actionlay-app -- samples/gopro/hero5.mp4` (or `./target/release/actionlay <file>`). Tick each box and fill in the blanks.

- [ ] PENDING — user check: **1. Placement.** Overlay appears within ~1 s of opening, inside the video rect. Make the window much wider, then much taller than the video: panels stay in the video corners, not the window corners. Also open a 4:3 video (hero5 is 16:9; use `samples/synthetic/hevc8-1440p100-sync.mp4`, 1920x1440) and check the same: no panel overlapping another or clipped. Outcome: ___
- [ ] PENDING — user check: **2. Live values (hero5, 34 s with GPS lock).** Press Space; speed, altitude, gradient, distance and coordinates change smoothly; the GPS icon is amber; date/time match the GoPro's GPS time converted to this Mac's time zone. Outcome: ___
- [ ] PENDING — user check: **3. No lock / lock lost.** Before the lock and after it is lost on hero5, and mid-video on `samples/gopro/hero6.mp4`: values are shown normally for up to 3 s, then dimmed, and the GPS icon is crossed out and dimmed. Pass: no stale number stays at full brightness. Outcome: ___
- [ ] PENDING — user check: **4. Sync.** Pause; press → and ←; click far on the seek bar while playing and while paused. Pass: the overlay changes in the same frame as the video (no visible lag). Dragging the seek bar: the overlay follows the keyframes. Resume after a seek: overlay stays in sync. Outcome: ___
- [ ] PENDING — user check: **5. O toggle and stats.** `O` hides and shows the overlay; the stats line shows `overlay N.N ms`. Typical value at window size ___x___ (physical ___x___): ___ ms. Expectation from the bench: about 3 ms at ~1.5 Mpixel.
- [ ] PENDING — user check: **6. Resize.** From tiny (a few pixels of video) to full screen on the Retina display: no crash; text crisp at full screen (rendered at physical pixels). Outcome: ___
- [ ] PENDING — user check: **7. Translucent panels.** Over a bright area the panels look dark translucent as in the goldens, not grey or washed out. The log here says `srgb: false`, so the normal shader path is used. Outcome: ___
- [ ] PENDING — user check: **8. Layout drop.** Copy `crates/layout/layouts/default.ovl.json`, add `"theme": {"palette": {"accent": "#00c8ff"}}` (replace an existing `theme` if present) and drop it on the window: icons turn cyan. Quit and relaunch: the edited layout is still used. Then drop a file with a syntax error: the error shows in the status bar and the previous layout stays. Outcome: ___
- [ ] PENDING — user check: **9. Empty states.** Open `samples/gopro/hero7.mp4` (no GPS) and `samples/synthetic/hevc8-1080p30-noaudio.mp4` (no telemetry at all): the panels show dimmed `—` (or are hidden where the layout says so), the GPS icon is crossed out, no error message. Outcome: ___
- [ ] PENDING — user check: **10. Legibility and quality.** On hero5 (16:9) and on a 4:3 video, no element of the default layout looks cramped or misaligned, and all text is readable over bright and dark footage. If something is off, edit `crates/layout/layouts/default.ovl.json`, run `ACTIONLAY_UPDATE_GOLDENS=1 cargo test -p actionlay-render --test golden`, review the new goldens and re-run the tests. Outcome: ___

## Known limitations
- On platforms whose video target is an sRGB surface, the overlay is blended in linear light, so translucent panels look lighter than in an export. Not the case on this Mac (`srgb: false`).
- Date/time uses the time zone of the machine running ActionLay, not the zone where the video was shot.
- Pace is shown as a decimal number (e.g. 5.5), not `m:ss`.
- The scale mode is chosen automatically: `fit` for videos narrower than the layout's design aspect, `height` otherwise.
- Only the M2 widget set exists: group, frame, text, metric, metric_unit, datetime, icon, gps_lock_icon. No maps, gauges or charts yet.

## Notes for M3
- Static-part caching (spec §4.5): at 1920x1440 text is 2.49 of 3.28 ms (76%), shapes 0.63 (19%), clear and icons 0.08 each (2.4% each); at 4K text is 3.50 of 5.21 ms (67%), shapes 1.30 (25%). Text shaping/rasterisation dominates and scales weakly with resolution, so caching glyph runs (static labels and units) is the first thing to try; the 15 ms target is already met 4x over, so caching is an optimisation for heavier M3 widgets, not a need today.
- Pace as `m:ss` and per-video time zones are deferred.
- The visual checks above are the open part of the gate.
