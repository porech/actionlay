# ActionLay icons

`gecko.svg` is the editable vector master: a lively three-quarter view with a
raised head, visible snout and expressive eyes. The subject follows the owner's
Mediterranean wall gecko: natural olive/tan mottling, raised scales,
five padded toes and a long banded tail.

The [original reference photograph](source/IMG_20261006_132445_1.jpg),
[generated 3D study](source/gecko-study.png) and [prompt](source/prompt.md)
are in `source/`. The study was generated with the built-in imagegen tool using
the photograph as the colour/anatomy reference, then traced into real SVG paths.
The SVG contains no embedded raster images or external references.

`actionlay.svg` places the gecko on a teal application tile.
`dmg.svg` places the same subject on an original silver disk illustration.
## Licence and attribution

The original reference photograph, generated study, SVG artwork and all derived
PNG, ICO and ICNS icons are available under either
[CC BY-SA 4.0](LICENSE-CC-BY-SA-4.0.txt) or
[GPL-3.0-or-later](../../LICENSE), at your option.
The previously granted GPL licence remains available.

Attribution: **ActionLay gecko photograph and artwork — Alessandro Rinaldi**,
[ActionLay](https://github.com/porech/actionlay).
The photograph is original; the illustration was generated with imagegen using
it as a reference, vectorized, and composed into application and disk icons as
described above. When sharing an adaptation under CC BY-SA, retain attribution
and indicate your changes as required by that licence.

## Regeneration

Regenerate both compositions, PNG sizes and Windows ICO/macOS ICNS with:

```sh
python3 scripts/build-icons.py
```

This uses the workspace's resvg/tiny-skia renderer and Python's standard library.
Regeneration does not need the original photograph or the generated study.
All generated assets are committed so release builders need no graphics tools.
