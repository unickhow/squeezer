import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { invoke } from "@tauri-apps/api/core";
import { ArrowDownToLine, ArrowUpRight, Ban, Check, CircleAlert, Eye, FileImage, FolderOpen, Image as ImageIcon, LoaderCircle, Minus, Plus, ShieldCheck, Sparkles, X } from "lucide-react";
import { addFiles, cancelJob, clearJobs, removeJob as removeQueuedJob, revealOutputs, startJobs, subscribeJobs } from "./lib/tauri";
import { calculateSavings, formatBytes } from "./lib/format";
import { getAdmissionAnnouncement, getJobActionLabel, getNextCompressionMode, getPrimaryActionState, getQueueArrivalDelay, getResultActionLabel, getResultCounts, getRevealableJobIds, getStatusAnnouncement, mergeAdmissionJobs } from "./lib/interactions";
import type { CompressionJob, CompressionMode, CompressionOptions, JobStatus } from "./types";

const modes: { id: CompressionMode; title: string; detail: string; icon: string }[] = [
  { id: "lossless", title: "Lossless", detail: "PNG 無損・JPEG 略過", icon: "01" },
  { id: "quality", title: "High quality", detail: "JPEG 90・PNG 無損", icon: "02" },
  { id: "balanced", title: "Balanced", detail: "JPEG 82・PNG 無損", icon: "03" },
  { id: "small", title: "Small size", detail: "JPEG 68・PNG 無損", icon: "04" },
];
const initialOptions: CompressionOptions = { mode: "quality", outputFormat: "original", preserveMetadata: true, minSavingsPercent: 10, pngLossy: false };

function statusLabel(status: JobStatus) {
  return ({ queued: "等待中", analyzing: "分析中", compressing: "壓縮中", validating: "驗證中", completed: "完成", skipped: "已略過", failed: "失敗", cancelled: "已取消" })[status];
}

function App() {
  const [jobs, setJobs] = useState<CompressionJob[]>([]);
  const [options, setOptions] = useState(initialOptions);
  const [dragging, setDragging] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [announcement, setAnnouncement] = useState("");
  const [arrivalDelays, setArrivalDelays] = useState<Map<string, number>>(() => new Map());
  const [cancelRequested, setCancelRequested] = useState<Set<string>>(() => new Set());
  const [preview, setPreview] = useState<{ job: CompressionJob; before: string; after?: string } | null>(null);
  const [previewingId, setPreviewingId] = useState<string | null>(null);
  const previewRef = useRef<HTMLElement>(null);
  const previewRequest = useRef(0);
  const previewRestoreFocus = useRef<HTMLElement | null>(null);
  const removedPreviewJobs = useRef(new Set<string>());
  const previewPendingId = useRef<string | null>(null);
  const modeRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const lastStatuses = useRef(new Map<string, JobStatus>());
  const latestJobs = useRef(new Map<string, CompressionJob>());

  const completed = jobs.filter((job) => ["completed", "skipped", "failed", "cancelled"].includes(job.status));
  const { bytes: savings, percent: savedPercent } = calculateSavings(jobs);
  const activeCount = jobs.filter((job) => ["analyzing", "compressing", "validating"].includes(job.status)).length;
  const allFinished = jobs.length > 0 && completed.length === jobs.length;
  const queuedCount = jobs.filter((job) => job.status === "queued").length;
  const primaryAction = getPrimaryActionState(queuedCount, activeCount > 0, busy);
  const resultCounts = getResultCounts(jobs);
  const revealableIds = getRevealableJobIds(jobs);
  const summary = useMemo(() => `${jobs.length} 個檔案`, [jobs.length]);

  const importPaths = useCallback(async (paths: string[]) => {
    if (!paths.length) return;
    try {
      const added = await addFiles(paths);
      setJobs((current) => mergeAdmissionJobs(current, added, latestJobs.current));
      setAnnouncement(getAdmissionAnnouncement(paths.length, added.length, added.filter((job) => job.status === "failed").length));
      if (added.length) setArrivalDelays((current) => {
        const next = new Map(current);
        added.slice(0, 3).forEach((job, index) => next.set(job.id, getQueueArrivalDelay(index)));
        return next;
      });
      setNotice("");
    } catch (error) { setNotice(String(error)); }
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    subscribeJobs((updated) => {
      const previous = lastStatuses.current.get(updated.id);
      lastStatuses.current.set(updated.id, updated.status);
      latestJobs.current.set(updated.id, updated);
      if (["completed", "skipped", "failed", "cancelled"].includes(updated.status) && previous !== updated.status) setAnnouncement(getStatusAnnouncement(updated));
      if (["completed", "skipped", "failed", "cancelled"].includes(updated.status)) {
        setCancelRequested((current) => { const next = new Set(current); next.delete(updated.id); return next; });
      }
      setJobs((current) => current.map((job) => job.id === updated.id ? updated : job));
    })
      .then((stop) => { if (disposed) stop(); else unlisten = stop; });
    const dropListener = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "enter" || event.payload.type === "over") setDragging(true);
      else if (event.payload.type === "leave") setDragging(false);
      else if (event.payload.type === "drop") { setDragging(false); void importPaths(event.payload.paths); }
    });
    return () => { disposed = true; unlisten?.(); void dropListener.then((stop) => stop()); };
  }, [importPaths]);

  useEffect(() => {
    if (!preview) return;
    const previousFocus = previewRestoreFocus.current ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
    previewRef.current?.focus();
    const handleDialogKeys = (event: KeyboardEvent) => {
      if (event.key === "Escape") { setPreview(null); return; }
      if (event.key !== "Tab") return;
      const buttons = previewRef.current?.querySelectorAll<HTMLElement>("button, a[href], input, select, textarea, [tabindex]:not([tabindex='-1'])");
      if (!buttons?.length) return;
      const first = buttons[0];
      const last = buttons[buttons.length - 1];
      if (event.shiftKey && (document.activeElement === first || document.activeElement === previewRef.current)) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && (document.activeElement === last || document.activeElement === previewRef.current)) { event.preventDefault(); first.focus(); }
    };
    window.addEventListener("keydown", handleDialogKeys);
    return () => {
      window.removeEventListener("keydown", handleDialogKeys);
      if (previousFocus?.isConnected) previousFocus.focus();
      else document.querySelector<HTMLElement>(".dropzone")?.focus();
      previewRestoreFocus.current = null;
    };
  }, [preview]);

  async function chooseFiles() {
    try {
      const selected = await invoke<string[]>("select_files");
      if (selected.length) await importPaths(selected);
    } catch (error) { setNotice(String(error)); }
  }
  async function compressAll() {
    setBusy(true); setNotice("");
    try { await startJobs(options); setAnnouncement(`開始壓縮 ${queuedCount} 張圖片`); } catch (error) { setNotice(String(error)); }
    finally { setBusy(false); }
  }
  async function removeJob(id: string) {
    const job = jobs.find((item) => item.id === id);
    try {
      if (job && ["analyzing", "compressing", "validating"].includes(job.status)) {
        setCancelRequested((current) => new Set(current).add(id));
        await cancelJob(id);
        return;
      }
      await removeQueuedJob(id);
      removedPreviewJobs.current.add(id);
      if (previewPendingId.current === id) { previewRequest.current++; previewPendingId.current = null; setPreviewingId(null); }
      setPreview((current) => current?.job.id === id ? null : current);
      setJobs((current) => current.filter((item) => item.id !== id));
    } catch (error) {
      setCancelRequested((current) => { const next = new Set(current); next.delete(id); return next; });
      setNotice(String(error));
    }
  }
  async function clearQueue() {
    try { await clearJobs(); previewRequest.current++; previewPendingId.current = null; setPreviewingId(null); setPreview(null); setJobs([]); } catch (error) { setNotice(String(error)); }
  }
  async function showPreview(job: CompressionJob, trigger: HTMLElement) {
    if (previewPendingId.current === job.id) return;
    previewRestoreFocus.current = trigger;
    const request = ++previewRequest.current;
    previewPendingId.current = job.id;
    setPreviewingId(job.id);
    try {
      const [before, after] = await Promise.all([
        invoke<string>("image_preview", { id: job.id, after: false }),
        job.outputPath ? invoke<string>("image_preview", { id: job.id, after: true }) : Promise.resolve(undefined),
      ]);
      if (request === previewRequest.current && !removedPreviewJobs.current.has(job.id)) setPreview({ job, before, after });
    } catch (error) { if (request === previewRequest.current) setNotice(String(error)); }
    finally { if (request === previewRequest.current) { previewPendingId.current = null; setPreviewingId(null); } }
  }
  async function reveal(ids: string[]) {
    try { await revealOutputs(ids); }
    catch (error) { setNotice(String(error)); }
  }

  return <main className="app-shell">
    <header className="topbar"><a className="brand" href="#top" aria-label="Squeezer 首頁"><span className="brand-mark"><span /><span /><span /></span><span className="brand-name">squeezer<span className="brand-period">.</span></span></a><div className="topbar-meta"><span className="local-pill"><span className="local-dot" />100% 在你的 Mac 上處理</span></div></header>
    <div className="workspace" id="top">
      <section className="intro"><div className="eyebrow"><span>IMAGE WORKFLOW</span><span className="eyebrow-line" /></div><div className="intro-row"><div><h1>Make room<br /><em>for what matters.</em></h1><p className="intro-copy">收小檔案，留下每個重要細節。<br />所有處理都在這台 Mac 上完成。</p></div><div className="intro-stamp"><ShieldCheck size={18} strokeWidth={1.5} /><span>原檔<br />完整保留</span></div></div></section>
      <button type="button" className={`dropzone ${dragging ? "is-dragging" : ""}`} onClick={() => void chooseFiles()} aria-label="選擇 JPEG 或 PNG 圖片">
        <div className="drop-decoration dec-one" /><div className="drop-decoration dec-two" /><div className="drop-symbol"><ImageIcon size={23} strokeWidth={1.5} /><span className="plus-badge"><Plus size={13} /></span></div>
        <div className="drop-copy"><strong>{dragging ? "放開，加入待處理清單" : "把圖片拖進來"}</strong><span>JPEG 與 PNG · 可一次加入多個檔案</span></div>
        <span className="button button-secondary"><FolderOpen size={16} />選擇檔案</span><span className="drop-footnote">本機處理，不需上傳 · 原始檔不會被修改</span>
      </button>
      <section className="settings-section"><div className="section-heading"><div><span className="section-index">01 / COMPRESSION</span><h2>選擇壓縮方式</h2></div><span className="section-aside">你可以隨時更改</span></div>
        <div className="mode-grid" role="radiogroup" aria-label="壓縮方式">{modes.map((mode, index) => <button key={mode.id} ref={(element) => { modeRefs.current[index] = element; }} role="radio" aria-checked={options.mode === mode.id} tabIndex={options.mode === mode.id ? 0 : -1} className={`mode-card ${options.mode === mode.id ? "selected" : ""}`} onClick={() => setOptions((value) => ({ ...value, mode: mode.id }))} onKeyDown={(event) => { if (event.key === "ArrowLeft" || event.key === "ArrowRight") { event.preventDefault(); const next = getNextCompressionMode(options.mode, event.key); const nextIndex = modes.findIndex((item) => item.id === next); setOptions((value) => ({ ...value, mode: next })); modeRefs.current[nextIndex]?.focus(); } }}><div className="mode-card-top"><span className="mode-number">{mode.icon}</span><span className="radio-dot">{options.mode === mode.id && <span />}</span></div><strong>{mode.title}</strong><span className="mode-detail">{mode.detail}</span></button>)}</div>
        <label className={`png-lossy-toggle ${options.pngLossy ? "enabled" : ""}`}><input type="checkbox" checked={options.pngLossy} onChange={(event) => setOptions((value) => ({ ...value, pngLossy: event.target.checked }))} /><span className="toggle-track" aria-hidden="true"><span /></span><span className="png-lossy-copy"><b>PNG 有損色彩縮減</b><small>最多 256 色，透明度保留；可能移除部分 PNG 附加資料。完成後可點縮圖放大比較。</small></span><span className="png-lossy-state">{options.pngLossy ? "已開啟" : "選用"}</span></label>
        <div className="setting-note"><Sparkles size={15} /><span><b>{options.mode === "lossless" ? "Lossless" : options.mode === "quality" ? "High quality" : options.mode === "balanced" ? "Balanced" : "Small size"}</b> JPEG 依所選模式編碼；PNG {options.pngLossy ? "採用有損色彩縮減" : "無損最佳化"}。未達 10% 節省時會自動略過。</span><span className="lossless-note">Lossless 不重新編碼 JPEG</span></div>
      </section>
      <section className="queue-section"><div className="section-heading queue-heading"><div><span className="section-index">02 / YOUR FILES</span><h2>待處理清單 <span className="count-badge">{jobs.length.toString().padStart(2, "0")}</span></h2></div><div className="queue-actions">{allFinished && revealableIds.length > 1 && <button className="text-button" title={getResultActionLabel(revealableIds.length)} onClick={() => void reveal(revealableIds)}><FolderOpen size={15} />{getResultActionLabel(revealableIds.length)}</button>}<button className="text-button" onClick={() => void chooseFiles()} title="加入圖片"><Plus size={15} />加入檔案</button>{jobs.length > 0 && <button className="text-button muted" onClick={() => void clearQueue()} disabled={activeCount > 0} title="清空清單">清空</button>}</div></div>
        {jobs.length === 0 ? <div className="empty-queue"><span className="empty-icon"><FileImage size={18} /></span><span><b>清單還是空的</b><small>加入 JPEG 或 PNG，就可以開始整理空間。</small></span></div> : <div className="job-table"><div className="table-head"><span>檔案</span><span>尺寸</span><span>原始大小</span><span>狀態</span><span /></div>{jobs.map((job) => <article className={`job-row ${arrivalDelays.has(job.id) ? "arriving" : ""}`} style={{ "--arrival-delay": `${arrivalDelays.get(job.id) ?? 0}ms` } as CSSProperties} onAnimationEnd={() => setArrivalDelays((current) => { const next = new Map(current); next.delete(job.id); return next; })} key={job.id}>
          <div className="job-file">{job.thumbnail ? <button className="thumbnail-button" title={`放大預覽 ${job.fileName}`} aria-label={`放大預覽 ${job.fileName}`} onClick={(event) => void showPreview(job, event.currentTarget)}><img className="job-thumbnail" src={job.thumbnail} alt="" /></button> : <span className={`file-badge ${job.extension === "png" ? "png" : "jpg"}`}>{job.extension === "png" ? "PNG" : "JPG"}</span>}<span className="job-name-wrap"><b title={job.fileName}>{job.fileName}</b><small>{job.inputPath.split(/[\\/]/).slice(-2, -1)[0] ?? "本機檔案"}</small></span></div><span className="job-dimensions">{job.width} × {job.height}</span><span className="job-size">{formatBytes(job.originalBytes)}</span>
          <span className={`job-status ${job.status}`} aria-label={`${job.fileName}：${cancelRequested.has(job.id) ? "正在安全取消" : statusLabel(job.status)}`}>{["analyzing", "compressing", "validating"].includes(job.status) ? <LoaderCircle className="spin" size={13} /> : job.status === "completed" ? <Check size={13} /> : job.status === "failed" ? <CircleAlert size={13} /> : job.status === "skipped" ? <Minus size={13} /> : job.status === "cancelled" ? <Ban size={13} /> : null}{cancelRequested.has(job.id) ? "正在安全取消…" : statusLabel(job.status)}{job.status === "completed" && job.outputBytes !== undefined && <small>−{Math.max(0, Math.round((1 - job.outputBytes / job.originalBytes) * 100))}%</small>}</span><button className="row-preview" title={`預覽 ${job.fileName}`} onClick={(event) => void showPreview(job, event.currentTarget)} aria-label={previewingId === job.id ? `正在準備 ${job.fileName} 預覽` : `預覽 ${job.fileName}`} disabled={previewingId === job.id}>{previewingId === job.id ? <LoaderCircle className="spin" size={14} /> : <Eye size={14} />}</button>{job.status === "completed" && job.outputPath && <button className="row-preview" title={`在 Finder 顯示 ${job.fileName}`} onClick={() => void reveal([job.id])} aria-label={`在 Finder 顯示 ${job.fileName}`}><FolderOpen size={14} /></button>}<button className="row-remove" title={getJobActionLabel(job, cancelRequested.has(job.id))} onClick={() => void removeJob(job.id)} aria-label={getJobActionLabel(job, cancelRequested.has(job.id))}><X size={15} /></button>
          {job.error && <span className="job-error">{job.error}</span>}{["compressing", "analyzing", "validating"].includes(job.status) && <span className="progress-track" role="progressbar" aria-label={`${job.fileName}，${statusLabel(job.status)}`}><span className="indeterminate" /></span>}
        </article>)}</div>}{notice && <div className="notice" role="alert" aria-live="assertive"><CircleAlert size={15} />{notice}</div>}
      </section>
    </div>
    <footer className="bottom-bar"><div className="bottom-summary">{jobs.length > 0 ? <><span className="summary-number">{jobs.length}</span><span>{summary}<small>{allFinished ? `完成 ${resultCounts.completed} · 略過 ${resultCounts.skipped} · 失敗 ${resultCounts.failed} · 取消 ${resultCounts.cancelled}` : "原始檔將保留不動"}</small></span></> : <><span className="bottom-lock"><ShieldCheck size={17} /></span><span>安全、私密、完全離線<small>不會覆寫原始檔案</small></span></>}</div><div className="bottom-result">{allFinished && savings > 0 ? <><span>總共省下</span><strong>{formatBytes(savings)}</strong><small>{savedPercent}%</small></> : allFinished ? <><span>容量變化</span><strong>未減少</strong><small>保留原始品質</small></> : <><span>可用空間</span><strong>—</strong><small>完成後計算</small></>}</div><button className="button button-primary" disabled={primaryAction.disabled} onClick={() => void compressAll()}>{activeCount > 0 ? <LoaderCircle className="spin" size={16} /> : <ArrowDownToLine size={16} />}{primaryAction.label}<ArrowUpRight size={14} className="button-arrow" /></button></footer>
    <div className="sr-only" aria-live="polite" aria-atomic="true">{announcement}</div>
    {preview && <div className="preview-overlay" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) setPreview(null); }}><section ref={previewRef} tabIndex={-1} className="preview-dialog" role="dialog" aria-modal="true" aria-label={`${preview.job.fileName} 前後對照`}><header className="preview-header"><div><span className="section-index">IMAGE COMPARISON</span><h2>{preview.job.fileName}</h2></div><button className="icon-button" title="關閉預覽" onClick={() => setPreview(null)} aria-label="關閉預覽"><X size={17} /></button></header><div className="preview-grid"><figure><div className="preview-image"><img src={preview.before} alt={`${preview.job.fileName} 原始圖片`} /></div><figcaption><b>Before</b><span>{preview.job.fileName} · {formatBytes(preview.job.originalBytes)}</span></figcaption></figure><figure><div className="preview-image">{preview.after ? <img src={preview.after} alt={`${preview.job.fileName} 壓縮後圖片`} /> : <div className="preview-empty">{preview.job.status === "failed" ? "這個檔案尚未產生輸出" : "完成壓縮後即可比較"}</div>}</div><figcaption><b>After</b><span>{preview.job.outputBytes !== undefined ? `${preview.job.fileName} · ${formatBytes(preview.job.outputBytes)}` : statusLabel(preview.job.status)}</span></figcaption></figure></div></section></div>}
  </main>;
}

export default App;
