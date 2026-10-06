# Interface fonts

ActionLay embeds compact subsets of Noto Sans for Arabic, Hebrew, Japanese,
Korean, Simplified Chinese and Traditional Chinese. This keeps the language
selector and translated interface readable without downloading fonts at runtime.
The original fonts are licensed under SIL OFL 1.1; each notice is included here
and in release packages. Modified subsets use the family name ActionLay Interface.

`sources.json` records pinned upstream URLs and SHA-256 checksums. To regenerate,
download each font as `CODE.ttf` and its notice as `CODE-OFL.txt` into a temporary
directory. In a separate Python environment with FontTools 4.66.1 installed, run:

```sh
python scripts/subset-interface-fonts.py /path/to/source-fonts
```

Regenerate after adding translated text. The script retains the catalogue's
characters, language names and Arabic presentation forms, and fixes the variable
font to its regular weight. Source fonts are not bundled in the repository.
