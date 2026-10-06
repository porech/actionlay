# DMG installation window

`background.svg` is the editable source for the 720 × 480 Finder background.
The gecko, teal palette and pale panels follow the application icon. Finder draws
ActionLay and the Applications link at the positions stored by
`scripts/configure-dmg.py`, with a visible drag direction and installation text.
Only those two items are visible at the volume root; licences are in the app bundle.

Regenerate the committed PNG with:

```sh
cargo run -p actionlay-render --example render-asset -- assets/dmg/background.svg assets/dmg/background.png
```

The artwork is by Alessandro Rinaldi, available under CC BY-SA 4.0 or
GPL-3.0-or-later, at your option. See the [artwork notices](../icons/README.md)
and [CC licence](../icons/LICENSE-CC-BY-SA-4.0.txt). Bundled Roboto is Apache-2.0.
