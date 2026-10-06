# Interface translations

English message text is the catalogue key. All 34 catalogues are embedded in the
executable: language changes and normal use never call a translation service.
Translations include menus, settings, editor controls, tooltips and export status.
Initial translation assistance was followed by contextual vocabulary corrections;
contributions from native speakers are welcome.

`formatted.json` identifies messages with Rust formatting fields. Their translated
values use numbered fields (`{0}`, `{1}`, …), allowing words and values to move.
Do not translate file-format IDs, metric IDs, unit IDs, URLs or user-entered names.
`properties.json`, `widgets.json` and `enums.json` map stable identifiers to visible
English labels. Add each new label to every language catalogue and run app tests.

`System default` is represented by an absent language preference. It remains the
first selector option and follows system language changes; unknown languages use
English. Measurement preferences are independent of interface language. Unit
selection follows widget → layout → user regional preference → system region.
An explicit selection takes precedence; `default` continues through the chain.

Arabic and Hebrew text uses logical Unicode in catalogues and native menus.
The egui interface shapes and orders it at display time, including wrapped help
text. The bundled Noto subsets must be regenerated after adding characters; see
`assets/fonts/interface/README.md`.
