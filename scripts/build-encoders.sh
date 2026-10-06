#!/usr/bin/env bash
# Static GPL encoder dependencies. No executables or shared libraries are shipped.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="${1:?Rust target required}"
PREFIX="$ROOT/third_party/encoders/$TARGET"
X264_REV=c24e06c2e184345ceb33eb20a15d1024d9fd3497
X265_REV=07295ba7ab551bb9c1580fdaee3200f1b45711b7
if [ -f "$PREFIX/static-v2" ]; then exit 0; fi
mkdir -p "$ROOT/third_party/src" "$PREFIX"
for name in x264 x265; do
  if [ ! -d "$ROOT/third_party/src/$name/.git" ]; then
    url="https://github.com/videolan/x265.git"
    [ "$name" = x264 ] && url="https://github.com/mirror/x264.git"
    git clone "$url" "$ROOT/third_party/src/$name"
  fi
done
git -C "$ROOT/third_party/src/x264" checkout "$X264_REV"
git -C "$ROOT/third_party/src/x265" checkout "$X265_REV"
JOBS="$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)"
cd "$ROOT/third_party/src/x264"
if [[ "$TARGET" = *windows-msvc ]]; then
  CC=cl ./configure --prefix="$PREFIX" --enable-static --disable-cli --disable-opencl --extra-cflags=-MT
else
  ./configure --prefix="$PREFIX" --enable-static --disable-cli --disable-opencl --enable-pic
fi
make -j"$JOBS"
make install
cd "$ROOT"
BUILD_DIR="${ACTIONLAY_ENCODER_BUILD_DIR:-$ROOT/third_party/build/x265-$TARGET}"
if [[ "$TARGET" = *windows-msvc ]]; then
  cmake -S third_party/src/x265/source -B "$BUILD_DIR" -G 'Visual Studio 17 2022' -A x64 \
    -DCMAKE_INSTALL_PREFIX="$(cygpath -m "$PREFIX")" -DCMAKE_POLICY_VERSION_MINIMUM=3.5 -DENABLE_SHARED=OFF -DENABLE_CLI=OFF -DENABLE_ASSEMBLY=OFF -DSTATIC_LINK_CRT=ON
else
  cmake -S third_party/src/x265/source -B "$BUILD_DIR" \
    -DCMAKE_INSTALL_PREFIX="$PREFIX" -DCMAKE_POLICY_VERSION_MINIMUM=3.5 -DENABLE_SHARED=OFF -DENABLE_CLI=OFF -DCMAKE_POSITION_INDEPENDENT_CODE=ON -DCMAKE_BUILD_TYPE=Release
fi
cmake --build "$BUILD_DIR" --config Release --parallel "$JOBS"
cmake --install "$BUILD_DIR" --config Release
if [[ "$TARGET" = *windows-msvc ]]; then
  # The pkg-config checks and MSVC linker both accept .lib archives.
  [ ! -f "$PREFIX/lib/libx264.lib" ] || cp "$PREFIX/lib/libx264.lib" "$PREFIX/lib/x264.lib"
  [ ! -f "$PREFIX/lib/x265-static.lib" ] || cp "$PREFIX/lib/x265-static.lib" "$PREFIX/lib/x265.lib"
fi
touch "$PREFIX/static-v2"
