# Project landing artwork

`gecko.png` rasterizes the existing `assets/icons/gecko.svg` master, without
changing the illustration. It is kept separate from desktop application assets
and the tag-built browser bundle. Attribution and the choice of CC BY-SA 4.0 or
GPL-3.0-or-later follow [the icon artwork](../icons/README.md).

Regenerate it with the repository's existing SVG renderer:

```sh
cargo run -p actionlay-render --example render-asset -- assets/icons/gecko.svg assets/site/gecko.png
```

The landing template, styles and platform recommendation script live in
`scripts/pages/`. `render-index.py` copies these files into the Pages artifact and
fills download URLs with the latest stable release version supplied by CI in
`ACTIONLAY_STABLE_TAG`. Without that variable, local previews use the workspace
version. No browser API call or external font is needed to render the landing.
