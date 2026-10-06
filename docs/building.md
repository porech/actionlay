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
```


Release packaging is documented in [releasing.md](releasing.md).
