> Historical milestone report. ActionLay 1.4.0 adds INSGPS parsing,
> multiple-file/folder matching, single-file start fallback, browser activity
> linking, linked-activity export and desktop video rotation. See the
> [current activity source guide](activity-sources.md); export/browser limits
> recorded below describe the original milestone implementation.

# M6 implementation and validation

GPX/FIT activities can be linked through File → Metric sources… or dropped onto
an open video. Alignment uses recording UTC plus a signed activity offset;
manual video UTC handles cameras without a usable clock. Activity samples win
where available and camera samples fill gaps. Links are retained per video
identity. GPX track segments, FIT timer events and missing GPS break derived
heading and acceleration rather than drawing or differentiating across gaps.

GoPro chapters are discovered conservatively: a contiguous recording must start
with its first chapter. The player reopens the decoder at chapter boundaries,
so this is one timeline rather than a promise of gapless decoder transitions.
Opening the selected file alone remains available. An intermediate chapter
opens by itself and offers a non-modal button to load the full sequence from
its first chapter. UI verification opened a synthetic second chapter alone
(5 s), then loaded both chapters on request (10 s); the offer disappeared.
That explicit choice is remembered per video. The CLI keeps
intermediate files standalone unless --all-chapters is provided.

Privacy zones are global Settings preferences, independent of layouts. Map
positions and crossing route segments are hidden. Source footage, telemetry
files and other widgets remain available; the configuration dialog explains
this scope and the inference risk from repeated starts/ends.

Maps support north-up/course-up, no/past/full route, independent past/future
colors, stroke width and marker size/color/visibility. Widget and bundled default
layout use north-up/no route. Green/yellow are the default route colors when
routes are enabled. The editor warns about additional loading for past/full.

For GoPro route backfill, the MP4 header provides the metadata sample index.
The dedicated reader does not call find_stream_info, discards audio/video,
seeks in gpmd timebase, and includes the predecessor metadata packet for
interpolation. Already loaded intervals are excluded. The worker retains the
opened header/index across subsequent requests and supports cancellation.
The default route setting makes no supplementary metadata requests.

Validation compares indexed ranges against full reads on five public GoPro
samples, checks merging route backfill with playback packets, and checks route
colors, rotation, marker size and backward-seek determinism on rendered pixels.
FFmpeg AVIO counters also check that metadata-only reads skip media payloads.
These counters measure demux I/O, not network traffic: filesystem/cloud-client
read-ahead can transfer additional bytes.

A pCloud mounted-drive check on three private GoPro files (682 MB and two
approximately 4 GB chapters) verified metadata seeks forward/backward and repeat
seeks. Private filenames and sensor values are omitted. On the 4 GB optimized
reader check, the header/index cost 1.32 MB once; one-second range requests read
8.4 KB each and took 0.10–0.30 s. A cached repeat took about 1 ms. Reading the
first 20 s required 88.7 KB and 1.01 s. Before optimizing packet buffering and
index-based termination, range requests read 98.3 KB and the 20 s prefix read
720.9 KB. These are separate recordings, so timing differences are not an A/B
latency benchmark. The actual player's paused, precise seeks on the optimized
check recording returned the expected frames in 0.41–0.44 s, with a cached
repeat at 0.17 s. No pCloud cache was cleared, so these observations do not
establish cold-cache performance or network traffic volume. SMB/NFS remain
untested.

The reader switches to direct AVIO packet reads after buffered header parsing
and checks the metadata sample index before fetching a packet beyond the
requested interval. Public-sample regression checks bound packet-read overhead,
compare seeks against complete reads, and verify the index is retained.
Reproduce diagnostics with the metadata-seek-check and paused-seek-check media
examples; neither prints location or sensor values.

External format tests use synthetic GPX and CRC-checked FIT plus optional official
Garmin SDK examples. scripts/fetch-external-samples.sh fetches pinned examples
into ignored local samples, verifies SHA-256 and downloads their license.
Those SDK assets are not bundled or redistributed. Private user videos and
telemetry are not published.

The native-camera adapter is tested with a synthetic Gyroflow IMU log and public
original recordings: DJI Avata FC8183 (143.794 s, 8619 frame metadata samples,
orientation quaternions) and Insta360 ONE X2 (4.371 s, 2257 IMU samples,
accelerometer). Both decode and seek through the actual player. Available
orientation/accelerometer metrics cover over 99% of each video; absent GPS and
acceleration in the DJI sample remain absent. Native orientation uses normalized
quaternions and conventional ZYX Euler angles in degrees; GoPro retains its
reference-compatible CORI conversion. Unit checks verify axes, scaling, invalid
quaternions and timestamp limits. `.insv` is included in the video chooser;
playback displays the raw camera stream, without 360 stitching/reframing.

Download originals locally with `python3 scripts/fetch-camera-samples.py`;
`--check` verifies SHA-256 without network access. Sources:
- [DJI: Gyroflow public test-data folder](https://github.com/gyroflow/gyroflow#test-data)
- [Insta360 ONE X2: publisher's original samples](https://360rumors.com/insta360-one-x2/)

Sample media is ignored, not bundled or redistributed. The Insta360 publisher
limits samples to personal use and forbids redistribution. GPS timeline alignment
on native camera recordings is not covered by these GPS-free samples; firmware
and device coverage remain parser-dependent. Parser failures are reported and
GPX/FIT linking is available as a fallback. Other-camera parsing can require
additional source reads and does not use the optimized GoPro reader.

Integration with M5 preserves export cancellation with the indexed metadata
reader and volume/mute settings across chapter transitions. The current export
pipeline still processes one source file and its embedded GoPro metadata; linked
activities, native-camera telemetry and joined chapter timelines are available
in playback but are not yet passed to export.
