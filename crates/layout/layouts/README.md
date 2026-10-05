# Bundled telemetry presets

`default.ovl.json` is the original ActionLay dashboard. `moto` and `training` share its visual style and
are new interpretations of the telemetry sections of
[gopro-dashboard-overlay](https://github.com/time4tea/gopro-dashboard-overlay),
by time4tea and contributors, GPL-3.0. Reference revision:
`8e26ee5b452c6aefa38aa6d08557f15139b61e6c` (retrieved 2026-10-05).

| Preset | Upstream reference | Implemented sections |
|---|---|---|
| Default | Original ActionLay dashboard | Date/time, GPS, speed, elevation, gradient, trip distance |
| Moto | `gopro_overlay/layouts/moto_1080_2bars.xml` | Date/time, GPS, speed gauge, course compass, braking/acceleration bars, elevation, gradient |
| Training | `gopro_overlay/layouts/power-1920x1080.xml` | Date/time, GPS, speed, heart-rate/power zones, cadence, elevation |

These are telemetry presets, not complete XML conversions. Maps, journey maps,
charts and the remaining specialised indicators will follow as their widgets
are implemented in M3. The presets do not silently include unsupported nodes.
The 1080p/4K variants use the same responsive preset. No upstream icons are
copied: ActionLay's embedded Tabler icons are used.
When map widgets are implemented, Default's coordinate panel will become a
map with a GPS status indicator. Coordinate metrics remain available for custom layouts.

## Geometry and styling

All dimensions, text sizes, corner radii and stroke widths use layout units:
1 unit is 1/1080 of the displayed video's height. These are not window pixels.
Anchors attach panels to the video edges. `offset_relative: [x, y]` adds an
inset as a fraction of the parent width/height; optional `offset` adds scaled
layout units. For portrait video, the existing automatic Fit mode reduces the
uniform scale so that opposing panels do not overlap. Video letterboxing is
outside the overlay coordinate system.

Palette roles (`primary`, `secondary`, `accent`, `panel`) inherit the layout's
theme. The selector's Appearance controls override accent, panel opacity and
unit system globally, with immediate preference saves. Reset restores the
selected preset/file's original appearance. Individual widget styles, bar
ranges, baseline, zone thresholds, orientation and units can be edited in the
JSON now; the visual editor is planned in M4. Explicit widget units take
precedence over the global unit system.

`bar` fills from its baseline (zero by default, clamped into the range), so
negative-only brake ranges are empty at zero and grow with deceleration.
`zone_bar` uses strictly ordered `up_to` bounds, ending at `max`, in displayed
units. Without explicit zones it uses thirds: green, theme accent, red.
Readings clamp to the range after unit conversion. The shared absent/stale
policy dims stale values for three seconds by default, then shows the empty
track and value. `when_absent: hide` only hides data absent from the entire
video.

## Circular instruments

`gauge` supports `mode: "arc"`, `"needle"` and `"donut"`. `min`/`max` are in
display units. `start_angle` is clockwise from the right and `sweep_angle` is
the span: the default arc starts at 135° and sweeps 270°; a donut defaults to
-90° and 360°. `ticks` sets the number of scale intervals (0 disables ticks,
maximum 72), and `show_labels` controls their numeric labels.

`compass` supports `mode: "rose"` and `"arrow"`. Use an angular metric, usually
`heading` for causal GPS course; it represents direction of travel, rather than magnetic
north from a sensor. Bearings wrap through 0°/360°. `rotate_rose: true` rotates
the scale beneath an upward-facing pointer. An absent bearing shows the muted
rose and a dash, with no direction arrow.

Each compass can optionally set `smoothing` in its layout. Omitting it disables
the filter. An object enables smoothing with these defaults: `seconds: 0.5`
(response time), `deadband: 1.5` (degrees), `max_rate: 120` (degrees/second),
and `min_speed: 1.5` (m/s; hold the last heading below this speed). Set
`smoothing.enabled: false` to bypass the filter while keeping its thresholds.
Zero response time, deadband or maximum rate disables that constraint. The Moto
preset enables these defaults. Filtering wraps through north and is deterministic
when seeking; the progressive-loading heading fix also works without smoothing.

```json
{
  "type": "compass",
  "metric": "heading",
  "smoothing": {
    "enabled": true,
    "seconds": 0.5,
    "deadband": 1.5,
    "max_rate": 120,
    "min_speed": 1.5
  }
}
```

Both widgets accept `diameter`, `thickness`, `fill`, `track`, `show_value`,
`format`, `value_style`, `label_style`, `when_absent` and `stale_secs`, plus the
normal anchor, relative offset and opacity. All geometry scales with the video.
Unknown fonts follow the same Roboto fallback as other widgets. For example:

```json
{
  "type": "gauge",
  "metric": "speed",
  "mode": "needle",
  "diameter": 270,
  "min": 0,
  "max": 240,
  "ticks": 12,
  "fill": "accent",
  "value_style": { "size": 36, "weight": "bold" },
  "label_style": { "color": "secondary" }
}
```

Generate visual previews with:

```sh
cargo run -p actionlay-render --example preview-presets
```

They use generated telemetry on a neutral background and are written to
`target/preset-previews/` at 16:9, 4:3 and 9:16. They contain no private footage.
Font packaging remains planned with the editor/package workflow (§6.4 of the
design); the current renderer still uses embedded Roboto.

## History widgets (M3)

`chart` and `gradient_chart` accept a metric, display units, `size`, a time window
(`seconds`, default 60), `samples` (2–2048), optional `min`/`max`, `journey`,
`show_value`, stroke/fill/background colours, radius and text styling. Auto range
fits the observed values with a margin. Gaps and unread seek ranges break lines;
gradient charts colour ascent/descent via `positive`/`negative`.

`map` accepts `mode` (`moving`, `journey`, `moving_journey`, `circuit`), `size`,
`zoom` (0–19), route/marker/background colours, route width, corner radius,
`opacity_tiles` and `label_style`. Circuit mode draws only the route. Map-provider
configuration lives in the app preferences; identifying requests, visible
attribution and a seven-day disk cache apply. Downloads cover only visible tiles,
never route bulk prefetch. Offline misses remain a muted background. Preview
renderers default to offline and can inject a TileStore explicitly.

`g_meter` uses `accel.lon`/`accel.lat`; units default to G and can be `mps2`.
It accepts diameter, range (default 1.5), rings, trail seconds, session peak markers,
rotation in degrees, fill/track colours and value styling. A confident mounting
calibration switches causally to gravity-corrected IMU readings. Otherwise the
widget labels its GPS estimate. With no GRAV stream its gravity estimate is labelled.

All three history widgets accept `when_absent` and `stale_secs` (default 3):
missing data can hide the widget; temporary gaps dim their last reading and expire
into an empty state. Resets remove overridden keys. Widget `required_metrics()`
declarations support future editor availability warnings.

The thirteen original XML layouts are embedded in `catalog::UPSTREAM_PRESETS`.
Their [pinned fixtures and provenance](../tests/upstream/README.md) are checked
against regenerated JSON by integration tests. Original Flaticon assets are
replaced by semantic MIT Tabler icons; rendering differences are reported by the
importer. Unknown components retain their source attributes in JSON.

Generate offline synthetic previews and measure rendering with:

```sh
cargo run --release -p actionlay-render --example preview-m3
cargo run --release -p actionlay-render --example m3-bench
```
