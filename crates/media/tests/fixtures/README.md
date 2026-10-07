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
