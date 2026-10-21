import type { CompressionJob, CompressionMode, CompressionOptions } from "../types";

const modes: CompressionMode[] = ["quality", "balanced", "small"];

export function getPrimaryActionState(queuedCount: number, active: boolean, busy: boolean): { label: string; disabled: boolean } {
  if (busy) return { label: `正在開始 ${queuedCount} 個檔案…`, disabled: true };
  if (active) return { label: "正在處理…", disabled: true };
  if (queuedCount === 0) return { label: "加入檔案後即可開始", disabled: true };
  return { label: `壓縮 ${queuedCount} 個檔案`, disabled: false };
}

export function getNextCompressionMode(current: CompressionMode, key: "ArrowLeft" | "ArrowRight"): CompressionMode {
  const index = modes.indexOf(current);
  const offset = key === "ArrowRight" ? 1 : -1;
  return modes[(index + offset + modes.length) % modes.length]!;
}

export function getJobActionLabel(job: CompressionJob, cancelRequested: boolean): string {
  if (cancelRequested) return `正在安全取消 ${job.fileName}`;
  if (["analyzing", "compressing", "validating"].includes(job.status)) return `取消此檔 ${job.fileName}`;
  return `從清單移除 ${job.fileName}`;
}

export function getResultActionLabel(outputCount: number): string {
  return outputCount === 1 ? "在 Finder 顯示輸出" : `在 Finder 顯示全部 ${outputCount} 個輸出`;
}

export function getResultCounts(jobs: CompressionJob[]): { completed: number; skipped: number; failed: number; cancelled: number } {
  return jobs.reduce((counts, job) => {
    if (job.status === "completed") counts.completed++;
    else if (job.status === "skipped") counts.skipped++;
    else if (job.status === "failed") counts.failed++;
    else if (job.status === "cancelled") counts.cancelled++;
    return counts;
  }, { completed: 0, skipped: 0, failed: 0, cancelled: 0 });
}

function parentPath(path: string): string {
  return path.split(/[\\/]/).slice(0, -1).join("/");
}

export function filterQueueJobs(jobs: CompressionJob[], query: string): CompressionJob[] {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  if (!normalizedQuery) return jobs;
  return jobs.filter((job) => job.fileName.toLocaleLowerCase().includes(normalizedQuery)
    || (job.inputPath.split(/[\\/]/).slice(-2, -1)[0] ?? "").toLocaleLowerCase().includes(normalizedQuery));
}

export function getAdjacentQueueJobId(jobs: CompressionJob[], currentId: string, key: "ArrowUp" | "ArrowDown"): string | null {
  const index = jobs.findIndex((job) => job.id === currentId);
  if (index < 0) return jobs[0]?.id ?? null;
  const nextIndex = Math.max(0, Math.min(jobs.length - 1, index + (key === "ArrowDown" ? 1 : -1)));
  return jobs[nextIndex]?.id ?? null;
}

export function getEffectiveOptions(jobId: string, defaults: CompressionOptions, overrides: Record<string, CompressionOptions>): CompressionOptions {
  return overrides[jobId] ?? defaults;
}

export function getOverrideAnnouncement(action: "custom" | "inherit", fileName: string): string {
  return action === "custom"
    ? `已為 ${fileName} 設定個別壓縮方式`
    : `已恢復 ${fileName} 沿用整批設定`;
}

export function getRevealableJobIds(jobs: CompressionJob[]): string[] {
  const outputs = jobs.filter((job) => job.status === "completed" && job.outputPath);
  if (outputs.length === 0) return [];
  const parent = parentPath(outputs[0]!.outputPath!);
  if (outputs.some((job) => parentPath(job.outputPath!) !== parent)) return [];
  return outputs.map((job) => job.id);
}

export function getStatusAnnouncement(job: CompressionJob): string {
  const outcome = ({ completed: "已完成", skipped: "已略過", failed: "失敗", cancelled: "已取消" } as const)[job.status as "completed" | "skipped" | "failed" | "cancelled"];
  return outcome ? `${job.fileName}，${outcome}` : `${job.fileName}，${job.status === "analyzing" ? "分析中" : job.status === "compressing" ? "壓縮中" : "等待中"}`;
}

export function mergeAdmissionJobs(current: CompressionJob[], admitted: CompressionJob[], latestEvents: Map<string, CompressionJob>): CompressionJob[] {
  const knownPaths = new Set(current.map((job) => job.inputPath));
  return [...current, ...admitted.filter((job) => !knownPaths.has(job.inputPath)).map((job) => latestEvents.get(job.id) ?? job)];
}

export function getQueueArrivalDelay(batchIndex: number): number {
  return batchIndex >= 0 && batchIndex < 3 ? batchIndex * 35 : 0;
}

export function getAdmissionAnnouncement(requestedCount: number, returnedCount: number, failedCount: number): string {
  if (requestedCount === 0) return "";
  if (returnedCount === 0) return "選取的檔案都已在清單中";
  const addedCount = Math.max(0, returnedCount - failedCount);
  const duplicateCount = Math.max(0, requestedCount - returnedCount);
  if (failedCount === 0 && duplicateCount === 0) return `已加入 ${addedCount} 個檔案`;
  const parts = [
    addedCount > 0 ? `已加入 ${addedCount} 個` : "",
    failedCount > 0 ? `${failedCount} 個無法使用` : "",
    duplicateCount > 0 ? `${duplicateCount} 個重複檔案略過` : "",
  ].filter(Boolean);
  return parts.join("；");
}

export function getActionErrorMessage(action: "preview" | "reveal", fileName: string, detail: string): string {
  const actionLabel = action === "preview" ? "預覽" : "在 Finder 顯示";
  return `無法${actionLabel} ${fileName}：${detail}`;
}
