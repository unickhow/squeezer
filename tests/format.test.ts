import assert from "node:assert/strict";
import test from "node:test";
import { calculateSavings, formatBytes } from "../src/lib/format.ts";
import type { CompressionJob } from "../src/types.ts";

function job(originalBytes: number, outputBytes?: number): CompressionJob {
  return {
    id: "test", inputPath: "/tmp/image.png", fileName: "image.png", extension: "png", mediaType: "image",
    width: 10, height: 10, status: "completed", progress: 100, originalBytes, outputBytes,
  };
}

test("formats sizes in bytes, kilobytes, and megabytes", () => {
  assert.equal(formatBytes(10), "10 B");
  assert.equal(formatBytes(1536), "1.5 KB");
  assert.equal(formatBytes(2 * 1024 * 1024), "2.00 MB");
  assert.equal(formatBytes(undefined), "—");
});

test("savings exclude skipped outputs and never become negative", () => {
  assert.deepEqual(calculateSavings([job(100, 60), job(50), job(10, 20)]), { bytes: 40, percent: 25 });
});
