# Squeezer Image Compression MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a local macOS desktop app with multi-file queue, cancellable safe jobs, JPEG compression, and lossless PNG optimization.

**Architecture:** React and TypeScript provide drag-and-drop, queue state, and progress display. Tauri 2 commands call Rust for analysis, scheduling, external encoder invocation, validation, and safe output placement. Encoder processes receive argument arrays; outputs use temporary sibling files and never overwrite source files.

**Tech Stack:** Tauri 2, React, TypeScript, Rust; local image encoders (MozJPEG and oxipng or equivalent verified tools).

**Spec:** `squeezer-spec.md`, Sections 2–4, 6–10, and 12.

## Global Constraints

- Local-first, offline processing; do not upload media.
- Never overwrite the original; validate temporary output before moving it into place.
- Support JPEG and PNG first; default output format is original.
- Lossless mode must not quantize PNG colors or perform lossy JPEG transcoding.
- Use safe process argument passing; never construct shell command strings from file paths.
- Rust owns job state transitions and emits progress/events to React.
- Keep the implementation scoped to Phase 1 and JPEG/PNG MVP; defer video, WebP, HEIC, pause/resume, and notarization.

## Review Focus

- Source and output paths with spaces, Unicode, and shell metacharacters must remain literal process arguments; test a non-ASCII fixture path.
- Existing output names and input/output identity must never overwrite an original; test collision handling.
- Cancellation during encoding must remove temporary output and preserve the source; test cancellation cleanup.
- Unsupported, malformed, or truncated media must fail with a per-job error instead of crashing the queue; test malformed image input.
- PNG alpha and dimensions must survive lossless optimization; test transparent PNG validation.

---

### Task 1: Tauri + React Application Foundation

**Files:**
- Create: `package.json`, `index.html`, `vite.config.ts`, `tsconfig.json`, `src/main.tsx`, `src/App.tsx`, `src/styles.css`
- Create: `src-tauri/Cargo.toml`, `src-tauri/build.rs`, `src-tauri/tauri.conf.json`, `src-tauri/src/lib.rs`, `src-tauri/src/main.rs`, `src-tauri/capabilities/default.json`
- Test: `src-tauri/src/lib.rs` unit tests for path validation and collision-free output naming

**Interfaces:**
- Produces: Tauri v2 app shell with React frontend and Rust command registration.

- [ ] **Step 1: Scaffold the minimal app and declare dependencies**
- [ ] **Step 2: Build the frontend and compile the Rust/Tauri application**
- [ ] **Step 3: Add Rust tests for same-path rejection and non-overwriting output naming**
- [ ] **Step 4: Run `cargo test --manifest-path src-tauri/Cargo.toml` and `pnpm build`**

### Task 2: Rust Media Analysis and Safe Output Paths

**Files:**
- Create: `src-tauri/src/engine/mod.rs`, `src-tauri/src/engine/analyzer.rs`, `src-tauri/src/engine/paths.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/engine/analyzer.rs`, `src-tauri/src/engine/paths.rs`

**Interfaces:**
- Produces: `analyze_media(path: &Path) -> Result<MediaInfo, AppError>` and `output_path_for(input: &Path, output_dir: &Path) -> Result<PathBuf, AppError>`.

- [ ] **Step 1: Test image metadata extraction for JPEG/PNG fixtures and malformed media rejection**
- [ ] **Step 2: Implement supported-format detection and dimensions/byte-size reporting**
- [ ] **Step 3: Test output collision resolution and source/output identity rejection**
- [ ] **Step 4: Implement deterministic non-overwriting output naming**
- [ ] **Step 5: Run targeted Rust tests**

### Task 3: Queue, Cancellation, Encoder Invocation, and Validation

**Files:**
- Create: `src-tauri/src/engine/jobs.rs`, `src-tauri/src/engine/encoders.rs`, `src-tauri/src/engine/validator.rs`, `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`
- Test: `src-tauri/src/engine/jobs.rs`, `src-tauri/src/engine/encoders.rs`, `src-tauri/src/engine/validator.rs`

**Interfaces:**
- Produces: `CompressionMode`, `JobStatus`, `CompressionOptions`, `CompressionJob`; Tauri commands `add_files`, `start_jobs`, and `cancel_job`; events `job_updated`.

- [ ] **Step 1: Test legal state transitions and cancellation cleanup**
- [ ] **Step 2: Test encoder argument construction with Unicode paths and per-format options**
- [ ] **Step 3: Implement separate process launch with no shell interpolation and temporary sibling outputs**
- [ ] **Step 4: Test output decode, dimensions, alpha, and minimum-savings skip behavior**
- [ ] **Step 5: Implement validation before atomic destination placement**
- [ ] **Step 6: Run all Rust tests**

### Task 4: Image Compression Commands

**Files:**
- Modify: `src-tauri/src/engine/encoders.rs`, `src-tauri/src/engine/validator.rs`, `src-tauri/src/commands.rs`
- Create: `src-tauri/src/engine/images.rs`
- Test: `src-tauri/src/engine/images.rs`

**Interfaces:**
- Consumes: analysis, output-path, job-state, cancellation, and validator APIs from prior tasks.
- Produces: `compress_image(job: &CompressionJob, options: &CompressionOptions, cancel: &CancellationToken) -> Result<JobResult, AppError>`.

- [ ] **Step 1: Test PNG lossless mode preserving dimensions/alpha and refusing lossy quantization**
- [ ] **Step 2: Implement PNG optimization through oxipng when available, with a clear actionable encoder error otherwise**
- [ ] **Step 3: Test JPEG quality-mode encoding and lossless-mode behavior**
- [ ] **Step 4: Implement JPEG quality encoding through MozJPEG when available; keep lossless mode as skip/no-reencode**
- [ ] **Step 5: Test insufficient savings results in `skipped` and removal of temporary output**
- [ ] **Step 6: Run all Rust tests**

### Task 5: React Dropzone, Queue, Progress, and Results UI

**Files:**
- Create: `src/types.ts`, `src/lib/tauri.ts`, `src/components/Dropzone.tsx`, `src/components/JobList.tsx`, `src/components/JobRow.tsx`, `src/components/ModeSelector.tsx`
- Modify: `src/App.tsx`, `src/styles.css`
- Test: `src/components/JobList.test.tsx`, `src/lib/tauri.test.ts`

**Interfaces:**
- Consumes: Tauri commands and `job_updated` events.
- Produces: drag/drop and picker intake, four mode choices, per-job progress/status/results, cancellation, and batch start UI.

- [ ] **Step 1: Test UI renders added jobs, status, progress, and errors**
- [ ] **Step 2: Implement drag/drop and file picker with the same file admission path**
- [ ] **Step 3: Test mode selection and batch/cancel command calls**
- [ ] **Step 4: Implement queue actions, event subscription, results, and concise errors**
- [ ] **Step 5: Build frontend and run UI tests**

### Task 6: Local Run and Usage Documentation

**Files:**
- Create: `README.md`
- Modify: `package.json`, `src-tauri/tauri.conf.json`

- [ ] **Step 1: Document prerequisites, local run/build commands, supported formats, encoder requirements, and known limits**
- [ ] **Step 2: Run the full frontend build and Rust test suite; record macOS-specific launch limitations if encountered**
