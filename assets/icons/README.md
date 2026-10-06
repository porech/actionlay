# ActionLay icons

`gecko.svg` is the editable vector master: a lively three-quarter view with a
raised head, visible snout and expressive eyes. The subject follows the owner's
Mediterranean wall gecko photographs: natural olive/tan mottling, raised scales,
five padded toes and a long banded tail. It is an illustration, rather than the
initial flat top-down photographic tracing.

The 3D study was generated with the built-in imagegen tool using
`IMG_20261006_132445_1.jpg` as the colour/anatomy reference, then traced into real
SVG paths. The original photograph is not bundled. The selected generated study
and its prompt are in `source/`; the SVG contains no embedded raster images or
external references.

`actionlay.svg` places the gecko on a teal application tile.
`dmg.svg` places the same subject on an original silver disk illustration.
The artwork is distributed under the repository's GPL-3.0-or-later licence.

Regenerate both compositions, PNG sizes and Windows ICO/macOS ICNS with:

```sh
python3 scripts/build-icons.py
```

This uses the workspace's resvg/tiny-skia renderer and Python's standard library.
Regeneration does not need the original photograph or the generated study.
All generated assets are committed so release builders need no graphics tools.
