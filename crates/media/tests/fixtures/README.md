# Playback output backlog fixture

`output-backlog.mp4` is synthetic video and audio, generated with:

```sh
ffmpeg -f lavfi -i testsrc2=size=160x90:rate=10 \
  -f lavfi -i sine=frequency=440:sample_rate=48000 -t 10 \
  -c:v libx264 -preset veryfast -crf 28 -pix_fmt yuv420p \
  -c:a aac -b:a 64k -movflags +faststart output-backlog.mp4
```

The ten-second duration exercises repeated recovery cycles when output conversion
is artificially slower than playback. It contains no personal footage.

# Source buffering fixture

`source-buffering.mp4` is a 15-second, 30 fps clip with 44.1 kHz AAC audio:

```sh
ffmpeg -f lavfi -i testsrc2=size=320x180:rate=30 \
  -f lavfi -i sine=frequency=440:sample_rate=44100 -t 15 \
  -c:v libx264 -preset veryfast -crf 28 -pix_fmt yuv420p -g 30 \
  -c:a aac -b:a 64k -movflags +faststart source-buffering.mp4
```

Buffering regressions use this compact fixture and 16 KiB cache/AVIO blocks
through the same source implementation used in production (whose blocks remain
1 MiB). Reads stay blocked until the test observes buffering or explicitly
releases them, rather than being released by a fixed sleep. This isolates source
starvation from decode throughput and hosted-runner hardware acceleration.
High-resolution decode/backpressure tests still use the synthetic sample suite.
