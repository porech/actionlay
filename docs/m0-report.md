# M0 – Player prototype results

Date: 2026-10-04 · Measured commit (code): 0c4de1d, branch `m0-player`; the report is a later commit · Host: Apple M4 Max, macOS (Darwin 27.0.0)

Gate status: **partial**. The automatic measurements are done; the visual checks, A/V by ear, and Windows are **PENDING — user check** (whoever wrote the report cannot see the window or hear the audio, and does not have a Windows machine). No result has been made up.

## Decoding (decode-bench, release, measured)
| File | Platform | Source | HW fps | SW fps | HW backend |
|---|---|---|---|---|---|
| GX013370.MP4 (1920x1440 @100 HEVC, 19866 frames) | macOS M4 Max | 100 | 166.5 | 515.9 | videotoolbox |
| hevc10-2160p60-sync.mp4 (3840x2160 @60 HEVC 10 bit) | macOS M4 Max | 60 | 165.3 | 209.0 | videotoolbox |
| hevc8-1440p100-sync.mp4 (1920x1440 @100) | macOS M4 Max | 100 | 186.2 | 1662.8 | videotoolbox |
| h264-1080p30-44k.mp4 (1080p30) | macOS M4 Max | 30 | 227.0 | 1968.7 | videotoolbox |
| (all) | Windows | | PENDING — user check | PENDING | d3d11va expected |

Criterion "HW >= source fps": met on all macOS files (1.65x–7.6x). Note: the synthetic samples have trivial content, so their SW fps are not representative; the realistic figure is GX013370.MP4.

Important observation: on Apple Silicon, software decoding is **faster** than VideoToolbox (516 vs 166 fps on the real file) because of the GPU->CPU download of every frame. Here, hardware acceleration saves CPU/energy, not throughput. The HW margin is still > 1.6x the source frame rate.

## Binary (measured)
- Static linking: **ok**. `otool -L target/release/actionlay` lists no libav*/libsw*; only system frameworks (AppKit, ApplicationServices, CoreGraphics, CoreVideo, Carbon, CoreFoundation, Foundation, QuartzCore, Metal, VideoToolbox, CoreMedia, CoreServices, AudioToolbox, AVFoundation, Security, OpenCL, OpenGL, VideoDecodeAcceleration, CoreAudio, ColorSync) and libSystem, libobjc, libiconv. No non-system libraries.
- Size: 19 919 568 bytes (19.0 MiB) in release; 16 527 584 bytes (15.8 MiB) after `strip` (copy). `decode-bench`: 5.06 MB.

## Latencies (release, measured; HEVC, audio on, 5 repetitions)
Time from the command to the first frame delivered to `poll_frame`. Open = `Player::open` -> first frame. Seek = precise seek to points beyond 5 s of the file. Resume = `pause()` + `play()` with audio (re-seek R2) -> first frame.

| File | Backend | Open | Precise seek (min–max) | Resume (min–max) |
|---|---|---|---|---|
| hevc8-1440p100-sync | videotoolbox | 140 ms | 8 ms – 1.44 s | 8 ms – 930 ms |
| hevc8-1440p100-sync | software | 18 ms | 14 – 155 ms | 5 – 105 ms |
| GX013370.MP4 | videotoolbox | 15 ms | 7 – 299 ms | 6 – 295 ms |
| GX013370.MP4 | software | 58 ms | 66 – 194 ms | 54 – 171 ms |

Reading: the values depend on the distance from the previous keyframe (precise seek and R2 re-decode from there). Keyframe interval measured with ffprobe: GX013370.MP4 one keyframe every 0.5 s; hevc8-1440p100-sync.mp4 keyframes at 0, 2.5, 5.0 s (2.5 s GOP, 5x longer). On the real file the worst case is ~0.3 s. On the synthetic file with VideoToolbox it reaches 0.9–1.4 s: this is consistent with the longer GOP, and the comparison with software (<= 155 ms on the same file) suggests, probably, that the per-frame cost of the GPU->CPU readback weighs on the re-decode from the keyframe; the cause was not isolated with a dedicated measurement. Individual repetitions are not reported. The temporary test used for the measurement was removed (not committed).

## Code quality (measured)
- `cargo test --workspace -- --test-threads=1`: all tests pass, 0 failed: app unit 5, media unit 21, audio_decode 2, ffmpeg_link 2, player 8, probe 4 (including `probe_tolerates_undecodable_audio`, PCM audio without a decoder -> `audio: None`), video_decode 5.
- `cargo fmt --all --check`: ok. `cargo clippy --workspace --all-targets -- -D warnings`: clean.

## Playback — PENDING — user check (macOS)
Not verifiable by the agent. Launch `source scripts/env.sh && ./target/release/actionlay samples/synthetic/hevc8-1440p100-sync.mp4` and fill in:

- [ ] PENDING — user check: **A/V.** The white flash and the beep of every second coincide by eye/ear; the `A/V` statistic stays within ±40 ms for all 10 s. Maximum observed: ___ ms.
- [ ] PENDING — user check: **Dropped frames.** On `hevc8-1440p100-sync.mp4` `dropped` grows by at most ~40 frames/s on a 60 Hz monitor (100 -> 60 shown); on `h264-1080p30-44k.mp4` `dropped` does **not** grow. Observed: ___
- [ ] PENDING — user check: **Colors vs ffplay.** Open the same instant with `ffplay -ss 3 samples/synthetic/hevc8-1440p100-sync.mp4` and also bring the app to 3 s (seek), so that both show the same instant; compare by eye (colored bars, blacks, whites). If the image is lighter/washed out, apply the sRGB correction (Task 8 Step 3) and repeat. Outcome: ok / correction applied.
- [ ] PENDING — user check: repeat A/V and colors with `samples/GX013370.MP4` (real scene, full range): no washed-out colors.
- [ ] PENDING — user check: **Robustness.** Repeated rapid seeks, seek to end of file, opening `samples/synthetic/hevc8-1080p30-noaudio.mp4`: no crash, no hang. At end of file: it stops on the last frame and stays paused; pressing play restarts from the beginning.

## Windows — PENDING — user check
Requires a Windows 10/11 machine with a GPU (the CI runners do not have one). Copy `actionlay.exe` and `decode-bench.exe` (from the CI artifact `actionlay-x86_64-pc-windows-msvc`, which contains `actionlay.exe` and `decode-bench.exe`; alternatively a local build with `cargo build --release -p actionlay-app -p actionlay-media --bins`) and the synthetic samples, then:

- [ ] PENDING — user check: `decode-bench.exe` HW/SW on the same files as the table; expected `decoder: d3d11va`, HW >= source fps.
- [ ] PENDING — user check: `dumpbin /dependents actionlay.exe` must not list `avcodec*.dll` (nor other FFmpeg DLLs).
- [ ] PENDING — user check: repeat the A/V, dropped frames, colors, and robustness checks from the macOS section.

## Decision
**Awaiting user checks** (visual, A/V, Windows). This is not yet a final decision.

Recommendation based only on the automatic data: **proceed with egui + wgpu + static FFmpeg**. Reasons: HW decoding well above the source frame rate on all files (even 4K60 10 bit), verified static linking, 16–19 MB binary, seek/resume latencies on the real file within ~0.3 s, clean test/fmt/clippy. I ask the user to judge by hand whether the 0.9–1.4 s seek with VideoToolbox on the synthetic file (2.5 s GOP) is acceptable.

The fallback to libmpv should be considered only if the user checks show A/V outside ±40 ms, uncorrectable colors, crashes in seeks, or a non-working d3d11va on Windows.
