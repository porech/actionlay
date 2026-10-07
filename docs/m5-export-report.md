# M5 export implementation

The desktop Export button and File menu use the same pipeline as
`actionlay export`. A request snapshots the layout, source and range. Export
decodes every presentation frame, renders at source resolution with the preview
renderer, then composites and encodes. It does not use the playback clock or
drop late frames. Bounded work queues and reorder tokens preserve frame order
while independent renderer workers run in parallel.

Output modes are video with overlay, transparent overlay, and overlay on a solid
colour. The last has a colour picker and a green-screen reset; its default is
`#00FF00`. H.264 and H.265 MP4 support opaque output. ProRes 4444 MOV and PNG
sequences also support straight alpha. Premultiplied renderer pixels are converted
to straight alpha for transparent output, avoiding darkened translucent edges.

Video files copy the source audio packets. Overlay-only outputs and PNG
sequences are silent. Source resolution is retained by default; an advanced override resizes the
composited output. Nominal frame rate and presentation timestamps are retained. A range selects frames in `[in, out)` and starts output
at the first selected frame. Copied compressed audio is clipped at packet
boundaries. PNG sequences include `sequence.json` with frame timestamps and FPS.

Progress reports completed frames, elapsed time, percentage and estimated time
remaining. Cancel in the app or Ctrl+C in the CLI stops further work and flushes
the encoder and container. Completed frames are retained as a valid partial
output; cancellation before the first frame produces no output. Closing the app
while an export is active is guarded. The selected source is never written.
Existing destinations are rejected. Failed exports discard their private
temporary output and report an error without replacing existing files.

CLI examples:

```text
actionlay export --layout dashboard.actionlay-layout --out ride.mp4 --start 5 --end 20 VIDEO.MP4
actionlay export --mode solid --background 00FF00 --out overlay.mp4 VIDEO.MP4
actionlay export --mode transparent --format prores --out overlay.mov VIDEO.MP4
actionlay export --mode transparent --format png --out overlay-frames VIDEO.MP4
```

Software encoding is the default. `--hardware` prefers an available hardware
encoder with software fallback; `--software` explicitly keeps software encoding.
An omitted layout uses the remembered layout or built-in default. The Balanced
preset copies the supported source codec (H.264, HEVC or ProRes), actual container
(MP4 or MOV), dimensions and video bitrate when available. Unsupported codecs or
containers fall back to H.264 or MP4; unavailable bitrate falls back to CRF 20.
Copying these parameters still re-encodes the rendered video. Transparent output
uses ProRes 4444 MOV by default. Source FPS/timestamps and video-file audio are
preserved by the pipeline rather than being independently reconfigured.

Quality presets are Balanced, High quality (CRF 18, slow software encoding), and
Fast export (CRF 23, veryfast software encoding). The desktop Advanced accordion
holds codec, container, resolution, rate control, CRF/bitrate, maximum bitrate,
buffer size, keyframe interval and encoder preference/speed. Modified options
are highlighted and offer **Set preset default**. Selecting any preset resets
these options and closes the accordion, retaining output mode/background.
Preferences remember the last exported settings and reopen Advanced if options
were edited. Zero maximum bitrate/buffer and keyframe interval use encoder
defaults. Hardware preference uses bitrate control; constant-quality presets
keep software encoding. CLI `--preset balanced|high-quality|fast` selects the
same defaults. One source file is accepted per invocation; chapter concatenation
belongs to M6.

Builds bundle pinned static x264/x265 and FFmpeg encoders. NVIDIA NVENC is enabled
on Windows/Linux, and VideoToolbox on macOS; unavailable hardware falls back to
the software encoder. QSV, AMF and VA-API encoding are not enabled in this build.
Windows dependencies and Rust use a static C/C++ runtime to keep the executable
independent of the Visual C++ redistributable. The current pipeline matches the
player's eight-bit SDR decoding; HDR preservation and ten-bit export are not
implemented. Maps use the existing visible-tile cache and request policy; missing
or pending tiles retain the renderer's empty state, without route bulk prefetch.

Validation on Windows: 393 workspace tests passed, including seven export tests;
workspace Clippy passed with warnings denied, formatting and release builds
passed. The tests cover range selection, copied audio, preserved frame count,
PNG alpha/compositing, ProRes alpha and a playable partial export after cancel.
CLI runs verified H.264 NVENC, H.265 NVENC, solid green, ProRes 4444 alpha and PNG
sequences. A three-second selection of local 1920×1440 / 100 fps GoPro footage
exported all 300 frames and copied audio; total duration was 3.008 seconds due to
compressed audio packet boundaries. No private footage is included in the repo.
Desktop testing also completed a green-background export via the native Save
dialog: 30 frames at 10 fps, 3 seconds, H.264 NVENC, no audio. The Windows binary
has no FFmpeg or Visual C++ runtime DLL dependency. Linux/macOS hardware and
runtime behaviour have not been tested locally; CI builds all three targets.
