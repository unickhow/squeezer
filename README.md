# Squeeze

Squeeze is a local-first macOS image compression app. It keeps the original file untouched and writes validated results beside it with a `.squeezed` suffix.

## Run locally

Prerequisites: macOS, Xcode Command Line Tools, Rust, and Node.js 20.19 or newer.

```sh
npm install
npm run tauri -- dev
```

Create a local app bundle with:

```sh
npm run tauri -- build
```

## Current scope

- Import JPEG and PNG files using the native picker or window drag and drop.
- Queue jobs, report per-file state, and cancel active work.
- JPEG quality modes use macOS `sips`/ImageIO. Lossless mode skips JPEG re-encoding.
- PNG uses `oxipng` lossless optimization; the selected mode adjusts optimization effort but does not quantize colors.
- Outputs are decoded and checked for dimensions; PNG transparency is checked before output placement.
- Images can be previewed before and after compression.
- Inputs are limited to 512 MiB, 20,000 pixels per edge, and 100 megapixels to bound decode memory use.
- Files are processed sequentially. Outputs that save less than 10% are skipped by default.

## Known limits

- The first version supports JPEG and PNG only. WebP, HEIC, video, output-folder selection, metadata controls, and pause/resume are not implemented.
- JPEG encoding uses the macOS built-in ImageIO tool and requires macOS. It does not bundle MozJPEG.
- PNG modes remain lossless, including Balanced and Small Size. They do not use `pngquant`.
- Video, HDR, multi-audio, signing, and notarization are outside the current MVP.

## Checks

```sh
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```
