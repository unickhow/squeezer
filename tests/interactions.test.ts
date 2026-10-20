import assert from "node:assert/strict";
import test from "node:test";

import {
  getJobActionLabel,
  getQueueArrivalDelay,
  getAdmissionAnnouncement,
  getActionErrorMessage,
  getNextCompressionMode,
  getPrimaryActionState,
  getResultActionLabel,
  getResultCounts,
  getRevealableJobIds,
  getStatusAnnouncement,
  mergeAdmissionJobs,
} from "../src/lib/interactions.ts";
import type { CompressionJob } from "../src/types.ts";

function job(overrides: Partial<CompressionJob> = {}): CompressionJob {
  return {
    id: "1", inputPath: "/photos/one.jpg", fileName: "one.jpg", extension: "jpg", mediaType: "image",
    width: 10, height: 10, status: "queued", progress: 0, originalBytes: 100, ...overrides,
  };
}

test("primary action describes count and busy state", () => {
  assert.deepEqual(getPrimaryActionState(1, false, false), { label: "壓縮 1 張", disabled: false });
  assert.deepEqual(getPrimaryActionState(3, false, false), { label: "壓縮 3 張", disabled: false });
  assert.deepEqual(getPrimaryActionState(2, true, false), { label: "正在處理…", disabled: true });
  assert.deepEqual(getPrimaryActionState(2, false, true), { label: "正在開始 2 張…", disabled: true });
  assert.deepEqual(getPrimaryActionState(0, false, false), { label: "加入圖片後即可開始", disabled: true });
});

test("quality modes cycle with wraparound", () => {
  assert.equal(getNextCompressionMode("quality", "ArrowRight"), "balanced");
  assert.equal(getNextCompressionMode("quality", "ArrowLeft"), "lossless");
  assert.equal(getNextCompressionMode("small", "ArrowRight"), "lossless");
  assert.equal(getNextCompressionMode("lossless", "ArrowLeft"), "small");
});

test("row action labels describe cancel, pending cancellation and removal", () => {
  assert.match(getJobActionLabel(job({ fileName: "a.jpg", status: "compressing" }), false), /取消此檔.*a.jpg/);
  assert.match(getJobActionLabel(job({ fileName: "a.jpg", status: "compressing" }), true), /正在安全取消.*a.jpg/);
  assert.match(getJobActionLabel(job({ fileName: "a.jpg", status: "completed" }), false), /移除.*a.jpg/);
});

test("result action distinguishes one output from a batch", () => {
  assert.equal(getResultActionLabel(1), "在 Finder 顯示輸出");
  assert.equal(getResultActionLabel(4), "在 Finder 顯示全部 4 個輸出");
});

test("result counts and same-folder reveal grouping use terminal jobs", () => {
  const jobs = [
    job({ id: "1", status: "completed", outputPath: "/out/a.jpg" }),
    job({ id: "2", status: "completed", outputPath: "/out/b.jpg" }),
    job({ id: "3", status: "skipped" }), job({ id: "4", status: "failed" }), job({ id: "5", status: "cancelled" }),
    job({ id: "6", status: "compressing", outputPath: "/other/c.jpg" }),
  ];
  assert.deepEqual(getResultCounts(jobs), { completed: 2, skipped: 1, failed: 1, cancelled: 1 });
  assert.deepEqual(getRevealableJobIds(jobs), ["1", "2"]);
  assert.deepEqual(getRevealableJobIds([jobs[0]!, job({ id: "2", status: "completed", outputPath: "/else/b.jpg" })]), []);
});

test("terminal status announcements distinguish outcomes", () => {
  assert.match(getStatusAnnouncement(job({ status: "completed" })), /one.jpg.*完成/);
  assert.match(getStatusAnnouncement(job({ status: "skipped" })), /one.jpg.*略過/);
  assert.match(getStatusAnnouncement(job({ status: "failed" })), /one.jpg.*失敗/);
  assert.match(getStatusAnnouncement(job({ status: "cancelled" })), /one.jpg.*取消/);
});

test("admission response keeps a newer analysis event that arrived first", () => {
  const admitted = job({ id: "fast", status: "analyzing", width: 0 });
  const updated = job({ id: "fast", status: "queued", width: 32, height: 24 });
  const merged = mergeAdmissionJobs([], [admitted], new Map([[updated.id, updated]]));
  assert.equal(merged[0]?.status, "queued");
  assert.equal(merged[0]?.width, 32);
});

test("queue arrival is staggered only across the first three rows", () => {
  assert.deepEqual([0, 1, 2, 3].map(getQueueArrivalDelay), [0, 35, 70, 0]);
});

test("admission announcement summarizes added, invalid and duplicate files", () => {
  assert.equal(getAdmissionAnnouncement(0, 0, 0), "");
  assert.equal(getAdmissionAnnouncement(3, 0, 0), "選取的檔案都已在清單中");
  assert.equal(getAdmissionAnnouncement(2, 2, 0), "已加入 2 個檔案");
  assert.equal(getAdmissionAnnouncement(3, 2, 1), "已加入 1 個；1 個無法使用；1 個重複檔案略過");
});

test("action errors name the affected file and recovery action", () => {
  assert.match(getActionErrorMessage("preview", "photo.png", "decode error"), /photo\.png/);
  assert.match(getActionErrorMessage("preview", "photo.png", "decode error"), /預覽/);
  assert.match(getActionErrorMessage("preview", "photo.png", "decode error"), /decode error/);
  assert.match(getActionErrorMessage("reveal", "photo.png", "permission denied"), /Finder/);
  assert.match(getActionErrorMessage("reveal", "photo.png", "permission denied"), /photo\.png/);
});
