# Export test fixture

`export-source.mp4` is synthetic, contains no location or personal data, and
may be committed and used in CI. It has three seconds of 160×90 H.264 at 10 fps
and a 1 kHz AAC tone at 48 kHz. Generated with:

```sh
ffmpeg -f lavfi -i 'testsrc2=size=160x90:rate=10:duration=3' \
  -f lavfi -i 'sine=frequency=1000:sample_rate=48000:duration=3' \
  -c:v libx264 -preset fast -crf 28 -pix_fmt yuv420p \
  -c:a aac -b:a 32k -shortest export-source.mp4
```

`export-source-rotated.mp4` has the same encoded frames and audio, with a
display matrix requesting 90° clockwise. Generated without re-encoding:

```sh
ffmpeg -display_rotation:v:0 -90 -i export-source.mp4 -map 0 -c copy \
  export-source-rotated.mp4
```
