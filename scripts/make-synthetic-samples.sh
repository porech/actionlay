#!/usr/bin/env bash
# Generates publishable synthetic test videos. Needs an ffmpeg CLI with libx265 and libx264.
# A/V sync marks: at every whole second one white frame and a 10 ms 1 kHz beep.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/samples/synthetic}"
FF="${FFMPEG_BIN:-ffmpeg}"
mkdir -p "$OUT"

beep() { # $1 = duration, $2 = sample rate; sample-accurate (aevalsrc), mono
  echo "aevalsrc=sin(2*PI*1000*t)*lt(mod(t\,1)\,0.01):s=$2:d=$1"
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

# HEVC with uncompressed PCM audio: the app build has no PCM decoder, so probe()
# must degrade to video-only. (CI regenerates all samples with this script.)
"$FF" -v error -y \
  -f lavfi -i "testsrc2=size=640x360:rate=30:duration=2" \
  -f lavfi -i "$(beep 2 48000)" \
  -c:v libx265 -preset ultrafast -pix_fmt yuv420p -tag:v hvc1 -x265-params log-level=error \
  -c:a pcm_s16le -shortest "$OUT/hevc8-pcm-audio.mov"

# Audio ends before the minimum resume watermark; video must still reach EOF.
"$FF" -v error -y \
  -f lavfi -i "testsrc2=size=160x90:rate=30:duration=3" \
  -f lavfi -i "$(beep 0.1 48000)" \
  -c:v libx264 -preset ultrafast -pix_fmt yuv420p \
  -c:a aac -b:a 128k "$OUT/h264-short-audio.mp4"

ls -l "$OUT"
