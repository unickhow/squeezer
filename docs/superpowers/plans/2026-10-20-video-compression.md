# Squeezer Video Compression Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Implementation status (2026-10-20):** The first integrated video-compression slice is implemented and built. It supports MP4/MOV/M4V intake, H.264 output, single AAC/no-audio sources, optional 720p/1080p downscale, target-byte mode with one bounded bitrate correction, secure native preview, cancellation, output validation, and mixed image/video settings. Cargo (35 tests), frontend (16 tests), frontend production build, and macOS app bundle pass. Manual end-to-end checks still outstanding: seek/source switching, keyboard/reduced-motion/window-size review, and the full media/edge-case matrix. Keep unchecked plan steps as outstanding work.

**Goal:** Add local video compression to Squeezer’s mixed-media workspace for storage reduction and share-size limits, with per-file control and trustworthy preview, progress, and results.

**Architecture:** Keep the current Rust job manager and Tauri event flow, but give video analysis, options, and encoding their own module and type. First run a bounded encoder feasibility spike; only integrate a backend that passes the quality, control, cancellation, and distribution criteria. Share queue interactions with images while preserving separate image and video settings.

**Tech Stack:** Tauri 2, Rust, React/TypeScript, macOS AVFoundation/VideoToolbox Swift helper, Node’s built-in test runner, Cargo tests.

**Spec:** `docs/superpowers/specs/2026-10-20-squeezer-video-compression-design.md`

## Global Constraints

- Accept `.mp4`, `.mov`, and `.m4v` initially; inspect real tracks and codec compatibility after extension filtering.
- First release targets ordinary SDR video; reject unsupported HDR, Dolby Vision, spatial/multiview, ProRes, subtitles, chapters, and alternate/multiple audio tracks with a clear reason.
- Preserve source resolution, orientation, frame rate, and primary audio by default; offer explicit 1080p/720p downscale options only after validating output behavior.
- Create a new `.squeezed.mp4` beside the source; resolve collisions incrementally and never overwrite the source or an existing output.
- Do not reuse the image 512 MiB input limit for video.
- Do not load full videos into base64 or grant the WebView broad home-directory access; stream preview only from exact authorized source/output paths.
- Do not report estimated bytes as actual output or claim a preset guarantees a fixed savings percentage.
- Keep image behavior and settings unchanged; video batch defaults and per-file overrides remain independent.

## Review Focus

- Renamed or corrupt files with video extensions must fail with a useful error while valid siblings are still admitted (Tasks 2–3).
- Long and large videos must not be rejected by the image byte limit or loaded fully into memory (Tasks 2–4).
- Cancellation during export must stop work, remove temporary output, and leave input and existing output files untouched (Task 4).
- Target-size requests that cannot be met must show an honest result and never claim the requested size was achieved (Tasks 1 and 4).
- Mixed image/video batches, per-file overrides, and streaming previews must preserve existing image behavior and enforce narrow file access (Tasks 2, 3, and 5).

---

### Task 1: Encoder feasibility spike and decision record

**Files:**
- Create: `tools/video-feasibility/README.md`
- Create: `tools/video-feasibility/fixtures.swift`
- Create: `tools/video-feasibility/avfoundation-export.swift`
- Create: `tools/video-feasibility/avfoundation-reader-writer.swift`
- Create: `tools/video-feasibility/run-ffmpeg.sh`
- Create: `docs/research/2026-10-20-video-encoder-feasibility.md`
- Read: `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`

**Interfaces:**
- Produces: comparison report; selected backend is AVAssetReader/Writer + VideoToolbox H.264 output.

- [x] **Step 1: Generate a deterministic fixture matrix with `fixtures.swift`** — emit named short/long clips covering H.264/HEVC, silent/AAC, portrait/orientation, 720p/1080p/4K and low/high motion; document generation commands and metadata in `README.md`.
- [x] **Step 2: Define decision criteria in `README.md`** — record output bytes, duration, playback/track validation, cancellation, resolution/audio/orientation preservation, target-size accuracy, and packaging/license burden.
- [x] **Step 3: Add separate standalone Swift harnesses for `AVAssetExportSession` and AVAssetReader/Writer + VideoToolbox** — query preset/output compatibility where applicable and record actual output metadata and bytes; neither harness is called by the app.
- [x] **Step 4: Add `run-ffmpeg.sh` for an explicitly configured local FFmpeg build** — reject unknown build configurations and record version, configure flags, encoders, output metadata, and license notices; do not substitute third-party prebuilt binaries.
- [x] **Step 5: Run each harness against the generated matrix** — record unavailable routes as unavailable with host/macOS or build reason; include measured cancellation and target-size miss behavior.
- [x] **Step 6: Write the measured results and select or reject a backend in `docs/research/2026-10-20-video-encoder-feasibility.md`** — if no route satisfies video quality, target-size behavior, output validation, and distribution constraints, stop and revise the spec before integration.
- [x] **Step 7: Review the spike artifacts**

Run: `swift tools/video-feasibility/fixtures.swift --help`
Expected: usage and fixture-generation options are printed without generating files.

Run: `git diff --check`
Expected: no whitespace errors.

### Task 2: Shared media job model and video intake

**Files:**
- Create: `src-tauri/src/engine/jobs.rs`
- Modify: `src-tauri/src/engine/mod.rs`
- Modify: `src-tauri/src/engine/images.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src/types.ts`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/commands.rs` unit tests
- Test: `tests/interactions.test.ts`

**Interfaces:**
- Produces: common `MediaType`, `JobStatus`, and serializable `CompressionJob`; video metadata adds `durationMs` and source track/codec summaries without changing image requirements.
- Produces: matching Rust/TypeScript image and video option payloads passed to `start_jobs` in camelCase; image settings continue using the existing `CompressionOptions` shape.

- [x] **Step 1: Add admission tests** — verify MP4/MOV/M4V are admitted as video, image extensions remain images, unsupported extensions fail with a specific message, duplicates are ignored, and a valid item in the same batch remains queued.
- [x] **Step 2: Generalize the job model** — move shared job/status types from `engine/images.rs` to `engine/jobs.rs`; change `mediaType` to the `image | video` enum and `extension` to a string in TypeScript; retain image-only fields and behavior.
- [x] **Step 3: Add video options and request types** — add H.264 output quality preset, `maxDimension` (original/1080/720), optional `targetBytes`, and batch/per-file settings. HEVC output was deferred based on Task 1 measurements; target size uses a bounded bitrate correction and reports misses honestly.
- [x] **Step 4: Extend native file selection and path admission** — add video filter extensions, inspect extension before analysis, and route video metadata analysis to the video module without applying the image 512 MiB limit.
- [x] **Step 5: Implement the admission/model tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml admission`
Expected: video/image routing, unsupported and malformed input, duplicate handling, and mixed-batch behavior pass.

Run: `pnpm test`
Expected: existing queue interaction tests pass with both media types represented.

### Task 3: Video analysis, thumbnails, and secure streaming preview

**Files:**
- Create: `src-tauri/src/engine/video_analysis.rs`
- Modify: `src-tauri/src/engine/mod.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src/lib/tauri.ts`
- Test: `src-tauri/src/engine/video_analysis.rs` unit tests

**Interfaces:**
- Produces: `analyze_video(path: &Path) -> Result<VideoInfo, VideoError>` with duration, dimensions, orientation, codec, audio-track summary, and bounded poster thumbnail.
- Produces: a video preview source for exact job input/output paths through a streaming protocol; callers cannot pass arbitrary filesystem paths from the frontend.

- [ ] **Step 1: Add video-analysis tests** — assert duration, dimensions, rotation, audio presence, codec, and thumbnail for supported fixtures; assert clear rejection for unsupported/corrupt fixtures.
- [x] **Step 2: Implement metadata analysis and bounded thumbnail extraction** — use the selected platform/media API; do not decode a full-duration video into memory.
- [ ] **Step 3: Add corrupt/renamed/unsupported-track tests** — files with a supported extension but invalid content or unhandled tracks fail analysis while valid siblings remain in the queue.
- [x] **Step 4: Add authorized preview-source command** — accept job ID and before/after selector, resolve the path only from `JobManager`, and return a streamable URL/source for the native player.
- [x] **Step 5: Configure Tauri asset protocol narrowly** — enable `protocol-asset`, seed an empty allow scope, allow only the selected job’s exact source/output files at runtime, and revoke them when the job is removed or cleared; do not use a home-directory wildcard. Add the minimum `media-src` CSP entry required by the asset URL.
- [ ] **Step 6: Verify preview authorization and streaming** — tests reject unknown IDs, missing outputs, and arbitrary frontend paths; verify removed jobs lose protocol access; manual check confirms seeking through a long clip without full-file buffering.

Run: `cargo test --manifest-path src-tauri/Cargo.toml video_analysis`
Expected: supported metadata, thumbnail bounds, and unsupported-input cases pass.

### Task 4: Video encoder integration and job lifecycle

**Files:**
- Create: `src-tauri/src/engine/videos.rs`
- Create: backend-specific files chosen in Task 1 (`src-tauri/video-helper/` for Swift, or pinned sidecar config and notice files for FFmpeg)
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/Cargo.toml` only if the selected backend requires a dependency
- Test: `src-tauri/src/engine/videos.rs` unit/integration tests
- Test: `src-tauri/src/commands.rs` unit tests

**Interfaces:**
- Consumes: `CompressionJob`, `VideoCompressionOptions`, and `CancellationToken` from Tasks 2–3.
- Produces: `compress_video(job: &CompressionJob, options: &VideoCompressionOptions, cancel: &CancellationToken, on_validating: impl FnOnce()) -> Result<CompressionResult, VideoError>`.
- Produces: verified output metadata and actual bytes; requested target size is recorded as achieved only if measured output bytes are at or below the requested target.

- [ ] **Step 1: Add lifecycle tests before encoding** — cover collision-safe `.squeezed.mp4` output, temporary-file cleanup on cancel/failure, input preservation, existing-output preservation, actual size reporting, and target-size miss reporting.
- [x] **Step 2: Integrate only the backend selected by Task 1** — keep encoder calls behind `compress_video`; pass validated options as structured arguments, never shell-interpolated strings.
- [x] **Step 3: Implement media-aware start, progress, cancellation, and retry** — retain serial queue order, emit real analysis/encoding/validation states, stop child/session on cancellation, and re-analyze source on retry.
- [x] **Step 4: Validate every temporary output before commit** — reopen it with the selected media API, verify expected video/audio tracks, dimensions/orientation and playable duration, apply the 10% savings skip policy, then atomically move it without overwriting.
- [ ] **Step 5: Run backend and lifecycle verification**

Run: `cargo test --manifest-path src-tauri/Cargo.toml video`
Expected: supported fixture encodes validate; cancellation leaves no temp file; source and pre-existing output bytes remain unchanged.

### Task 5: Video controls, mixed queue, and per-file overrides

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/types.ts`
- Modify: `src/lib/tauri.ts`
- Modify: `src/lib/interactions.ts`
- Modify: `src/styles.css`
- Test: `tests/interactions.test.ts`

**Interfaces:**
- Consumes: `StartJobsRequest`, video job fields, and video preview source from Tasks 2–4.
- Produces: independent image/video batch controls; a selected-video inspector with codec, resolution, target size where supported, inheritance/reset actions, and clearly labeled estimated output size if the chosen backend provides a defensible estimate.

- [ ] **Step 1: Add UI-state tests** — cover image/video settings isolation, per-file video override and reset, target-size validation, mixed queue filtering/selection, and display labels distinguishing estimate from actual result.
- [x] **Step 2: Implement video-specific batch settings** — add H.264 compatibility default, HEVC only if Task 1 validates it, original/1080p/720p resolution selection, and target-size input only if feasibility approves it.
- [x] **Step 3: Implement per-file override flow** — inherit batch defaults by default, mark overridden rows, reset to batch, and lock controls once processing starts.
- [x] **Step 4: Generalize queue labels and feedback** — show video poster, duration, dimensions, size, state, actual savings, error, cancel, retry, and progress without changing image labels or controls.
- [ ] **Step 5: Run frontend checks**

Run: `pnpm test`
Expected: all existing image interactions and new mixed-media settings tests pass.

Run: `pnpm build`
Expected: TypeScript checks and Vite production build complete successfully.

### Task 6: Video comparison player and interaction polish

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/styles.css`
- Modify: `src/lib/tauri.ts`
- Test: manual desktop interaction checklist recorded in `docs/superpowers/plans/2026-10-20-video-compression.md`

**Interfaces:**
- Consumes: the secure preview source and actual output metadata from Tasks 3–4.
- Produces: one comparison player that switches original/output while preserving playhead when possible; only one source plays audio at a time.

- [ ] **Step 1: Add preview-state checks** — verify unknown/missing output states, source switching, closing, and focus restoration in the existing preview flow where unit coverage is available.
- [x] **Step 2: Add native-feeling video controls** — explicit play/pause, scrubber, current source label, duration and byte size; video never autoplays in queue rows.
- [ ] **Step 3: Add keyboard and reduced-motion behavior** — Escape closes, focus returns to the opener, accessible names/status are present, and transitions respect `prefers-reduced-motion`.
- [ ] **Step 4: Check the window at 800×600 and 1120×780** — ensure queue, controls, inspector, preview, and primary action remain readable and reachable.

Manual verification:
- Load a long source and completed output; seek near the end, switch source, and confirm playback position is retained as closely as the media API permits.
- Confirm switching versions pauses the previous source and no queue thumbnail autoplays.
- Confirm keyboard-only open, playback, Escape close, and focus restoration.

### Task 7: End-to-end media matrix and release review

**Files:**
- Modify: `docs/superpowers/plans/2026-10-20-video-compression.md` only to record executed checklist results
- Test: generated/local media fixtures described by Task 1

- [ ] **Step 1: Run the complete native suite**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: all Rust unit/integration tests pass.

Run: `pnpm test && pnpm build`
Expected: all frontend tests, type checks, and production build pass.

- [ ] **Step 2: Exercise the media matrix** — supported H.264/HEVC, silent/AAC, portrait, SD/HD/4K, long clip; malformed/renamed/unsupported/HDR/multitrack; duplicate and mixed image/video batch; cancel/retry; target-size hit/miss; collision and Finder reveal.
- [ ] **Step 3: Record actual bytes, resolution, duration, audio/orientation results, elapsed time, and selected encoder for each fixture** — compare quality with source at representative motion and detail; never infer success from process exit alone.
- [ ] **Step 4: Review distribution/security** — inspect Tauri protocol scope, packaged backend, third-party notices/source/build configuration if FFmpeg was selected, and ensure no arbitrary command execution path exists.
- [ ] **Step 5: Confirm spec coverage** — link each acceptance criterion to a test or recorded manual check and list any unsupported cases with their user-visible reason.
