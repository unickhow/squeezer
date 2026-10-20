# Squeezer End-to-End Interaction Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Squeezer’s full image workflow feel responsive and visually alive from launch through import, compression, preview, and results without delaying work or inventing progress.

**Architecture:** Keep the existing single-screen React/Tauri workflow and express motion as short, event-driven CSS transitions around existing job and UI states. Add no animation dependency, persistence layer, splash screen, or new workflow page; keep interaction copy and pure state helpers in `src/lib/interactions.ts` and preserve current backend-truthful job events.

**Tech Stack:** React 19, TypeScript 6, CSS animations/transitions, Tauri 2, Rust, Node’s built-in test runner.

**Spec:** `docs/superpowers/specs/2026-10-20-squeezer-interaction-design.md`, especially “全程操作節奏與動態規劃”, “微互動規格”, accessibility, and first-stage acceptance.

## Global Constraints

- Keep the full import-to-result journey in the existing main view; do not add a blocking splash screen or mandatory onboarding.
- Make the main view actionable on first paint; a visible brand cue leads into the workflow over approximately 1.5 seconds and never gates input.
- Keep micro-interactions within 120–180 ms and view transitions within 180–240 ms as starting values; honor `prefers-reduced-motion` by removing movement and stagger.
- Never invent file progress, completion estimates, savings, or ETA; display only confirmed job states and measured output sizes.
- Preserve row order, scroll position, selected compression mode, and keyboard focus during asynchronous updates.
- Do not add dependencies or introduce advanced workbench features (scrub comparison, previous/next navigation, per-file overrides) in this pass.

## Review Focus

- **Reduced-motion preference:** every entrance, row, modal, and progress animation becomes immediate or opacity-only while all status information remains visible. Test with `prefers-reduced-motion` enabled during manual acceptance.
- **Large imports:** stagger no more than the first three new rows; remaining rows appear immediately and do not delay analysis. Test a batch above three files.
- **Mixed valid, invalid, and duplicate inputs:** valid files remain usable, errors stay attached to affected rows, and duplicate feedback does not cover the queue. Reuse `commands::tests::admit_paths_keeps_valid_and_invalid_siblings` and verify the visible result manually.
- **Asynchronous updates during interaction:** a job event must not replay the launch or row-arrival animation, move the focused control, or reorder rows. Verify while analysis and compression events arrive.
- **Preview failure and close:** loading errors stay actionable, Escape/backdrop/close behavior remains available, and focus returns to the opener. Verify success and failure paths manually.

---

### Task 1: Motion foundation and fast app entry

**Files:**
- Modify: `src/styles.css`
- Test: manual launch and reduced-motion acceptance in the final task

**Interfaces:**
- Consumes: current `.app-shell`, `.topbar`, `.intro`, `.dropzone`, and reduced-motion CSS.
- Produces: `--motion-micro: 150ms`, `--motion-view: 210ms`, `--motion-stagger-step: 35ms`, and `--motion-ease: cubic-bezier(.2, .7, .2, 1)`; one short, first-entry animation on the brand and primary content; no JavaScript timer or startup gate.

- [ ] **Step 1: Add shared motion tokens and static state styles**

Define the exact CSS custom properties `--motion-micro: 150ms`, `--motion-view: 210ms`, `--motion-stagger-step: 35ms`, and `--motion-ease: cubic-bezier(.2, .7, .2, 1)`. Ensure hover, focus, pressed, busy, disabled, success, and error feedback use more than color alone where each state exists.

- [ ] **Step 2: Add the first-entry animation**

Animate the brand mark first, then settle the main content with a visible but restrained opacity/position change. Keep the full sequence between 1 and 2 seconds with no delay that blocks interaction. Do not animate on every React render or when returning from the preview.

- [ ] **Step 3: Respect reduced motion**

Extend the existing `prefers-reduced-motion` rule to neutralize launch and later motion tokens, with immediate state changes and intact focus indicators.

- [ ] **Step 4: Inspect the CSS diff and run `npm run build`**

Expected: production build succeeds; opening the app does not wait for an animation before controls work.

- [ ] **Step 5: Commit**

```bash
git add src/styles.css
git commit -m "feat: add restrained app entry motion"
```

---

### Task 2: File intake and queue feedback

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/lib/interactions.ts`
- Modify: `src/styles.css`
- Test: `tests/interactions.test.ts`

**Interfaces:**
- Consumes: `addFiles`, `mergeAdmissionJobs`, `CompressionJob`, existing queue event flow, and Task 1 `--motion-stagger-step` token.
- Produces: `getQueueArrivalDelay(batchIndex: number): number` returning 0, 35, 70 ms for the first three new rows and 0 thereafter; `getAdmissionAnnouncement(requestedCount: number, returnedCount: number, failedCount: number): string`; one-shot arrival state for newly admitted rows; and distinct visual states for analyzing/queued/completed/skipped/failed/cancelled.

- [ ] **Step 1: Add pure helper tests for arrival and admission feedback**

Test `getQueueArrivalDelay(0..3)` returns `0, 35, 70, 0`. Test `getAdmissionAnnouncement` for zero requested paths (`""`), duplicate-only (`"選取的檔案都已在清單中"`), added valid files (`"已加入 2 個檔案"`), and a mixed batch (`"已加入 1 個；1 個無法使用；1 個重複檔案略過"`). `returnedCount` includes each newly admitted row, including failed rows; `failedCount` is the failed subset, and duplicate count is `requestedCount - returnedCount`.

- [ ] **Step 2: Run `npm test` and confirm the new helper tests fail**

Expected: failure because the arrival and batch-feedback helpers do not yet exist.

- [ ] **Step 3: Implement the helpers in `src/lib/interactions.ts`**

Add `getQueueArrivalDelay(batchIndex: number): number` and `getAdmissionAnnouncement(requestedCount: number, returnedCount: number, failedCount: number): string` in `src/lib/interactions.ts`. Admission copy uses `已加入 N 個檔案` for all-valid adds, omits zero-valued clauses, and says `選取的檔案都已在清單中` when no rows were returned. Do not add timers to the job event path.

- [ ] **Step 4: Animate only new queue rows and clarify row states**

Track newly admitted IDs until their entry animation ends. Apply a delay of `batchIndex × --motion-stagger-step` only to the first three new rows in a batch. Do not use `nth-child` entrance animation that replays after later state updates. Give each terminal state a distinct icon, label, and restrained local feedback; retain current row order and inline error text.

- [ ] **Step 5: Add queue and drag-target interaction states**

Give drag-enter/drop/leave, thumbnail hover/focus, row actions, and the empty queue clear pressed/focus feedback. Keep the full drop area keyboard-operable and do not place a loading overlay over the queue.

- [ ] **Step 6: Run `npm test` and `npm run build`**

Expected: helper tests pass and TypeScript/Vite production build succeeds.

- [ ] **Step 7: Commit**

```bash
git add src/App.tsx src/lib/interactions.ts src/styles.css tests/interactions.test.ts
git commit -m "feat: polish file intake and queue feedback"
```

---

### Task 3: Compression controls and truthful processing transitions

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/styles.css`
- Test: existing frontend tests and manual status-state acceptance

**Interfaces:**
- Consumes: existing `getPrimaryActionState`, `getStatusAnnouncement`, `CompressionOptions`, and `job_updated` states.
- Produces: responsive mode selection and PNG toggle feedback, explicit batch-start acknowledgement, current-row emphasis, and visibly different completed/skipped/failed/cancelled results without synthetic progress. No new job-state model or frontend progress timer.
- Motion consumes Task 1 `--motion-micro` and `--motion-ease` tokens.

- [ ] **Step 1: Map current states to visible treatments**

List the existing visual treatment for queued, analyzing, compressing, validating, completed, skipped, failed, and cancelled. Keep existing copy where it is already clear; identify only missing visual distinctions before editing CSS.

- [ ] **Step 2: Refine mode selection and PNG toggle states**

Make selection changes immediately visible through a moving selection marker, pressed/focus states, and synchronized one-line explanation. Do not add extra settings or change compression behavior.

- [ ] **Step 3: Refine the primary action and active-row emphasis**

On start, acknowledge the exact queued count and lock repeat submission while the request is active. Emphasize the currently compressing or validating row locally. Keep the indeterminate sweep only while the backend reports an active phase.

- [ ] **Step 4: Refine terminal outcomes without inventing progress**

On terminal events, update only that row and measured totals. Keep completed, skipped, failed, and cancelled outcomes visually and textually distinct. Do not add success styling when the batch saved no space.

- [ ] **Step 5: Run `npm test` and `npm run build`**

Expected: existing interaction regressions pass and TypeScript/Vite production build succeeds.

- [ ] **Step 6: Commit**

```bash
git add src/App.tsx src/styles.css
git commit -m "feat: clarify compression and result feedback"
```

---

### Task 4: Preview and recovery interactions

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/styles.css`
- Modify: `src/lib/interactions.ts`
- Test: `tests/interactions.test.ts`

**Interfaces:**
- Consumes: `showPreview`, the existing preview dialog focus trap, `image_preview`, `revealOutputs`, and current error display.
- Produces: `getActionErrorMessage(action: "preview" | "reveal", fileName: string, detail: string): string`, a visible preview-loading state, a restrained open/close transition, reliable return focus, and actionable local feedback for preview/Finder failures.
- Preview motion consumes Task 1 `--motion-view` and `--motion-ease` tokens.

- [ ] **Step 1: Add or extend tests for preview and recovery copy**

Test `getActionErrorMessage("preview", "photo.png", "decode error")` includes the filename, preview action, and detail; test the reveal variant names Finder and the file. Do not add a mock image or fake after result.

- [ ] **Step 2: Run `npm test` and confirm new assertions fail**

Expected: failure only for newly specified helper behavior.

- [ ] **Step 3: Implement missing copy and loading state**

Implement `getActionErrorMessage` in `src/lib/interactions.ts` and call it from preview and Finder error handlers. Keep preview requests bound to the selected job and ignore stale results using the existing request ID. Do not expand the dialog into a new workbench in this pass.

- [ ] **Step 4: Add restrained preview entry and exit feedback**

Use the view-transition duration from Task 1; close with Escape, close button, and backdrop as currently supported. Restore focus to the originating row control. Under reduced motion, remove scale and movement.

- [ ] **Step 5: Keep errors close to their source**

Show preview and Finder errors without clearing jobs or covering the list. Successful transient acknowledgements may clear after a short interval; actionable errors stay visible until the next relevant action.

- [ ] **Step 6: Run `npm test` and `npm run build`**

Expected: tests and production build succeed.

- [ ] **Step 7: Commit**

```bash
git add src/App.tsx src/styles.css src/lib/interactions.ts tests/interactions.test.ts
git commit -m "feat: polish preview and recovery feedback"
```

---

### Task 5: End-to-end interaction acceptance

**Files:**
- Modify: `README.md` only if shipped behavior descriptions change.
- Test: existing `tests/interactions.test.ts` and current Rust command/engine tests; manual macOS app pass.

**Interfaces:**
- Consumes: all prior tasks and the current Rust job event contract.
- Produces: a verified launch-to-next-batch experience with stable focus, row order, and truthful results.

- [ ] **Step 1: Run all project checks**

Run `npm test && npm run build && cargo test --offline --manifest-path src-tauri/Cargo.toml && cargo check --offline --manifest-path src-tauri/Cargo.toml`.

Expected: all tests, type checks, production build, and Rust check pass.

- [ ] **Step 2: Verify the complete path in the macOS app**

Open the app and verify first-paint availability during the short brand motion; empty state and file picker; drag enter/drop/leave; imports of one, four-plus, duplicate, invalid, and mixed files; option selection; repeat-click prevention; analyzing/compressing/validating/terminal row states; preview load/close/failure focus return; Finder reveal; errors; adding a second batch; and `prefers-reduced-motion`.

- [ ] **Step 3: Check interaction readability at minimum window size**

At the configured minimum `800 × 600` window size and a narrower responsive layout, verify labels remain readable, primary actions remain visible, motion does not cause clipping, and no queue update changes scroll position unexpectedly.

- [ ] **Step 4: Update README behavior notes**

Document only the interaction behaviors actually shipped; do not promise persistent queue restoration, advanced workbench controls, or measured progress if they remain unimplemented.

- [ ] **Step 5: Commit documentation updates if behavior notes changed**

```bash
git add README.md
git commit -m "docs: describe polished Squeezer workflow"
```

