#!/usr/bin/env bash
# Builds a static, GPL (never nonfree) FFmpeg for the host Rust target.
# Output: third_party/ffmpeg/<target>/{include,lib,extralibs.txt}
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
source "$ROOT/scripts/ffmpeg-version.env"
TARGET="${1:-$(rustc -vV | sed -n 's/^host: //p')}"
PREFIX="$ROOT/third_party/ffmpeg/$TARGET"
SRC="$ROOT/third_party/src/ffmpeg-$FFMPEG_TAG"

if [ -f "$PREFIX/export-v2-static" ]; then
  echo "FFmpeg already built in $PREFIX"
  exit 0
fi

bash "$ROOT/scripts/build-encoders.sh" "$TARGET"
ENCODERS="$ROOT/third_party/encoders/$TARGET"
export PKG_CONFIG_PATH="$ENCODERS/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
ENCODER_FLAGS=(--extra-cflags="-I$ENCODERS/include" --extra-ldflags="-L$ENCODERS/lib")
if [[ "$TARGET" = *windows-msvc || "$TARGET" = *linux-gnu ]]; then
  HEADERS="$ROOT/third_party/src/nv-codec-headers"
  if [ ! -d "$HEADERS" ]; then
    git clone --depth 1 --branch n12.2.72.0 https://github.com/FFmpeg/nv-codec-headers.git "$HEADERS"
  fi
  make -C "$HEADERS" PREFIX="$ENCODERS" install
fi
if [[ "$TARGET" = *windows-msvc ]]; then
  ENCODER_FLAGS=(--extra-cflags="-MT -I$(cygpath -m "$ENCODERS")/include" --extra-ldflags="-libpath:$(cygpath -m "$ENCODERS")/lib")
fi

mkdir -p "$ROOT/third_party/src"
if [ ! -d "$SRC" ]; then
  git clone --depth 1 --branch "$FFMPEG_TAG" https://git.ffmpeg.org/ffmpeg.git "$SRC"
fi

COMMON=(
  --prefix="$PREFIX"
  --enable-gpl
  --enable-static --disable-shared
  --pkg-config-flags=--static
  --disable-programs --disable-doc
  --disable-autodetect --disable-network
  --disable-everything
  --disable-avdevice --disable-avfilter
  --enable-swresample --enable-swscale
  --enable-protocol=file
  --enable-demuxer=mov
  --enable-decoder=hevc,h264,aac,prores
  --enable-parser=hevc,h264,aac
  --enable-muxer=mp4,mov
  --enable-encoder=libx264,libx265,prores_ks
  --enable-libx264 --enable-libx265
  "${ENCODER_FLAGS[@]}"
)

case "$TARGET" in
  *apple-darwin)
    PLATFORM=(--enable-pthreads --enable-videotoolbox --enable-encoder=h264_videotoolbox,hevc_videotoolbox
              --enable-hwaccel=hevc_videotoolbox,h264_videotoolbox) ;;
  *windows-msvc)
    PLATFORM=(--toolchain=msvc --enable-w32threads --enable-d3d11va --enable-ffnvcodec --enable-nvenc --enable-encoder=h264_nvenc,hevc_nvenc
              --enable-hwaccel=hevc_d3d11va,hevc_d3d11va2,h264_d3d11va,h264_d3d11va2) ;;
  *linux-gnu)
    PLATFORM=(--enable-pthreads --enable-pic --enable-vaapi --enable-ffnvcodec --enable-nvenc --enable-encoder=h264_nvenc,hevc_nvenc
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

case "$TARGET" in
  *windows-msvc)
    # rustc looks for avcodec.lib, FFmpeg's MSVC build installs libavcodec.a
    shopt -s nullglob
    for f in "$PREFIX"/lib/lib*.a; do
      base="$(basename "$f" .a)"
      mv "$f" "$PREFIX/lib/${base#lib}.lib"
    done
    shopt -u nullglob ;;
esac

# FFmpeg's own system-library requirements (frameworks, -lm, -lva, ...),
# consumed by crates/media/build.rs. Written last: it doubles as the
# "build complete" marker checked at the top of this script.
grep '^EXTRALIBS' ffbuild/config.mak | cut -d= -f2- > "$PREFIX/extralibs.txt"
shopt -s nullglob
archives=("$ENCODERS"/lib/*.a "$ENCODERS"/lib/*.lib)
if [ "${#archives[@]}" = 0 ]; then echo "Missing static encoder archives" >&2; exit 1; fi
cp "${archives[@]}" "$PREFIX/lib/"
touch "$PREFIX/export-v2-static"

echo "FFmpeg $FFMPEG_TAG installed in $PREFIX"
