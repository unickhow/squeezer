import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { invoke } from "@tauri-apps/api/core";
import { ArrowDownToLine, ArrowUpRight, Ban, Check, CircleAlert, Eye, FileImage, FolderOpen, Image as ImageIcon, LoaderCircle, Minus, Plus, Search, ShieldCheck, Sparkles, X } from "lucide-react";
import { addFiles, cancelJob, clearJobs, removeJob as removeQueuedJob, revealOutputs, startJobs, subscribeJobs } from "./lib/tauri";
import { calculateSavings, formatBytes } from "./lib/format";
import { filterQueueJobs, getActionErrorMessage, getAdmissionAnnouncement, getEffectiveOptions, getJobActionLabel, getNextCompressionMode, getPrimaryActionState, getQueueArrivalDelay, getResultActionLabel, getResultCounts, getRevealableJobIds, getStatusAnnouncement, mergeAdmissionJobs } from "./lib/interactions";
import type { CompressionJob, CompressionMode, CompressionOptions, JobStatus } from "./types";

const modes: { id: CompressionMode; title: string; detail: string; icon: string }[] = [
  { id: "lossless", title: "無損", detail: "PNG 無損・JPEG 略過", icon: "01" },
  { id: "quality", title: "高畫質", detail: "JPEG 90・PNG 無損", icon: "02" },
  { id: "balanced", title: "平衡", detail: "JPEG 82・PNG 無損", icon: "03" },
  { id: "small", title: "小檔案", detail: "JPEG 68・PNG 無損", icon: "04" },
];
const initialOptions: CompressionOptions = { mode: "quality", outputFormat: "original", preserveMetadata: true, minSavingsPercent: 10, pngLossy: false };

function statusLabel(status: JobStatus) {
  return ({ queued: "等待中", analyzing: "分析中", compressing: "壓縮中", validating: "驗證中", completed: "完成", skipped: "已略過", failed: "失敗", cancelled: "已取消" })[status];
}

function App() {
  const [jobs, setJobs] = useState<CompressionJob[]>([]);
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [options, setOptions] = useState(initialOptions);
  const [overrides, setOverrides] = useState<Record<string, CompressionOptions>>({});
  const [dragging, setDragging] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [announcement, setAnnouncement] = useState("");
  const [arrivalDelays, setArrivalDelays] = useState<Map<string, number>>(() => new Map());
  const [resultFeedbackIds, setResultFeedbackIds] = useState<Set<string>>(() => new Set());
  const [cancelRequested, setCancelRequested] = useState<Set<string>>(() => new Set());
  const [preview, setPreview] = useState<{ job: CompressionJob; before: string; after?: string } | null>(null);
  const [previewClosing, setPreviewClosing] = useState(false);
  const [previewingId, setPreviewingId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<{ jobId: string; message: string } | null>(null);
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
  const visibleJobs = filterQueueJobs(jobs, query);
  const selectedJob = visibleJobs.find((job) => job.id === selectedId) ?? null;
  const selectedOptions = selectedJob ? getEffectiveOptions(selectedJob.id, options, overrides) : options;
  const summary = useMemo(() => `${jobs.length} 個檔案`, [jobs.length]);

  const importPaths = useCallback(async (paths: string[]) => {
    if (!paths.length) return;
    try {
      const added = await addFiles(paths);
      setJobs((current) => mergeAdmissionJobs(current, added, latestJobs.current));
      setSelectedId((current) => current ?? added.find((job) => job.status !== "failed")?.id ?? added[0]?.id ?? null);
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
      if (["completed", "skipped", "failed", "cancelled"].includes(updated.status) && previous !== updated.status) {
        setAnnouncement(getStatusAnnouncement(updated));
        setResultFeedbackIds((current) => new Set(current).add(updated.id));
      }
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
      if (event.key === "Escape") { closePreview(); return; }
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
    setAnnouncement(`正在送出 ${queuedCount} 張圖片`);
    try {
      const queuedIds = new Set(jobs.filter((job) => job.status === "queued").map((job) => job.id));
      const queuedOverrides = Object.fromEntries(Object.entries(overrides).filter(([id]) => queuedIds.has(id)));
      await startJobs(options, queuedOverrides);
      setAnnouncement(`開始壓縮 ${queuedCount} 張圖片`);
    } catch (error) { setNotice(String(error)); }
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
      if (selectedId === id) setSelectedId(null);
      if (previewPendingId.current === id) { previewRequest.current++; previewPendingId.current = null; setPreviewingId(null); }
      setPreview((current) => current?.job.id === id ? null : current);
      setJobs((current) => current.filter((item) => item.id !== id));
    } catch (error) {
      setCancelRequested((current) => { const next = new Set(current); next.delete(id); return next; });
      setNotice(String(error));
    }
  }
  async function clearQueue() {
    try { await clearJobs(); previewRequest.current++; previewPendingId.current = null; setPreviewingId(null); setPreview(null); setSelectedId(null); setJobs([]); } catch (error) { setNotice(String(error)); }
  }
  async function showPreview(job: CompressionJob, trigger: HTMLElement) {
    if (previewPendingId.current === job.id) return;
    setActionError(null);
    setPreviewClosing(false);
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
    } catch (error) { if (request === previewRequest.current) setActionError({ jobId: job.id, message: getActionErrorMessage("preview", job.fileName, String(error)) }); }
    finally { if (request === previewRequest.current) { previewPendingId.current = null; setPreviewingId(null); } }
  }
  async function reveal(ids: string[]) {
    const source = jobs.find((job) => job.id === ids[0]);
    setActionError(null);
    try { await revealOutputs(ids); }
    catch (error) { if (source) setActionError({ jobId: source.id, message: getActionErrorMessage("reveal", source.fileName, String(error)) }); }
  }

  function closePreview() {
    if (preview) setPreviewClosing(true);
  }

  function updateSelectedOverride(patch: Partial<CompressionOptions>) {
    if (!selectedJob || selectedJob.status !== "queued") return;
    setOverrides((current) => ({ ...current, [selectedJob.id]: { ...getEffectiveOptions(selectedJob.id, options, current), ...patch } }));
  }

  function resetSelectedOverride() {
    if (!selectedJob) return;
    setOverrides((current) => { const next = { ...current }; delete next[selectedJob.id]; return next; });
  }

  useEffect(() => {
    if (selectedId && !visibleJobs.some((job) => job.id === selectedId)) setSelectedId(null);
  }, [query, selectedId, visibleJobs]);

  return <main className={`app-shell ${dragging ? "is-dragging" : ""}`}>
    <header className="topbar"><div className="brand"><span className="brand-mark"><span /><span /><span /></span><span className="brand-name">Squeezer</span></div><div className="topbar-meta"><span className="local-pill"><span className="local-dot" />本機處理</span></div></header>
    <div className="workspace" id="top">
      <section className="queue-section"><div className="section-heading queue-heading"><div><span className="section-index">工作區</span><h1>圖片清單 <span className="count-badge">{jobs.length}</span></h1></div><div className="queue-actions">{allFinished && revealableIds.length > 1 && <button className="text-button" title={getResultActionLabel(revealableIds.length)} onClick={() => void reveal(revealableIds)}><FolderOpen size={16} />{getResultActionLabel(revealableIds.length)}</button>}<button className="text-button add-action" onClick={() => void chooseFiles()} title="加入照片"><Plus size={17} />加入照片</button>{jobs.length > 0 && <button className="text-button muted" onClick={() => void clearQueue()} disabled={activeCount > 0} title="清空清單">清空</button>}</div></div>
        <div className="queue-toolbar"><label className="search-box"><Search size={16} /><input aria-label="搜尋檔名或資料夾" placeholder="搜尋檔名或資料夾…" value={query} onChange={(event) => setQuery(event.target.value)} onKeyDown={(event) => { if (event.key === "Escape") { setQuery(""); event.currentTarget.blur(); } }} />{query && <button type="button" className="clear-search" aria-label="清除搜尋" onClick={() => setQuery("")}><X size={14} /></button>}</label><span className="local-caption"><ShieldCheck size={14} />原始檔不會被修改</span></div>
        {jobs.length === 0 ? <button type="button" className={`empty-workspace ${dragging ? "is-dragging" : ""}`} onClick={() => void chooseFiles()}><span className="empty-icon"><FileImage size={22} /></span><b>{dragging ? "放開即可加入照片" : "將照片拖放到這裡"}</b><span>支援 JPEG 與 PNG，所有處理都在這台 Mac 上完成</span><span className="button button-secondary"><FolderOpen size={16} />選擇照片</span></button> : visibleJobs.length === 0 ? <div className="no-results"><Search size={20} /><b>找不到符合的照片</b><span>試試其他檔名或資料夾名稱。</span><button className="text-button" onClick={() => setQuery("")}>清除搜尋</button></div> : <div className="workspace-grid"><div className="job-table"><div className="table-head"><span>照片</span><span>尺寸</span><span>原始大小</span><span>狀態</span><span /></div>{visibleJobs.map((job) => <article tabIndex={0} aria-label={`${job.fileName}，${statusLabel(job.status)}`} aria-current={selectedId === job.id ? "true" : undefined} className={`job-row ${selectedId === job.id ? "selected" : ""} ${arrivalDelays.has(job.id) ? "arriving" : ""} ${resultFeedbackIds.has(job.id) ? `result-${job.status}` : ""} ${["analyzing", "compressing", "validating"].includes(job.status) ? `active-${job.status}` : ""}`} onClick={() => setSelectedId(job.id)} onKeyDown={(event) => { if ((event.key === "Enter" || event.key === " ") && event.target === event.currentTarget) { event.preventDefault(); setSelectedId(job.id); } }} style={{ "--arrival-delay": `${arrivalDelays.get(job.id) ?? 0}ms` } as CSSProperties} onAnimationEnd={(event) => {
          if (event.animationName === "row-arrive") setArrivalDelays((current) => { const next = new Map(current); next.delete(job.id); return next; });
          if (event.animationName.startsWith("row-result-")) {
            setResultFeedbackIds((current) => { const next = new Set(current); next.delete(job.id); return next; });
            setArrivalDelays((current) => { const next = new Map(current); next.delete(job.id); return next; });
          }
        }} key={job.id}>
          <div className="job-file">{job.thumbnail ? <button className="thumbnail-button" title={`放大預覽 ${job.fileName}`} aria-label={`放大預覽 ${job.fileName}`} onClick={(event) => { event.stopPropagation(); void showPreview(job, event.currentTarget); }}><img className="job-thumbnail" src={job.thumbnail} alt="" /></button> : <span className={`file-badge ${job.extension === "png" ? "png" : "jpg"}`}>{job.extension === "png" ? "PNG" : "JPG"}</span>}<span className="job-name-wrap"><b title={job.fileName}>{job.fileName}</b><small>{job.inputPath.split(/[\\/]/).slice(-2, -1)[0] ?? "本機檔案"} · {job.extension.toUpperCase()}{overrides[job.id] && <em className="override-mark">個別設定</em>}</small></span></div><span className="job-dimensions">{job.width} × {job.height}</span><span className="job-size">{formatBytes(job.originalBytes)}</span>
          <span className={`job-status ${job.status}`} aria-label={`${job.fileName}：${cancelRequested.has(job.id) ? "正在安全取消" : statusLabel(job.status)}`}>{["analyzing", "compressing", "validating"].includes(job.status) ? <LoaderCircle className="spin" size={13} /> : job.status === "completed" ? <Check size={13} /> : job.status === "failed" ? <CircleAlert size={13} /> : job.status === "skipped" ? <Minus size={13} /> : job.status === "cancelled" ? <Ban size={13} /> : null}{cancelRequested.has(job.id) ? "正在安全取消…" : statusLabel(job.status)}{job.status === "completed" && job.outputBytes !== undefined && <small>−{Math.max(0, Math.round((1 - job.outputBytes / job.originalBytes) * 100))}%</small>}</span><button className="row-preview" title={`預覽 ${job.fileName}`} onClick={(event) => { event.stopPropagation(); void showPreview(job, event.currentTarget); }} aria-label={previewingId === job.id ? `正在準備 ${job.fileName} 預覽` : `預覽 ${job.fileName}`} disabled={previewingId === job.id}>{previewingId === job.id ? <LoaderCircle className="spin" size={14} /> : <Eye size={14} />}</button>{job.status === "completed" && job.outputPath && <button className="row-preview" title={`在 Finder 顯示 ${job.fileName}`} onClick={(event) => { event.stopPropagation(); void reveal([job.id]); }} aria-label={`在 Finder 顯示 ${job.fileName}`}><FolderOpen size={14} /></button>}<button className="row-remove" title={getJobActionLabel(job, cancelRequested.has(job.id))} onClick={(event) => { event.stopPropagation(); void removeJob(job.id); }} aria-label={getJobActionLabel(job, cancelRequested.has(job.id))}><X size={15} /></button>
          {previewingId === job.id && <span className="preview-loading" role="status" aria-live="polite"><LoaderCircle className="spin" size={12} />正在準備預覽…</span>}{job.error && <span className="job-error">{job.error}</span>}{actionError?.jobId === job.id && <span className="job-error action-error" role="alert">{actionError.message}</span>}{["compressing", "analyzing", "validating"].includes(job.status) && <span className="progress-track" role="progressbar" aria-label={`${job.fileName}，${statusLabel(job.status)}`}><span className="indeterminate" /></span>}
        </article>)}</div><aside className={`inspector ${selectedJob ? "has-selection" : ""}`}>{selectedJob ? <><div className="inspector-preview">{selectedJob.thumbnail ? <img src={selectedJob.thumbnail} alt={`${selectedJob.fileName} 縮圖`} /> : <FileImage size={32} />}</div><div className="inspector-heading"><span className="section-index">照片資訊</span><h2 title={selectedJob.fileName}>{selectedJob.fileName}</h2><span className={`job-status ${selectedJob.status}`}>{statusLabel(selectedJob.status)}{overrides[selectedJob.id] && <small className="override-mark">個別設定</small>}</span></div><dl className="inspector-details"><div><dt>格式</dt><dd>{selectedJob.extension.toUpperCase()}</dd></div><div><dt>尺寸</dt><dd>{selectedJob.width} × {selectedJob.height}</dd></div><div><dt>原始大小</dt><dd>{formatBytes(selectedJob.originalBytes)}</dd></div><div><dt>來源資料夾</dt><dd>{selectedJob.inputPath.split(/[\\/]/).slice(-2, -1)[0] ?? "本機檔案"}</dd></div></dl>{selectedJob.status === "queued" ? <div className="file-settings"><div className="file-settings-heading"><b>此照片的壓縮方式</b>{overrides[selectedJob.id] && <button className="text-button reset-override" onClick={resetSelectedOverride}>沿用整批</button>}</div><label className="inspector-field"><span>品質</span><select value={selectedOptions.mode} onChange={(event) => updateSelectedOverride({ mode: event.target.value as CompressionMode })}>{modes.map((mode) => <option key={mode.id} value={mode.id}>{mode.title}</option>)}</select></label><label className="inspector-checkbox"><input type="checkbox" checked={selectedOptions.pngLossy} onChange={(event) => updateSelectedOverride({ pngLossy: event.target.checked })} /><span>PNG 有損色彩縮減</span></label><small className="inherit-caption">{overrides[selectedJob.id] ? "這張照片使用個別設定" : "目前沿用整批設定"}</small></div> : <div className="file-settings locked-settings"><b>壓縮方式已鎖定</b><span>此照片開始處理後，設定無法更改。</span></div>}<button className="inspector-preview-action" onClick={(event) => void showPreview(selectedJob, event.currentTarget)}><Eye size={15} />檢視原圖</button>{selectedJob.error && <p className="inspector-error">{selectedJob.error}</p>}</> : <div className="inspector-empty"><ImageIcon size={24} /><b>{query && jobs.length ? "沒有選取的照片" : "選取一張照片"}</b><span>{query && jobs.length ? "清除搜尋或從清單選取其他項目。" : "照片詳細資訊會顯示在這裡。"}</span></div>}</aside></div>}{notice && <div className="notice" role="alert" aria-live="assertive"><CircleAlert size={15} />{notice}</div>}
      </section>
      <section className="settings-section"><div className="section-heading"><div><span className="section-index">壓縮設定</span><h2>整批品質</h2></div><span className="section-aside">套用於待處理照片</span></div>
        <div className="mode-grid" data-selected-index={modes.findIndex((mode) => mode.id === options.mode)} role="radiogroup" aria-label="壓縮方式">{modes.map((mode, index) => <button key={mode.id} ref={(element) => { modeRefs.current[index] = element; }} role="radio" aria-checked={options.mode === mode.id} tabIndex={options.mode === mode.id ? 0 : -1} className={`mode-card ${options.mode === mode.id ? "selected" : ""}`} onClick={() => setOptions((value) => ({ ...value, mode: mode.id }))} onKeyDown={(event) => { if (event.key === "ArrowLeft" || event.key === "ArrowRight") { event.preventDefault(); const next = getNextCompressionMode(options.mode, event.key); const nextIndex = modes.findIndex((item) => item.id === next); setOptions((value) => ({ ...value, mode: next })); modeRefs.current[nextIndex]?.focus(); } }}><div className="mode-card-top"><span className="mode-number">{mode.icon}</span><span className="radio-dot">{options.mode === mode.id && <span />}</span></div><strong>{mode.title}</strong><span className="mode-detail">{mode.detail}</span></button>)}</div>
        <label className={`png-lossy-toggle ${options.pngLossy ? "enabled" : ""}`}><input type="checkbox" checked={options.pngLossy} onChange={(event) => setOptions((value) => ({ ...value, pngLossy: event.target.checked }))} /><span className="toggle-track" aria-hidden="true"><span /></span><span className="png-lossy-copy"><b>PNG 有損色彩縮減</b><small>最多 256 色，透明度保留；可能移除部分 PNG 附加資料。完成後可點縮圖放大比較。</small></span><span className="png-lossy-state">{options.pngLossy ? "已開啟" : "選用"}</span></label>
        <div className="setting-note"><Sparkles size={15} /><span><b>{modes.find((mode) => mode.id === options.mode)?.title}</b> JPEG 依所選模式編碼；PNG {options.pngLossy ? "採用有損色彩縮減" : "無損最佳化"}。未達 10% 節省時會自動略過。</span><span className="lossless-note">無損模式不重新編碼 JPEG</span></div>
      </section>
    </div>
    <footer className="bottom-bar"><div className="bottom-summary">{jobs.length > 0 ? <><span className="summary-number">{jobs.length}</span><span>{summary}<small>{allFinished ? `完成 ${resultCounts.completed} · 略過 ${resultCounts.skipped} · 失敗 ${resultCounts.failed} · 取消 ${resultCounts.cancelled}` : activeCount > 0 ? `已處理 ${completed.length} · 正在處理 ${activeCount} · 等待 ${queuedCount}` : "原始檔將保留不動"}</small></span></> : <><span className="bottom-lock"><ShieldCheck size={17} /></span><span>安全、私密、完全離線<small>不會覆寫原始檔案</small></span></>}</div><div className="bottom-result">{allFinished && savings > 0 ? <><span>總共省下</span><strong>{formatBytes(savings)}</strong><small>{savedPercent}%</small></> : allFinished ? <><span>容量變化</span><strong>未減少</strong><small>保留原始品質</small></> : <><span>可用空間</span><strong>—</strong><small>完成後計算</small></>}</div><button className={`button button-primary ${busy || activeCount > 0 ? "is-busy" : ""}`} disabled={primaryAction.disabled} onClick={() => void compressAll()} aria-live="polite">{activeCount > 0 || busy ? <LoaderCircle className="spin" size={16} /> : <ArrowDownToLine size={16} />}{primaryAction.label}<ArrowUpRight size={14} className="button-arrow" /></button></footer>
    <div className="sr-only" aria-live="polite" aria-atomic="true">{announcement}</div>
    {preview && <div className={`preview-overlay ${previewClosing ? "is-closing" : ""}`} role="presentation" onAnimationEnd={(event) => { if (event.target === event.currentTarget && previewClosing) setPreview(null); }} onMouseDown={(event) => { if (event.target === event.currentTarget) closePreview(); }}><section ref={previewRef} tabIndex={-1} className="preview-dialog" role="dialog" aria-modal="true" aria-label={`${preview.job.fileName} 前後對照`}><header className="preview-header"><div><span className="section-index">IMAGE COMPARISON</span><h2>{preview.job.fileName}</h2></div><button className="icon-button" title="關閉預覽" onClick={closePreview} aria-label="關閉預覽"><X size={17} /></button></header><div className="preview-grid"><figure><div className="preview-image"><img src={preview.before} alt={`${preview.job.fileName} 原始圖片`} /></div><figcaption><b>Before</b><span>{preview.job.fileName} · {formatBytes(preview.job.originalBytes)}</span></figcaption></figure><figure><div className="preview-image">{preview.after ? <img src={preview.after} alt={`${preview.job.fileName} 壓縮後圖片`} /> : <div className="preview-empty">{preview.job.status === "failed" ? "這個檔案尚未產生輸出" : "完成壓縮後即可比較"}</div>}</div><figcaption><b>After</b><span>{preview.job.outputBytes !== undefined ? `${preview.job.fileName} · ${formatBytes(preview.job.outputBytes)}` : statusLabel(preview.job.status)}</span></figcaption></figure></div></section></div>}
  </main>;
}

export default App;
