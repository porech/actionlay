# Building ActionLay

You need Rust (the version is pinned in `rust-toolchain.toml` and installed
automatically by rustup), Git, Make, CMake, pkg-config, and the tools needed to build FFmpeg:
`nasm` and a C compiler. On Linux you also need the `libva` and ALSA
development headers.

```bash
git clone https://github.com/porech/actionlay.git
cd actionlay
make run                           # builds FFmpeg if needed, then launches the app
make run ARGS="path/to/video.mp4"   # optionally open a video on launch
```

The Makefile sets `FFMPEG_DIR` automatically for each command. `make build`
builds the release binaries in `target/release/`; `make check` checks formatting
and runs Clippy, and `make fmt` formats Rust code. Run `make help` for all commands.
The first build also compiles pinned static x264/x265 encoder libraries and takes
several minutes; subsequent calls reuse them. Windows uses a static C/C++ runtime.
CMake must be on PATH; MSYS2 provides pkg-config through the `pkgconf` package.
CI uses the same Makefile targets on all three platforms.

To use Cargo directly, run `bash scripts/build-ffmpeg.sh`, then
`source scripts/env.sh` in your terminal before running Cargo commands.

On Windows, build FFmpeg from an MSYS2 shell that inherits the Visual Studio
environment. The exact steps are in [.github/workflows/ci.yml](../.github/workflows/ci.yml).

To run the tests, first generate the synthetic sample videos. This needs an
`ffmpeg` command with libx264 and libx265:

```bash
make samples                       # synthetic clips + public GoPro samples (~33 MB)
make test

# Limited playback/seek diagnostics using the UI's player and telemetry decoder:
source scripts/env.sh
cargo run --release -p actionlay-app --example playback-check -- /path/to/video.mp4 240
# Optional third argument: milliseconds between polls, to simulate a slow UI.
cargo run --release -p actionlay-app --example playback-check -- /path/to/video.mp4 240 45
```


Release packaging is documented in [releasing.md](releasing.md).

For playback diagnostics, enable **Show diagnostic data** in **Settings → Advanced**.
The same window offers **Use software video decoding**, applied when a video is
next opened; leave it disabled to prefer hardware with software fallback.
`ACTIONLAY_NO_HW=1` still forces software decoding for command-line comparisons.
Set `RUST_LOG=actionlay_media=debug` and capture stderr.
The player reports a snapshot once per second: monotonic elapsed time, Unix wall
time in milliseconds, generation, audio/system clocks, decoded/presented video
PTS, queue sizes, buffering state, backend, and audio device format. The audio
tuple contains clock seconds, queued/consumed stereo frames, sample rate, then
device latency in microseconds, cumulative underrun callback count, and cumulative
silence frames inserted because samples were missing. Silence caused by user
volume zero or internal suspension during buffering is not counted as starvation.
Slow cache-miss reads (at least 100 ms) and buffering transitions are also logged.
No logging occurs in the audio data callback. A negative A/V diagnostic means the
last presented video PTS trails the audio clock; it alone does not establish
audible-content synchronization. Compare a captured output with the source audio
when investigating gaps or drift.
