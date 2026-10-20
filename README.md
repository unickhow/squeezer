# Squeezer

Squeezer is a local-first macOS app for compressing images and videos. Files are processed on your Mac; originals stay untouched, and successful results are written beside them with a `.squeezed` suffix.

## Features

- Import JPEG, PNG, MP4, MOV, and M4V files through the native picker or drag and drop.
- Review image and video thumbnails, dimensions, file sizes, and video duration in one queue.
- Set image and video compression defaults independently, with per-file overrides.
- Choose JPEG quality modes and optionally reduce PNGs to a palette of up to 256 colors while retaining transparency.
- Encode video to H.264 MP4 with high, balanced, or small-file presets; optionally limit the long edge to 1080p or 720p.
- Set a best-effort target size. Squeezer measures the result and reports when the target could not be met.
- Preview images side by side and compare a video's original and compressed versions in one player.
- Cancel active work. Results are validated before they are kept, and files that save less than 10% are skipped by default.

## Requirements

- macOS (tested on macOS 26, Apple Silicon).
- For development: Xcode Command Line Tools, Rust, Node.js 20.19 or newer, and pnpm 10.27.0.

## Download and install

GitHub release packages will be published under the repository's **Releases** page when release assets are ready. The v1.0.0 app bundle is not signed or notarized yet; macOS may show a security warning when opening a locally built copy.

## Build from source

```sh
pnpm install --frozen-lockfile
pnpm tauri dev
```

Build a macOS app bundle (and DMG when configured by Tauri):

```sh
pnpm build
pnpm tauri build
```

## Video support

Video output is H.264 MP4. Squeezer accepts H.264 or HEVC sources that AVFoundation can read, with no audio or one AAC audio track. It does not apply the image input-size limit to videos. A target size is best effort, not a guarantee; the displayed result uses the measured output size.

HDR and Dolby Vision, ProRes, spatial or multiview video, subtitles, chapter tracks, and multiple audio tracks are not supported in v1. HEVC output, WebP, HEIC, output-folder selection, metadata controls, and pause/resume are also not included.

## Image support and limits

- JPEG quality encoding uses macOS ImageIO. Lossless mode does not re-encode JPEG.
- PNG uses `oxipng` lossless optimization by default. Optional palette reduction can change appearance and remove ancillary PNG data.
- Images are limited to 512 MiB, 20,000 pixels per edge, and 100 megapixels to bound decode memory use.
- The PNG palette reducer is local; its quality and savings vary and are not guaranteed to match online services such as TinyPNG.

## Checks

```sh
pnpm test
pnpm build
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```
