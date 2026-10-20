# Squeezer

Squeezer is a local-first macOS image and video compression app. It keeps the original file untouched and writes validated results beside it with a `.squeezed` suffix.

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
- Import MP4, MOV, and M4V videos into the same mixed-media queue.
- Queue jobs, report per-file state, and cancel active work.
- JPEG quality modes use macOS `sips`/ImageIO. Lossless mode skips JPEG re-encoding.
- PNG uses `oxipng` lossless optimization by default. An optional lossy mode reduces PNGs to an indexed palette of up to 256 colors while retaining transparency.
- Outputs are decoded and checked for dimensions; PNG transparency is checked before output placement.
- Images can be previewed before and after compression.
- Videos show a poster and duration in the queue, and can be played before and after compression through a narrowly scoped local stream.
- Videos can be compressed to H.264 MP4 with high, balanced, or small-file quality, optional 1080p/720p downscaling, and a best-effort target size.
- Video target size is measured after encoding and can receive one bounded bitrate correction; the app reports actual size when the target is missed.
- The main workflow follows file import, compression options, and batch start in that order. Import and per-file result changes receive local feedback, and motion follows the system reduced-motion preference.
- Inputs are limited to 512 MiB, 20,000 pixels per edge, and 100 megapixels to bound decode memory use.
- Files are processed sequentially. Outputs that save less than 10% are skipped by default.

## Known limits

- Video output is H.264 MP4. H.264/HEVC sources with no audio or one AAC audio track are supported when AVFoundation can read them. HDR/Dolby Vision, ProRes, spatial/multiview, subtitle/chapter tracks, and multiple audio tracks are not supported.
- HEVC output, WebP, HEIC, output-folder selection, metadata controls, and pause/resume are not implemented.
- JPEG encoding uses the macOS built-in ImageIO tool and requires macOS. It does not bundle MozJPEG.
- The optional PNG color reduction is a local palette quantizer; its visual quality and savings vary by image and are not guaranteed to match TinyPNG.
- Signing and notarization are outside the current MVP.

## Checks

```sh
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```
