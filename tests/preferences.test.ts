import assert from "node:assert/strict";
import test from "node:test";
import { createPreferenceSaver, defaultPreferences, imageOptionsFromPreferences, resolveBatchOutputDirectory, runBatchWithOutputLocation, videoOptionsFromPreferences } from "../src/lib/preferences.ts";

test("saved defaults map to workspace image and video controls", () => {
  assert.deepEqual(imageOptionsFromPreferences(defaultPreferences), { mode: "quality", outputFormat: "original", preserveMetadata: true, minSavingsPercent: 10 });
  assert.deepEqual(videoOptionsFromPreferences(defaultPreferences), { quality: "balanced", maxDimension: 0, targetBytes: undefined, minSavingsPercent: 10 });
});

test("output location resolution distinguishes all modes and picker cancellation", () => {
  assert.deepEqual(resolveBatchOutputDirectory({ kind: "sourceFolder" }), { cancelled: false });
  assert.deepEqual(resolveBatchOutputDirectory({ kind: "fixedFolder", path: "/tmp/out" }), { cancelled: false, directory: "/tmp/out" });
  assert.deepEqual(resolveBatchOutputDirectory({ kind: "askEachBatch" }, "/tmp/picked"), { cancelled: false, directory: "/tmp/picked" });
  assert.deepEqual(resolveBatchOutputDirectory({ kind: "askEachBatch" }, null), { cancelled: true });
});


test("cancelling the batch folder picker never starts queued work", async () => {
  let started = false;
  const result = await runBatchWithOutputLocation(
    { kind: "askEachBatch" },
    async () => null,
    async () => { started = true; },
  );
  assert.deepEqual(result, { cancelled: true });
  assert.equal(started, false);
});

test("a picked batch folder is resolved once and passed to start", async () => {
  let selected = 0;
  let destination: string | undefined;
  const result = await runBatchWithOutputLocation(
    { kind: "askEachBatch" },
    async () => { selected += 1; return "/tmp/batch"; },
    async (path) => { destination = path; },
  );
  assert.deepEqual(result, { cancelled: false });
  assert.equal(selected, 1);
  assert.equal(destination, "/tmp/batch");
});

test("preference saves serialize and apply functional updates to the newest saved value", async () => {
  const persisted: typeof defaultPreferences[] = [];
  let releaseFirst!: () => void;
  const saver = createPreferenceSaver(async (value) => {
    persisted.push(value);
    if (persisted.length === 1) await new Promise<void>((resolve) => { releaseFirst = resolve; });
  }, () => {});
  saver.setCurrent(defaultPreferences);
  const first = saver.save((current) => ({ ...current, image: { ...current.image, mode: "small" } }));
  const second = saver.save((current) => ({ ...current, video: { ...current.video, quality: "high" } }));
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(persisted.length, 1);
  releaseFirst();
  await Promise.all([first, second]);
  assert.equal(persisted.length, 2);
  assert.equal(persisted[1].image.mode, "small");
  assert.equal(persisted[1].video.quality, "high");
});
