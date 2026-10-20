# Squeezer Native Workspace Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the page-like image-compression screen with a readable macOS batch workspace, including queue search and per-file compression overrides.

**Architecture:** Keep the existing React/Tauri screen and Rust job manager. Reshape the React view around the queue, selected-file inspector, global settings, and fixed primary action; store per-file overrides in the frontend and pass them with the batch command so Rust uses the selected options for each queued job. Keep the existing backend job events and real output previews as the source of truth.

**Tech Stack:** React 19, TypeScript 6, Tauri 2, Rust, Node's built-in test runner, existing CSS.

**Spec:** `docs/superpowers/specs/2026-10-20-squeezer-native-workspace-design.md`

## Global Constraints

- Display the queue, thumbnails, and file states as the primary workspace; do not retain a promotional hero or fixed format-category sidebar.
- Search case-insensitively by filename and source-folder name. Filtering only hides visible rows; it never removes or changes jobs.
- Batch quality changes apply to queued jobs without an explicit override; a per-file override is identifiable and can be reset to the batch setting.
- Per-file settings are locked after that job starts; batch settings can still be changed for the next queued batch.
- Use only confirmed backend status and measured output bytes; never fabricate progress, ETA, output previews, or savings.
- Preserve source files, local-only processing, queue order, selected compression settings, and interaction focus across asynchronous job updates.
- Use Traditional Chinese for product controls and statuses; use the agreed 1–2 second non-blocking launch motion and honor `prefers-reduced-motion`.
- Keep the current minimum Tauri window size of 800 × 600; do not shrink typography below the spec minimums to fit the layout.
- Do not add new dependencies or expand into video, format conversion, history, sync, or presets.

## Review Focus

- **A search hides an active job:** the Rust job continues unchanged; clearing search reveals its latest status. Pin filtering semantics with `tests/interactions.test.ts` test `queue search filters names and parent folders without mutating jobs` (Task 1).
- **An override belongs to one queued file:** the Rust encoder receives that file's options while sibling jobs use the batch default. Pin option resolution with `src-tauri/src/commands.rs` tests `queued_job_uses_override_when_present` and `queued_job_falls_back_to_batch_options` (Task 3).
- **A file lacks a thumbnail or analysis fails:** the row remains readable and actionable without shifting the other rows. Verify with mixed valid/invalid imports during manual acceptance (Task 5).
- **The user changes search or selection while job events arrive:** updates must not reset the query, selection of a still-visible row, or list order. Verify while analysis and compression events are received (Task 5).
- **The app is resized to 800 × 600 or reduced motion is enabled:** the main action and essential row data remain reachable, and state feedback remains visible without movement. Verify both cases in the macOS app (Task 5).

---

### Task 1: Queue search and option-resolution helpers

**Files:**
- Modify: `src/lib/interactions.ts`
- Modify: `tests/interactions.test.ts`

**Interfaces:**
- Produces: `filterQueueJobs(jobs: CompressionJob[], query: string): CompressionJob[]`, matching filename or parent folder name case-insensitively without mutating the input; `getEffectiveOptions(jobId: string, defaults: CompressionOptions, overrides: Record<string, CompressionOptions>): CompressionOptions`, returning the per-file override or batch default.
- These helpers do not change backend jobs or statuses.

- [ ] **Step 1: Add failing helper tests**

Add `queue search filters names and parent folders without mutating jobs`: assert blank query returns all jobs, case-insensitive filename and parent-folder substrings match, a missing term returns an empty list, and the input array and job objects remain unchanged. Add `effective options use a per-file override or batch default`: assert the matching job gets its override and other jobs receive the default object values.

- [ ] **Step 2: Run the focused frontend tests and confirm failure**

Run: `node --experimental-strip-types --test tests/interactions.test.ts`

Expected: the two new tests fail because the helpers do not exist.

- [ ] **Step 3: Implement the two pure helpers**

In `src/lib/interactions.ts`, derive the source folder from `inputPath`, normalize query and candidate strings with `toLocaleLowerCase()`, and return a filtered array. Resolve options by job ID without mutating either argument.

- [ ] **Step 4: Run the focused frontend tests**

Run: `node --experimental-strip-types --test tests/interactions.test.ts`

Expected: all tests in that file pass.

- [ ] **Step 5: Commit**

```bash
git add src/lib/interactions.ts tests/interactions.test.ts
git commit -m "feat: add queue search and file option helpers"
```

### Task 2: Native workspace layout and selected-file inspector

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/styles.css`
- Consume: Task 1 `filterQueueJobs`.

**Interfaces:**
- Produces: a single-window workspace with native Tauri titlebar, compact toolbar, always-visible queue thumbnails and metadata, searchable rows, selected-file inspector, batch quality controls, PNG lossy control, and a persistent primary action.
- Row selection and preview opening remain separate actions; the existing large before/after preview remains available from the thumbnail.

- [ ] **Step 1: Replace the page-like composition**

Reshape `src/App.tsx` from hero → dropzone → queue → settings into a full-height app workspace. Keep the OS-provided Tauri titlebar; do not draw fake traffic lights in the webview. Put add/search/remove actions above the queue, the selected-file inspector beside it, batch quality controls at the lower edge, and the primary compression action where it remains visible.

- [ ] **Step 2: Make queue rows directly scannable**

Render each `job.thumbnail` as an always-visible thumbnail. Show filename, source folder/format, dimensions, original size, status, and available actions with the spec's minimum type sizes. Selecting a row updates the inspector; the thumbnail control opens the existing large preview. Default to the first actionable row when no selection exists. The inspector shows the selected image, filename, format, dimensions, original size, and applicable controls. Preserve insertion order; render an empty-state drop target and picker when no jobs exist, and a separate no-search-results state when filtering hides every row.

- [ ] **Step 3: Wire search without changing queue state**

Use `filterQueueJobs` for visible rows only. Clear selection when its row becomes filtered out and show the inspector's selection prompt. Escape clears the search query. Clearing search restores the complete queue without re-adding jobs or changing their backend status.

- [ ] **Step 4: Reflow at the minimum window size**

At 800 × 600, collapse the inspector into a same-window panel or equivalent responsive state while keeping thumbnails, filename, size, status, and the primary action accessible. Keep headings and controls at readable sizes; allow the queue body to scroll without moving persistent actions.

- [ ] **Step 5: Run the frontend build**

Run: `npm run build`

Expected: TypeScript and Vite production build succeed.

- [ ] **Step 6: Commit**

```bash
git add src/App.tsx src/styles.css
git commit -m "feat: reshape Squeezer into a native batch workspace"
```

### Task 3: Per-file option overrides through Tauri and Rust

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/lib/interactions.ts`
- Modify: `src/lib/tauri.ts`
- Modify: `src-tauri/src/commands.rs`
- Test: `src-tauri/src/commands.rs`
- Consume: Task 1 `getEffectiveOptions`.

**Interfaces:**
- Frontend produces: `startJobs(options: CompressionOptions, overrides: Record<string, CompressionOptions>): Promise<void>` and submits an override only for queued jobs with a custom setting.
- Rust command consumes: `options: CompressionOptions` plus `overrides: HashMap<String, CompressionOptions>`; each queued batch tuple carries the resolved options for its job into `compress_image`.
- The override map is transient and keyed by existing job IDs; it does not add persistence or alter `CompressionJob` serialization.

- [ ] **Step 1: Add failing Rust option-resolution tests**

In `src-tauri/src/commands.rs`, add tests calling `options_for_job(job_id: &str, defaults: &CompressionOptions, overrides: &HashMap<String, CompressionOptions>) -> CompressionOptions`, which does not yet exist. Test `queued_job_uses_override_when_present` plus `queued_job_falls_back_to_batch_options`; assert the chosen mode and `png_lossy` values.

- [ ] **Step 2: Run the targeted Rust tests and confirm failure**

Run: `cargo test --offline --manifest-path src-tauri/Cargo.toml queued_job_`

Expected: compilation/test failure because the helper does not yet exist.

- [ ] **Step 3: Implement safe per-job option capture**

Update the `start_jobs` command to accept the override map, select only currently queued jobs as it does now, resolve and clone each job's options before spawning the batch thread, and pass those options to that job's `compress_image` call. Unknown IDs and overrides for non-queued jobs are never scheduled.

- [ ] **Step 4: Add per-file controls in the inspector**

Keep global options as the default. For a selected queued file, show `沿用整批` or its current mode, allow the four existing modes and the PNG lossy option, and mark a custom setting as `個別設定` in both inspector and row. Resetting to `沿用整批` removes the override. Disable editing for analyzing, compressing, validating, completed, skipped, failed, and cancelled jobs; batch options remain available for later queued files.

- [ ] **Step 5: Wire frontend invocation and batch inheritance**

Change `src/lib/tauri.ts` to send both defaults and overrides. When the global batch setting changes, update inherited queued files without replacing explicit overrides. On start, send only overrides that belong to queued job IDs; keep active and terminal job options unchanged.

- [ ] **Step 6: Run focused Rust tests and frontend checks**

Run: `cargo test --offline --manifest-path src-tauri/Cargo.toml queued_job_ && npm test && npm run build`

Expected: option-resolution tests, existing frontend tests, and the frontend build pass.

- [ ] **Step 7: Commit**

```bash
git add src/App.tsx src/lib/interactions.ts src/lib/tauri.ts src-tauri/src/commands.rs
git commit -m "feat: support per-image compression settings"
```

### Task 4: Retry and result recovery

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/lib/tauri.ts`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/commands.rs`

**Interfaces:**
- Consumes: queue removal and per-file override state from Tasks 2–3.
- Produces: `retryJob(id: string): Promise<void>` in `src/lib/tauri.ts` and Tauri command `retry_job(id: String) -> Result<(), String>`; a failed job returns to queued, clears its error, and emits the updated job while preserving its ID and per-file override.

- [ ] **Step 1: Add failing Rust retry-state tests**

Add `retry_job_requeues_failed_job_and_clears_error` and `retry_job_rejects_non_failed_jobs` in `src-tauri/src/commands.rs`. Assert the failed job transitions to queued, the error and output data are cleared, the job ID remains stable, and non-failed jobs return an actionable error.

- [ ] **Step 2: Run the targeted Rust tests and confirm failure**

Run: `cargo test --offline --manifest-path src-tauri/Cargo.toml retry_job_`

Expected: compilation/test failure because the retry command and state transition are not implemented.

- [ ] **Step 3: Implement retry and register the Tauri command**

Implement `retry_job` for failed jobs only; set status to queued, clear error/output result fields, preserve the job ID, and emit the update. Register it in `src-tauri/src/lib.rs`. Do not start compression automatically.

- [ ] **Step 4: Wire retry into the row and inspector**

Show `重試` only for failed jobs. Keep other successful rows and the failed job's per-file override; after retry, show it in the queued state and let the user explicitly start it.

- [ ] **Step 5: Run the targeted Rust tests**

Run: `cargo test --offline --manifest-path src-tauri/Cargo.toml retry_job_`

Expected: both retry tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/App.tsx src/lib/tauri.ts src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat: let users retry failed image jobs"
```

### Task 5: Interaction polish, Traditional Chinese copy, and motion

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/styles.css`
- Modify: `src/lib/interactions.ts`
- Test: `tests/interactions.test.ts`

**Interfaces:**
- Consumes: the workspace layout, per-file option state, and retry behavior from Tasks 2–4.
- Produces: consistent Traditional Chinese labels and errors; immediate press/focus/busy/success/error feedback; selection, inspector, search, import, preview, cancellation, and result transitions that preserve focus and do not invent state.
- Adds `getOverrideAnnouncement(action: "custom" | "inherit", fileName: string): string` with exact copy tested in `tests/interactions.test.ts`.

- [ ] **Step 1: Add failing override-announcement tests**

Test exact output for a custom setting (`已為 coastline.jpg 設定個別壓縮方式`) and reset (`已恢復 coastline.jpg 沿用整批設定`).

- [ ] **Step 2: Run the focused frontend tests and confirm failure**

Run: `node --experimental-strip-types --test tests/interactions.test.ts`

Expected: the two new assertions fail because `getOverrideAnnouncement` does not exist.

- [ ] **Step 3: Implement the announcement helper and consistent labels**

Implement the helper in `src/lib/interactions.ts`. Remove the marketing hero and section indexes from the active interface. Change mode titles and state copy to the spec's Traditional Chinese vocabulary. Keep source paths and technical file extensions available as secondary details.

- [ ] **Step 4: Implement complete control feedback**

Provide visible hover, keyboard focus, pressed, busy, disabled, selected, success, skipped, cancelled, and error states for applicable controls. Keep errors attached to the affected file. Use inline confirmation for settings/removal and reserve transient notifications for brief acknowledgements.

- [ ] **Step 5: Preserve the short launch cue and honor reduced motion**

Retain one 1–2 second brand-to-workspace entrance without delaying input. Ensure job events and returning from preview do not replay it. Remove movement/stagger under `prefers-reduced-motion` while retaining focus and state cues.

- [ ] **Step 6: Run frontend tests and build**

Run: `npm test && npm run build`

Expected: all frontend tests pass and the production build succeeds.

- [ ] **Step 7: Commit**

```bash
git add src/App.tsx src/styles.css src/lib/interactions.ts tests/interactions.test.ts
git commit -m "feat: polish native workspace interactions"
```

### Task 6: End-to-end acceptance and final review

**Files:**
- Modify: `README.md` only if its described interface or controls become inaccurate.
- Test: frontend tests, Rust tests, build/check commands, manual macOS interaction pass.

**Interfaces:**
- Consumes: all prior tasks and current Tauri job events.
- Produces: verified intake → searchable queue → batch/per-file settings → compression → real comparison/results flow.

- [ ] **Step 1: Run the complete automated checks**

Run: `npm test && npm run build && cargo test --offline --manifest-path src-tauri/Cargo.toml && cargo check --offline --manifest-path src-tauri/Cargo.toml`

Expected: all tests, frontend compilation, Rust tests, and Rust check pass.

- [ ] **Step 2: Verify interactions in the macOS app**

Verify empty-state picker and drag/drop; mixed valid/invalid/duplicate inputs; persistent thumbnails; filename/folder search and clear; selection and inspector; large preview and focus return; all four batch modes; PNG lossy option; one per-file override and reset; start/active/terminal states; cancellation, failure and retry; Finder reveal; adding another batch; and `prefers-reduced-motion`.

- [ ] **Step 3: Check minimum window dimensions and legibility**

Run at 800 × 600 and 1120 × 780. Confirm the main action stays visible, inspector reflows without clipping, thumbnails and status remain visible, and all specified minimum text sizes remain readable.

- [ ] **Step 4: Review the final diff against the spec**

Confirm no new dependencies, generated output, persistent queue, fabricated preview, unrequested workflow page, or unrelated cleanup is present. Commit README only if necessary with the relevant task.
