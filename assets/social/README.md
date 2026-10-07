# GitHub social preview

`github-preview.png` is the upload-ready, opaque 1280 × 640 preview (under 1 MB).
The slogan is: **Your videos. Your telemetry. No strings attached.**

Upload it in the repository's Settings → Social preview → Edit → Upload an image.
Committing the image does not set GitHub's social preview automatically.
See [GitHub's instructions](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/customizing-your-repositorys-social-media-preview).

Regenerate the editable SVG and PNG from the vector gecko with the normal Rust
build environment:

```sh
python3 scripts/build-social-image.py
```

Artwork: Alessandro Rinaldi, available under CC BY-SA 4.0 or GPL-3.0-or-later,
at your option. See the [artwork notices](../icons/README.md) and
[CC licence](../icons/LICENSE-CC-BY-SA-4.0.txt). Bundled Roboto is Apache-2.0.
