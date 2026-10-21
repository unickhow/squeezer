# Squeezer Image Format Coverage Design

## Intent

Bring Squeezer's image compression coverage closer to Tinify's current image formats while making strong compression the default behavior. Users should add supported images and receive the best practical size reduction with visually faithful output without having to enable a separate lossy-compression switch.

Tinify currently lists JPEG/JPG, PNG, WebP, AVIF, JPEG XL (JXL), and animated PNG (APNG) as supported image formats. Squeezer should accept, inspect, preview, compress, validate, and safely write outputs for those formats.

## Goals

- Support JPEG/JPG, PNG, WebP, AVIF, JXL, and APNG from file selection through completed output.
- Automatically choose a format-appropriate optimization path and compression strength per image.
- Preserve source dimensions, alpha where the output format supports it, and animation/frame timing for animated PNG.
- Preserve the original format by default and never overwrite source files.
- Keep output validation and skip-if-not-smaller behavior.
- Remove the user-facing PNG lossy toggle, per-file override, and explicit lossless mode; the remaining quality, balanced, and smaller-file presets express compression strength while the engine selects codecs and trade-offs internally.
- Keep image processing local; do not upload user files to Tinify or another service.

## Non-goals

- Reproduce Tinify's proprietary algorithm or guarantee identical output sizes.
- Add SVG, GIF, HEIC, or video image-sequence support in this project. (HEIC may be reconsidered separately if the desktop compressor product scope includes it.)
- Add user-directed format conversion in this change. Inputs are optimized to their original format; conversion to another format is a separate product decision.
- Add resizing or crop operations.

## User Experience

- The quality, balanced, and smaller-file presets express how aggressively to compress. There is no lossless/lossy switch or lossless preset; the engine chooses the best supported path for each format and preset.
- The queue, metadata inspector, thumbnails, enlarged previews, progress state, and completion summary work for every supported format.
- The input extension is retained in the output name. If an encoder cannot produce a smaller result at the selected quality floor, report the file as skipped with a clear reason.
- Animation remains animated after compression; a static first-frame preview is acceptable in the queue and comparison viewer, with an animation badge in the queue.

## Technical Direction

- Centralize the supported image-format registry so file dialogs, admission, analysis, preview, output path validation, and error copy cannot drift.
- Use mature format-specific encoders rather than extending the hand-written PNG quantizer. PNG should use a perceptual palette quantizer with alpha-aware mapping and dithering followed by lossless PNG optimization. JPEG, WebP, AVIF, and JXL should use format-specific quality encoders and a bounded quality search. APNG requires an animation-aware optimizer that preserves all frames and timing.
- Evaluate native macOS codecs and vendored Rust/C libraries for format coverage, quality, Apple Silicon release packaging, maintenance, and license compatibility before locking dependencies. Processing remains offline and self-contained in the shipped app.
- Use quality metrics as a guardrail, not as a claim that metric score equals human perception. Keep conservative floors for gradients, text, transparency edges, and image detail; never force an arbitrary target-size reduction.
- Validate decoder readability, dimensions, alpha, frame count, and frame timing as appropriate before committing output. Write to a temporary sibling file and atomically publish without overwriting.

## Constraints and Open Builder Decisions

- Tinify format list is scoped to JPEG, PNG, WebP, AVIF, JXL, and APNG; this list must be rechecked before release because vendor support can change.
- Runtime must remain local-first with no network service requirement.
- Existing 512 MiB input and 100 MP image limits remain unless codec memory testing demonstrates a need to lower them; do not raise limits as part of format expansion without measured memory behavior.
- Dependency and encoder choices are left to implementation planning after a focused compatibility and licensing check. JXL encoding and APNG preservation are the highest-risk format paths.
- The app currently targets macOS and uses Tauri 2 with Rust backend and React frontend.

## Acceptance Criteria

- Each listed format can be selected, admitted, analyzed, thumbnailed, previewed, compressed, validated, and exported without changing its format.
- PNG compression automatically uses a perceptual quantizer when it improves size while remaining above the chosen quality floor; no user-facing lossy toggle remains.
- WebP, AVIF, and JXL results are not larger than their source to be accepted; otherwise they are skipped.
- APNG output preserves animation frame count and timing, alpha, and canvas dimensions.
- JPEG orientation and embedded color handling are explicitly validated; output displays with the same orientation and dimensions.
- Corrupt, oversized, unsupported, or encoder-failing files fail individually and do not block other queue items.
- Source files remain byte-for-byte unchanged and output naming never overwrites existing files.
- The test corpus includes photographs, flat illustrations, gradients, transparency edges, high-noise images, animated PNG, and already-optimized inputs. For representative still images, compare output size and decoded visual differences against TinyPNG outputs when user-provided samples are available; do not encode vendor-specific size guarantees in tests.
