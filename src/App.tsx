import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { convertFileSrc, invoke, isTauri } from "@tauri-apps/api/core";
import { ArrowDownToLine, ArrowUpRight, Ban, Check, CircleAlert, Eye, FileImage, FolderOpen, Image as ImageIcon, LoaderCircle, Minus, Plus, RotateCcw, Search, Settings2, ShieldCheck, Sparkles, X } from "lucide-react";
import { addFiles, cancelJob, clearJobs, loadPreferences, removeJob as removeQueuedJob, retryJob as retryFailedJob, revealOutputs, savePreferences, selectOutputFolder, startJobs, subscribeJobs, videoPreviewPath } from "./lib/tauri";
import { PreferencesPanel } from "./components/PreferencesPanel";
import { createPreferenceSaver, imageOptionsFromPreferences, runBatchWithOutputLocation, videoOptionsFromPreferences } from "./lib/preferences";
import { calculateSavings, formatBytes } from "./lib/format";
import { filterQueueJobs, getActionErrorMessage, getAdjacentQueueJobId, getAdmissionAnnouncement, getEffectiveOptions, getJobActionLabel, getNextCompressionMode, getOverrideAnnouncement, getPrimaryActionState, getQueueArrivalDelay, getResultActionLabel, getResultCounts, getRevealableJobIds, getStatusAnnouncement, mergeAdmissionJobs } from "./lib/interactions";
import type { AppPreferences, CompressionJob, CompressionMode, CompressionOptions, JobStatus, VideoOptions, VideoQuality } from "./types";

const modes: { id: CompressionMode; title: string; detail: string; icon: string }[] = [
  { id: "quality", title: "高畫質", detail: "保留更多細節", icon: "01" },
  { id: "balanced", title: "平衡", detail: "畫質與容量兼顧", icon: "02" },
  { id: "small", title: "小檔案", detail: "更積極縮小容量", icon: "03" },
];
const initialOptions: CompressionOptions = { mode: "quality", outputFormat: "original", preserveMetadata: true, minSavingsPercent: 10 };
const initialVideoOptions: VideoOptions = { quality: "balanced", maxDimension: 0, minSavingsPercent: 10 };
const videoQualities: { id: VideoQuality; title: string; detail: string }[] = [
  { id: "high", title: "高畫質", detail: "保留更多細節" },
  { id: "balanced", title: "平衡", detail: "畫質與容量兼顧" },
  { id: "small", title: "小檔案", detail: "更積極縮小容量" },
];

function statusLabel(status: JobStatus) {
  return ({ queued: "等待中", analyzing: "分析中", compressing: "壓縮中", validating: "驗證中", completed: "已完成", skipped: "已略過", failed: "失敗", cancelled: "已取消" })[status];
}

function VideoComparison({ job, before, after }: { job: CompressionJob; before: string; after?: string }) {
  const [version, setVersion] = useState<"before" | "after">("before");
  const position = useRef(0);
  const videoRef = useRef<HTMLVideoElement>(null);
  const source = version === "before" ? before : after;
  function selectVersion(next: "before" | "after") {
    if (videoRef.current) position.current = videoRef.current.currentTime;
    setVersion(next);
  }
  return <div className="video-comparison">
    <div className="video-version-switch" role="group" aria-label="選擇播放版本">
      <button type="button" aria-pressed={version === "before"} className={version === "before" ? "selected" : ""} onClick={() => selectVersion("before")}>原片</button>
      <button type="button" aria-pressed={version === "after"} className={version === "after" ? "selected" : ""} disabled={!after} onClick={() => selectVersion("after")}>壓縮後</button>
    </div>
    <div className="preview-image video-preview-single">{source ? <video key={source} ref={videoRef} controls playsInline preload="metadata" src={source} aria-label={`${job.fileName} ${version === "before" ? "原片" : "壓縮後"}`} onTimeUpdate={(event) => { position.current = event.currentTarget.currentTime; }} onLoadedMetadata={(event) => { event.currentTarget.currentTime = Math.min(position.current, event.currentTarget.duration || position.current); }} /> : <div className="preview-empty">完成壓縮後即可比較</div>}</div>
    <div className="video-preview-caption"><b>{version === "before" ? "原始影片" : "壓縮後影片"}</b><span>{job.fileName} · {formatBytes(version === "before" ? job.originalBytes : job.outputBytes ?? 0)}</span></div>
  </div>;
}

function App() {
  const [jobs, setJobs] = useState<CompressionJob[]>([]);
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [options, setOptions] = useState(initialOptions);
  const [preferences, setPreferences] = useState<AppPreferences | null>(null);
  const [preferencesOpen, setPreferencesOpen] = useState(false);
  const [preferencesSaving, setPreferencesSaving] = useState(false);
  const [outputFolderError, setOutputFolderError] = useState("");
  const preferencesSaver = useRef<ReturnType<typeof createPreferenceSaver> | null>(null);
  const preferencesPendingCount = useRef(0);
  const preferencesButton = useRef<HTMLButtonElement>(null);
  const [overrides, setOverrides] = useState<Record<string, CompressionOptions>>({});
  const [videoOptions, setVideoOptions] = useState<VideoOptions>(initialVideoOptions);
  const [videoOverrides, setVideoOverrides] = useState<Record<string, VideoOptions>>({});
  const [dragging, setDragging] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [acknowledgement, setAcknowledgement] = useState("");
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
  const jobSelectRefs = useRef(new Map<string, HTMLButtonElement>());
  const addButtonRef = useRef<HTMLButtonElement>(null);
  const focusAfterRemove = useRef<string | null>(null);
  const lastStatuses = useRef(new Map<string, JobStatus>());
  const latestJobs = useRef(new Map<string, CompressionJob>());
  if (!preferencesSaver.current) {
    preferencesSaver.current = createPreferenceSaver(savePreferences, (next) => {
      setPreferences(next);
      setOptions(imageOptionsFromPreferences(next));
      setVideoOptions(videoOptionsFromPreferences(next));
    });
  }

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
  const selectedVideoOptions = selectedJob ? videoOverrides[selectedJob.id] ?? videoOptions : videoOptions;
  const summary = useMemo(() => `${jobs.length} 個檔案`, [jobs.length]);

  const importPaths = useCallback(async (paths: string[]) => {
    if (!paths.length) return;
    try {
      const added = await addFiles(paths);
      setJobs((current) => mergeAdmissionJobs(current, added, latestJobs.current));
      setSelectedId((current) => current ?? added.find((job) => job.status !== "failed")?.id ?? added[0]?.id ?? null);
      setAnnouncement(getAdmissionAnnouncement(paths.length, added.length, added.filter((job) => job.status === "failed").length));
      if (paths.length > added.length || (paths.length > 0 && added.length === 0)) {
        setAcknowledgement(getAdmissionAnnouncement(paths.length, added.length, added.filter((job) => job.status === "failed").length));
      }
      if (added.length) setArrivalDelays((current) => {
        const next = new Map(current);
        added.slice(0, 3).forEach((job, index) => next.set(job.id, getQueueArrivalDelay(index)));
        return next;
      });
      setNotice("");
    } catch (error) { setNotice(String(error)); }
  }, []);

  useEffect(() => {
    void loadPreferences().then((loaded) => {
      preferencesSaver.current?.setCurrent(loaded);
      setPreferences(loaded);
      setOptions(imageOptionsFromPreferences(loaded));
      setVideoOptions(videoOptionsFromPreferences(loaded));
      setNotice("");
    }).catch((error) => setNotice(`偏好設定載入失敗：${String(error)}`));
  }, []);

  async function updatePreferences(change: AppPreferences | ((current: AppPreferences) => AppPreferences)) {
    setNotice("");
    preferencesPendingCount.current += 1;
    setPreferencesSaving(true);
    try { await preferencesSaver.current!.save(change); }
    finally {
      preferencesPendingCount.current -= 1;
      setPreferencesSaving(preferencesPendingCount.current > 0);
    }
  }

  useEffect(() => {
    if (!isTauri()) return;
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
      else document.querySelector<HTMLElement>(".add-action")?.focus();
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
    setAnnouncement(`正在送出 ${queuedCount} 個媒體檔案`);
    try {
      const queuedIds = new Set(jobs.filter((job) => job.status === "queued").map((job) => job.id));
      const queuedOverrides = Object.fromEntries(Object.entries(overrides).filter(([id]) => queuedIds.has(id)));
      const queuedVideoOverrides = Object.fromEntries(Object.entries(videoOverrides).filter(([id]) => queuedIds.has(id)));
      if (!preferences) throw new Error("偏好設定尚未載入，請先重新載入設定。");
      const result = await runBatchWithOutputLocation(
        preferences.outputLocation,
        selectOutputFolder,
        (directory) => startJobs(options, queuedOverrides, videoOptions, queuedVideoOverrides, directory),
      );
      if (result.cancelled) { setAnnouncement("已取消選擇資料夾，檔案仍保留在清單中"); return; }
      setAnnouncement(`開始壓縮 ${queuedCount} 個媒體檔案`);
    } catch (error) {
      setNotice(String(error));
      if (preferences?.outputLocation.kind === "fixedFolder" && String(error).includes("輸出資料夾")) {
        setOutputFolderError(`已設定的輸出資料夾無法使用：${preferences.outputLocation.path}。請選擇其他資料夾後再開始。`);
        setPreferencesOpen(true);
      }
    }
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
      const visibleIndex = visibleJobs.findIndex((item) => item.id === id);
      focusAfterRemove.current = visibleJobs[visibleIndex + 1]?.id ?? visibleJobs[visibleIndex - 1]?.id ?? "";
      removedPreviewJobs.current.add(id);
      if (selectedId === id) setSelectedId(null);
      if (previewPendingId.current === id) { previewRequest.current++; previewPendingId.current = null; setPreviewingId(null); }
      setPreview((current) => current?.job.id === id ? null : current);
      setJobs((current) => current.filter((item) => item.id !== id));
      setAnnouncement(`已從清單移除 ${job?.fileName ?? "檔案"}；原始檔未變更`);
    } catch (error) {
      setCancelRequested((current) => { const next = new Set(current); next.delete(id); return next; });
      setNotice(String(error));
    }
  }
  async function retryJob(id: string) {
    setNotice("");
    try { await retryFailedJob(id); setAnnouncement("已送交重新分析；完成後會回到待處理清單"); }
    catch (error) { setNotice(String(error)); }
  }
  async function clearQueue() {
    try { await clearJobs(); previewRequest.current++; previewPendingId.current = null; setPreviewingId(null); setPreview(null); setSelectedId(null); setJobs([]); setAnnouncement("已清空媒體清單；原始檔未變更"); } catch (error) { setNotice(String(error)); }
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
      const [before, after] = job.mediaType === "video" ? await Promise.all([
        videoPreviewPath(job.id, false).then(convertFileSrc),
        job.outputPath ? videoPreviewPath(job.id, true).then(convertFileSrc) : Promise.resolve(undefined),
      ]) : await Promise.all([
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
    setAnnouncement(getOverrideAnnouncement("custom", selectedJob.fileName));
  }

  function resetSelectedOverride() {
    if (!selectedJob) return;
    setOverrides((current) => { const next = { ...current }; delete next[selectedJob.id]; return next; });
    setVideoOverrides((current) => { const next = { ...current }; delete next[selectedJob.id]; return next; });
    setAnnouncement(getOverrideAnnouncement("inherit", selectedJob.fileName));
  }

  useEffect(() => {
    if (selectedId && !visibleJobs.some((job) => job.id === selectedId)) { setSelectedId(null); return; }
    if (!selectedId && !query.trim()) setSelectedId(jobs.find((job) => job.status === "queued" || job.status === "failed")?.id ?? null);
  }, [query, selectedId, visibleJobs]);

  useEffect(() => {
    if (!acknowledgement) return;
    const timer = window.setTimeout(() => setAcknowledgement(""), 2400);
    return () => window.clearTimeout(timer);
  }, [acknowledgement]);

  useEffect(() => {
    const nextId = focusAfterRemove.current;
    if (nextId === null) return;
    focusAfterRemove.current = null;
    if (nextId) jobSelectRefs.current.get(nextId)?.focus();
    else addButtonRef.current?.focus();
  }, [jobs]);

  return <main className={`app-shell ${dragging ? "is-dragging" : ""}`}>
    <header className="topbar"><div className="brand"><span className="brand-mark"><span /><span /><span /></span><span className="brand-name">Squeezer</span></div><div className="topbar-meta"><span className="local-pill"><span className="local-dot" />本機處理</span><button ref={preferencesButton} className="icon-button preferences-trigger" title="偏好設定" aria-label="偏好設定" disabled={preferencesSaving} onClick={() => setPreferencesOpen(true)}><Settings2 size={17} /></button></div></header>
    <div className="workspace" id="top">
      <section className="queue-section"><div className="section-heading queue-heading"><div><span className="section-index">工作區</span><h1>媒體清單 <span className="count-badge">{jobs.length}</span></h1></div><div className="queue-actions">{allFinished && revealableIds.length > 1 && <button className="text-button" title={getResultActionLabel(revealableIds.length)} onClick={() => void reveal(revealableIds)}><FolderOpen size={16} />{getResultActionLabel(revealableIds.length)}</button>}<button ref={addButtonRef} className="text-button add-action" onClick={() => void chooseFiles()} title="加入檔案"><Plus size={17} />加入檔案</button>{jobs.length > 0 && <button className="text-button muted" onClick={() => void clearQueue()} disabled={activeCount > 0} title="清空清單">清空</button>}</div></div>
        <div className="queue-toolbar"><label className="search-box"><Search size={16} /><input aria-label="搜尋檔名或資料夾" placeholder="搜尋檔名或資料夾…" value={query} onChange={(event) => setQuery(event.target.value)} onKeyDown={(event) => { if (event.key === "Escape") { setQuery(""); event.currentTarget.blur(); } }} />{query && <button type="button" className="clear-search" aria-label="清除搜尋" onClick={() => setQuery("")}><X size={14} /></button>}</label><span className="local-caption"><ShieldCheck size={14} />原始檔不會被修改</span></div>
        {jobs.length === 0 ? <button type="button" className={`empty-workspace ${dragging ? "is-dragging" : ""}`} onClick={() => void chooseFiles()}><span className="empty-icon"><FileImage size={22} /></span><b>{dragging ? "放開即可加入檔案" : "將照片或影片拖放到這裡"}</b><span>支援 JPEG、PNG、MP4、MOV 與 M4V；所有處理都在這台 Mac 上完成</span><span className="button button-secondary"><FolderOpen size={16} />選擇檔案</span></button> : visibleJobs.length === 0 ? <div className="no-results"><Search size={20} /><b>找不到符合的檔案</b><span>試試其他檔名或資料夾名稱。</span><button className="text-button" onClick={() => setQuery("")}>清除搜尋</button></div> : <div className="workspace-grid"><div className="job-table"><div className="table-head"><span>檔案</span><span>尺寸／時長</span><span>原始大小</span><span>狀態</span><span /></div>{visibleJobs.map((job) => <article aria-label={`${job.fileName}，${statusLabel(job.status)}`} className={`job-row ${selectedId === job.id ? "selected" : ""} ${arrivalDelays.has(job.id) ? "arriving" : ""} ${resultFeedbackIds.has(job.id) ? `result-${job.status}` : ""} ${["analyzing", "compressing", "validating"].includes(job.status) ? `active-${job.status}` : ""}`} onClick={() => setSelectedId(job.id)} style={{ "--arrival-delay": `${arrivalDelays.get(job.id) ?? 0}ms` } as CSSProperties} onAnimationEnd={(event) => {
          if (event.animationName === "row-arrive") setArrivalDelays((current) => { const next = new Map(current); next.delete(job.id); return next; });
          if (event.animationName.startsWith("row-result-")) {
            setResultFeedbackIds((current) => { const next = new Set(current); next.delete(job.id); return next; });
            setArrivalDelays((current) => { const next = new Map(current); next.delete(job.id); return next; });
          }
        }} key={job.id}>
          <div className="job-file">{job.thumbnail ? <button className="thumbnail-button" title={`放大預覽 ${job.fileName}`} aria-label={`放大預覽 ${job.fileName}`} onClick={(event) => { event.stopPropagation(); void showPreview(job, event.currentTarget); }}><img className="job-thumbnail" src={job.thumbnail} alt="" />{job.mediaType === "video" && job.durationMs !== undefined && <span className="thumbnail-duration">{Math.floor(job.durationMs / 60000)}:{String(Math.floor(job.durationMs / 1000) % 60).padStart(2, "0")}</span>}</button> : <span className={`file-badge ${job.extension === "png" ? "png" : "jpg"}`}>{job.extension.toUpperCase()}</span>}<button type="button" className="job-select" ref={(element) => { if (element) jobSelectRefs.current.set(job.id, element); else jobSelectRefs.current.delete(job.id); }} aria-current={selectedId === job.id ? "true" : undefined} onClick={(event) => { event.stopPropagation(); setSelectedId(job.id); }} onKeyDown={(event) => { if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return; event.preventDefault(); const nextId = getAdjacentQueueJobId(visibleJobs, job.id, event.key); if (nextId) { setSelectedId(nextId); jobSelectRefs.current.get(nextId)?.focus(); } }}><span className="job-name-wrap"><b title={job.fileName}>{job.fileName}</b><small>{job.inputPath.split(/[\\/]/).slice(-2, -1)[0] ?? "本機檔案"} · {job.extension.toUpperCase()}{overrides[job.id] && <em className="override-mark">個別設定</em>}</small></span></button></div><span className="job-dimensions">{job.mediaType === "video" && job.durationMs !== undefined ? `${Math.floor(job.durationMs / 60000)}:${String(Math.floor(job.durationMs / 1000) % 60).padStart(2, "0")}` : `${job.width} × ${job.height}`}</span><span className="job-size"><span>{formatBytes(job.originalBytes)}</span>{job.status === "completed" && job.outputBytes !== undefined && <small>→ {formatBytes(job.outputBytes)}</small>}</span>
          <span className={`job-status ${job.status}`} aria-label={`${job.fileName}：${cancelRequested.has(job.id) ? "正在安全取消" : statusLabel(job.status)}`}>{["analyzing", "compressing", "validating"].includes(job.status) ? <LoaderCircle className="spin" size={13} /> : job.status === "completed" ? <Check size={13} /> : job.status === "failed" ? <CircleAlert size={13} /> : job.status === "skipped" ? <Minus size={13} /> : job.status === "cancelled" ? <Ban size={13} /> : null}{cancelRequested.has(job.id) ? "正在安全取消…" : statusLabel(job.status)}{job.status === "completed" && job.outputBytes !== undefined && <small>−{Math.max(0, Math.round((1 - job.outputBytes / job.originalBytes) * 100))}%</small>}</span><button className="row-preview" title={`預覽 ${job.fileName}`} onClick={(event) => { event.stopPropagation(); void showPreview(job, event.currentTarget); }} aria-label={previewingId === job.id ? `正在準備 ${job.fileName} 預覽` : `預覽 ${job.fileName}`} disabled={previewingId === job.id}>{previewingId === job.id ? <LoaderCircle className="spin" size={14} /> : <Eye size={14} />}</button>{job.status === "completed" && job.outputPath && <button className="row-preview" title={`在 Finder 顯示 ${job.fileName}`} onClick={(event) => { event.stopPropagation(); void reveal([job.id]); }} aria-label={`在 Finder 顯示 ${job.fileName}`}><FolderOpen size={14} /></button>}{job.status === "failed" && <button className="row-preview retry-control" title={`重試 ${job.fileName}`} onClick={(event) => { event.stopPropagation(); void retryJob(job.id); }} aria-label={`重試 ${job.fileName}`}><RotateCcw size={14} /></button>}<button className="row-remove" title={getJobActionLabel(job, cancelRequested.has(job.id))} onClick={(event) => { event.stopPropagation(); void removeJob(job.id); }} aria-label={getJobActionLabel(job, cancelRequested.has(job.id))}><X size={15} /></button>
          {previewingId === job.id && <span className="preview-loading" role="status" aria-live="polite"><LoaderCircle className="spin" size={12} />正在準備預覽…</span>}{job.skipReason && <span className="job-skip-reason">{job.skipReason}</span>}{actionError?.jobId === job.id && <span className="job-error action-error" role="alert">{actionError.message}</span>}{["compressing", "analyzing", "validating"].includes(job.status) && <span className="progress-track" role="progressbar" aria-label={`${job.fileName}，${statusLabel(job.status)}`} aria-valuemin={0} aria-valuemax={100} aria-valuenow={job.status === "compressing" ? job.progress : undefined}>{job.status === "compressing" ? <span className="determinate" style={{ width: `${job.progress}%` }} /> : <span className="indeterminate" />}</span>}
        </article>)}</div><aside className={`inspector ${selectedJob ? "has-selection" : ""}`}>
          {selectedJob ? <>
            <div className="inspector-preview">{selectedJob.thumbnail ? <img src={selectedJob.thumbnail} alt={`${selectedJob.fileName} 縮圖`} /> : <FileImage size={32} />}</div>
            <div className="inspector-heading"><span className="section-index">{selectedJob.mediaType === "video" ? "影片資訊" : "照片資訊"}</span><h2 title={selectedJob.fileName}>{selectedJob.fileName}</h2><span className={`job-status ${selectedJob.status}`}>{statusLabel(selectedJob.status)}{(selectedJob.mediaType === "video" ? videoOverrides[selectedJob.id] : overrides[selectedJob.id]) && <small className="override-mark">個別設定</small>}</span></div>
            <dl className="inspector-details"><div><dt>格式</dt><dd>{selectedJob.extension.toUpperCase()}</dd></div><div><dt>尺寸</dt><dd>{selectedJob.width} × {selectedJob.height}</dd></div>{selectedJob.mediaType === "video" && selectedJob.durationMs !== undefined && <div><dt>時長</dt><dd>{Math.floor(selectedJob.durationMs / 60000)}:{String(Math.floor(selectedJob.durationMs / 1000) % 60).padStart(2, "0")}</dd></div>}{selectedJob.mediaType === "video" && <div><dt>視訊編碼</dt><dd>{selectedJob.codec?.toUpperCase() ?? "—"}</dd></div>}<div><dt>原始大小</dt><dd>{formatBytes(selectedJob.originalBytes)}</dd></div><div><dt>來源資料夾</dt><dd>{selectedJob.inputPath.split(/[\/]/).slice(-2, -1)[0] ?? "本機檔案"}</dd></div></dl>
            {selectedJob.status === "queued" ? selectedJob.mediaType === "video" ? <div className="file-settings"><div className="file-settings-heading"><b>此影片的壓縮方式</b>{videoOverrides[selectedJob.id] && <button className="text-button reset-override" onClick={resetSelectedOverride}>沿用整批</button>}</div><label className="inspector-field"><span>品質</span><select value={selectedVideoOptions.quality} onChange={(event) => setVideoOverrides((current) => ({ ...current, [selectedJob.id]: { ...selectedVideoOptions, quality: event.target.value as VideoQuality } }))}>{videoQualities.map((quality) => <option key={quality.id} value={quality.id}>{quality.title}</option>)}</select></label><label className="inspector-field"><span>長邊解析度</span><select value={selectedVideoOptions.maxDimension} onChange={(event) => setVideoOverrides((current) => ({ ...current, [selectedJob.id]: { ...selectedVideoOptions, maxDimension: Number(event.target.value) as VideoOptions["maxDimension"] } }))}><option value={0}>保留原尺寸</option><option value={1080}>最多 1080p</option><option value={720}>最多 720p</option></select></label><label className="inspector-field"><span>目標容量（MB）</span><input type="number" min="0.1" step="0.1" value={selectedVideoOptions.targetBytes ? (selectedVideoOptions.targetBytes / 1_000_000).toString() : ""} placeholder="不指定" onChange={(event) => setVideoOverrides((current) => ({ ...current, [selectedJob.id]: { ...selectedVideoOptions, targetBytes: event.target.value ? Math.round(Number(event.target.value) * 1_000_000) : undefined } }))} /></label><small className="inherit-caption">{videoOverrides[selectedJob.id] ? "這部影片使用個別設定" : "目前沿用整批影片設定"} · 目標容量為盡力接近</small></div> : <div className="file-settings"><div className="file-settings-heading"><b>此照片的壓縮方式</b>{overrides[selectedJob.id] && <button className="text-button reset-override" onClick={resetSelectedOverride}>沿用整批</button>}</div><label className="inspector-field"><span>品質</span><select value={selectedOptions.mode} onChange={(event) => updateSelectedOverride({ mode: event.target.value as CompressionMode })}>{modes.map((mode) => <option key={mode.id} value={mode.id}>{mode.title}</option>)}</select></label><small className="inherit-caption">{overrides[selectedJob.id] ? "這張照片使用個別設定" : "目前沿用整批設定"}</small></div> : <div className="file-settings locked-settings"><b>壓縮方式已鎖定</b><span>此檔案開始處理後，設定無法更改。</span></div>}
            <button className="inspector-preview-action" onClick={(event) => void showPreview(selectedJob, event.currentTarget)}><Eye size={15} />{selectedJob.mediaType === "video" ? "播放原片" : "檢視原圖"}</button>{selectedJob.status === "failed" && <button className="inspector-preview-action retry-action" onClick={() => void retryJob(selectedJob.id)}><RotateCcw size={15} />重新分析並重試</button>}{selectedJob.error && <p className="inspector-error">{selectedJob.error}</p>}
          </> : <div className="inspector-empty"><ImageIcon size={24} /><b>{query && jobs.length ? "沒有選取的檔案" : "選取一個檔案"}</b><span>{query && jobs.length ? "清除搜尋或從清單選取其他項目。" : "媒體詳細資訊會顯示在這裡。"}</span></div>}
        </aside></div>}{notice && <div className="notice" role="alert" aria-live="assertive"><CircleAlert size={15} />{notice}</div>}{acknowledgement && <div className="acknowledgement" role="status">{acknowledgement}</div>}
      </section>
      <section className="settings-section"><div className="section-heading"><div><span className="section-index">壓縮設定</span><h2>媒體壓縮</h2></div><span className="section-aside">分別套用於圖片與影片</span></div>
        {(jobs.length === 0 || jobs.some((job) => job.mediaType === "image")) && <div className="mode-grid" role="radiogroup" aria-label="整批圖片壓縮方式">{modes.map((mode, index) => <button key={mode.id} ref={(element) => { modeRefs.current[index] = element; }} role="radio" aria-checked={options.mode === mode.id} tabIndex={options.mode === mode.id ? 0 : -1} className={`mode-card ${options.mode === mode.id ? "selected" : ""}`} disabled={!preferences || preferencesSaving} onClick={() => { if (preferences) void updatePreferences((current) => ({ ...current, image: { ...current.image, mode: mode.id } })).catch((error) => setNotice(`偏好設定儲存失敗：${String(error)}`)); setAnnouncement(`整批圖片品質已設為${mode.title}`); }} onKeyDown={(event) => { if (event.key === "ArrowLeft" || event.key === "ArrowRight") { event.preventDefault(); const next = getNextCompressionMode(options.mode, event.key); const nextIndex = modes.findIndex((item) => item.id === next); if (preferences) void updatePreferences((current) => ({ ...current, image: { ...current.image, mode: next } })).catch((error) => setNotice(`偏好設定儲存失敗：${String(error)}`)); setAnnouncement(`整批圖片品質已設為${modes[nextIndex]?.title}`); modeRefs.current[nextIndex]?.focus(); } }}><div className="mode-card-top"><span className="mode-number">{mode.icon}</span><span className="radio-dot">{options.mode === mode.id && <span />}</span></div><strong>{mode.title}</strong><span className="mode-detail">{mode.detail}</span></button>)}</div>}
        {jobs.some((job) => job.mediaType === "video") && <div className="video-settings"><div className="file-settings-heading"><b>影片壓縮</b><small>輸出 H.264 MP4</small></div><div className="video-quality-grid" role="radiogroup" aria-label="整批影片品質">{videoQualities.map((quality) => <button key={quality.id} type="button" role="radio" aria-checked={videoOptions.quality === quality.id} className={videoOptions.quality === quality.id ? "selected" : ""} disabled={!preferences || preferencesSaving} onClick={() => { if (preferences) void updatePreferences((current) => ({ ...current, video: { ...current.video, quality: quality.id } })).catch((error) => setNotice(`偏好設定儲存失敗：${String(error)}`)); }}><b>{quality.title}</b><small>{quality.detail}</small></button>)}</div><div className="video-batch-fields"><label className="inspector-field"><span>長邊解析度</span><select disabled={!preferences || preferencesSaving} value={videoOptions.maxDimension} onChange={(event) => { if (preferences) void updatePreferences((current) => ({ ...current, video: { ...current.video, maxDimension: Number(event.target.value) as VideoOptions["maxDimension"] } })).catch((error) => setNotice(`偏好設定儲存失敗：${String(error)}`)); }}><option value={0}>保留原尺寸</option><option value={1080}>最多 1080p</option><option value={720}>最多 720p</option></select></label><label className="inspector-field"><span>目標容量（MB）</span><input disabled={!preferences || preferencesSaving} type="number" min="0.1" step="0.1" value={videoOptions.targetBytes ? (videoOptions.targetBytes / 1_000_000).toString() : ""} placeholder="不指定" onChange={(event) => { if (!preferences) return; const bytes = event.target.value ? Math.round(Number(event.target.value) * 1_000_000) : null; if (bytes === null || bytes > 0) void updatePreferences((current) => ({ ...current, video: { ...current.video, targetBytes: bytes } })).catch((error) => setNotice(`偏好設定儲存失敗：${String(error)}`)); }} /></label></div><small className="inherit-caption">目標容量是編碼目標，完成後會顯示實際容量；未達時不會冒稱成功。</small></div>}

        {(jobs.length === 0 || jobs.some((job) => job.mediaType === "image")) && (        <div className="setting-note"><Sparkles size={15} /><span><b>{modes.find((mode) => mode.id === options.mode)?.title}</b> 系統會按格式自動最佳化；未達 10% 節省時會保留原檔。</span></div>)}
      </section>
    </div>
    <footer className="bottom-bar"><div className="bottom-summary">{jobs.length > 0 ? <><span className="summary-number">{jobs.length}</span><span>{summary}<small>{allFinished ? `完成 ${resultCounts.completed} · 略過 ${resultCounts.skipped} · 失敗 ${resultCounts.failed} · 取消 ${resultCounts.cancelled}` : activeCount > 0 ? `已處理 ${completed.length} · 正在處理 ${activeCount} · 等待 ${queuedCount}` : "原始檔將保留不動"}</small></span></> : <><span className="bottom-lock"><ShieldCheck size={17} /></span><span>安全、私密、完全離線<small>不會覆寫原始檔案</small></span></>}</div><div className="bottom-result">{allFinished && savings > 0 ? <><span>總共省下</span><strong>{formatBytes(savings)}</strong><small>{savedPercent}%</small></> : allFinished ? <><span>容量變化</span><strong>未減少</strong><small>保留原始品質</small></> : <><span>可用空間</span><strong>—</strong><small>完成後計算</small></>}</div><button className={`button button-primary ${busy || activeCount > 0 ? "is-busy" : ""}`} disabled={primaryAction.disabled || !preferences} onClick={() => void compressAll()} aria-live="polite">{activeCount > 0 || busy ? <LoaderCircle className="spin" size={16} /> : <ArrowDownToLine size={16} />}{primaryAction.label}<ArrowUpRight size={14} className="button-arrow" /></button></footer>
    <div className="sr-only" aria-live="polite" aria-atomic="true">{announcement}</div>
    {preferencesOpen && <PreferencesPanel value={preferences} onSave={async (next) => { await updatePreferences(next); setOutputFolderError(""); }} outputFolderError={outputFolderError} onClose={() => { setPreferencesOpen(false); preferencesButton.current?.focus(); }} onReload={async () => { const loaded = await loadPreferences(); preferencesSaver.current?.setCurrent(loaded); setPreferences(loaded); setOptions(imageOptionsFromPreferences(loaded)); setVideoOptions(videoOptionsFromPreferences(loaded)); setNotice(""); }} />}
    {preview && <div className={`preview-overlay ${previewClosing ? "is-closing" : ""}`} role="presentation" onAnimationEnd={(event) => { if (event.target === event.currentTarget && previewClosing) setPreview(null); }} onMouseDown={(event) => { if (event.target === event.currentTarget) closePreview(); }}><section ref={previewRef} tabIndex={-1} className="preview-dialog" role="dialog" aria-modal="true" aria-label={`${preview.job.fileName} 前後對照`}><header className="preview-header"><div><span className="section-index">前後對照</span><h2>{preview.job.fileName}</h2></div><button className="icon-button" title="關閉預覽" onClick={closePreview} aria-label="關閉預覽"><X size={17} /></button></header>{preview.job.mediaType === "video" ? <VideoComparison job={preview.job} before={preview.before} after={preview.after} /> : <div className="preview-grid"><figure><div className="preview-image"><img src={preview.before} alt={`${preview.job.fileName} 原始圖片`} /></div><figcaption><b>原始圖片</b><span>{preview.job.fileName} · {formatBytes(preview.job.originalBytes)}</span></figcaption></figure><figure><div className="preview-image">{preview.after ? <img src={preview.after} alt={`${preview.job.fileName} 壓縮後圖片`} /> : <div className="preview-empty">{preview.job.status === "failed" ? "這個檔案尚未產生輸出" : "完成壓縮後即可比較"}</div>}</div><figcaption><b>壓縮後</b><span>{preview.job.outputBytes !== undefined ? `${preview.job.fileName} · ${formatBytes(preview.job.outputBytes)}` : statusLabel(preview.job.status)}</span></figcaption></figure></div>}</section></div>}
  </main>;
}

export default App;
