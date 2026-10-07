# Windows installer artwork

The welcome and completion pages use the ActionLay gecko, teal palette, wordmark
and a green/gold route motif. Other pages use the application tile in the header.
The SVG sources are composed from the existing vector masters, without new
raster illustrations. Text uses the bundled Roboto font.

`wizard.bmp` is 656 × 1256 (the Inno Setup 164:314 aspect ratio at 4× resolution).
`wizard-small.bmp` is 256 × 256. Both are opaque, uncompressed 24-bit BMPs,
compatible with Inno Setup 6 and scaled by the wizard for the user's DPI.
The native modern wizard controls retain their standard contrast and behaviour.

Regenerate the SVGs and BMPs from the repository root, using the normal Rust
build environment:

```sh
python3 scripts/build-installer-artwork.py
```

Generated assets are committed; CI does not need to regenerate artwork.
The artwork is by Alessandro Rinaldi, available under CC BY-SA 4.0 or
GPL-3.0-or-later, at your option. See the [artwork notices](../icons/README.md)
and [CC licence](../icons/LICENSE-CC-BY-SA-4.0.txt). Roboto is Apache-2.0.
