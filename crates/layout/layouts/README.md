# Bundled telemetry presets

`default.ovl.json` is the original ActionLay dashboard. `moto` and `training` share its visual style and
are new interpretations of the telemetry sections of
[gopro-dashboard-overlay](https://github.com/time4tea/gopro-dashboard-overlay),
by time4tea and contributors, GPL-3.0. Reference revision:
`8e26ee5b452c6aefa38aa6d08557f15139b61e6c` (retrieved 2026-10-05).

| Preset | Upstream reference | Implemented sections |
|---|---|---|
| Default | Original ActionLay dashboard | Date/time, GPS, speed, elevation, gradient, trip distance |
| Moto | `gopro_overlay/layouts/moto_1080_2bars.xml` | Date/time, GPS, speed, braking/acceleration bars, elevation, gradient |
| Training | `gopro_overlay/layouts/power-1920x1080.xml` | Date/time, GPS, speed, heart-rate/power zones, cadence, elevation |

These are telemetry presets, not complete XML conversions. Maps, journey maps,
charts and the circular motor-speed indicators will follow as their widgets
are implemented in M3. The presets do not silently include unsupported nodes.
The 1080p/4K variants use the same responsive preset. No upstream icons are
copied: ActionLay's embedded Tabler icons are used.

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

Generate visual previews with:

```sh
cargo run -p actionlay-render --example preview-presets
```

They use generated telemetry on a neutral background and are written to
`target/preset-previews/` at 16:9, 4:3 and 9:16. They contain no private footage.
Font packaging remains planned with the editor/package workflow (§6.4 of the
design); the current renderer still uses embedded Roboto.
