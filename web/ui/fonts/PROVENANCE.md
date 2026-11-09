# Alegreya Sans (editor overlay type)

- Typeface: Alegreya Sans by Huerta Tipográfica (https://github.com/huertatipografica/Alegreya-Sans), SIL Open Font License 1.1 (`OFL.txt`, copied unchanged from the package).
- Source: npm `@fontsource/alegreya-sans@5.3.0` (`npm pack`, tarball sha256 `f52077bb977c913b05ec2cc016b30858533005d376cff4280c7fa95dbace448b`), fetched 2026-11-09 for P3-T5.
- Files: the package's latin-subset woff2 files, unmodified.

| File | Weight | sha256 |
|---|---|---|
| `alegreya-sans-latin-400-normal.woff2` | 400 | `b2a5a35a2563a2f9bf9fb91939fc6ea6c115e9811cda5c4a37d02f623c7c4dbe` |
| `alegreya-sans-latin-500-normal.woff2` | 500 | `55b61d22cc7afd0bca94d17a84f0e92963b8f5216dbfbde56b2f6cf530c9b2dd` |
| `alegreya-sans-latin-700-normal.woff2` | 700 | `5acd19a588614d23e52b741a21dd8555178ab4932c3195c6d8fb4b296f84acae` |

`make ui-build` stages them into `web/ui/dist/fonts/`; `web/ui/src/editor/style.ts` declares them with `@font-face`. The page makes no Google Fonts request.
