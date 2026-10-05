# M3 implementation and validation

M3 adds native chart/gradient-chart, moving/journey/combined/circuit map and
G-meter widgets, with per-widget schema, metric requirements, style controls,
unit conversion, missing-data policy and responsive geometry. Default replaces
GPS coordinates with a map; Moto includes a route map and G-meter; Training
includes an elevation/gradient chart. Each compass owns optional smoothing and
its thresholds; disabling the filter keeps those thresholds in the layout.

The XML importer maps all thirteen pinned upstream fixtures to known native
widgets and embeds their converted layouts in the binary. It preserves unsupported
components and reports native styling equivalents. Reference resolution is inferred
or requested. Imported copies are saved in the local library without overwriting
name collisions. Tests check fixture conversion, text alignment, JSON round trips,
schema validity and unknown-component preservation. Upstream provenance is in
`crates/layout/tests/upstream/README.md`.

Maps request visible tiles on a background thread with identifying User-Agent,
visible attribution, bounded memory/job queues and a seven-day disk cache. Corrupt
cache entries are refetched. Offline cache misses, network errors and service
backoff keep rendering responsive. Bulk route prefetch from the initial design
was replaced with viewport requests to follow the
[OSM tile policy](https://operations.osmfoundation.org/policies/tiles/).
Privacy masks suppress location/route drawing and requests at hidden positions.

The compass spike was caused by forward/backward pair selection changing at the
end of a progressively loaded telemetry prefix. Causal heading uses only prior
positions. A metadata-only replay on the private sample found 316 legacy revisions
above 2°, up to 108.90°, versus zero causal-heading revisions. Synthetic regression
tests reproduce the legacy defect without GPS noise and check prefix invariance
and recovery after GPS gaps. No private footage or coordinates are committed.

G-meter calibration freezes at the first confident IMU/GPS estimate, removes
GRAV or estimated gravity and never retrospectively rotates earlier readings.
Tests exercise a sideways-mounted camera, progressive-prefix invariance and
stationary data that cannot establish a mounting orientation. GPS estimates remain
explicitly labelled until calibration; rotation is configurable per widget.

Rendering caches shaped text, rasterised text, map bases and instrument grids.
Chart segments and route strokes are batched. Goldens cover the new Default and
all four history instruments; additional tests cover stale expiry, backwards seeks
and privacy request suppression. Previews use generated telemetry at 16:9, 4:3
and 9:16. On the local Apple Silicon machine, 1920×1440 p95 was 2.22 ms Default,
3.72 ms Moto, 3.17 ms Training and 10.60 ms for the dense upstream example, below
the 15 ms target. At 4K, the dense demo can exceed 15 ms; overlay scheduling stays
independent of video decoding and uses the latest frame.

`make check`, release `make build` and the complete workspace suite (353 tests)
passed locally with public/synthetic samples. Audible player checks were routed
through BlackHole 2ch via `ACTIONLAY_TEST_AUDIO_DEVICE`. The new Settings → Audio
window supports system default or an explicit output, immediate persistence,
refreshing devices and an unavailable-device notice. Changing output keeps time,
pause state and speed. BlackHole drove the playback clock, but the input capture
returned silence, so acoustic loopback is not claimed as verified. Manual window
inspection was unavailable because Computer Use returned cgWindowNotFound for
all inspected apps; offline renderer previews were inspected instead.

The visual widget editor, undoable reset controls, font packaging and export
remain M4/M5 work.
