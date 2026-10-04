# M0 follow-ups

Findings deferred during the M0 reviews (none blocks the M0 merge). Triage for M1/M2.

## Promoted (do early in M1/M2)
- Audio clock is early by up to one callback period and moves in steps: interpolate with the callback Instant (the overlay renderer will sample this clock).
- Test the muted-callback contract (extract the cpal callback body into a function).
- Player::open returns Ok even if the worker cannot open the decoder; surface worker errors in the UI; show open errors while a video is loaded.
- CI: fail loudly when samples are missing on macOS/Linux (ACTIONLAY_REQUIRE_SAMPLES); add a static-link check step (otool/ldd/dumpbin).
- Player: forward frame step from the queue instead of re-seeking; avoid the double seek on scrub release; consider keeping decoder state on resume and re-syncing audio only (VideoToolbox seek/resume up to ~1.4 s on long-GOP files).

## Deferred (by task)
- Task 1: no commit hash pin on FFmpeg clone; missing doc comments on BuildInfo/build_info; QTKit.tbd linker warning (env noise from SDK framework list); static-link check manual only.
- Task 2: system ffmpeg installed before Task 4 needs it; mixed path separators in FFMPEG_DIR on Windows (cygpath -m); no synthetic samples on Windows (tests must skip); no timeout-minutes / unpinned action tags; push+pull_request double runs.
- Task 3: matrix tests cover only BT.709 (601/2020/full-range chroma untested); YUVJ440P/411P not in full-range match; SMPTE240M/FCC fall to height heuristic; audio_clock_time sample_rate==0 undocumented; set_speed panics on non-positive (UI must only pass presets); no test for seek while running.
- Task 4: pack/p010 panics on short buffers, no odd-size P010 test; fps 0 without r_frame_rate fallback; time_base zero-denominator; NOPTS duration reported 0.0; ten_bit misses other high-depth formats. (Unsupported audio codec failing the whole probe: fixed in the final fix wave.)
- Task 5: ACTIONLAY_NO_HW=0 also disables HW; receive() None ambiguous need-input vs drained; ACTIONLAY_NO_HW/flush/send_eof untested; 10-bit test ignores uv.len(); VT session may fail per-frame on CI VMs (unverified); HW slower than SW on Apple Silicon (record in M0 report).
- Task 5: YUVJ422P/444P still through swscale (range compression for those formats); warn-once path untested (no triggering sample).
- Task 6: audio clock early by up to one callback period + stepwise (interpolate with callback Instant) [plan-mandated]; R2 contract untested (extract callback body into fn); resampler not rebuilt on mid-stream format change; chunk pts ignores swr delay; missing pts → 0; no EOF drain (last ~17 ms lost); forced 2ch f32 fails on some devices (falls back to system clock); odd sample-rate capacity could split frames; lock().unwrap() in realtime callback.
- Task 7: short audio track → ~500 ms stall at end; pause() stores unclamped position; set_speed(same) re-seeks; failed input.seek continues from old position; Player::open Ok even if worker can't open decoder (+ wrong error variant on spawn failure); silence pad capped at 1 s; paused worker wakes every 5 ms; pause test may flake on slow machines; test gaps (seek under load, step at bounds, forced HW→SW).
- Task 7: device with >500 ms before first callback while samples queued would be flagged stalled (policy).
- Task 8: open error invisible while a player is loaded; chroma siting quarter-texel offset; old frame shown briefly on reopen.
- Task 9: focused widgets may double-handle Space/arrows; format_time float floor (0.29 → 0:00.28); slider with duration 0. (A/V offset now shows n/a when not audio-driven: fixed in the final fix wave.) Glyphs may render as tofu.
- Task 9: same-frame drag start+stop untested; shortcut may fire in the frame a drag begins.
- Task 10: ffprobe logs "Duplicate POC" on hevc8-1440p100-sync.mp4 (x265 ultrafast sample quirk).
