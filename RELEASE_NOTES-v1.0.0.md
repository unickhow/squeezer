# Squeezer v1.0.0

Squeezer v1.0.0 is the first release of a local-first macOS app for compressing images and videos. It brings both media types into one queue while keeping their settings and previews tailored to each format.

## Highlights

- Add JPEG, PNG, MP4, MOV, and M4V files using the native picker or drag and drop.
- Review thumbnails, dimensions, file sizes, and video duration directly in the queue.
- Compress images with JPEG quality modes, PNG lossless optimization, or optional PNG palette reduction.
- Compress video to H.264 MP4 with high, balanced, and small-file presets, optional 1080p/720p downscaling, and a best-effort target size.
- Set independent batch defaults and per-file overrides for images and videos.
- Preview images before and after compression; compare video versions in a single player.
- Cancel active jobs. Squeezer validates outputs, avoids overwriting existing files, and skips results with less than 10% savings by default.
- All media processing happens locally. Source files are left untouched.

## Supported video

Video sources must be readable by AVFoundation and contain either no audio or one AAC audio track. H.264 and HEVC input are supported; output is H.264 MP4. Target size is best effort, and Squeezer reports the measured result if it misses the requested size.

HDR and Dolby Vision, ProRes, spatial or multiview video, subtitles, chapter tracks, multiple audio tracks, and HEVC output are not supported in this release.

## Requirements and distribution

- macOS; tested on macOS 26 with Apple Silicon.
- The app bundle is not signed or notarized in v1.0.0. macOS may warn when opening a locally built app.
- Download links will be added when release assets are published.

## Verification

- `pnpm install --frozen-lockfile` passed with the committed pnpm lockfile.
- `pnpm test` passed (16 frontend tests).
- 35 Rust tests passed.
- `pnpm build` passed.
- `pnpm tauri build --bundles app` passed; the bundle includes the arm64 native video helper and the configured pnpm build hook.

## Known limitations

- HEVC output, WebP, HEIC, output-folder selection, metadata controls, and pause/resume are not available.
- The optional PNG palette reducer can change appearance and remove ancillary PNG data; results vary by image.
- The complete manual matrix for video formats, edge cases, keyboard interaction, and window sizes remains to be exercised on release hardware.
