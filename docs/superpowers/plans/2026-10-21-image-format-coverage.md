# Squeezer Image Format Coverage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Support Tinify's current still and animated image formats locally, with automatic high-quality compression and no user-facing lossy toggle.

**Architecture:** Centralize format capabilities, then use a format-specific decoder/encoder pipeline behind the existing image-job interface. Keep analysis, preview, output naming, compression, and validation consistent with the registry; preserve input format and animation, write temporary outputs, validate, and only then publish. Use mature encoders and bounded quality selection. For PNG, use permissively licensed Quantette for opaque images and retain the alpha-aware palette path for transparent images.

**Tech Stack:** Rust/Tauri 2 backend, React/TypeScript UI, existing `image`, `png`, and `oxipng` crates; add only pinned, self-contained encoder dependencies that pass the compatibility check.

**Spec:** `docs/superpowers/specs/2026-10-21-squeezer-image-format-coverage-design.md`

## Global Constraints

- Supported formats: JPEG/JPG, PNG, WebP, AVIF, JPEG XL (JXL), and animated PNG (APNG).
- Preserve the source format and dimensions; do not add format conversion in this change.
- Process files locally; do not require a network service.
- Never overwrite input files or existing output files.
- Preserve alpha where supported and preserve APNG frame count and timing.
- Keep the existing 512 MiB input and 100 MP image limits unless measured codec memory requires a stricter limit.
- Remove the PNG lossy option and explicit lossless mode from the UI and per-job compression options; quality, balanced, and smaller-file presets remain compression-strength preferences.
- Do not guarantee a fixed savings percentage. Skip output that is not smaller or fails the format's quality floor.

## Review Focus

- APNG is decoded as a still image or loses frames/timing → APNG round-trip tests assert frame count, per-frame delay, alpha, and canvas dimensions.
- Transparent RGB is mishandled at antialiased edges → transparent-edge fixtures assert alpha preservation and bounded visual error.
- JPEG EXIF orientation or color profile handling changes appearance → oriented JPEG fixtures verify display orientation and color metadata behavior.
- Unsupported/corrupt formats block a mixed batch → admission and worker tests assert per-file failure isolation.
- A codec allocates beyond current image limits or is unavailable in packaged macOS builds → bounded-decode tests and release-target build checks.

---

### Task 1: Verify the local codec set and lock encoder choices

**Files:**
- Create: `docs/research/2026-10-21-image-codec-matrix.md`
- Inspect: `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json`

**Interfaces:**
- Produces: a codec matrix for decode, encode, animation, alpha, macOS arm64/x86_64 packaging, licensing, maintenance, and offline operation for all six formats.
- Candidate direction: Quantette (MIT OR Apache-2.0) perceptual quantization for opaque PNG plus alpha-aware mapping for transparent PNG; mature JPEG/WebP/AVIF encoders; BSD libjxl wrapper if its CMake/C++ build can be shipped; APNG-aware lossless optimization or frame-aware palette encoding.

- [ ] **Step 1: Record current gaps and supported release targets**

Inspect current Rust crate versions and the app's macOS release targets. Record which formats can be decoded and encoded without spawning an external binary.

- [ ] **Step 2: Verify candidate libraries against source documentation**

Check build/link behavior, license, alpha and animation support, quality controls, and whether artifacts can be bundled in the existing Tauri release. Do not add dependencies in this step.

- [ ] **Step 3: Select one codec path per format and record rejected alternatives**

Choose dependencies that work offline in a clean Apple Silicon release build. If any format has no maintainable, shippable encoder, record the concrete blocker and an implementation fallback before proceeding.

- [ ] **Step 4: Verify the decision record**

Run: `sed -n '1,240p' docs/research/2026-10-21-image-codec-matrix.md`
Expected: all six formats have an explicit decoder, encoder/optimization strategy, license, release-build check, and fallback.

---

### Task 2: Centralize image format capabilities and complete intake/preview

**Files:**
- Create: `src-tauri/src/engine/image_formats.rs`
- Modify: `src-tauri/src/engine/mod.rs`
- Modify: `src-tauri/src/engine/analyzer.rs`
- Modify: `src-tauri/src/engine/preview.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src/types.ts`, `src/App.tsx` only for format labels/badges and APNG animation indicator
- Test: Rust unit tests beside the touched engine modules

**Interfaces:**
- Produces: `ImageFormatKind::from_extension(&str) -> Option<ImageFormatKind>` and per-format capabilities for MIME type, alpha, and animation.
- Consumes: codec decisions from Task 1.

- [ ] **Step 1: Add registry tests**

Test case-insensitive aliases for JPG/JPEG and every supported extension; reject GIF, SVG, HEIC, and arbitrary files. Test capability flags for alpha and APNG animation.

- [ ] **Step 2: Implement the central format registry**

Move accepted extension checks and format metadata behind `ImageFormatKind`; update the file picker, admission checks, bounded decoder, error copy, and preview MIME generation to use it.

- [ ] **Step 3: Enable decoders and bounded inspection**

Enable the Task 1 decoder dependencies. Preserve existing byte, edge, and pixel limits before full decode. Analyze width, height, file size, and APNG animation metadata without discarding frames in the compression source path.

- [ ] **Step 4: Add representative fixtures and intake/preview tests**

Test one valid file per format, APNG animation metadata, malformed files, oversized dimensions, thumbnail MIME, and mixed valid/invalid file selection.

- [ ] **Step 5: Run focused backend checks**

Run: `cargo test --manifest-path src-tauri/Cargo.toml engine:: --lib`
Expected: registry, analysis, and preview tests pass; unsupported fixtures fail as individual jobs.

---

### Task 3: Replace the PNG toggle with automatic PNG optimization

**Files:**
- Modify: `src-tauri/src/engine/images.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src/types.ts`
- Modify: `src/App.tsx`
- Modify: `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`
- Test: `src-tauri/src/engine/images.rs`

**Interfaces:**
- Consumes: image format registry and selected quantizer from Tasks 1–2.
- Produces: PNG always runs a bounded perceptual optimization attempt, then `oxipng`; the source remains the fallback if the optimized result is larger or below quality floor.

- [ ] **Step 1: Add failing PNG behavior tests**

Cover photograph-like gradients, flat artwork, transparency edges, high-noise input, already-optimized indexed PNG, and source-file immutability. Assert indexed optimization is attempted automatically, alpha is retained, output is smaller when safe, and unsafe output is skipped.

- [ ] **Step 2: Remove `pngLossy` state end to end**

Remove `png_lossy` from Rust options, frontend types/defaults, persisted/per-file overrides, and toggle copy. Remove the explicit `lossless` compression mode and its UI copy; retain quality, balanced, and smaller-file presets as compression-strength intent. Do not expose codec implementation details.

- [ ] **Step 3: Implement quality-bounded PNG optimization**

Use the selected perceptual quantizer to generate candidate palettes and dithered output. Apply oxipng after quantization. Compare decoded candidate to source using the selected objective quality guardrail and reduce compression strength or fall back when the floor is violated.

- [ ] **Step 4: Verify focused PNG cases**

Run: `cargo test --manifest-path src-tauri/Cargo.toml engine::images::tests --lib`
Expected: PNG fixtures pass with transparency preserved and source untouched; outputs failing size/quality gates are skipped.

- [ ] **Step 5: Verify frontend state removal**

Run: `pnpm test`
Expected: type and interaction tests pass; no UI or serialized option references to `pngLossy`/`png_lossy` remain.

---

### Task 4: Add same-format lossy encoders for JPEG, WebP, AVIF, and JXL

**Files:**
- Create: `src-tauri/src/engine/image_encoders.rs`
- Modify: `src-tauri/src/engine/images.rs`
- Modify: `src-tauri/src/engine/validator.rs`
- Modify: `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`
- Test: `src-tauri/src/engine/image_encoders.rs`, `src-tauri/src/engine/images.rs`

**Interfaces:**
- Consumes: codec decisions and shared format registry.
- Produces: `encode_still(source, format, quality) -> Result<Vec<u8>, EncodeError>` using the original format and alpha-capable mode where available.

- [ ] **Step 1: Add encoder contract tests**

For each format, test decode/encode round-trip, unchanged dimensions, alpha preservation where supported, quality bounds, output format identity, and malformed encoder errors.

- [ ] **Step 2: Implement one encoder adapter per selected codec**

Keep codecs behind the common interface. Normalize orientation before encoding only if validation proves displayed orientation remains unchanged; retain compatible color profile metadata when the chosen encoder supports it.

- [ ] **Step 3: Implement bounded quality search**

Try at most five descending quality candidates within the format's configured floor. Select the smallest candidate that passes the shared quality guardrail; stop early when size no longer decreases. If no candidate beats the source, skip.

- [ ] **Step 4: Extend validation by format capabilities**

Validate successful decode, same format, dimensions, alpha preservation, and quality floor before atomic publish. Keep the existing no-overwrite output commit behavior.

- [ ] **Step 5: Run backend format tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml engine:: --lib`
Expected: still-image format tests pass; failed or larger candidates leave source untouched and create no final output.

---

### Task 5: Preserve and optimize APNG animation

**Files:**
- Modify: `src-tauri/src/engine/image_formats.rs`
- Create or modify: `src-tauri/src/engine/apng.rs`
- Modify: `src-tauri/src/engine/images.rs`
- Modify: `src-tauri/src/engine/validator.rs`
- Modify: `src/App.tsx` for an APNG animation badge if not completed in Task 2
- Test: `src-tauri/src/engine/apng.rs`

**Interfaces:**
- Produces: APNG decode/encode path retaining canvas size, frame count, frame delays, blend/disposal semantics, and per-frame alpha.

- [ ] **Step 1: Add animated fixtures and round-trip tests**

Use a small multi-frame APNG with distinct frame delays, partial-frame updates, and transparency. Assert all animation properties survive a no-op round-trip.

- [ ] **Step 2: Implement APNG-aware optimization**

Use `png`'s APNG reader and writer to retain frame controls, re-encode each frame, and optimize frame encoding. Do not pass APNG through oxipng 9.1.5 because its current parser strips the `acTL` animation-control chunk. If frame-level palette reduction cannot preserve animation semantics safely, use lossless frame encoding and report that image as compressed using the conservative path.

- [ ] **Step 3: Add animation integrity validation**

Decode output animation metadata and compare frame count, delays, canvas dimensions, and alpha behavior before publish.

- [ ] **Step 4: Run APNG tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml engine::apng --lib`
Expected: optimized output retains timing and animation; any invalid candidate is rejected without modifying the source.

---

### Task 6: End-to-end matrix, packaging, and user-facing copy

**Files:**
- Modify: `README.md`
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/engine/paths.rs`, `src-tauri/src/engine/validator.rs` as needed
- Test: `src-tauri/src/engine/*` and existing `tests/format.test.ts`, `tests/interactions.test.ts`

**Interfaces:**
- Consumes: all codec adapters and format registry.
- Produces: full queue workflow and published support table, with no stale JPEG/PNG-only or lossy-toggle copy.

- [ ] **Step 1: Add mixed-batch workflow coverage**

Queue one file per supported format plus a corrupt file; assert valid files finish independently, the corrupt file fails independently, and outputs use the original extension without overwriting source/existing outputs.

- [ ] **Step 2: Add image corpus comparison report**

For photographs, illustrations, gradients, noise, transparency, and already-optimized images, record original/output sizes and quality metrics. Compare with TinyPNG when matching sample inputs and outputs are available; report per format and image class without hard-coded TinyPNG size assertions.

- [ ] **Step 3: Verify release packaging**

Build the macOS release targets from a clean dependency cache and confirm all codec libraries are bundled, offline, and notarization-compatible. Record app size and build duration changes.

- [ ] **Step 4: Update README and UI language**

Document the six supported formats, automatic format-specific optimization, local processing, and cases where the app skips because smaller output would fail its quality floor.

- [ ] **Step 5: Run required project checks**

Run: `pnpm test`
Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`
Run: `pnpm tauri build --bundles app`
Expected: all checks pass and a clean macOS app bundle contains every encoder without runtime downloads.

- [ ] **Step 6: Review the diff and commit the completed change**

Confirm there are no remaining lossy-toggle references, no format allowlist drift, and no changes outside the requested image compression flow. Commit in feature-sized commits after each independently reviewed task.
