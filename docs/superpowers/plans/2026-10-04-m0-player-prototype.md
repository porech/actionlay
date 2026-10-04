# M0 – Prototipo player: piano di implementazione

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Dimostrare che uno stack Rust con egui + wgpu + FFmpeg statico riproduce in modo fluido, con audio sincronizzato, i video GoPro (HEVC 1440p a 100 fps, 4K HEVC 10 bit) su macOS e Windows, prima di costruire il resto di ActionLay.

**Architecture:** Workspace Cargo con due crate: `actionlay-media` (FFmpeg statico, decodifica HW, audio, orologio, thread del player) e `actionlay-app` (finestra eframe, rendering YUV→RGB in uno shader wgpu, controlli di trasporto). FFmpeg è compilato da uno script del repo come librerie statiche GPL e collegato con `ffmpeg-sys-next` tramite `FFMPEG_DIR`. I frame decodificati sono normalizzati in NV12 a 8 bit sulla CPU e caricati come due texture.

**Tech Stack:** Rust 1.98 (edition 2024), FFmpeg n9.0.2 statico, `ffmpeg-next` 9.0, `eframe`/`egui-wgpu` 0.36, `cpal` 0.18, `ringbuf` 0.5, `crossbeam-channel` 0.5, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-10-04-actionlay-design.md` (§3 player, §9 distribuzione, §10 fase M0).

## Global Constraints

- Licenza del progetto: GPL-3.0-or-later. FFmpeg configurato con `--enable-gpl`, **mai `--enable-nonfree`**.
- FFmpeg versione fissata: tag **`n9.0.2`**, binding `ffmpeg-next`/`ffmpeg-sys-next` **9.0**.
- FFmpeg collegato **staticamente**: il binario non deve dipendere da `libav*` dinamiche.
- Rust edition 2024, toolchain fissata in `rust-toolchain.toml` (1.98.1).
- Piattaforme M0: macOS arm64 (host locale, macOS 12+ come target), Windows 10 1809+ x64; Linux x64 solo compilazione + test in CI.
- Decodifica HW: VideoToolbox (macOS), D3D11VA (Windows), VA-API (Linux), con ripiego software e avviso.
- L'audio è l'orologio master a velocità 1x; senza audio o a velocità ≠ 1x si usa l'orologio di sistema e l'audio è silenziato.
- Colori: matrice e range corretti; `yuvj420p` è **full range**.
- Lingua: codice, commenti, messaggi di commit in inglese.
- Il campione `samples/GX013370.MP4` **non va mai pubblicato** né usato in CI; in CI si usano solo i campioni sintetici generati dallo script.
- Ogni commit termina con:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01JdSQ8H18o7xo5KZi58mp5c
  ```

## Review Focus

1. **Video senza traccia audio** (timelapse GoPro): deve riprodursi con l'orologio di sistema, senza bloccarsi in attesa dell'audio → test in Task 7 (`plays_file_without_audio`).
2. **Range colore full (`yuvj420p`) e video 10 bit**: colori non slavati, 10 bit visualizzato correttamente → test in Task 3 (`color_info_*`) e Task 4 (`p010_*`), verifica visiva al gate (Task 10).
3. **Seek oltre la fine o prima dell'inizio, seek ripetuti velocemente**: nessun crash, nessun frame "vecchio" mostrato dopo il seek → test in Task 7 (`seek_clamps_and_discards_stale_frames`).
4. **Frequenza audio del dispositivo diversa da quella del file** (44,1 kHz vs 48 kHz): niente deriva A/V → test in Task 6 (`resamples_44100_to_device_rate`) e misura A/V al gate.
5. **Decodifica HW non disponibile** (runner CI senza GPU, profili non supportati): ripiego automatico al software senza errori → test in Task 5 (`falls_back_to_software_when_hw_disabled`).

---

## Struttura dei file

```
Cargo.toml                         workspace
rust-toolchain.toml
.gitignore                         (+ /third_party/)
scripts/ffmpeg-version.env         FFMPEG_TAG=n9.0.2
scripts/build-ffmpeg.sh            build statica FFmpeg per il target host
scripts/env.sh                     esporta FFMPEG_DIR
scripts/make-synthetic-samples.sh  campioni pubblicabili con marcatori A/V
.github/workflows/ci.yml
crates/media/
  Cargo.toml
  build.rs                         collega le EXTRALIBS di FFmpeg
  src/lib.rs
  src/ffmpeg_info.rs               init + versione/licenza/configurazione
  src/clock.rs                     SystemClock, audio_clock_time
  src/color.rs                     ColorInfo, matrice YUV→RGB
  src/frame.rs                     Nv12Frame, conversioni piani/P010
  src/hw.rs                        attach del device HW, download frame (unsafe)
  src/probe.rs                     MediaInfo
  src/video.rs                     VideoDecoder → Nv12Frame
  src/audio.rs                     AudioDecoder (resample) + AudioOutput (cpal)
  src/present.rs                   scelta del frame da mostrare (pura)
  src/player.rs                    thread demux/decodifica, comandi, stato
  src/bin/decode-bench.rs          misura fps di decodifica
  tests/common/mod.rs              ricerca dei campioni
  tests/ffmpeg_link.rs
  tests/probe.rs
  tests/video_decode.rs
  tests/audio_decode.rs
  tests/player.rs
crates/app/
  Cargo.toml
  src/main.rs                      eframe, apertura file, layout UI
  src/video_view.rs                pipeline wgpu, texture Y/UV, callback
  src/yuv.wgsl                     shader YUV→RGB
  src/transport.rs                 barra di avanzamento, play/pausa, velocità, statistiche
docs/m0-report.md                  esito del gate M0
```

---

### Task 1: Workspace e FFmpeg statico (macOS/Linux)

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `scripts/ffmpeg-version.env`, `scripts/build-ffmpeg.sh`, `scripts/env.sh`, `crates/media/Cargo.toml`, `crates/media/build.rs`, `crates/media/src/lib.rs`, `crates/media/src/ffmpeg_info.rs`
- Modify: `.gitignore`
- Test: `crates/media/tests/ffmpeg_link.rs`

**Interfaces:**
- Produces: `actionlay_media::ffmpeg_info::{init() -> (), build_info() -> BuildInfo}`, `BuildInfo { version: String, configuration: String, license: String }`. Variabile d'ambiente `FFMPEG_DIR` impostata da `source scripts/env.sh`.

- [ ] **Step 1: Workspace e toolchain**

`Cargo.toml`:
```toml
[workspace]
resolver = "3"
members = ["crates/media", "crates/app"]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "GPL-3.0-or-later"
publish = false

[workspace.dependencies]
ffmpeg-next = { version = "9.0", default-features = false, features = ["codec", "format", "software-resampling", "software-scaling", "static"] }
crossbeam-channel = "0.5"
cpal = "0.18"
ringbuf = "0.5"
thiserror = "2"
log = "0.4"
anyhow = "1"
```
Nota: `crates/app` viene creato nel Task 8; fino ad allora togliere `"crates/app"` da `members` e rimetterlo nel Task 8.

`rust-toolchain.toml`:
```toml
[toolchain]
channel = "1.98.1"
components = ["rustfmt", "clippy"]
```

Aggiungere a `.gitignore`:
```
/third_party/
```

- [ ] **Step 2: Script di build di FFmpeg**

`scripts/ffmpeg-version.env`:
```bash
FFMPEG_TAG=n9.0.2
```

`scripts/build-ffmpeg.sh` (eseguibile, `chmod +x`):
```bash
#!/usr/bin/env bash
# Builds a static, GPL (never nonfree) FFmpeg for the host Rust target.
# Output: third_party/ffmpeg/<target>/{include,lib,extralibs.txt}
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
source "$ROOT/scripts/ffmpeg-version.env"
TARGET="${1:-$(rustc -vV | sed -n 's/^host: //p')}"
PREFIX="$ROOT/third_party/ffmpeg/$TARGET"
SRC="$ROOT/third_party/src/ffmpeg-$FFMPEG_TAG"

if [ -f "$PREFIX/extralibs.txt" ]; then
  echo "FFmpeg already built in $PREFIX"
  exit 0
fi

mkdir -p "$ROOT/third_party/src"
if [ ! -d "$SRC" ]; then
  git clone --depth 1 --branch "$FFMPEG_TAG" https://git.ffmpeg.org/ffmpeg.git "$SRC"
fi

COMMON=(
  --prefix="$PREFIX"
  --enable-gpl
  --enable-static --disable-shared
  --disable-programs --disable-doc
  --disable-autodetect --disable-network
  --disable-everything
  --disable-avdevice --disable-avfilter
  --enable-swresample --enable-swscale
  --enable-protocol=file
  --enable-demuxer=mov
  --enable-decoder=hevc,h264,aac
  --enable-parser=hevc,h264,aac
)

case "$TARGET" in
  *apple-darwin)
    PLATFORM=(--enable-pthreads --enable-videotoolbox
              --enable-hwaccel=hevc_videotoolbox,h264_videotoolbox) ;;
  *windows-msvc)
    PLATFORM=(--toolchain=msvc --enable-w32threads --enable-d3d11va
              --enable-hwaccel=hevc_d3d11va,hevc_d3d11va2,h264_d3d11va,h264_d3d11va2) ;;
  *linux-gnu)
    PLATFORM=(--enable-pthreads --enable-pic --enable-vaapi
              --enable-hwaccel=hevc_vaapi,h264_vaapi) ;;
  *)
    echo "Unsupported target: $TARGET" >&2
    exit 1 ;;
esac

cd "$SRC"
make distclean >/dev/null 2>&1 || true
./configure "${COMMON[@]}" "${PLATFORM[@]}"
make -j"$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)"
make install

# FFmpeg's own system-library requirements (frameworks, -lm, -lva, ...),
# consumed by crates/media/build.rs.
grep '^EXTRALIBS' ffbuild/config.mak | cut -d= -f2- > "$PREFIX/extralibs.txt"

case "$TARGET" in
  *windows-msvc)
    # rustc looks for avcodec.lib, FFmpeg's MSVC build installs libavcodec.a
    for f in "$PREFIX"/lib/lib*.a; do
      base="$(basename "$f" .a)"
      mv "$f" "$PREFIX/lib/${base#lib}.lib"
    done ;;
esac

echo "FFmpeg $FFMPEG_TAG installed in $PREFIX"
```

`scripts/env.sh`:
```bash
# Usage: source scripts/env.sh
ACTIONLAY_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." && pwd)"
export FFMPEG_DIR="$ACTIONLAY_ROOT/third_party/ffmpeg/$(rustc -vV | sed -n 's/^host: //p')"
```

- [ ] **Step 3: Costruire FFmpeg in locale**

Prerequisiti macOS: Xcode Command Line Tools, `brew install nasm pkg-config`.
Run: `cd ~/actionlay && ./scripts/build-ffmpeg.sh`
Expected: termina con `FFmpeg n9.0.2 installed in .../third_party/ffmpeg/aarch64-apple-darwin`; `ls third_party/ffmpeg/aarch64-apple-darwin/lib` mostra `libavcodec.a libavformat.a libavutil.a libswresample.a libswscale.a`; `cat .../extralibs.txt` contiene `-framework VideoToolbox`.

- [ ] **Step 4: Crate media con build.rs**

`crates/media/Cargo.toml`:
```toml
[package]
name = "actionlay-media"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true

[dependencies]
ffmpeg-next.workspace = true
crossbeam-channel.workspace = true
cpal.workspace = true
ringbuf.workspace = true
thiserror.workspace = true
log.workspace = true

[dev-dependencies]
anyhow.workspace = true
```

`crates/media/build.rs`:
```rust
//! Links the system libraries FFmpeg's static build depends on.
//! ffmpeg-sys-next links the libav* archives; this adds what their
//! configure step recorded in EXTRALIBS (see scripts/build-ffmpeg.sh).
use std::{collections::BTreeSet, env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");
    let dir = env::var("FFMPEG_DIR")
        .expect("FFMPEG_DIR is not set: run scripts/build-ffmpeg.sh, then `source scripts/env.sh`");
    let path = PathBuf::from(dir).join("extralibs.txt");
    println!("cargo:rerun-if-changed={}", path.display());
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

    let mut seen = BTreeSet::new();
    let mut tokens = text.split_whitespace();
    while let Some(token) = tokens.next() {
        let directive = if token == "-framework" {
            tokens.next().map(|f| format!("framework={f}"))
        } else if let Some(lib) = token.strip_prefix("-l") {
            Some(lib.to_string())
        } else {
            token.strip_suffix(".lib").map(str::to_string)
        };
        if let Some(d) = directive {
            if seen.insert(d.clone()) {
                println!("cargo:rustc-link-lib={d}");
            }
        }
    }
}
```

`crates/media/src/lib.rs`:
```rust
//! Media layer of ActionLay: FFmpeg access, decoding, audio output, playback.
pub mod ffmpeg_info;
```

- [ ] **Step 5: Scrivere il test che fallisce**

`crates/media/tests/ffmpeg_link.rs`:
```rust
use actionlay_media::ffmpeg_info;
use ffmpeg_next as ffmpeg;

#[test]
fn ffmpeg_is_pinned_gpl_and_free() {
    ffmpeg_info::init();
    let info = ffmpeg_info::build_info();
    assert!(info.version.contains("9.0.2"), "version: {}", info.version);
    assert!(info.configuration.contains("--enable-gpl"), "{}", info.configuration);
    assert!(!info.configuration.contains("nonfree"), "{}", info.configuration);
    assert_eq!(info.license, "GPL version 2 or later");
}

#[test]
fn required_decoders_are_present() {
    ffmpeg_info::init();
    for id in [ffmpeg::codec::Id::HEVC, ffmpeg::codec::Id::H264, ffmpeg::codec::Id::AAC] {
        assert!(ffmpeg::decoder::find(id).is_some(), "missing decoder {id:?}");
    }
}
```

- [ ] **Step 6: Eseguire il test e verificare che fallisca**

Run: `source scripts/env.sh && cargo test -p actionlay-media --test ffmpeg_link`
Expected: FAIL in compilazione, `could not find ffmpeg_info` / `init` non definito.

- [ ] **Step 7: Implementazione minima**

`crates/media/src/ffmpeg_info.rs`:
```rust
//! FFmpeg initialisation and build metadata.
use std::{ffi::CStr, sync::Once};

use ffmpeg_next as ffmpeg;
use ffmpeg_next::ffi;

#[derive(Debug, Clone)]
pub struct BuildInfo {
    pub version: String,
    pub configuration: String,
    pub license: String,
}

/// Initialises FFmpeg once per process and keeps its logging quiet.
pub fn init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        ffmpeg::init().expect("FFmpeg failed to initialise");
        ffmpeg::util::log::set_level(ffmpeg::util::log::Level::Warning);
    });
}

pub fn build_info() -> BuildInfo {
    // SAFETY: these functions return pointers to static, NUL-terminated strings.
    unsafe {
        BuildInfo {
            version: CStr::from_ptr(ffi::av_version_info()).to_string_lossy().into_owned(),
            configuration: CStr::from_ptr(ffi::avcodec_configuration()).to_string_lossy().into_owned(),
            license: CStr::from_ptr(ffi::avcodec_license()).to_string_lossy().into_owned(),
        }
    }
}
```

- [ ] **Step 8: Eseguire i test e verificare che passino, e che il collegamento sia statico**

Run: `source scripts/env.sh && cargo test -p actionlay-media --test ffmpeg_link`
Expected: PASS (2 test).
Run: `otool -L $(ls -t target/debug/deps/ffmpeg_link-* | grep -v '\.d$' | head -1) | grep -i -E 'libav|libsw' || echo STATIC-OK`
Expected: `STATIC-OK`.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml rust-toolchain.toml .gitignore scripts crates/media
git commit -m "build: static GPL FFmpeg n9.0.2 and media crate skeleton"
```

---

### Task 2: CI su macOS, Windows, Linux con FFmpeg statico

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: `scripts/build-ffmpeg.sh`, `scripts/env.sh` (Task 1).
- Produces: job CI `build-test` per `macos-14`, `windows-2022`, `ubuntu-22.04`, con `FFMPEG_DIR` esportato e cache di `third_party/ffmpeg`.

- [ ] **Step 1: Scrivere il workflow**

`.github/workflows/ci.yml`:
```yaml
name: ci
on:
  push:
  pull_request:

jobs:
  build-test:
    strategy:
      fail-fast: false
      matrix:
        include:
          - os: macos-14
            target: aarch64-apple-darwin
          - os: windows-2022
            target: x86_64-pc-windows-msvc
          - os: ubuntu-22.04
            target: x86_64-unknown-linux-gnu
    runs-on: ${{ matrix.os }}
    defaults:
      run:
        shell: bash
    steps:
      - uses: actions/checkout@v4

      - name: Install build tools (macOS)
        if: runner.os == 'macOS'
        run: brew install nasm ffmpeg

      - name: Install build tools (Linux)
        if: runner.os == 'Linux'
        run: sudo apt-get update && sudo apt-get install -y nasm libva-dev libasound2-dev ffmpeg

      - name: MSVC environment (Windows)
        if: runner.os == 'Windows'
        uses: ilammy/msvc-dev-cmd@v1

      - name: MSYS2 tools (Windows)
        if: runner.os == 'Windows'
        uses: msys2/setup-msys2@v2
        with:
          path-type: inherit
          install: make nasm diffutils git

      - name: Cache FFmpeg
        uses: actions/cache@v4
        with:
          path: third_party/ffmpeg
          key: ffmpeg-${{ matrix.target }}-${{ hashFiles('scripts/ffmpeg-version.env', 'scripts/build-ffmpeg.sh') }}

      - name: Build FFmpeg (macOS/Linux)
        if: runner.os != 'Windows'
        run: ./scripts/build-ffmpeg.sh ${{ matrix.target }}

      - name: Build FFmpeg (Windows)
        if: runner.os == 'Windows'
        shell: msys2 {0}
        run: ./scripts/build-ffmpeg.sh ${{ matrix.target }}

      - name: Export FFMPEG_DIR
        run: echo "FFMPEG_DIR=$GITHUB_WORKSPACE/third_party/ffmpeg/${{ matrix.target }}" >> "$GITHUB_ENV"

      - name: libclang for bindgen (Windows)
        if: runner.os == 'Windows'
        run: echo "LIBCLANG_PATH=C:\\Program Files\\LLVM\\bin" >> "$GITHUB_ENV"

      - name: Synthetic samples (macOS/Linux)
        if: runner.os != 'Windows'
        run: ./scripts/make-synthetic-samples.sh && echo "ACTIONLAY_SAMPLES=$GITHUB_WORKSPACE/samples/synthetic" >> "$GITHUB_ENV"

      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace
```
Note: lo step "Synthetic samples" fallisce finché il Task 4 non crea lo script: in questo task commentarlo e riattivarlo nel Task 4. I runner CI non hanno GPU: i test di decodifica usano il ripiego software (Review Focus 5).

- [ ] **Step 2: Verifica locale della sintassi**

Run: `python3 -c "import yaml,sys;yaml.safe_load(open('.github/workflows/ci.yml'))" && echo YAML-OK`
Expected: `YAML-OK` (se PyYAML manca, usare `ruby -ryaml -e 'YAML.load_file(".github/workflows/ci.yml")' && echo YAML-OK`).

- [ ] **Step 3: Verifica su CI**

Il repo non ha ancora un remote: questo step si esegue quando l'utente crea il repository GitHub e fa push. Expected: job verde sulle tre piattaforme; se il job Windows fallisce al link, leggere i simboli mancanti, aggiungere la libreria di sistema corrispondente nel case `*windows-msvc` di `build-ffmpeg.sh` (es. `--extra-libs=...`) e annotare la correzione in `docs/m0-report.md`.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: build static FFmpeg and test on macOS, Windows, Linux"
```

---

### Task 3: Orologio e colore (logica pura)

**Files:**
- Create: `crates/media/src/clock.rs`, `crates/media/src/color.rs`
- Modify: `crates/media/src/lib.rs`

**Interfaces:**
- Produces:
  - `clock::SystemClock::{new(start: f64, now: Instant) -> Self, time(&self, now: Instant) -> f64, set_paused(&mut self, paused: bool, now: Instant), set_speed(&mut self, speed: f64, now: Instant), seek(&mut self, to: f64, now: Instant), is_paused(&self) -> bool, speed(&self) -> f64}`
  - `clock::audio_clock_time(base_pts: f64, frames_played: u64, sample_rate: u32, output_latency: Duration) -> f64`
  - `color::{Matrix, Range, ColorInfo { matrix, range }}`, `ColorInfo::from_ffmpeg(space: ffmpeg::color::Space, range: ffmpeg::color::Range, pixel: ffmpeg::format::Pixel, height: u32) -> ColorInfo`, `color::yuv_to_rgb(c: ColorInfo) -> [[f32; 4]; 3]`

- [ ] **Step 1: Scrivere i test che falliscono**

In fondo a `crates/media/src/clock.rs` (file nuovo con solo i test per ora, più `use super::*;`):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn approx(a: f64, b: f64) -> bool { (a - b).abs() < 1e-9 }

    #[test]
    fn paused_clock_does_not_advance() {
        let t0 = Instant::now();
        let c = SystemClock::new(5.0, t0);
        assert!(c.is_paused());
        assert!(approx(c.time(t0 + Duration::from_secs(3)), 5.0));
    }

    #[test]
    fn running_clock_advances_with_speed() {
        let t0 = Instant::now();
        let mut c = SystemClock::new(0.0, t0);
        c.set_paused(false, t0);
        assert!(approx(c.time(t0 + Duration::from_secs(2)), 2.0));
        c.set_speed(2.0, t0 + Duration::from_secs(2));
        assert!(approx(c.time(t0 + Duration::from_secs(3)), 4.0));
    }

    #[test]
    fn pause_keeps_position_and_seek_moves_it() {
        let t0 = Instant::now();
        let mut c = SystemClock::new(0.0, t0);
        c.set_paused(false, t0);
        c.set_paused(true, t0 + Duration::from_millis(1500));
        assert!(approx(c.time(t0 + Duration::from_secs(10)), 1.5));
        c.seek(42.0, t0 + Duration::from_secs(10));
        assert!(approx(c.time(t0 + Duration::from_secs(11)), 42.0));
    }

    #[test]
    fn audio_clock_subtracts_latency_but_not_below_base() {
        let t = audio_clock_time(10.0, 48_000, 48_000, Duration::from_millis(20));
        assert!(approx(t, 10.98));
        assert!(approx(audio_clock_time(10.0, 0, 48_000, Duration::from_millis(20)), 10.0));
    }
}
```

In fondo a `crates/media/src/color.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ffmpeg_next::{color, format::Pixel};

    fn apply(m: [[f32; 4]; 3], y: u8, u: u8, v: u8) -> [f32; 3] {
        let (y, u, v) = (y as f32 / 255.0, u as f32 / 255.0, v as f32 / 255.0);
        let mut out = [0.0; 3];
        for (i, row) in m.iter().enumerate() {
            out[i] = row[0] * y + row[1] * u + row[2] * v + row[3];
        }
        out
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool { a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.02) }

    #[test]
    fn limited_bt709_black_white_red() {
        let m = yuv_to_rgb(ColorInfo { matrix: Matrix::Bt709, range: Range::Limited });
        assert!(close(apply(m, 16, 128, 128), [0.0, 0.0, 0.0]));
        assert!(close(apply(m, 235, 128, 128), [1.0, 1.0, 1.0]));
        assert!(close(apply(m, 63, 102, 240), [1.0, 0.0, 0.0]));
    }

    #[test]
    fn full_range_uses_whole_scale() {
        let m = yuv_to_rgb(ColorInfo { matrix: Matrix::Bt709, range: Range::Full });
        assert!(close(apply(m, 0, 128, 128), [0.0, 0.0, 0.0]));
        assert!(close(apply(m, 255, 128, 128), [1.0, 1.0, 1.0]));
    }

    #[test]
    fn color_info_gopro_yuvj_is_full_range_bt709() {
        let c = ColorInfo::from_ffmpeg(color::Space::Unspecified, color::Range::Unspecified, Pixel::YUVJ420P, 1440);
        assert_eq!(c, ColorInfo { matrix: Matrix::Bt709, range: Range::Full });
    }

    #[test]
    fn color_info_defaults_by_height_and_tags() {
        let sd = ColorInfo::from_ffmpeg(color::Space::Unspecified, color::Range::Unspecified, Pixel::YUV420P, 480);
        assert_eq!(sd, ColorInfo { matrix: Matrix::Bt601, range: Range::Limited });
        let uhd = ColorInfo::from_ffmpeg(color::Space::BT2020NCL, color::Range::MPEG, Pixel::YUV420P10LE, 2160);
        assert_eq!(uhd, ColorInfo { matrix: Matrix::Bt2020, range: Range::Limited });
        let jpeg = ColorInfo::from_ffmpeg(color::Space::BT709, color::Range::JPEG, Pixel::YUV420P, 1080);
        assert_eq!(jpeg.range, Range::Full);
    }
}
```

Aggiungere a `lib.rs`: `pub mod clock;` e `pub mod color;`.

- [ ] **Step 2: Eseguire i test e verificare che falliscano**

Run: `source scripts/env.sh && cargo test -p actionlay-media --lib`
Expected: FAIL in compilazione (`SystemClock`, `yuv_to_rgb`, `ColorInfo` non definiti).

- [ ] **Step 3: Implementazione**

Inizio di `crates/media/src/clock.rs`:
```rust
//! Playback clocks. Audio drives playback at 1x; otherwise a system clock
//! scaled by the playback speed does.
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct SystemClock {
    anchor_media: f64,
    anchor_instant: Instant,
    speed: f64,
    paused: bool,
}

impl SystemClock {
    /// A paused clock positioned at `start` seconds of media time.
    pub fn new(start: f64, now: Instant) -> Self {
        Self { anchor_media: start, anchor_instant: now, speed: 1.0, paused: true }
    }

    pub fn time(&self, now: Instant) -> f64 {
        if self.paused {
            self.anchor_media
        } else {
            let elapsed = now.saturating_duration_since(self.anchor_instant).as_secs_f64();
            self.anchor_media + elapsed * self.speed
        }
    }

    pub fn set_paused(&mut self, paused: bool, now: Instant) {
        self.rebase(now);
        self.paused = paused;
    }

    pub fn set_speed(&mut self, speed: f64, now: Instant) {
        assert!(speed > 0.0, "speed must be positive");
        self.rebase(now);
        self.speed = speed;
    }

    pub fn seek(&mut self, to: f64, now: Instant) {
        self.anchor_media = to;
        self.anchor_instant = now;
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn speed(&self) -> f64 {
        self.speed
    }

    fn rebase(&mut self, now: Instant) {
        self.anchor_media = self.time(now);
        self.anchor_instant = now;
    }
}

/// Media time of the sample currently leaving the speakers.
pub fn audio_clock_time(base_pts: f64, frames_played: u64, sample_rate: u32, output_latency: Duration) -> f64 {
    let played = frames_played as f64 / sample_rate as f64;
    (base_pts + played - output_latency.as_secs_f64()).max(base_pts)
}
```

Inizio di `crates/media/src/color.rs`:
```rust
//! YUV colour description and the YUV→RGB matrix used by the video shader.
use ffmpeg_next::{color, format::Pixel};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matrix { Bt601, Bt709, Bt2020 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range { Limited, Full }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorInfo {
    pub matrix: Matrix,
    pub range: Range,
}

impl ColorInfo {
    /// Untagged streams: BT.709 from 720 lines up, BT.601 below; `yuvj*` formats are full range.
    pub fn from_ffmpeg(space: color::Space, range: color::Range, pixel: Pixel, height: u32) -> Self {
        let matrix = match space {
            color::Space::BT709 => Matrix::Bt709,
            color::Space::BT2020NCL | color::Space::BT2020CL => Matrix::Bt2020,
            color::Space::BT470BG | color::Space::SMPTE170M => Matrix::Bt601,
            _ if height >= 720 => Matrix::Bt709,
            _ => Matrix::Bt601,
        };
        let range = match range {
            color::Range::JPEG => Range::Full,
            color::Range::MPEG => Range::Limited,
            _ if matches!(pixel, Pixel::YUVJ420P | Pixel::YUVJ422P | Pixel::YUVJ444P) => Range::Full,
            _ => Range::Limited,
        };
        Self { matrix, range }
    }
}

/// Row-major 3x4 matrix: `rgb = M · [y, u, v, 1]`, with y/u/v normalised to 0..1
/// as sampled from 8-bit textures.
pub fn yuv_to_rgb(c: ColorInfo) -> [[f32; 4]; 3] {
    let (kr, kb) = match c.matrix {
        Matrix::Bt601 => (0.299_f32, 0.114_f32),
        Matrix::Bt709 => (0.2126, 0.0722),
        Matrix::Bt2020 => (0.2627, 0.0593),
    };
    let kg = 1.0 - kr - kb;
    let (y_scale, y_offset, c_scale) = match c.range {
        Range::Limited => (255.0 / 219.0, 16.0 / 255.0, 255.0 / 224.0),
        Range::Full => (1.0, 0.0, 1.0),
    };
    let c_offset = 128.0 / 255.0;

    let r_v = 2.0 * (1.0 - kr) * c_scale;
    let g_u = -2.0 * kb * (1.0 - kb) / kg * c_scale;
    let g_v = -2.0 * kr * (1.0 - kr) / kg * c_scale;
    let b_u = 2.0 * (1.0 - kb) * c_scale;
    let y0 = -y_scale * y_offset;

    [
        [y_scale, 0.0, r_v, y0 - r_v * c_offset],
        [y_scale, g_u, g_v, y0 - (g_u + g_v) * c_offset],
        [y_scale, b_u, 0.0, y0 - b_u * c_offset],
    ]
}
```
Nota: `r_v`, `g_u`… includono già `c_scale`, quindi il termine costante usa `c_offset` direttamente.

- [ ] **Step 4: Eseguire i test e verificare che passino**

Run: `source scripts/env.sh && cargo test -p actionlay-media --lib`
Expected: PASS (8 test).

- [ ] **Step 5: Commit**

```bash
git add crates/media/src
git commit -m "feat(media): playback clocks and YUV colour matrices"
```

---

### Task 4: Campioni sintetici, frame NV12 e probe

**Files:**
- Create: `scripts/make-synthetic-samples.sh`, `crates/media/src/frame.rs`, `crates/media/src/probe.rs`, `crates/media/tests/common/mod.rs`, `crates/media/tests/probe.rs`
- Modify: `crates/media/src/lib.rs`, `.github/workflows/ci.yml` (riattivare lo step "Synthetic samples")

**Interfaces:**
- Consumes: `ffmpeg_info::init()`, `color::ColorInfo::from_ffmpeg` (Task 1, 3).
- Produces:
  - `frame::Nv12Frame { width: u32, height: u32, y: Vec<u8>, uv: Vec<u8>, pts: f64 }` (piani compatti: `y.len() == w*h`, `uv.len() == chroma_w*2 * chroma_h` con `chroma_w = (w+1)/2`, `chroma_h = (h+1)/2`)
  - `frame::pack_nv12(width: u32, height: u32, y: &[u8], y_stride: usize, uv: &[u8], uv_stride: usize) -> (Vec<u8>, Vec<u8>)`
  - `frame::p010_to_nv12(width: u32, height: u32, y: &[u8], y_stride: usize, uv: &[u8], uv_stride: usize) -> (Vec<u8>, Vec<u8>)`
  - `probe::{MediaInfo, VideoInfo, AudioInfo}`, `probe::probe(path: &Path) -> Result<MediaInfo, MediaError>`
  - `MediaInfo { duration: f64, video: VideoInfo, audio: Option<AudioInfo> }`, `VideoInfo { stream_index: usize, codec: String, width: u32, height: u32, fps: f64, time_base: f64, color: ColorInfo, ten_bit: bool }`, `AudioInfo { stream_index: usize, sample_rate: u32, channels: u16 }`
  - `MediaError` (thiserror): `Ffmpeg(ffmpeg::Error)`, `NoVideoStream`, `Hw(String)`, `Audio(String)`
  - Test helper: `common::sample(name: &str) -> Option<PathBuf>` (cerca in `$ACTIONLAY_SAMPLES`, poi `samples/synthetic`, poi `samples/`).

- [ ] **Step 1: Script dei campioni sintetici**

`scripts/make-synthetic-samples.sh` (eseguibile):
```bash
#!/usr/bin/env bash
# Generates publishable synthetic test videos. Needs an ffmpeg CLI with libx265 and libx264.
# A/V sync marks: at every whole second one white frame and a 10 ms 1 kHz beep.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/samples/synthetic}"
FF="${FFMPEG_BIN:-ffmpeg}"
mkdir -p "$OUT"

beep() { # $1 = duration, $2 = sample rate
  echo "sine=frequency=1000:sample_rate=$2:duration=$1,volume=enable='gte(mod(t\,1)\,0.01)':volume=0"
}

# GoPro-like: 1920x1440 100 fps HEVC 8-bit full range, 48 kHz audio
"$FF" -v error -y \
  -f lavfi -i "testsrc2=size=1920x1440:rate=100:duration=10,drawbox=enable='lt(mod(t\,1)\,0.01)':color=white:t=fill" \
  -f lavfi -i "$(beep 10 48000)" \
  -c:v libx265 -preset ultrafast -pix_fmt yuvj420p -tag:v hvc1 -x265-params log-level=error \
  -c:a aac -b:a 128k -shortest "$OUT/hevc8-1440p100-sync.mp4"

# 4K 60 fps HEVC Main10, limited range BT.709, 48 kHz audio
"$FF" -v error -y \
  -f lavfi -i "testsrc2=size=3840x2160:rate=60:duration=10,drawbox=enable='lt(mod(t\,1)\,0.0166)':color=white:t=fill" \
  -f lavfi -i "$(beep 10 48000)" \
  -c:v libx265 -preset ultrafast -pix_fmt yuv420p10le -profile:v main10 -tag:v hvc1 -x265-params log-level=error \
  -color_range tv -colorspace bt709 -c:a aac -b:a 128k -shortest "$OUT/hevc10-2160p60-sync.mp4"

# No audio track (timelapse-like)
"$FF" -v error -y \
  -f lavfi -i "testsrc2=size=1920x1080:rate=30:duration=5" \
  -c:v libx265 -preset ultrafast -pix_fmt yuv420p -tag:v hvc1 -x265-params log-level=error \
  "$OUT/hevc8-1080p30-noaudio.mp4"

# H.264 with 44.1 kHz audio
"$FF" -v error -y \
  -f lavfi -i "testsrc2=size=1920x1080:rate=30:duration=10,drawbox=enable='lt(mod(t\,1)\,0.0333)':color=white:t=fill" \
  -f lavfi -i "$(beep 10 44100)" \
  -c:v libx264 -preset ultrafast -pix_fmt yuv420p \
  -c:a aac -b:a 128k -shortest "$OUT/h264-1080p30-44k.mp4"

ls -l "$OUT"
```
Run: `./scripts/make-synthetic-samples.sh`
Expected: quattro file in `samples/synthetic/` (circa 1–40 MB ciascuno). Verificare con `ffprobe -v error -show_entries stream=codec_name,pix_fmt,color_range,sample_rate -of compact samples/synthetic/hevc8-1440p100-sync.mp4` → `pix_fmt=yuvj420p`, `color_range=pc`, `sample_rate=48000`.

Riattivare lo step "Synthetic samples" in `.github/workflows/ci.yml`.

- [ ] **Step 2: Scrivere i test che falliscono**

`crates/media/tests/common/mod.rs`:
```rust
use std::path::{Path, PathBuf};

/// Finds a sample video; returns None (test is skipped) when it is not available.
pub fn sample(name: &str) -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut dirs: Vec<PathBuf> = std::env::var_os("ACTIONLAY_SAMPLES").map(PathBuf::from).into_iter().collect();
    dirs.push(root.join("samples/synthetic"));
    dirs.push(root.join("samples"));
    let found = dirs.into_iter().map(|d| d.join(name)).find(|p| p.exists());
    if found.is_none() {
        eprintln!("sample {name} not found, skipping");
    }
    found
}
```

`crates/media/tests/probe.rs`:
```rust
mod common;
use actionlay_media::{color::{Matrix, Range}, probe::probe};

#[test]
fn probes_gopro_like_sample() {
    let Some(path) = common::sample("hevc8-1440p100-sync.mp4") else { return };
    let info = probe(&path).unwrap();
    assert_eq!(info.video.codec, "hevc");
    assert_eq!((info.video.width, info.video.height), (1920, 1440));
    assert!((info.video.fps - 100.0).abs() < 0.01, "fps {}", info.video.fps);
    assert_eq!(info.video.color.range, Range::Full);
    assert_eq!(info.video.color.matrix, Matrix::Bt709);
    assert!(!info.video.ten_bit);
    assert_eq!(info.audio.as_ref().unwrap().sample_rate, 48_000);
    assert!((info.duration - 10.0).abs() < 0.1);
}

#[test]
fn probes_ten_bit_and_missing_audio() {
    if let Some(path) = common::sample("hevc10-2160p60-sync.mp4") {
        let info = probe(&path).unwrap();
        assert!(info.video.ten_bit);
        assert_eq!(info.video.color.range, Range::Limited);
    }
    if let Some(path) = common::sample("hevc8-1080p30-noaudio.mp4") {
        assert!(probe(&path).unwrap().audio.is_none());
    }
}

#[test]
fn probe_reports_missing_file() {
    assert!(probe(std::path::Path::new("/nonexistent/video.mp4")).is_err());
}
```

In fondo a `crates/media/src/frame.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_nv12_drops_stride_padding() {
        // 3x2 image: chroma is 2x1 samples (4 bytes), strides padded
        let y = [1, 2, 3, 0, 4, 5, 6, 0];
        let uv = [7, 8, 9, 10, 0, 0];
        let (py, puv) = pack_nv12(3, 2, &y, 4, &uv, 6);
        assert_eq!(py, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(puv, vec![7, 8, 9, 10]);
    }

    #[test]
    fn p010_keeps_high_byte() {
        // 2x2 luma, 1x1 chroma pair; values are little-endian u16 with 10 bits in the top bits
        let y = [0x00, 0xFF, 0x40, 0x80, 0x00, 0x00, /*pad*/ 0x00, 0x10, 0xC0, 0x20, 0, 0];
        let uv = [0x00, 0x7F, 0x00, 0x81];
        let (py, puv) = p010_to_nv12(2, 2, &y, 6, &uv, 4);
        assert_eq!(py, vec![0xFF, 0x80, 0x10, 0x20]);
        assert_eq!(puv, vec![0x7F, 0x81]);
    }
}
```

Aggiungere a `lib.rs`: `pub mod frame; pub mod probe; mod error; pub use error::MediaError;` e creare `crates/media/src/error.rs` nello Step 4.

- [ ] **Step 3: Eseguire i test e verificare che falliscano**

Run: `source scripts/env.sh && cargo test -p actionlay-media`
Expected: FAIL in compilazione (`pack_nv12`, `probe` non definiti).

- [ ] **Step 4: Implementazione**

`crates/media/src/error.rs`:
```rust
use ffmpeg_next as ffmpeg;

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("FFmpeg: {0}")]
    Ffmpeg(#[from] ffmpeg::Error),
    #[error("the file has no video stream")]
    NoVideoStream,
    #[error("hardware decoding: {0}")]
    Hw(String),
    #[error("audio: {0}")]
    Audio(String),
}
```

Inizio di `crates/media/src/frame.rs`:
```rust
//! Decoded frames, always normalised to tightly packed 8-bit NV12.

#[derive(Debug, Clone)]
pub struct Nv12Frame {
    pub width: u32,
    pub height: u32,
    pub y: Vec<u8>,
    pub uv: Vec<u8>,
    /// Presentation time in seconds of media time.
    pub pts: f64,
}

fn chroma_dims(width: u32, height: u32) -> (usize, usize) {
    (width.div_ceil(2) as usize, height.div_ceil(2) as usize)
}

pub fn pack_nv12(width: u32, height: u32, y: &[u8], y_stride: usize, uv: &[u8], uv_stride: usize) -> (Vec<u8>, Vec<u8>) {
    let (w, h) = (width as usize, height as usize);
    let (cw, ch) = chroma_dims(width, height);
    let mut out_y = Vec::with_capacity(w * h);
    for row in 0..h {
        out_y.extend_from_slice(&y[row * y_stride..row * y_stride + w]);
    }
    let mut out_uv = Vec::with_capacity(cw * 2 * ch);
    for row in 0..ch {
        out_uv.extend_from_slice(&uv[row * uv_stride..row * uv_stride + cw * 2]);
    }
    (out_y, out_uv)
}

/// P010 stores 10-bit samples in the top bits of little-endian u16; the high byte is the 8-bit value.
pub fn p010_to_nv12(width: u32, height: u32, y: &[u8], y_stride: usize, uv: &[u8], uv_stride: usize) -> (Vec<u8>, Vec<u8>) {
    let (w, h) = (width as usize, height as usize);
    let (cw, ch) = chroma_dims(width, height);
    let mut out_y = Vec::with_capacity(w * h);
    for row in 0..h {
        let line = &y[row * y_stride..row * y_stride + w * 2];
        out_y.extend(line.chunks_exact(2).map(|s| s[1]));
    }
    let mut out_uv = Vec::with_capacity(cw * 2 * ch);
    for row in 0..ch {
        let line = &uv[row * uv_stride..row * uv_stride + cw * 4];
        out_uv.extend(line.chunks_exact(2).map(|s| s[1]));
    }
    (out_y, out_uv)
}
```

`crates/media/src/probe.rs`:
```rust
//! Static description of a media file.
use std::path::Path;

use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::Pixel;

use crate::{color::ColorInfo, ffmpeg_info, MediaError};

#[derive(Debug, Clone)]
pub struct VideoInfo {
    pub stream_index: usize,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub time_base: f64,
    pub color: ColorInfo,
    pub ten_bit: bool,
}

#[derive(Debug, Clone)]
pub struct AudioInfo {
    pub stream_index: usize,
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Debug, Clone)]
pub struct MediaInfo {
    pub duration: f64,
    pub video: VideoInfo,
    pub audio: Option<AudioInfo>,
}

pub fn probe(path: &Path) -> Result<MediaInfo, MediaError> {
    ffmpeg_info::init();
    let input = ffmpeg::format::input(path)?;
    let duration = input.duration().max(0) as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE);

    let vstream = input.streams().best(ffmpeg::media::Type::Video).ok_or(MediaError::NoVideoStream)?;
    let vctx = ffmpeg::codec::Context::from_parameters(vstream.parameters())?;
    let vdec = vctx.decoder().video()?;
    let rate = vstream.avg_frame_rate();
    let pixel = vdec.format();
    let video = VideoInfo {
        stream_index: vstream.index(),
        codec: vdec.codec().map(|c| c.name().to_string()).unwrap_or_default(),
        width: vdec.width(),
        height: vdec.height(),
        fps: if rate.denominator() == 0 { 0.0 } else { f64::from(rate) },
        time_base: f64::from(vstream.time_base()),
        color: ColorInfo::from_ffmpeg(vdec.color_space(), vdec.color_range(), pixel, vdec.height()),
        ten_bit: matches!(pixel, Pixel::YUV420P10LE | Pixel::YUV422P10LE | Pixel::P010LE),
    };

    let audio = match input.streams().best(ffmpeg::media::Type::Audio) {
        Some(astream) => {
            let actx = ffmpeg::codec::Context::from_parameters(astream.parameters())?;
            let adec = actx.decoder().audio()?;
            Some(AudioInfo {
                stream_index: astream.index(),
                sample_rate: adec.rate(),
                channels: adec.channels(),
            })
        }
        None => None,
    };

    Ok(MediaInfo { duration, video, audio })
}
```
Se in `ffmpeg-next` 9.0 `adec.channels()` restituisce un tipo diverso da `u16`, convertire con `as u16`; controllare con `cargo doc -p ffmpeg-next --open`.

- [ ] **Step 5: Eseguire i test e verificare che passino**

Run: `source scripts/env.sh && cargo test -p actionlay-media`
Expected: PASS (i test di probe passano con i campioni sintetici presenti; `probes_gopro_like_sample` stampa "skipping" solo se i campioni mancano).

- [ ] **Step 6: Commit**

```bash
git add scripts/make-synthetic-samples.sh crates/media .github/workflows/ci.yml
git commit -m "feat(media): synthetic samples, NV12 frames and media probe"
```

---

### Task 5: Decodifica video con accelerazione hardware

**Files:**
- Create: `crates/media/src/hw.rs`, `crates/media/src/video.rs`, `crates/media/src/bin/decode-bench.rs`, `crates/media/tests/video_decode.rs`
- Modify: `crates/media/src/lib.rs`

**Interfaces:**
- Consumes: `probe::VideoInfo`, `frame::{Nv12Frame, pack_nv12, p010_to_nv12}`, `MediaError` (Task 4).
- Produces:
  - `hw::HwKind::{VideoToolbox, D3d11va, Vaapi}`, `HwKind::for_platform() -> Option<HwKind>`, `HwKind::name(self) -> &'static str`
  - `video::VideoDecoder::open(params: ffmpeg::codec::Parameters, time_base: f64, prefer_hw: bool) -> Result<VideoDecoder, MediaError>`
  - `VideoDecoder::send(&mut self, packet: &ffmpeg::Packet) -> Result<(), MediaError>`
  - `VideoDecoder::send_eof(&mut self) -> Result<(), MediaError>`
  - `VideoDecoder::receive(&mut self) -> Result<Option<Nv12Frame>, MediaError>` (`None` = serve un altro pacchetto o fine)
  - `VideoDecoder::flush(&mut self)`
  - `VideoDecoder::active_backend(&self) -> &'static str` (`"videotoolbox"`, `"d3d11va"`, `"vaapi"`, `"software"`, `"unknown"` prima del primo frame)
  - Variabile d'ambiente `ACTIONLAY_NO_HW=1` → `prefer_hw` ignorato (sempre software).

- [ ] **Step 1: Scrivere i test che falliscono**

`crates/media/tests/video_decode.rs`:
```rust
mod common;
use actionlay_media::{probe::probe, video::VideoDecoder};
use ffmpeg_next as ffmpeg;

fn decode_first(path: &std::path::Path, n: usize, prefer_hw: bool) -> (Vec<f64>, &'static str, (u32, u32, usize, usize)) {
    let info = probe(path).unwrap();
    let mut input = ffmpeg::format::input(path).unwrap();
    let params = input.stream(info.video.stream_index).unwrap().parameters();
    let mut dec = VideoDecoder::open(params, info.video.time_base, prefer_hw).unwrap();
    let mut pts = Vec::new();
    let mut dims = (0, 0, 0, 0);
    for (stream, packet) in input.packets() {
        if stream.index() != info.video.stream_index { continue; }
        dec.send(&packet).unwrap();
        while let Some(f) = dec.receive().unwrap() {
            dims = (f.width, f.height, f.y.len(), f.uv.len());
            pts.push(f.pts);
            if pts.len() == n { return (pts, dec.active_backend(), dims); }
        }
    }
    (pts, dec.active_backend(), dims)
}

#[test]
fn decodes_frames_with_monotonic_pts() {
    let Some(path) = common::sample("hevc8-1440p100-sync.mp4") else { return };
    let (pts, backend, (w, h, ylen, uvlen)) = decode_first(&path, 50, true);
    assert_eq!(pts.len(), 50);
    assert!(pts.windows(2).all(|p| p[1] > p[0]), "pts not increasing: {pts:?}");
    assert!((pts[1] - pts[0] - 0.01).abs() < 1e-3);
    assert_eq!((w, h, ylen, uvlen), (1920, 1440, 1920 * 1440, 1920 * 720));
    eprintln!("backend: {backend}");
    if std::env::var("ACTIONLAY_EXPECT_HW").is_ok() {
        assert_ne!(backend, "software");
    }
}

#[test]
fn decodes_ten_bit_to_nv12() {
    let Some(path) = common::sample("hevc10-2160p60-sync.mp4") else { return };
    let (pts, _, (w, h, ylen, _)) = decode_first(&path, 5, true);
    assert_eq!(pts.len(), 5);
    assert_eq!((w, h, ylen), (3840, 2160, 3840 * 2160));
}

#[test]
fn falls_back_to_software_when_hw_disabled() {
    let Some(path) = common::sample("hevc8-1440p100-sync.mp4") else { return };
    let (pts, backend, _) = decode_first(&path, 3, false);
    assert_eq!(pts.len(), 3);
    assert_eq!(backend, "software");
}
```

Aggiungere a `lib.rs`: `pub mod hw; pub mod video;`.

- [ ] **Step 2: Eseguire i test e verificare che falliscano**

Run: `source scripts/env.sh && cargo test -p actionlay-media --test video_decode`
Expected: FAIL in compilazione (`video::VideoDecoder` non definito).

- [ ] **Step 3: Implementare `hw.rs`**

```rust
//! Hardware-accelerated decoding through FFmpeg hwaccels.
use std::{ffi::c_void, ptr};

use ffmpeg_next::ffi::*;
use ffmpeg_next::frame;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HwKind {
    VideoToolbox,
    D3d11va,
    Vaapi,
}

impl HwKind {
    pub fn for_platform() -> Option<HwKind> {
        if cfg!(target_os = "macos") {
            Some(HwKind::VideoToolbox)
        } else if cfg!(target_os = "windows") {
            Some(HwKind::D3d11va)
        } else if cfg!(target_os = "linux") {
            Some(HwKind::Vaapi)
        } else {
            None
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            HwKind::VideoToolbox => "videotoolbox",
            HwKind::D3d11va => "d3d11va",
            HwKind::Vaapi => "vaapi",
        }
    }

    fn device_type(self) -> AVHWDeviceType {
        match self {
            HwKind::VideoToolbox => AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX,
            HwKind::D3d11va => AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
            HwKind::Vaapi => AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
        }
    }

    fn pix_fmt(self) -> AVPixelFormat {
        match self {
            HwKind::VideoToolbox => AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX,
            HwKind::D3d11va => AVPixelFormat::AV_PIX_FMT_D3D11,
            HwKind::Vaapi => AVPixelFormat::AV_PIX_FMT_VAAPI,
        }
    }
}

/// Picks the hardware format stored in `ctx.opaque`; if the decoder does not offer it
/// for this stream (unsupported profile), falls back to the first software format.
unsafe extern "C" fn get_format(ctx: *mut AVCodecContext, fmts: *const AVPixelFormat) -> AVPixelFormat {
    unsafe {
        let wanted = (*ctx).opaque as isize as i32;
        let mut p = fmts;
        while *p != AVPixelFormat::AV_PIX_FMT_NONE {
            if *p as i32 == wanted {
                return *p;
            }
            p = p.add(1);
        }
        let mut p = fmts;
        while *p != AVPixelFormat::AV_PIX_FMT_NONE {
            let desc = av_pix_fmt_desc_get(*p);
            if !desc.is_null() && ((*desc).flags & u64::from(AV_PIX_FMT_FLAG_HWACCEL)) == 0 {
                return *p;
            }
            p = p.add(1);
        }
        AVPixelFormat::AV_PIX_FMT_NONE
    }
}

/// Attaches a hardware device to a not-yet-opened codec context.
///
/// # Safety
/// `ctx` must be a valid, unopened `AVCodecContext`.
pub unsafe fn attach(ctx: *mut AVCodecContext, kind: HwKind) -> Result<(), String> {
    unsafe {
        let mut device: *mut AVBufferRef = ptr::null_mut();
        let ret = av_hwdevice_ctx_create(&mut device, kind.device_type(), ptr::null(), ptr::null_mut(), 0);
        if ret < 0 {
            return Err(format!("av_hwdevice_ctx_create({}) failed with {ret}", kind.name()));
        }
        // The codec context takes ownership of the reference.
        (*ctx).hw_device_ctx = device;
        (*ctx).opaque = kind.pix_fmt() as i32 as isize as *mut c_void;
        (*ctx).get_format = Some(get_format);
        Ok(())
    }
}

pub fn is_hw_frame(f: &frame::Video) -> bool {
    // SAFETY: reading a field of a valid frame.
    unsafe { !(*f.as_ptr()).hw_frames_ctx.is_null() }
}

/// Copies a GPU frame into system memory (NV12 or P010).
pub fn download(hw: &frame::Video) -> Result<frame::Video, String> {
    let mut sw = frame::Video::empty();
    // SAFETY: both frames are valid; `sw` is empty so FFmpeg allocates its buffers.
    unsafe {
        let ret = av_hwframe_transfer_data(sw.as_mut_ptr(), hw.as_ptr(), 0);
        if ret < 0 {
            return Err(format!("av_hwframe_transfer_data failed with {ret}"));
        }
        (*sw.as_mut_ptr()).pts = (*hw.as_ptr()).pts;
        (*sw.as_mut_ptr()).best_effort_timestamp = (*hw.as_ptr()).best_effort_timestamp;
    }
    Ok(sw)
}
```

- [ ] **Step 4: Implementare `video.rs`**

```rust
//! Video decoding to NV12 frames, hardware first with software fallback.
use ffmpeg_next as ffmpeg;
use ffmpeg_next::{format::Pixel, frame, software::scaling};

use crate::{frame::{p010_to_nv12, pack_nv12, Nv12Frame}, hw::{self, HwKind}, MediaError};

pub struct VideoDecoder {
    decoder: ffmpeg::decoder::Video,
    time_base: f64,
    hw: Option<HwKind>,
    backend: &'static str,
    scaler: Option<(Pixel, scaling::Context)>,
}

impl VideoDecoder {
    pub fn open(params: ffmpeg::codec::Parameters, time_base: f64, prefer_hw: bool) -> Result<Self, MediaError> {
        let want_hw = prefer_hw && std::env::var_os("ACTIONLAY_NO_HW").is_none();
        if want_hw {
            if let Some(kind) = HwKind::for_platform() {
                match Self::open_hw(params.clone(), kind) {
                    Ok(decoder) => return Ok(Self { decoder, time_base, hw: Some(kind), backend: "unknown", scaler: None }),
                    Err(e) => log::warn!("hardware decoding unavailable, using software: {e}"),
                }
            }
        }
        let mut ctx = ffmpeg::codec::Context::from_parameters(params)?;
        ctx.set_threading(ffmpeg::codec::threading::Config::kind(ffmpeg::codec::threading::Type::Frame));
        let decoder = ctx.decoder().video()?;
        Ok(Self { decoder, time_base, hw: None, backend: "software", scaler: None })
    }

    fn open_hw(params: ffmpeg::codec::Parameters, kind: HwKind) -> Result<ffmpeg::decoder::Video, MediaError> {
        let mut ctx = ffmpeg::codec::Context::from_parameters(params)?;
        // SAFETY: the context is valid and not opened yet.
        unsafe { hw::attach(ctx.as_mut_ptr(), kind) }.map_err(MediaError::Hw)?;
        Ok(ctx.decoder().video()?)
    }

    pub fn active_backend(&self) -> &'static str {
        self.backend
    }

    pub fn send(&mut self, packet: &ffmpeg::Packet) -> Result<(), MediaError> {
        Ok(self.decoder.send_packet(packet)?)
    }

    pub fn send_eof(&mut self) -> Result<(), MediaError> {
        Ok(self.decoder.send_eof()?)
    }

    pub fn flush(&mut self) {
        self.decoder.flush();
    }

    pub fn receive(&mut self) -> Result<Option<Nv12Frame>, MediaError> {
        let mut decoded = frame::Video::empty();
        match self.decoder.receive_frame(&mut decoded) {
            Ok(()) => {}
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::util::error::EAGAIN => return Ok(None),
            Err(ffmpeg::Error::Eof) => return Ok(None),
            Err(e) => return Err(e.into()),
        }
        let ts = decoded.timestamp().or(decoded.pts()).unwrap_or(0);
        let pts = ts as f64 * self.time_base;
        let sw = if hw::is_hw_frame(&decoded) {
            self.backend = self.hw.map(HwKind::name).unwrap_or("unknown");
            hw::download(&decoded).map_err(MediaError::Hw)?
        } else {
            self.backend = "software";
            decoded
        };
        Ok(Some(self.to_nv12(&sw, pts)?))
    }

    fn to_nv12(&mut self, f: &frame::Video, pts: f64) -> Result<Nv12Frame, MediaError> {
        let (w, h) = (f.width(), f.height());
        let (y, uv) = match f.format() {
            Pixel::NV12 => pack_nv12(w, h, f.data(0), f.stride(0), f.data(1), f.stride(1)),
            Pixel::P010LE => p010_to_nv12(w, h, f.data(0), f.stride(0), f.data(1), f.stride(1)),
            other => {
                let scaler = match &mut self.scaler {
                    Some((fmt, s)) if *fmt == other => s,
                    slot => {
                        let s = scaling::Context::get(other, w, h, Pixel::NV12, w, h, scaling::Flags::BILINEAR)?;
                        &mut slot.insert((other, s)).1
                    }
                };
                let mut out = frame::Video::new(Pixel::NV12, w, h);
                scaler.run(f, &mut out)?;
                pack_nv12(w, h, out.data(0), out.stride(0), out.data(1), out.stride(1))
            }
        };
        Ok(Nv12Frame { width: w, height: h, y, uv, pts })
    }
}
```
Nota: lo scaler software converte `yuv420p`, `yuvj420p` e `yuv420p10le` in NV12; il range colore resta quello originale perché la matrice viene da `probe` (Task 3), non dai dati del frame.

- [ ] **Step 5: Eseguire i test e verificare che passino**

Run: `source scripts/env.sh && ACTIONLAY_EXPECT_HW=1 cargo test -p actionlay-media --test video_decode -- --nocapture`
Expected: PASS (3 test), output `backend: videotoolbox`.

- [ ] **Step 6: Benchmark di decodifica**

`crates/media/src/bin/decode-bench.rs`:
```rust
//! Usage: decode-bench <video> [--sw]
//! Decodes the whole video stream to NV12 and reports frames per second.
use std::time::Instant;

use actionlay_media::{probe::probe, video::VideoDecoder};
use ffmpeg_next as ffmpeg;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: decode-bench <video> [--sw]")?;
    let prefer_hw = args.next().as_deref() != Some("--sw");
    let info = probe(path.as_ref())?;
    let mut input = ffmpeg::format::input(&path)?;
    let params = input.stream(info.video.stream_index).ok_or("no video")?.parameters();
    let mut dec = VideoDecoder::open(params, info.video.time_base, prefer_hw)?;

    let start = Instant::now();
    let mut frames = 0u64;
    for (stream, packet) in input.packets() {
        if stream.index() != info.video.stream_index {
            continue;
        }
        dec.send(&packet)?;
        while dec.receive()?.is_some() {
            frames += 1;
        }
    }
    dec.send_eof()?;
    while dec.receive()?.is_some() {
        frames += 1;
    }
    let secs = start.elapsed().as_secs_f64();
    println!(
        "{path}: {}x{} @ {:.2} fps source, backend={}, {frames} frames in {secs:.2}s = {:.1} fps decoded",
        info.video.width, info.video.height, info.video.fps, dec.active_backend(), frames as f64 / secs
    );
    Ok(())
}
```
Run: `source scripts/env.sh && cargo run --release -p actionlay-media --bin decode-bench -- samples/GX013370.MP4`
Expected: `backend=videotoolbox` e fps decodificati **≥ 100** (sorgente a 100 fps). Ripetere con `samples/synthetic/hevc10-2160p60-sync.mp4` → **≥ 60**. Annotare i numeri (serviranno per `docs/m0-report.md`).

- [ ] **Step 7: Commit**

```bash
git add crates/media
git commit -m "feat(media): hardware video decoding to NV12 with software fallback"
```

---

### Task 6: Audio: decodifica, ricampionamento, uscita

**Files:**
- Create: `crates/media/src/audio.rs`, `crates/media/tests/audio_decode.rs`
- Modify: `crates/media/src/lib.rs`

**Interfaces:**
- Consumes: `probe::AudioInfo`, `clock::audio_clock_time`, `MediaError`.
- Produces:
  - `audio::AudioDecoder::open(params: ffmpeg::codec::Parameters, time_base: f64, out_rate: u32) -> Result<AudioDecoder, MediaError>`
  - `AudioDecoder::send(&mut self, packet: &ffmpeg::Packet) -> Result<(), MediaError>`, `flush(&mut self)`
  - `AudioDecoder::receive(&mut self) -> Result<Option<AudioChunk>, MediaError>`
  - `audio::AudioChunk { pts: f64, samples: Vec<f32> }` (stereo interleaved, `out_rate` Hz)
  - `audio::AudioOutput::open() -> Result<AudioOutput, MediaError>`, `AudioOutput::sample_rate(&self) -> u32`, `AudioOutput::push(&mut self, samples: &[f32]) -> usize` (quanti campioni accettati), `AudioOutput::queued_frames(&self) -> usize`, `AudioOutput::reset(&mut self, base_pts: f64)` (svuota il buffer, azzera il contatore), `AudioOutput::clock(&self) -> f64`, `AudioOutput::set_muted(&self, muted: bool)`
  - `audio::AudioOutput::CAPACITY_SECONDS: f64 = 0.5`

- [ ] **Step 1: Scrivere i test che falliscono**

`crates/media/tests/audio_decode.rs`:
```rust
mod common;
use actionlay_media::{audio::AudioDecoder, probe::probe};
use ffmpeg_next as ffmpeg;

fn decode_all(path: &std::path::Path, out_rate: u32) -> Vec<actionlay_media::audio::AudioChunk> {
    let info = probe(path).unwrap();
    let a = info.audio.unwrap();
    let mut input = ffmpeg::format::input(path).unwrap();
    let stream = input.stream(a.stream_index).unwrap();
    let mut dec = AudioDecoder::open(stream.parameters(), f64::from(stream.time_base()), out_rate).unwrap();
    let mut chunks = Vec::new();
    for (s, packet) in input.packets() {
        if s.index() != a.stream_index { continue; }
        dec.send(&packet).unwrap();
        while let Some(c) = dec.receive().unwrap() { chunks.push(c); }
    }
    chunks
}

#[test]
fn decodes_stereo_f32_at_requested_rate() {
    let Some(path) = common::sample("hevc8-1440p100-sync.mp4") else { return };
    let chunks = decode_all(&path, 48_000);
    let frames: usize = chunks.iter().map(|c| c.samples.len() / 2).sum();
    assert!((frames as f64 / 48_000.0 - 10.0).abs() < 0.1, "decoded {frames} frames");
    assert!(chunks.windows(2).all(|w| w[1].pts > w[0].pts));
}

#[test]
fn resamples_44100_to_device_rate() {
    let Some(path) = common::sample("h264-1080p30-44k.mp4") else { return };
    let chunks = decode_all(&path, 48_000);
    let frames: usize = chunks.iter().map(|c| c.samples.len() / 2).sum();
    // 10 s of audio must stay 10 s after resampling (no drift)
    assert!((frames as f64 / 48_000.0 - 10.0).abs() < 0.1, "decoded {frames} frames");
    // the beep at t = 1 s must still be at 1 s
    let all: Vec<f32> = chunks.iter().flat_map(|c| c.samples.iter().copied()).collect();
    let first_loud_after_half_second = all.chunks(2).enumerate()
        .skip(24_000)
        .find(|(_, s)| s[0].abs() > 0.1)
        .map(|(i, _)| i as f64 / 48_000.0)
        .unwrap();
    assert!((first_loud_after_half_second - 1.0).abs() < 0.01, "beep at {first_loud_after_half_second}");
}
```

Aggiungere a `lib.rs`: `pub mod audio;`.

- [ ] **Step 2: Eseguire i test e verificare che falliscano**

Run: `source scripts/env.sh && cargo test -p actionlay-media --test audio_decode`
Expected: FAIL in compilazione (`audio::AudioDecoder` non definito).

- [ ] **Step 3: Implementare `audio.rs`**

```rust
//! Audio decoding (resampled to stereo f32) and output through cpal.
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::{format::sample::Type as SampleType, format::Sample, frame, software::resampling, ChannelLayout};
use ringbuf::{traits::*, HeapCons, HeapProd, HeapRb};

use crate::{clock::audio_clock_time, MediaError};

pub struct AudioChunk {
    pub pts: f64,
    /// Interleaved stereo samples.
    pub samples: Vec<f32>,
}

pub struct AudioDecoder {
    decoder: ffmpeg::decoder::Audio,
    resampler: Option<resampling::Context>,
    time_base: f64,
    out_rate: u32,
}

impl AudioDecoder {
    pub fn open(params: ffmpeg::codec::Parameters, time_base: f64, out_rate: u32) -> Result<Self, MediaError> {
        let decoder = ffmpeg::codec::Context::from_parameters(params)?.decoder().audio()?;
        Ok(Self { decoder, resampler: None, time_base, out_rate })
    }

    pub fn send(&mut self, packet: &ffmpeg::Packet) -> Result<(), MediaError> {
        Ok(self.decoder.send_packet(packet)?)
    }

    pub fn flush(&mut self) {
        self.decoder.flush();
        self.resampler = None;
    }

    pub fn receive(&mut self) -> Result<Option<AudioChunk>, MediaError> {
        let mut decoded = frame::Audio::empty();
        match self.decoder.receive_frame(&mut decoded) {
            Ok(()) => {}
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::util::error::EAGAIN => return Ok(None),
            Err(ffmpeg::Error::Eof) => return Ok(None),
            Err(e) => return Err(e.into()),
        }
        let pts = decoded.timestamp().or(decoded.pts()).unwrap_or(0) as f64 * self.time_base;
        let resampler = match &mut self.resampler {
            Some(r) => r,
            slot => slot.insert(resampling::Context::get(
                decoded.format(),
                decoded.channel_layout(),
                decoded.rate(),
                Sample::F32(SampleType::Packed),
                ChannelLayout::STEREO,
                self.out_rate,
            )?),
        };
        let mut out = frame::Audio::empty();
        resampler.run(&decoded, &mut out)?;
        let n = out.samples() * 2;
        let bytes = &out.data(0)[..n * 4];
        let samples = bytes.chunks_exact(4).map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]])).collect();
        Ok(Some(AudioChunk { pts, samples }))
    }
}

struct Shared {
    frames_played: AtomicU64,
    latency_us: AtomicU64,
    muted: AtomicBool,
}

pub struct AudioOutput {
    _stream: cpal::Stream,
    producer: HeapProd<f32>,
    consumer_slot: Arc<Mutex<Option<HeapCons<f32>>>>,
    shared: Arc<Shared>,
    sample_rate: u32,
    base_pts: f64,
}

impl AudioOutput {
    pub const CAPACITY_SECONDS: f64 = 0.5;

    pub fn open() -> Result<Self, MediaError> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| MediaError::Audio("no output device".into()))?;
        let supported = device.default_output_config().map_err(|e| MediaError::Audio(e.to_string()))?;
        let sample_rate = supported.sample_rate();
        let mut config = supported.config();
        config.channels = 2;

        let (producer, consumer) = HeapRb::<f32>::new((sample_rate as f64 * 2.0 * Self::CAPACITY_SECONDS) as usize).split();
        let consumer_slot = Arc::new(Mutex::new(Some(consumer)));
        let shared = Arc::new(Shared { frames_played: AtomicU64::new(0), latency_us: AtomicU64::new(0), muted: AtomicBool::new(false) });

        let slot = consumer_slot.clone();
        let sh = shared.clone();
        let stream = device
            .build_output_stream::<f32, _, _>(
                config,
                move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                    let ts = info.timestamp();
                    if let Some(lat) = ts.playback.duration_since(&ts.callback) {
                        sh.latency_us.store(lat.as_micros() as u64, Ordering::Relaxed);
                    }
                    let mut guard = slot.lock().unwrap();
                    let got = guard.as_mut().map(|c| c.pop_slice(data)).unwrap_or(0);
                    data[got..].fill(0.0);
                    if sh.muted.load(Ordering::Relaxed) {
                        data.fill(0.0);
                    }
                    sh.frames_played.fetch_add((got / 2) as u64, Ordering::Relaxed);
                },
                |e| log::error!("audio stream error: {e}"),
                None,
            )
            .map_err(|e| MediaError::Audio(e.to_string()))?;
        stream.play().map_err(|e| MediaError::Audio(e.to_string()))?;
        Ok(Self { _stream: stream, producer, consumer_slot, shared, sample_rate, base_pts: 0.0 })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn push(&mut self, samples: &[f32]) -> usize {
        self.producer.push_slice(samples)
    }

    pub fn queued_frames(&self) -> usize {
        self.producer.occupied_len() / 2
    }

    /// Drops queued audio and restarts the clock at `base_pts` (used on seek).
    pub fn reset(&mut self, base_pts: f64) {
        if let Some(c) = self.consumer_slot.lock().unwrap().as_mut() {
            c.clear();
        }
        self.shared.frames_played.store(0, Ordering::Relaxed);
        self.base_pts = base_pts;
    }

    pub fn clock(&self) -> f64 {
        audio_clock_time(
            self.base_pts,
            self.shared.frames_played.load(Ordering::Relaxed),
            self.sample_rate,
            Duration::from_micros(self.shared.latency_us.load(Ordering::Relaxed)),
        )
    }

    pub fn set_muted(&self, muted: bool) {
        self.shared.muted.store(muted, Ordering::Relaxed);
    }
}
```
Nota: se il dispositivo non accetta 2 canali, `build_output_stream` fallisce: in quel caso `AudioOutput::open` restituisce errore e il player userà l'orologio di sistema (Task 7). Se il nome `ChannelLayout::STEREO` in `ffmpeg-next` 9.0 differisce, usare `ChannelLayout::default(2)`. Se `HeapCons::clear` non esiste, svuotare con `c.skip(c.occupied_len())`.

- [ ] **Step 4: Eseguire i test e verificare che passino**

Run: `source scripts/env.sh && cargo test -p actionlay-media --test audio_decode`
Expected: PASS (2 test).

- [ ] **Step 5: Commit**

```bash
git add crates/media
git commit -m "feat(media): audio decoding, resampling and cpal output with audio clock"
```

---

### Task 7: Player: thread, comandi, scelta del frame, seek

**Files:**
- Create: `crates/media/src/present.rs`, `crates/media/src/player.rs`, `crates/media/tests/player.rs`
- Modify: `crates/media/src/lib.rs`

**Interfaces:**
- Consumes: tutto quanto sopra.
- Produces:
  - `present::select_frame(queue: &mut VecDeque<Nv12Frame>, clock: f64) -> (Option<Nv12Frame>, usize)` — restituisce l'ultimo frame con `pts <= clock` (se c'è) e il numero di frame scartati perché superati.
  - `player::Player::open(path: &Path, options: PlayerOptions) -> Result<Player, MediaError>`
  - `player::PlayerOptions { prefer_hw: bool, audio: bool }` (`Default`: entrambi `true`)
  - `Player::{play(&mut self), pause(&mut self), toggle(&mut self), is_paused(&self) -> bool, set_speed(&mut self, speed: f64), speed(&self) -> f64, seek(&mut self, to: f64, precise: bool), step(&mut self, frames: i32), position(&self) -> f64, info(&self) -> &MediaInfo, poll_frame(&mut self) -> Option<Nv12Frame>, stats(&self) -> PlayerStats, at_end(&self) -> bool}`
  - `player::PlayerStats { backend: &'static str, dropped: u64, presented: u64, audio_active: bool, av_offset: f64 }`

- [ ] **Step 1: Scrivere i test che falliscono**

In fondo a `crates/media/src/present.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    fn f(pts: f64) -> Nv12Frame { Nv12Frame { width: 2, height: 2, y: vec![0; 4], uv: vec![0; 2], pts } }

    #[test]
    fn nothing_due_yet() {
        let mut q: VecDeque<_> = [f(1.0), f(1.01)].into();
        let (frame, dropped) = select_frame(&mut q, 0.5);
        assert!(frame.is_none());
        assert_eq!((dropped, q.len()), (0, 2));
    }

    #[test]
    fn picks_latest_due_and_drops_older() {
        let mut q: VecDeque<_> = [f(0.00), f(0.01), f(0.02), f(0.03)].into();
        let (frame, dropped) = select_frame(&mut q, 0.025);
        assert_eq!(frame.unwrap().pts, 0.02);
        assert_eq!(dropped, 2);
        assert_eq!(q.len(), 1);
    }
}
```

`crates/media/tests/player.rs`:
```rust
mod common;
use std::time::{Duration, Instant};
use actionlay_media::player::{Player, PlayerOptions};

fn wait_for_frame(p: &mut Player, timeout: Duration) -> Option<f64> {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if let Some(f) = p.poll_frame() { return Some(f.pts); }
        std::thread::sleep(Duration::from_millis(2));
    }
    None
}

fn no_audio() -> PlayerOptions { PlayerOptions { prefer_hw: true, audio: false } }

#[test]
fn shows_first_frame_while_paused() {
    let Some(path) = common::sample("hevc8-1440p100-sync.mp4") else { return };
    let mut p = Player::open(&path, no_audio()).unwrap();
    assert!(p.is_paused());
    let pts = wait_for_frame(&mut p, Duration::from_secs(5)).expect("no first frame");
    assert!(pts < 0.02);
}

#[test]
fn plays_file_without_audio() {
    let Some(path) = common::sample("hevc8-1080p30-noaudio.mp4") else { return };
    let mut p = Player::open(&path, PlayerOptions::default()).unwrap();
    assert!(!p.stats().audio_active);
    p.play();
    std::thread::sleep(Duration::from_millis(600));
    let mut last = 0.0;
    for _ in 0..50 { if let Some(f) = p.poll_frame() { last = f.pts; } std::thread::sleep(Duration::from_millis(5)); }
    assert!(last > 0.4, "playback did not advance: {last}");
}

#[test]
fn seek_clamps_and_discards_stale_frames() {
    let Some(path) = common::sample("hevc8-1440p100-sync.mp4") else { return };
    let mut p = Player::open(&path, no_audio()).unwrap();
    wait_for_frame(&mut p, Duration::from_secs(5));
    // rapid seeks: only the last one must win
    p.seek(2.0, false);
    p.seek(7.5, true);
    let pts = wait_for_frame(&mut p, Duration::from_secs(5)).unwrap();
    assert!((pts - 7.5).abs() < 0.011, "precise seek landed at {pts}");
    p.seek(1_000.0, true);
    let pts = wait_for_frame(&mut p, Duration::from_secs(5)).unwrap();
    assert!(pts <= p.info().duration && pts > p.info().duration - 0.1, "clamped seek at {pts}");
    p.seek(-5.0, true);
    let pts = wait_for_frame(&mut p, Duration::from_secs(5)).unwrap();
    assert!(pts < 0.02);
}

#[test]
fn frame_step_moves_one_frame() {
    let Some(path) = common::sample("hevc8-1440p100-sync.mp4") else { return };
    let mut p = Player::open(&path, no_audio()).unwrap();
    p.seek(3.0, true);
    let a = wait_for_frame(&mut p, Duration::from_secs(5)).unwrap();
    p.step(1);
    let b = wait_for_frame(&mut p, Duration::from_secs(5)).unwrap();
    p.step(-1);
    let c = wait_for_frame(&mut p, Duration::from_secs(5)).unwrap();
    assert!((b - a - 0.01).abs() < 1e-3, "{a} -> {b}");
    assert!((c - a).abs() < 1e-3, "{a} -> {c}");
}
```

Aggiungere a `lib.rs`: `pub mod present; pub mod player;`.

- [ ] **Step 2: Eseguire i test e verificare che falliscano**

Run: `source scripts/env.sh && cargo test -p actionlay-media present player`
Expected: FAIL in compilazione.

- [ ] **Step 3: Implementare `present.rs`**

Inizio del file:
```rust
//! Choosing which decoded frame to show at a given clock time.
use std::collections::VecDeque;

use crate::frame::Nv12Frame;

/// Returns the most recent frame due at `clock` (if any) and how many older
/// due frames were skipped. Frames in the future stay queued.
pub fn select_frame(queue: &mut VecDeque<Nv12Frame>, clock: f64) -> (Option<Nv12Frame>, usize) {
    let mut chosen = None;
    let mut dropped = 0;
    while queue.front().is_some_and(|f| f.pts <= clock) {
        if chosen.replace(queue.pop_front().unwrap()).is_some() {
            dropped += 1;
        }
    }
    (chosen, dropped)
}
```

- [ ] **Step 4: Implementare `player.rs`**

Progetto dei thread:
- **thread di decodifica** (uno solo, possiede `Input`, `VideoDecoder`, `AudioDecoder`): legge pacchetti, decodifica, invia i frame video su un canale `crossbeam_channel::bounded(8)` e i campioni audio a `AudioOutput` (tramite `Arc<Mutex<AudioOutput>>`). Quando il buffer audio è pieno o il canale video è pieno, attende 2 ms e riprova (il pacchetto in mano non va perso). Riceve comandi su un canale `unbounded`: `Seek { to, precise, generation }`, `Quit`.
- **generazione**: ogni seek incrementa un `Arc<AtomicU64>`; ogni frame porta la generazione con cui è stato decodificato; `poll_frame` scarta i frame di generazioni vecchie (Review Focus 3).
- **seek preciso**: `input.seek(ts, ..ts)` al keyframe precedente, `flush()` dei decoder, poi si scartano i frame con `pts < target - half_frame` e quel primo frame utile diventa la nuova posizione.
- **orologio**: se l'audio è attivo e la velocità è 1x, `AudioOutput::clock()`; altrimenti `SystemClock`. In pausa l'audio è silenziato e il buffer non viene consumato perché il thread smette di spingere campioni solo dopo averlo riempito; alla ripresa `AudioOutput::reset(position)` riallinea.

```rust
//! Playback engine: one decode thread feeding a small frame queue and the audio output.
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, unbounded, Receiver, Sender, TryRecvError};
use ffmpeg_next as ffmpeg;

use crate::{
    audio::{AudioDecoder, AudioOutput},
    clock::SystemClock,
    frame::Nv12Frame,
    present::select_frame,
    probe::{probe, MediaInfo},
    video::VideoDecoder,
    MediaError,
};

#[derive(Debug, Clone, Copy)]
pub struct PlayerOptions {
    pub prefer_hw: bool,
    pub audio: bool,
}

impl Default for PlayerOptions {
    fn default() -> Self {
        Self { prefer_hw: true, audio: true }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PlayerStats {
    pub backend: &'static str,
    pub dropped: u64,
    pub presented: u64,
    pub audio_active: bool,
    pub av_offset: f64,
}

enum Command {
    Seek { to: f64, precise: bool, generation: u64 },
    Quit,
}

struct Tagged {
    generation: u64,
    frame: Nv12Frame,
}

pub struct Player {
    info: MediaInfo,
    commands: Sender<Command>,
    frames: Receiver<Tagged>,
    queue: VecDeque<Nv12Frame>,
    generation: Arc<AtomicU64>,
    backend: Arc<Mutex<&'static str>>,
    eof: Arc<AtomicBool>,
    audio: Option<Arc<Mutex<AudioOutput>>>,
    clock: SystemClock,
    position: f64,
    awaiting_seek_frame: bool,
    dropped: u64,
    presented: u64,
    last_frame_pts: f64,
    thread: Option<JoinHandle<()>>,
}

impl Player {
    pub fn open(path: &Path, options: PlayerOptions) -> Result<Self, MediaError> {
        let info = probe(path)?;
        let audio = if options.audio && info.audio.is_some() {
            match AudioOutput::open() {
                Ok(out) => Some(Arc::new(Mutex::new(out))),
                Err(e) => {
                    log::warn!("audio disabled: {e}");
                    None
                }
            }
        } else {
            None
        };
        if let Some(a) = &audio {
            a.lock().unwrap().set_muted(true); // starts paused
        }

        let (cmd_tx, cmd_rx) = unbounded();
        let (frame_tx, frame_rx) = bounded(8);
        let generation = Arc::new(AtomicU64::new(0));
        let backend = Arc::new(Mutex::new("unknown"));
        let eof = Arc::new(AtomicBool::new(false));

        let worker = Worker {
            path: path.to_path_buf(),
            info: info.clone(),
            prefer_hw: options.prefer_hw,
            audio: audio.clone(),
            commands: cmd_rx,
            frames: frame_tx,
            generation: generation.clone(),
            backend: backend.clone(),
            eof: eof.clone(),
        };
        let thread = std::thread::Builder::new()
            .name("actionlay-decode".into())
            .spawn(move || {
                if let Err(e) = worker.run() {
                    log::error!("decode thread stopped: {e}");
                }
            })
            .expect("spawn decode thread");

        Ok(Self {
            info,
            commands: cmd_tx,
            frames: frame_rx,
            queue: VecDeque::new(),
            generation,
            backend,
            eof,
            audio,
            clock: SystemClock::new(0.0, Instant::now()),
            position: 0.0,
            awaiting_seek_frame: true,
            dropped: 0,
            presented: 0,
            last_frame_pts: 0.0,
            thread: Some(thread),
        })
    }

    pub fn info(&self) -> &MediaInfo {
        &self.info
    }

    pub fn is_paused(&self) -> bool {
        self.clock.is_paused()
    }

    pub fn play(&mut self) {
        let now = Instant::now();
        if self.at_end() {
            self.seek(0.0, true);
        }
        self.clock.seek(self.position, now);
        self.clock.set_paused(false, now);
        self.sync_audio();
    }

    pub fn pause(&mut self) {
        self.position = self.current_time();
        self.clock.set_paused(true, Instant::now());
        self.sync_audio();
    }

    pub fn toggle(&mut self) {
        if self.is_paused() { self.play() } else { self.pause() }
    }

    pub fn speed(&self) -> f64 {
        self.clock.speed()
    }

    pub fn set_speed(&mut self, speed: f64) {
        self.position = self.current_time();
        let now = Instant::now();
        self.clock.seek(self.position, now);
        self.clock.set_speed(speed, now);
        self.sync_audio();
    }

    pub fn seek(&mut self, to: f64, precise: bool) {
        let frame = self.frame_duration();
        let to = to.clamp(0.0, (self.info.duration - frame).max(0.0));
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.queue.clear();
        self.eof.store(false, Ordering::SeqCst);
        self.position = to;
        self.clock.seek(to, Instant::now());
        self.awaiting_seek_frame = true;
        let _ = self.commands.send(Command::Seek { to, precise, generation });
        if let Some(a) = &self.audio {
            a.lock().unwrap().reset(to);
        }
    }

    pub fn step(&mut self, frames: i32) {
        self.pause();
        let target = self.last_frame_pts + f64::from(frames) * self.frame_duration();
        self.seek(target, true);
    }

    pub fn position(&self) -> f64 {
        self.position
    }

    pub fn at_end(&self) -> bool {
        self.eof.load(Ordering::SeqCst) && self.queue.is_empty() && self.frames.is_empty()
    }

    pub fn stats(&self) -> PlayerStats {
        PlayerStats {
            backend: *self.backend.lock().unwrap(),
            dropped: self.dropped,
            presented: self.presented,
            audio_active: self.audio.is_some(),
            av_offset: self.audio_driven().then(|| self.last_frame_pts - self.current_time()).unwrap_or(0.0),
        }
    }

    /// Call once per UI frame: returns the frame to show now, if it changed.
    pub fn poll_frame(&mut self) -> Option<Nv12Frame> {
        let current = self.generation.load(Ordering::SeqCst);
        loop {
            match self.frames.try_recv() {
                Ok(t) if t.generation == current => self.queue.push_back(t.frame),
                Ok(_) => {} // stale frame from before a seek
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        if self.awaiting_seek_frame {
            let f = self.queue.pop_front()?;
            self.awaiting_seek_frame = false;
            self.position = f.pts;
            self.clock.seek(f.pts, Instant::now());
            if let Some(a) = &self.audio {
                a.lock().unwrap().reset(f.pts);
            }
            return Some(self.present(f));
        }
        if self.is_paused() {
            return None;
        }
        let now = self.current_time();
        let (frame, dropped) = select_frame(&mut self.queue, now);
        self.dropped += dropped as u64;
        self.position = now.min(self.info.duration);
        if self.at_end() {
            self.pause();
        }
        frame.map(|f| self.present(f))
    }

    fn present(&mut self, f: Nv12Frame) -> Nv12Frame {
        self.presented += 1;
        self.last_frame_pts = f.pts;
        f
    }

    fn frame_duration(&self) -> f64 {
        if self.info.video.fps > 0.0 { 1.0 / self.info.video.fps } else { 1.0 / 30.0 }
    }

    fn audio_driven(&self) -> bool {
        self.audio.is_some() && !self.is_paused() && (self.speed() - 1.0).abs() < f64::EPSILON
    }

    fn current_time(&self) -> f64 {
        if self.audio_driven() {
            self.audio.as_ref().unwrap().lock().unwrap().clock()
        } else {
            self.clock.time(Instant::now())
        }
    }

    fn sync_audio(&mut self) {
        let driven = self.audio_driven();
        if let Some(a) = &self.audio {
            let mut a = a.lock().unwrap();
            a.set_muted(!driven);
            if driven {
                a.reset(self.position);
            }
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Quit);
        self.queue.clear();
        while self.frames.try_recv().is_ok() {}
        if let Some(t) = self.thread.take() {
            // unblock a worker waiting on a full channel
            drop(std::mem::replace(&mut self.frames, bounded(0).1));
            let _ = t.join();
        }
    }
}

struct Worker {
    path: PathBuf,
    info: MediaInfo,
    prefer_hw: bool,
    audio: Option<Arc<Mutex<AudioOutput>>>,
    commands: Receiver<Command>,
    frames: Sender<Tagged>,
    generation: Arc<AtomicU64>,
    backend: Arc<Mutex<&'static str>>,
    eof: Arc<AtomicBool>,
}

enum Flow {
    Continue,
    Restart,
    Quit,
}

impl Worker {
    fn run(self) -> Result<(), MediaError> {
        let mut input = ffmpeg::format::input(&self.path)?;
        let vindex = self.info.video.stream_index;
        let vparams = input.stream(vindex).ok_or(MediaError::NoVideoStream)?.parameters();
        let mut video = VideoDecoder::open(vparams, self.info.video.time_base, self.prefer_hw)?;

        let mut audio_dec = match (&self.audio, &self.info.audio) {
            (Some(out), Some(a)) => {
                let s = input.stream(a.stream_index).ok_or(MediaError::Audio("stream vanished".into()))?;
                let rate = out.lock().unwrap().sample_rate();
                Some((a.stream_index, AudioDecoder::open(s.parameters(), f64::from(s.time_base()), rate)?))
            }
            _ => None,
        };

        let mut generation = self.generation.load(Ordering::SeqCst);
        let mut skip_before: Option<f64> = None;
        let half_frame = 0.5 / self.info.video.fps.max(1.0);
        // A seek received while idling at end of file, handled on the next turn.
        let mut pending: Option<(f64, bool, u64)> = None;

        'outer: loop {
            // Pending commands: only the latest seek matters.
            let mut seek = pending.take();
            loop {
                match self.commands.try_recv() {
                    Ok(Command::Quit) | Err(TryRecvError::Disconnected) => break 'outer,
                    Ok(Command::Seek { to, precise, generation: g }) => seek = Some((to, precise, g)),
                    Err(TryRecvError::Empty) => break,
                }
            }
            if let Some((to, precise, g)) = seek {
                generation = g;
                let ts = (to * f64::from(ffmpeg::ffi::AV_TIME_BASE)) as i64;
                input.seek(ts, ..ts)?;
                video.flush();
                if let Some((_, a)) = &mut audio_dec {
                    a.flush();
                }
                skip_before = Some(if precise { to - half_frame } else { f64::NEG_INFINITY });
                self.eof.store(false, Ordering::SeqCst);
            }

            let Some((stream, packet)) = input.packets().next() else {
                video.send_eof()?;
                while let Some(f) = video.receive()? {
                    if let Flow::Quit = self.emit(f, generation, &mut skip_before) { break 'outer; }
                }
                self.eof.store(true, Ordering::SeqCst);
                // idle until a seek or quit arrives
                match self.commands.recv() {
                    Ok(Command::Seek { to, precise, generation: g }) => {
                        pending = Some((to, precise, g));
                        continue 'outer;
                    }
                    _ => break 'outer,
                }
            };

            if stream.index() == vindex {
                video.send(&packet)?;
                while let Some(f) = video.receive()? {
                    *self.backend.lock().unwrap() = video.active_backend();
                    match self.emit(f, generation, &mut skip_before) {
                        Flow::Continue => {}
                        Flow::Restart => continue 'outer,
                        Flow::Quit => break 'outer,
                    }
                }
            } else if let Some((aindex, a)) = &mut audio_dec {
                if stream.index() == *aindex && skip_before.is_none() {
                    a.send(&packet)?;
                    while let Some(chunk) = a.receive()? {
                        let mut offset = 0;
                        while offset < chunk.samples.len() {
                            let pushed = self.audio.as_ref().unwrap().lock().unwrap().push(&chunk.samples[offset..]);
                            offset += pushed;
                            if pushed == 0 {
                                if !self.commands.is_empty() { continue 'outer; }
                                std::thread::sleep(Duration::from_millis(2));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn emit(&self, frame: Nv12Frame, generation: u64, skip_before: &mut Option<f64>) -> Flow {
        if let Some(limit) = *skip_before {
            if frame.pts < limit {
                return Flow::Continue;
            }
            *skip_before = None;
        }
        let mut tagged = Tagged { generation, frame };
        loop {
            match self.frames.send_timeout(tagged, Duration::from_millis(5)) {
                Ok(()) => return Flow::Continue,
                Err(crossbeam_channel::SendTimeoutError::Timeout(t)) => {
                    if self.generation.load(Ordering::SeqCst) != generation {
                        return Flow::Restart; // a seek is pending, stop filling the queue
                    }
                    tagged = t;
                }
                Err(crossbeam_channel::SendTimeoutError::Disconnected(_)) => return Flow::Quit,
            }
        }
    }
}
```

- [ ] **Step 5: Eseguire i test e verificare che passino**

Run: `source scripts/env.sh && cargo test -p actionlay-media present player -- --test-threads=1`
Expected: PASS (6 test). `--test-threads=1` evita che più player aprano il dispositivo audio insieme.

- [ ] **Step 6: Commit**

```bash
git add crates/media
git commit -m "feat(media): playback engine with frame selection, seek and frame step"
```

---

### Task 8: App: finestra e video su GPU (YUV→RGB)

**Files:**
- Create: `crates/app/Cargo.toml`, `crates/app/src/main.rs`, `crates/app/src/video_view.rs`, `crates/app/src/yuv.wgsl`
- Modify: `Cargo.toml` (rimettere `"crates/app"` in `members`)

**Interfaces:**
- Consumes: `actionlay_media::{player::{Player, PlayerOptions}, frame::Nv12Frame, color::{yuv_to_rgb, ColorInfo}}`.
- Produces:
  - `video_view::VideoView::new(render_state: &egui_wgpu::RenderState) -> VideoView`
  - `VideoView::upload(&mut self, frame: &Nv12Frame, color: ColorInfo)` — memorizza il frame da caricare al prossimo paint
  - `VideoView::show(&mut self, ui: &mut egui::Ui, rect: egui::Rect)` — disegna il video adattato a `rect` mantenendo le proporzioni
  - `video_view::fit_rect(available: egui::Rect, video_w: u32, video_h: u32) -> egui::Rect` (pura)
  - eseguibile `actionlay` (`cargo run -p actionlay-app -- <video>`), apertura anche con trascinamento del file nella finestra.

- [ ] **Step 1: Scrivere il test che fallisce**

In fondo a `crates/app/src/video_view.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, Rect};

    #[test]
    fn fit_rect_letterboxes_and_pillarboxes() {
        let wide = Rect::from_min_max(pos2(0.0, 0.0), pos2(1600.0, 900.0));
        let r = fit_rect(wide, 1920, 1440); // 4:3 inside 16:9 -> pillarbox
        assert_eq!((r.width(), r.height()), (1200.0, 900.0));
        assert_eq!(r.center(), wide.center());
        let tall = Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 900.0));
        let r = fit_rect(tall, 1920, 1080); // 16:9 inside a tall area -> letterbox
        assert_eq!((r.width(), r.height()), (800.0, 450.0));
    }
}
```

`crates/app/Cargo.toml`:
```toml
[package]
name = "actionlay-app"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true

[[bin]]
name = "actionlay"
path = "src/main.rs"

[dependencies]
actionlay-media = { path = "../media" }
eframe = { version = "0.36", default-features = false, features = ["default_fonts", "wgpu", "wayland", "x11", "accesskit"] }
egui-wgpu = "0.36"
bytemuck = { version = "1", features = ["derive"] }
log.workspace = true
env_logger = "0.11"
```

- [ ] **Step 2: Eseguire il test e verificare che fallisca**

Run: `source scripts/env.sh && cargo test -p actionlay-app`
Expected: FAIL in compilazione (`fit_rect` non definito).

- [ ] **Step 3: Shader**

`crates/app/src/yuv.wgsl`:
```wgsl
// Full-screen quad sampling NV12 (Y + interleaved UV) and converting to RGB.
struct Params {
    // rows of the 3x4 YUV->RGB matrix
    r: vec4<f32>,
    g: vec4<f32>,
    b: vec4<f32>,
};

@group(0) @binding(0) var y_tex: texture_2d<f32>;
@group(0) @binding(1) var uv_tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var<uniform> params: Params;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    // two triangles covering the viewport set by egui to the callback rect
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    let c = corners[i];
    var out: VsOut;
    out.pos = vec4(c.x * 2.0 - 1.0, 1.0 - c.y * 2.0, 0.0, 1.0);
    out.uv = c;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let y = textureSample(y_tex, samp, in.uv).r;
    let uv = textureSample(uv_tex, samp, in.uv).rg;
    let yuv = vec4(y, uv.x, uv.y, 1.0);
    let rgb = clamp(vec3(dot(params.r, yuv), dot(params.g, yuv), dot(params.b, yuv)), vec3(0.0), vec3(1.0));
    return vec4(rgb, 1.0);
}
```
Nota: il formato di destinazione di egui può essere sRGB o lineare (`render_state.target_format`). Se è un formato `*Srgb`, i valori calcolati sono già gamma-encoded e verrebbero ricodificati: in quel caso convertire in lineare nello shader (`pow(rgb, vec3(2.2))`) — da verificare visivamente al gate confrontando con `ffplay` (Task 10).

- [ ] **Step 4: `video_view.rs`**

```rust
//! GPU presentation of NV12 frames inside an egui layout.
use std::sync::{Arc, Mutex};

use actionlay_media::{color::{yuv_to_rgb, ColorInfo}, frame::Nv12Frame};
use eframe::egui;
use egui_wgpu::wgpu;

pub fn fit_rect(available: egui::Rect, video_w: u32, video_h: u32) -> egui::Rect {
    let aspect = video_w as f32 / video_h as f32;
    let mut size = available.size();
    if size.x / size.y > aspect {
        size.x = size.y * aspect;
    } else {
        size.y = size.x / aspect;
    }
    egui::Rect::from_center_size(available.center(), size)
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    rows: [[f32; 4]; 3],
}

struct Textures {
    width: u32,
    height: u32,
    y: wgpu::Texture,
    uv: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

struct Resources {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    params: wgpu::Buffer,
    textures: Option<Textures>,
}

type Pending = Arc<Mutex<Option<(Nv12Frame, ColorInfo)>>>;

pub struct VideoView {
    pending: Pending,
    size: Option<(u32, u32)>,
}

impl VideoView {
    pub fn new(rs: &egui_wgpu::RenderState) -> Self {
        let device = &rs.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("yuv"),
            source: wgpu::ShaderSource::Wgsl(include_str!("yuv.wgsl").into()),
        });
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yuv"),
            entries: &[
                tex_entry(0),
                tex_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yuv"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yuv"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(rs.target_format.into())],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("yuv"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yuv-params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        rs.renderer.write().callback_resources.insert(Resources { pipeline, layout, sampler, params, textures: None });
        Self { pending: Arc::new(Mutex::new(None)), size: None }
    }

    pub fn upload(&mut self, frame: &Nv12Frame, color: ColorInfo) {
        self.size = Some((frame.width, frame.height));
        *self.pending.lock().unwrap() = Some((frame.clone(), color));
    }

    pub fn show(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        ui.painter().rect_filled(rect, 0.0, egui::Color32::BLACK);
        let Some((w, h)) = self.size else { return };
        let target = fit_rect(rect, w, h);
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(target, Paint { pending: self.pending.clone() }));
    }
}

struct Paint {
    pending: Pending,
}

impl egui_wgpu::CallbackTrait for Paint {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some((frame, color)) = self.pending.lock().unwrap().take() else { return Vec::new() };
        let res: &mut Resources = resources.get_mut().unwrap();
        let needs_new = res.textures.as_ref().is_none_or(|t| t.width != frame.width || t.height != frame.height);
        if needs_new {
            res.textures = Some(create_textures(device, res, frame.width, frame.height));
        }
        let t = res.textures.as_ref().unwrap();
        let (cw, ch) = (frame.width.div_ceil(2), frame.height.div_ceil(2));
        write_plane(queue, &t.y, &frame.y, frame.width, frame.height, 1);
        write_plane(queue, &t.uv, &frame.uv, cw, ch, 2);
        queue.write_buffer(&res.params, 0, bytemuck::bytes_of(&Params { rows: yuv_to_rgb(color) }));
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let res: &Resources = resources.get().unwrap();
        if let Some(t) = &res.textures {
            pass.set_pipeline(&res.pipeline);
            pass.set_bind_group(0, &t.bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
    }
}

fn create_textures(device: &wgpu::Device, res: &Resources, width: u32, height: u32) -> Textures {
    let make = |label, w, h, format| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    };
    let y = make("y", width, height, wgpu::TextureFormat::R8Unorm);
    let uv = make("uv", width.div_ceil(2), height.div_ceil(2), wgpu::TextureFormat::Rg8Unorm);
    let yv = y.create_view(&Default::default());
    let uvv = uv.create_view(&Default::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("yuv"),
        layout: &res.layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&yv) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&uvv) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&res.sampler) },
            wgpu::BindGroupEntry { binding: 3, resource: res.params.as_entire_binding() },
        ],
    });
    Textures { width, height, y, uv, bind_group }
}

fn write_plane(queue: &wgpu::Queue, tex: &wgpu::Texture, data: &[u8], w: u32, h: u32, bytes_per_px: u32) {
    queue.write_texture(
        tex.as_image_copy(),
        data,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * bytes_per_px), rows_per_image: Some(h) },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
}
```
Se un nome di `wgpu` 30 differisce (es. `TexelCopyBufferLayout`, `as_image_copy`), il compilatore lo indica: controllare con `cargo doc -p wgpu --open` e adattare senza cambiare la logica.

- [ ] **Step 5: `main.rs`**

```rust
//! ActionLay M0 prototype: plays a video with hardware decoding and synced audio.
mod transport;
mod video_view;

use std::path::PathBuf;

use actionlay_media::player::{Player, PlayerOptions};
use eframe::egui;
use video_view::VideoView;

struct App {
    player: Option<Player>,
    view: VideoView,
    error: Option<String>,
    scrub: transport::ScrubState,
}

impl App {
    fn open(&mut self, path: PathBuf) {
        match Player::open(&path, PlayerOptions::default()) {
            Ok(p) => {
                self.player = Some(p);
                self.error = None;
            }
            Err(e) => self.error = Some(format!("{}: {e}", path.display())),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let dropped = ui.ctx().input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone()));
        if let Some(path) = dropped {
            self.open(path);
        }

        if let Some(p) = &mut self.player {
            if let Some(frame) = p.poll_frame() {
                let color = p.info().video.color;
                self.view.upload(&frame, color);
            }
        }

        egui::TopBottomPanel::bottom("transport").show_inside(ui, |ui| {
            if let Some(p) = &mut self.player {
                transport::show(ui, p, &mut self.scrub);
            } else {
                ui.label(self.error.as_deref().unwrap_or("Drop a video here or pass it on the command line."));
            }
        });
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let rect = ui.available_rect_before_wrap();
            self.view.show(ui, rect);
        });

        // keep repainting while playing; when paused, egui repaints on input
        if self.player.as_ref().is_some_and(|p| !p.is_paused()) {
            ui.ctx().request_repaint();
        }
    }
}

fn main() -> eframe::Result {
    env_logger::init();
    let path = std::env::args().nth(1).map(PathBuf::from);
    eframe::run_native(
        "ActionLay",
        eframe::NativeOptions { vsync: true, ..Default::default() },
        Box::new(move |cc| {
            let rs = cc.wgpu_render_state.as_ref().expect("wgpu renderer required");
            let mut app = App { player: None, view: VideoView::new(rs), error: None, scrub: Default::default() };
            if let Some(path) = path {
                app.open(path);
            }
            Ok(Box::new(app))
        }),
    )
}
```
Se `TopBottomPanel::show_inside`/`CentralPanel::show_inside` non esistono in egui 0.36, usare le varianti indicate dal compilatore (l'esempio ufficiale 0.36 usa `CentralPanel::default().show(ui, …)`).

Creare un `crates/app/src/transport.rs` provvisorio con:
```rust
use actionlay_media::player::Player;
use eframe::egui;

#[derive(Default)]
pub struct ScrubState;

pub fn show(ui: &mut egui::Ui, player: &mut Player, _scrub: &mut ScrubState) {
    if ui.button(if player.is_paused() { "Play" } else { "Pause" }).clicked() {
        player.toggle();
    }
}
```
(sostituito nel Task 9).

- [ ] **Step 6: Eseguire test e prova manuale**

Run: `source scripts/env.sh && cargo test -p actionlay-app`
Expected: PASS (1 test).
Run: `cargo run --release -p actionlay-app -- samples/synthetic/hevc8-1440p100-sync.mp4`
Expected: si apre una finestra con il primo frame del pattern di test, proporzionato 4:3 con bande nere ai lati; "Play" avvia la riproduzione con il beep ogni secondo.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/app
git commit -m "feat(app): eframe window with NV12 video rendered by a wgpu shader"
```

---

### Task 9: Controlli di trasporto e statistiche

**Files:**
- Modify: `crates/app/src/transport.rs`, `crates/app/src/main.rs` (scorciatoie da tastiera)

**Interfaces:**
- Consumes: `Player::{toggle, seek, step, set_speed, speed, position, info, stats, is_paused}`.
- Produces: `transport::ScrubState { dragging: bool, was_playing: bool }`, `transport::show(ui, player, scrub)`, `transport::format_time(seconds: f64) -> String`, `transport::handle_keys(ctx: &egui::Context, player: &mut Player)`.

- [ ] **Step 1: Scrivere il test che fallisce**

In fondo a `crates/app/src/transport.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::format_time;

    #[test]
    fn formats_minutes_seconds_and_hundredths() {
        assert_eq!(format_time(0.0), "0:00.00");
        assert_eq!(format_time(198.677), "3:18.67");
        assert_eq!(format_time(3725.5), "62:05.50");
    }
}
```

- [ ] **Step 2: Eseguire il test e verificare che fallisca**

Run: `source scripts/env.sh && cargo test -p actionlay-app transport`
Expected: FAIL (`format_time` non definito).

- [ ] **Step 3: Implementazione**

`crates/app/src/transport.rs`:
```rust
//! Transport bar: play/pause, scrubbing, frame step, speed, playback stats.
use actionlay_media::player::Player;
use eframe::egui;

#[derive(Default)]
pub struct ScrubState {
    pub dragging: bool,
    pub was_playing: bool,
}

pub fn format_time(seconds: f64) -> String {
    let hundredths = (seconds * 100.0).floor() as u64;
    format!("{}:{:02}.{:02}", hundredths / 6000, (hundredths / 100) % 60, hundredths % 100)
}

const SPEEDS: [f64; 6] = [0.25, 0.5, 1.0, 1.5, 2.0, 4.0];

pub fn show(ui: &mut egui::Ui, player: &mut Player, scrub: &mut ScrubState) {
    let duration = player.info().duration;
    ui.horizontal(|ui| {
        if ui.button(if player.is_paused() { "▶" } else { "⏸" }).clicked() {
            player.toggle();
        }
        if ui.button("⏮ frame").clicked() {
            player.step(-1);
        }
        if ui.button("frame ⏭").clicked() {
            player.step(1);
        }

        let mut pos = player.position();
        ui.label(format_time(pos));
        ui.spacing_mut().slider_width = (ui.available_width() - 260.0).max(100.0);
        let response = ui.add(egui::Slider::new(&mut pos, 0.0..=duration).show_value(false));
        if response.drag_started() {
            scrub.dragging = true;
            scrub.was_playing = !player.is_paused();
            player.pause();
        }
        if response.changed() {
            // keyframe seek while dragging keeps scrubbing responsive
            player.seek(pos, !scrub.dragging);
        }
        if response.drag_stopped() {
            scrub.dragging = false;
            player.seek(pos, true);
            if scrub.was_playing {
                player.play();
            }
        }
        ui.label(format_time(duration));

        let mut speed = player.speed();
        egui::ComboBox::from_id_salt("speed")
            .selected_text(format!("{speed}x"))
            .show_ui(ui, |ui| {
                for s in SPEEDS {
                    ui.selectable_value(&mut speed, s, format!("{s}x"));
                }
            });
        if speed != player.speed() {
            player.set_speed(speed);
        }
    });

    let st = player.stats();
    let v = &player.info().video;
    ui.small(format!(
        "{}x{} {} @ {:.2} fps · decoder: {} · presented {} · dropped {} · audio: {} · A/V {:+.0} ms",
        v.width, v.height, v.codec, v.fps, st.backend, st.presented, st.dropped,
        if st.audio_active { "on" } else { "off" }, st.av_offset * 1000.0
    ));
}

pub fn handle_keys(ctx: &egui::Context, player: &mut Player) {
    ctx.input(|i| {
        if i.key_pressed(egui::Key::Space) {
            player.toggle();
        }
        if i.key_pressed(egui::Key::ArrowRight) {
            player.step(1);
        }
        if i.key_pressed(egui::Key::ArrowLeft) {
            player.step(-1);
        }
    });
}
```

In `main.rs`, dentro `if let Some(p) = &mut self.player { … }` prima di `poll_frame`, aggiungere `transport::handle_keys(ui.ctx(), p);`.

- [ ] **Step 4: Eseguire test e prova manuale**

Run: `source scripts/env.sh && cargo test -p actionlay-app`
Expected: PASS (2 test).
Run: `cargo run --release -p actionlay-app -- samples/GX013370.MP4`
Expected: riproduzione fluida; barra di stato con `decoder: videotoolbox`; trascinando la barra l'immagine segue; al rilascio si ferma sul frame esatto; frecce = frame ±1; velocità 2x silenzia l'audio e scorre il doppio più veloce.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "feat(app): transport controls, keyboard shortcuts and playback stats"
```

---

### Task 10: Gate M0 su macOS e Windows

**Files:**
- Create: `docs/m0-report.md`

**Interfaces:**
- Consumes: l'app completa (Task 8–9), `decode-bench` (Task 5), i campioni sintetici e `samples/GX013370.MP4` (solo in locale).
- Produces: `docs/m0-report.md` con l'esito e la decisione **"proseguire con lo stack attuale"** oppure **"ripiego su libmpv per la riproduzione"**.

- [ ] **Step 1: Misure su macOS (host locale)**

Run:
```bash
source scripts/env.sh
cargo build --release -p actionlay-app -p actionlay-media
for f in samples/GX013370.MP4 samples/synthetic/hevc10-2160p60-sync.mp4 samples/synthetic/hevc8-1440p100-sync.mp4; do
  ./target/release/decode-bench "$f"; ./target/release/decode-bench "$f" --sw
done
otool -L target/release/actionlay | grep -i -E 'libav|libsw' || echo STATIC-OK
ls -lh target/release/actionlay
```
Expected: HW ≥ fps sorgente su tutti i file; `STATIC-OK`; dimensione del binario annotata.

- [ ] **Step 2: Verifiche visive e A/V su macOS**

Con `./target/release/actionlay samples/synthetic/hevc8-1440p100-sync.mp4`:
1. In riproduzione, il lampo bianco e il beep di ogni secondo devono coincidere a orecchio/occhio; la statistica `A/V` deve restare entro ±40 ms per tutti i 10 s.
2. `dropped` deve crescere al massimo di circa 40 frame al secondo su un monitor a 60 Hz (100 fps → 60 mostrati) e **non** deve crescere su `h264-1080p30-44k.mp4`.
3. Confronto colori: aprire lo stesso istante con `ffplay -ss 3 samples/synthetic/hevc8-1440p100-sync.mp4` e confrontare a vista il pattern (barre colorate, neri e bianchi). Se l'immagine è più chiara/slavata, applicare la correzione sRGB indicata nel Task 8 Step 3 e ripetere.
4. Ripetere 1 e 3 con `samples/GX013370.MP4` (scena reale, full range): niente colori slavati.
5. Seek rapidi ripetuti, seek a fine file, apertura di `hevc8-1080p30-noaudio.mp4`: nessun crash, nessun blocco.

- [ ] **Step 3: Verifica su Windows**

Serve una macchina Windows 10/11 con GPU (i runner CI non ne hanno). Copiare `actionlay.exe` e `decode-bench.exe` prodotti dalla CI (o compilati in locale seguendo gli step Windows del Task 2) e i campioni sintetici. Ripetere gli Step 1–2 (al posto di `otool` usare `dumpbin /dependents actionlay.exe`, che non deve elencare `avcodec*.dll`). Expected: `decoder: d3d11va`, stessi criteri.

- [ ] **Step 4: Scrivere il report**

`docs/m0-report.md` con questa struttura, compilata con i valori misurati:
```markdown
# M0 – Esito del prototipo player

Data: AAAA-MM-GG · Commit: <sha>

## Decodifica (decode-bench)
| File | Piattaforma | HW fps | SW fps | Backend |
|---|---|---|---|---|
| GX013370.MP4 (1440p100 HEVC) | macOS M? | … | … | videotoolbox |
| hevc10-2160p60-sync.mp4 | macOS | … | … | videotoolbox |
| … | Windows | … | … | d3d11va |

## Riproduzione
- A/V massimo osservato: … ms (criterio ±40 ms)
- Frame scartati: …
- Colori vs ffplay: ok / correzione applicata
- Seek, fine file, video senza audio: ok / problemi

## Binario
- Dimensione: … MB · collegamento statico: ok

## Decisione
Proseguire con egui + wgpu + FFmpeg statico / Ripiego su libmpv (motivo: …)
```

- [ ] **Step 5: Commit**

```bash
git add docs/m0-report.md
git commit -m "docs: M0 player prototype gate report"
```
