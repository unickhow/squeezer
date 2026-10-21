import { useEffect, useRef, useState } from "react";
import { Check, FolderOpen, X } from "lucide-react";
import type { AppPreferences } from "../types";
import { selectOutputFolder } from "../lib/tauri";

interface Props {
  value: AppPreferences | null;
  onSave: (value: AppPreferences) => Promise<void>;
  onClose: () => void;
  outputFolderError?: string;
  onReload: () => Promise<void>;
}

export function PreferencesPanel({ value, onSave, onClose, outputFolderError = "", onReload }: Props) {
  const dialog = useRef<HTMLElement>(null);
  const opener = useRef<HTMLElement | null>(document.activeElement instanceof HTMLElement ? document.activeElement : null);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  const [feedback, setFeedback] = useState("");
  const [pending, setPending] = useState(false);
  useEffect(() => {
    dialog.current?.focus();
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeRef.current();
      if (event.key === "Tab") {
        const items = dialog.current?.querySelectorAll<HTMLElement>("button, input, select, [tabindex]:not([tabindex='-1'])");
        if (!items?.length) return;
        if (event.shiftKey && (document.activeElement === items[0] || document.activeElement === dialog.current)) { event.preventDefault(); items[items.length - 1].focus(); }
        else if (!event.shiftKey && (document.activeElement === items[items.length - 1] || document.activeElement === dialog.current)) { event.preventDefault(); items[0].focus(); }
      }
    };
    window.addEventListener("keydown", keydown);
    return () => { window.removeEventListener("keydown", keydown); if (opener.current?.isConnected) opener.current.focus(); };
  }, []);

  async function save(next: AppPreferences) {
    setPending(true); setFeedback("儲存中…");
    try { await onSave(next); setFeedback("已儲存"); }
    catch (error) { setFeedback(String(error)); }
    finally { setPending(false); }
  }
  if (!value) return <div className="preferences-overlay" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}><section ref={dialog} tabIndex={-1} className="preferences-dialog" role="dialog" aria-modal="true" aria-labelledby="preferences-title"><header className="preferences-header"><div><span className="section-index">SQUEEZER</span><h2 id="preferences-title">偏好設定</h2></div><button className="icon-button" aria-label="關閉偏好設定" onClick={onClose}><X size={17} /></button></header><p className="preferences-error">目前無法讀取偏好設定。原有設定檔未被修改。</p><div className="preferences-actions"><button className="button button-secondary" onClick={() => { void onReload().catch((error) => setFeedback(String(error))); }}>重新載入</button><button className="button button-secondary" onClick={() => { if (window.confirm("還原建議值會覆寫目前偏好設定。要繼續嗎？")) void save({ schemaVersion: 1, outputLocation: { kind: "sourceFolder" }, image: { mode: "quality", minSavingsPercent: 10 }, video: { quality: "balanced", maxDimension: 0, targetBytes: null, minSavingsPercent: 10 } }); }}>還原建議值</button></div><small role="status">{feedback}</small></section></div>;

  const update = (patch: Partial<AppPreferences>) => void save({ ...value, ...patch });
  return <div className="preferences-overlay" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}><section ref={dialog} tabIndex={-1} className="preferences-dialog" role="dialog" aria-modal="true" aria-labelledby="preferences-title"><header className="preferences-header"><div><span className="section-index">SQUEEZER</span><h2 id="preferences-title">偏好設定</h2></div><button className="icon-button" aria-label="關閉偏好設定" onClick={onClose}><X size={17} /></button></header>
    {outputFolderError && <p className="preferences-error" role="alert">{outputFolderError}</p>}
    <section className="preference-group"><h3>輸出位置</h3><label className="preference-choice"><span>與原檔相同資料夾</span><input type="radio" checked={value.outputLocation.kind === "sourceFolder"} disabled={pending} onChange={() => update({ outputLocation: { kind: "sourceFolder" } })} /></label><label className="preference-choice"><span>每批次詢問</span><input type="radio" checked={value.outputLocation.kind === "askEachBatch"} disabled={pending} onChange={() => update({ outputLocation: { kind: "askEachBatch" } })} /></label><div className="preference-folder"><span>固定資料夾</span><button className="text-button" disabled={pending} onClick={async () => { try { const path = await selectOutputFolder(); if (path) update({ outputLocation: { kind: "fixedFolder", path } }); } catch (error) { setFeedback(String(error)); } }}><FolderOpen size={15} />選擇…</button></div>{value.outputLocation.kind === "fixedFolder" && <small className="preference-path">{value.outputLocation.path}</small>}</section>
    <section className="preference-group"><h3>圖片預設</h3><label className="preference-field"><span>壓縮方式</span><select disabled={pending} value={value.image.mode} onChange={(event) => update({ image: { ...value.image, mode: event.target.value as AppPreferences["image"]["mode"] } })}><option value="quality">高畫質</option><option value="balanced">平衡</option><option value="small">小檔案</option></select></label><label className="preference-field"><span>最低節省比例</span><input disabled={pending} type="number" min="0" max="99" value={value.image.minSavingsPercent} onChange={(event) => { const n = Number(event.target.value); if (Number.isInteger(n) && n >= 0 && n <= 99) update({ image: { ...value.image, minSavingsPercent: n } }); }} /></label></section>
    <section className="preference-group"><h3>影片預設</h3><label className="preference-field"><span>品質</span><select disabled={pending} value={value.video.quality} onChange={(event) => update({ video: { ...value.video, quality: event.target.value as AppPreferences["video"]["quality"] } })}><option value="high">高畫質</option><option value="balanced">平衡</option><option value="small">小檔案</option></select></label><label className="preference-field"><span>長邊解析度</span><select disabled={pending} value={value.video.maxDimension} onChange={(event) => update({ video: { ...value.video, maxDimension: Number(event.target.value) as 0 | 720 | 1080 } })}><option value={0}>保留原尺寸</option><option value={1080}>最多 1080p</option><option value={720}>最多 720p</option></select></label><label className="preference-field"><span>目標容量（MB）</span><input disabled={pending} type="number" min="0.1" step="0.1" value={value.video.targetBytes ? value.video.targetBytes / 1_000_000 : ""} placeholder="不指定" onChange={(event) => { const bytes = event.target.value ? Math.round(Number(event.target.value) * 1_000_000) : null; if (bytes === null || (Number.isFinite(bytes) && bytes > 0)) update({ video: { ...value.video, targetBytes: bytes } }); }} /></label><label className="preference-field"><span>最低節省比例</span><input disabled={pending} type="number" min="0" max="99" value={value.video.minSavingsPercent} onChange={(event) => { const n = Number(event.target.value); if (Number.isInteger(n) && n >= 0 && n <= 99) update({ video: { ...value.video, minSavingsPercent: n } }); }} /></label></section>
    <footer className="preferences-footer"><button className="text-button muted" disabled={pending} onClick={() => { if (window.confirm("還原建議值？目前偏好設定將被取代。")) void save({ schemaVersion: 1, outputLocation: { kind: "sourceFolder" }, image: { mode: "quality", minSavingsPercent: 10 }, video: { quality: "balanced", maxDimension: 0, targetBytes: null, minSavingsPercent: 10 } }); }}>還原建議值</button><span role={feedback && feedback !== "已儲存" && feedback !== "儲存中…" ? "alert" : "status"}>{feedback === "已儲存" ? <Check size={14} /> : null}{feedback}</span></footer>
  </section></div>;
}
