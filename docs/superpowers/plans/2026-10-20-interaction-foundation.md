# Squeezer Interaction Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Polish Squeezer’s existing image-compression flow with immediate, truthful, accessible interaction feedback while keeping the current one-screen batch workflow.

**Architecture:** Keep the current React/Tauri structure and make focused updates to `App.tsx` and `styles.css`. Make file admission return visible pending jobs before sequential background analysis completes, add a small interaction-copy helper module with Node built-in tests, and add one Rust command that reveals completed job outputs in Finder by job IDs. Do not introduce a new UI framework, animation dependency, forced wizard, or image-workbench redesign in this phase.

**Tech Stack:** React 19, TypeScript 6, Vite 8, Tauri 2, Rust 2024, Node’s built-in test runner, existing `image` and `tokio-util` dependencies.

**Spec:** `docs/superpowers/specs/2026-10-20-squeezer-interaction-design.md`, Phase 1 scope and acceptance.

## Global Constraints

- Keep image processing local; do not introduce network calls or upload paths.
- Never overwrite or delete source files.
- Keep image decode analysis off the interactive Tauri command path; return queue rows first and analyze sequentially in the background.
- Do not display fabricated progress, compression estimates, or ETA.
- Keep the current single-screen batch workflow; do not force users through onboarding or a multi-page wizard.
- Respect `prefers-reduced-motion`; interaction must remain clear without animation.
- Keep Rust as the authority for job state and output paths; expose Finder reveal by job ID, never by an arbitrary path argument.
- Do not add animation or UI dependencies for these changes.
- The workspace currently has no `.git` directory. Do not add commit steps; report this if version-control-based review is requested.
- A prior debug `.app` bundle built successfully, but LaunchServices failed to open it in the current host (`kLSNoExecutableErr`). If interactive launch still fails, retain build/test evidence and report the environment limitation instead of making speculative launch changes.

## Review Focus

- A dropped invalid file must not prevent valid files in the same drop from entering the queue; test that the single admission batch represents valid paths and records invalid paths as failed jobs.
- Repeated start clicks must not launch a duplicate batch; test the derived disabled/busy state and verify one command invocation in the manual interaction pass.
- Cancelling a job must be announced as cancellation, not failure; test the generated status announcement.
- Validation must not look like 80% of a measurable process when the backend only knows its current phase; test the indeterminate state and retain phase text.
- Finder reveal must only use a completed job's registered output and must reject missing or non-terminal output paths; test these command preconditions.

---

### Task 1: Immediate file intake with background analysis

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src/App.tsx`
- Test: `src-tauri/src/commands.rs`

**Interfaces:**
- Consumes: `analyze_media(path: &Path) -> Result<MediaInfo, AnalyzeError>` and existing `job_updated` events.
- Produces: a fast `add_files(paths: Vec<String>) -> Vec<CompressionJob>` admission step that returns displayable jobs in `Analyzing` status; a sequential background analyzer updates each job to `Queued` or `Failed` and emits `job_updated`.
- The manager owns a FIFO analysis-ID queue and a single-worker flag so separate drops still share one decoder and stay within bounded memory.
- Produces: `fn claim_analysis_worker(state: &mut JobState) -> bool`, which returns `true` only when queued analysis work exists and no worker is running. The worker clears the flag under the same lock when the queue is empty.
- Produces: `fn admit_paths(paths: Vec<String>, known_paths: &mut HashSet<String>) -> Vec<CompressionJob>`, `fn admit_file(input_path: String) -> CompressionJob`, `fn pending_job(input_path: String, original_bytes: u64) -> CompressionJob`, and `fn apply_analysis(job: &mut CompressionJob, result: Result<MediaInfo, String>)` for explicit, testable state changes. Unsupported extensions or unreadable paths become immediately failed jobs with their specific admission error.

- [x] **Step 1: Add tests for pending and analyzed job states**

Test that a valid supported file is immediately represented with filename, readable file size, and `Analyzing` status; unsupported/missing paths immediately become failed jobs; successful analysis fills extension/dimensions and changes status to `Queued`; failed image decode changes only that job to `Failed` with a useful error; and a worker claim cannot be acquired twice while active. Test `admit_paths` with valid and invalid siblings plus duplicate paths: preserve both valid/invalid rows and add only one row for a duplicate input.

- [x] **Step 2: Run the focused Rust tests and confirm they fail**

Run: `cargo test --offline --manifest-path src-tauri/Cargo.toml commands::tests::admit_paths_keeps_valid_and_invalid_siblings`, `cargo test --offline --manifest-path src-tauri/Cargo.toml commands::tests::analysis_result_updates_one_job`, and `cargo test --offline --manifest-path src-tauri/Cargo.toml commands::tests::analysis_worker_is_singleton_across_batches`.
Expected: FAIL until pending-job construction and analysis application are implemented.

- [x] **Step 3: Return pending jobs before image decode**

In `add_files`, build the known-path set from `JobManager`, call `admit_paths`, insert each resulting job, and return without calling `analyze_media`. Push valid pending IDs into a manager-owned FIFO queue. Start a worker only if none is active; it drains the queue sequentially to bound concurrent memory. Apply each result under the manager lock only if the job still exists, remains `Analyzing`, and was not cancelled; emit the update after releasing the lock. When the queue empties, clear the worker flag under the same lock so a concurrent drop either sees the active worker or starts a new one.

- [x] **Step 4: Handle cancellation and queue actions during analysis**

Allow cancelling a pending analysis job by marking it `Cancelled` and cancelling its token. A worker that finishes decoding after cancellation or removal must not resurrect the job. Keep `start_jobs` restricted to `Queued` jobs; the UI already disables batch start while any job is analyzing.

- [x] **Step 5: Show progressive intake in the existing queue**

Render the immediately returned job rows as `分析中` with an indeterminate indicator, then update the same row from the emitted event. Do not wait for all selected files to decode before showing the first row or allowing safe queue navigation.

- [x] **Step 6: Run focused Rust tests**

Run: `cargo test --offline --manifest-path src-tauri/Cargo.toml commands::tests::admit_paths_keeps_valid_and_invalid_siblings`, `cargo test --offline --manifest-path src-tauri/Cargo.toml commands::tests::analysis_result_updates_one_job`, and `cargo test --offline --manifest-path src-tauri/Cargo.toml commands::tests::analysis_worker_is_singleton_across_batches`.
Expected: all three focused tests pass.

---

### Task 2: Accessible action copy and immediate interaction states

**Files:**
- Create: `src/lib/interactions.ts`
- Create: `tests/interactions.test.ts`
- Modify: `src/App.tsx`
- Modify: `src/styles.css`

**Interfaces:**
- Consumes: `CompressionJob`, `JobStatus`, `CompressionMode` from `src/types.ts`.
- Produces: `getPrimaryActionState(queuedCount: number, active: boolean, busy: boolean): { label: string; disabled: boolean }`, `getNextCompressionMode(current: CompressionMode, key: "ArrowLeft" | "ArrowRight"): CompressionMode`, `getJobActionLabel(job: CompressionJob, cancelRequested: boolean): string`, `getResultActionLabel(outputCount: number): string`, `getResultCounts(jobs: CompressionJob[]): { completed: number; skipped: number; failed: number; cancelled: number }`, `getRevealableJobIds(jobs: CompressionJob[]): string[]`, and `getStatusAnnouncement(job: CompressionJob): string`. The reveal grouping helper returns all completed output IDs only when at least one output exists and every completed output is in one directory; otherwise it returns an empty list. The UI shows “顯示全部” only when that list has more than one ID; single-file reveal remains per-row.

- [x] **Step 1: Add Node tests for action and announcement copy**

Test queued count singular/plural, the disabled/busy state, left/right quality-mode cycling including wrap-around, active-job cancel label vs terminal-job remove label, cancel-requested state copy, single vs batch result action copy, same-folder Finder grouping vs mixed-folder grouping, counts for each terminal result, and terminal announcements for completed, skipped, failed, and cancelled jobs. Ensure filenames are included where the action is per-file.

- [x] **Step 2: Run the focused tests and confirm they fail**

Run: `node --experimental-strip-types --test tests/interactions.test.ts`
Expected: FAIL because the interaction helpers do not exist yet.

- [x] **Step 3: Implement the pure interaction helpers**

Add the seven functions in `src/lib/interactions.ts`. Use Traditional Chinese copy consistent with the existing UI. Do not put React state or Tauri calls in this module.

- [x] **Step 4: Wire accessible labels and live feedback into `src/App.tsx`**

Use the helpers for primary action state, result action, row action accessible names, terminal result counts, and same-folder Finder grouping. Give icon-only controls a visible tooltip (`title`) and a descriptive `aria-label`. Add a compact polite live region for discrete events (files added, batch started, terminal job state); do not announce every progress update. Make the drop area keyboard reachable and let Enter/Space open the picker without causing nested button semantics. Make the quality mode cards a labelled group with roving focus; Left/Right cycles modes and wrap-around, while Enter/Space selects.

- [x] **Step 5: Add visible hover, focus, pressed, disabled, and busy states**

In `src/styles.css`, provide distinct focus and pressed styles for all interactive controls; maintain a visible disabled state; add restrained transitions only to state-changing controls. Preserve existing focus-visible outline and reduced-motion rule. Do not add continuous ambient animation.

- [x] **Step 6: Run focused and full frontend checks**

Run: `node --experimental-strip-types --test tests/interactions.test.ts` and `pnpm test`.
Expected: all interaction helper and existing frontend tests pass.

---

### Task 3: Truthful processing and cancellation feedback

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src/App.tsx`
- Modify: `src/styles.css`
- Modify: `src/lib/interactions.ts`
- Test: `src-tauri/src/commands.rs`
- Test: `tests/interactions.test.ts`

**Interfaces:**
- Consumes: current `CompressionJob` event payload and `JobStatus` state model.
- Produces: phase-only indeterminate presentation while the encoder/validator cannot report byte or time progress; a discrete polite announcement when the batch starts and when a job reaches a terminal state.
- Produces: `fn progress_for_status(status: JobStatus) -> u8`, returning 100 only for `Completed` and 0 for all other statuses.
- Produces: a local cancel-request marker in React state. After `cancel_job` acknowledges the request, the row says “正在安全取消…” until the terminal `Cancelled` event arrives; a failed request clears the marker and shows an inline error.

- [x] **Step 1: Add a Rust regression test for phase-only progress**

Test that `Queued`, `Analyzing`, `Compressing`, `Validating`, `Skipped`, `Failed`, and `Cancelled` do not receive a fabricated percentage, and only `Completed` has 100% terminal progress. Test that a cancel-requested active job shows the cancellation copy and that a terminal cancel is announced as cancelled.

- [x] **Step 2: Run the focused Rust test and confirm it fails**

Run: `cargo test --offline --manifest-path src-tauri/Cargo.toml commands::tests::progress_is_only_reported_when_known`
Expected: FAIL until progress assignment is factored into and wired through a tested status-to-progress helper.

- [x] **Step 3: Remove the hard-coded validation percentage**

Keep progress at 0 for the `Validating` stage; use the existing `status` field to show “驗證中” and an indeterminate visual indicator. Keep 100 only for `Completed`; keep `Skipped`, `Failed`, and `Cancelled` at 0.

- [x] **Step 4: Make row progress accessible without noisy announcements**

Render indeterminate progress with `role="progressbar"`, an accessible label containing the filename and phase, and omit `aria-valuenow` when no numeric progress exists. After an acknowledged cancel request, show “正在安全取消…” without claiming the job is already cancelled; clear the local request marker when the terminal event arrives. Keep the live region at discrete state changes rather than the whole frequently changing job table.

- [x] **Step 5: Run focused Rust and frontend tests**

Run: `cargo test --offline --manifest-path src-tauri/Cargo.toml commands::tests::progress_is_only_reported_when_known` and `pnpm test`.
Expected: both commands pass.

---

### Task 4: Focused preview and completion interaction

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/styles.css`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/tauri.ts`
- Test: `src-tauri/src/commands.rs`
- Test: `tests/interactions.test.ts`

**Interfaces:**
- Produces: Tauri command `reveal_outputs(ids: Vec<String>) -> Result<(), String>`; frontend wrapper `revealOutputs(ids: string[]): Promise<void>`.
- The command resolves output paths from `JobManager` entries, requires every requested job to be terminal `Completed` with an existing registered output file, and requires all output files to share a parent directory for the batch action. It invokes `/usr/bin/open -R <paths...>` with separate arguments and does not accept paths from the frontend.
- Produces: `fn collect_revealable_outputs(jobs: &HashMap<String, CompressionJob>, ids: &[String]) -> Result<Vec<PathBuf>, String>` to validate the requested jobs before any external process starts. Result copy consumes `getResultActionLabel(outputCount)` from Task 2.

- [x] **Step 1: Add tests for Finder reveal preconditions and result action labels**

Test that queued, failed, skipped, and cancelled jobs cannot be revealed; completed jobs without existing output files are rejected; one completed output and a set of completed outputs in one directory are accepted; mixed or cross-directory batches are rejected. Test that the completion action copy distinguishes one output from a batch.

- [x] **Step 2: Run the focused tests and confirm they fail**

Run: `cargo test --offline --manifest-path src-tauri/Cargo.toml commands::tests::reveal_outputs_require_completed_files` and `pnpm test`.
Expected: the new Rust test fails before command implementation; the Node test fails before result-copy helper implementation.

- [x] **Step 3: Implement the Rust reveal command and register it**

Look up all IDs while holding the manager lock; reject unknown IDs, non-completed jobs, missing paths, missing files, empty lists, and outputs in different directories. Copy the validated paths and release the lock before launching `/usr/bin/open` once with `-R` followed by the exact output paths as arguments. Surface launch errors as an actionable string. Register the command in `src-tauri/src/lib.rs`.

- [x] **Step 4: Add the TypeScript wrapper and result controls**

Add `revealOutputs` in `src/lib/tauri.ts`. In `src/App.tsx`, show a per-row action only for completed outputs; expose a one-call “顯示全部” action only when `getRevealableJobIds` returns more than one completed output in one directory. Show per-status counts and actual saved bytes when the batch reaches a terminal state; do not show positive celebration copy when nothing was saved. Show an inline error if Finder cannot open. Do not add a control that claims to reveal an output for skipped/failed jobs.

- [x] **Step 5: Give preview open/close and result updates restrained transitions**

Use short opacity/scale transitions for the existing preview overlay and modal only. Preserve the existing Escape handling, focus trap, and focus restoration; ensure the close control has a visible focus state and the comparison image labels include filename and size. Under reduced motion, remove scale/slide motion and keep a direct opacity or immediate state change.

- [x] **Step 6: Run focused Rust and frontend tests**

Run: `cargo test --offline --manifest-path src-tauri/Cargo.toml commands::tests::reveal_outputs_require_completed_files` and `pnpm test`.
Expected: all focused tests pass.

---

### Task 5: Integration and interaction acceptance

**Files:**
- Modify: `README.md` only if the existing behavior description changes.

- [x] **Step 1: Run the full offline Rust test suite**

Run: `cargo test --offline --manifest-path src-tauri/Cargo.toml`.
Expected: all Rust tests pass with no ignored failures.

- [x] **Step 2: Run the frontend test and production build**

Run: `pnpm test && pnpm build`.
Expected: all Node tests pass and TypeScript/Vite production build succeeds.

- [x] **Step 3: Run Rust type checking and Tauri app bundle build**

Run: `cargo check --offline --manifest-path src-tauri/Cargo.toml` and `pnpm tauri build --debug --bundles app`.
Expected: Rust check and debug `.app` bundle complete successfully.

- [ ] **Step 4: Complete a manual interaction pass in the running app**

Verify mouse and keyboard paths for empty-state import, drag enter/drop/leave, quality selection, batch start and duplicate-click blocking, cancellation acknowledgement and terminal state, preview open/close focus restoration, reduced-motion behavior, window resizing without clipped controls, terminal result reveal in Finder, and mixed valid/invalid files. Confirm that no false ETA or percentage is shown and errors do not clear other jobs.

---
