# Squeeze — macOS 本地多媒體壓縮工具規格書

> 版本：v0.1 草案｜日期：2026-10-20｜目標：個人使用、品質優先、易用、離線

## 1. 產品定位

Squeeze（暫定名稱）是一款完全本地運作的 macOS 多媒體壓縮 App，透過成熟的圖片與影片編碼器，在視覺保真、容量與速度之間取得平衡。核心理念：**Drop files. Keep quality. Save space.**

### 設計原則
- Local-first：不需上傳雲端。
- Quality-first：保留肉眼可感知的細節。
- Non-destructive：不覆蓋原檔，失敗時不破壞來源。
- Smart defaults：不要求使用者理解 CRF、bitrate、codec。
- Transparent results：呈現容量、節省比例、品質策略及失敗原因。
- Predictable：不靜默丟失透明度、音軌、方向、色彩資訊或 HDR。

### MVP 非目標
不自行發明壓縮演算法；不做雲端服務、帳號或跨平台；GIF、PDF、獨立音訊壓縮、資料夾監控及暫停／恢復留待後續。不能預先宣稱壓縮率必定優於 Compresto。

## 2. 技術架構

| 層級 | 技術 | 職責 |
|---|---|---|
| 桌面 App | Tauri 2 | macOS 視窗、原生整合、IPC、打包 |
| UI | React + TypeScript | 拖放、佇列、設定、進度、預覽 |
| 原生引擎 | Rust | 格式分析、排程、子程序、驗證、錯誤處理 |
| 圖片 | MozJPEG、oxipng、pngquant、libwebp、macOS ImageIO（候選） | 圖片壓縮與格式轉換 |
| 影片 | FFmpeg、x264、x265、VideoToolbox（候選） | 影片壓縮與硬體加速 |
| 品質評估 | SSIM、Butteraugli、VMAF（分階段） | 品質比較及回歸測試 |

React 只透過 Tauri Commands / Events 與 Rust 溝通；Rust 統一管理任務生命週期，使用 adapter 封裝各編碼器。輸出先寫入暫存檔，驗證後安全移動至目的地。執行外部程序時使用安全的參數傳遞，不拼接 shell 命令。引擎架構保留日後獨立 CLI 的可能。

**打包風險：** 需確認各工具授權、Apple Silicon / Intel 支援、macOS 簽署與 notarization；FFmpeg 的授權取決於實際編譯選項及連結元件。

## 3. 功能範圍

| 功能 | MVP 規劃 |
|---|---|
| JPEG、PNG | 優先實作 |
| WebP、HEIC | 圖片階段後續加入 |
| MP4、MOV | 影片階段加入 |
| 多檔拖放、批次佇列 | 支援 |
| 四種品質模式 | 支援 |
| 保留原格式 | 預設 |
| 自動格式轉換 | 使用者可選 |
| 圖片 Before / After | 支援縮放／切換或滑桿 |
| 影片預覽 | 後續；MVP 先提供媒體資訊與結果 |
| 硬體加速 | 可選 VideoToolbox |
| 取消任務 | 支援，並清理暫存檔 |
| 暫停／恢復 | 後續 |
| 離線處理 | 必須 |

## 4. 使用流程與 UI

1. 使用者拖入檔案或點選檔案選擇器。
2. App 分析格式、尺寸、大小、影片時長與必要串流資訊。
3. 預設 **High Quality**、保留原格式與原始檔；可調整品質模式。
4. 點選 **Compress All**，Rust 依佇列與資源限制執行。
5. UI 顯示每個檔案的狀態、進度、取消按鈕。
6. 完成後驗證輸出、計算節省比例；收益不足可略過。
7. 使用者比較圖片、開啟輸出檔或在 Finder 中顯示。

**主要視窗：** 頂部品牌與設定、中央拖放區、四種壓縮模式、檔案列表、底部批次執行按鈕。檔案列顯示檔名、格式、原始／輸出容量、進度、節省比例與錯誤。設定頁提供輸出資料夾、metadata 政策、最低節省門檻、格式策略與影片硬體加速。

## 5. 壓縮模式

| 模式 | 目標 | 原則 |
|---|---|---|
| Lossless | 保留媒體內容 | 無損編碼／最佳化，不靜默降色深或有損重編碼 |
| High Quality | 視覺保真優先 | 保守有損設定，必要時品質驗證 |
| Balanced | 畫質與容量平衡 | 依素材特性選擇中等壓縮 |
| Small Size | 最小化容量 | 更積極壓縮，提示可能出現可見失真 |

**定義注意：** 媒體內容無損不必然代表檔案位元組、metadata、封裝完全相同。影片 remux 是避免重新編碼，不保證縮小檔案；真正無損重編碼甚至可能增大檔案。

## 6. 各格式策略

### 圖片
- **JPEG：** 評估 MozJPEG；保留正確方向及色彩描述檔，明確處理 metadata。
- **PNG：** 預設 oxipng 無損最佳化；pngquant 為可能失真的色彩量化，僅在允許有損時使用。
- **WebP：** libwebp 的 lossless／lossy 模式；檢查透明度。
- **HEIC：** 評估 macOS ImageIO；測試色彩、方向、metadata 與相容性。
- **自動轉格式：** 必須由使用者啟用；不能犧牲透明度或造成不預期的不相容。

### 影片
- **高品質：** 評估 x264／x265 軟體編碼。
- **快速：** VideoToolbox 硬體編碼；不能假設一定比軟體編碼壓得更小。
- **容器：** MP4、MOV；驗證音軌、時長、影格率、方向、必要字幕及其他串流。
- **HDR：** MVP 偵測並保守處理，未驗證保留能力前不得靜默轉 SDR。
- **Lossless：** 可先嘗試不重新編碼的封裝處理，並說明收益限制。

## 7. 工作模型

```ts
type CompressionMode = "lossless" | "quality" | "balanced" | "small";
type MediaType = "image" | "video";
type JobStatus =
  | "queued" | "analyzing" | "compressing" | "validating"
  | "completed" | "skipped" | "failed" | "cancelled";

interface CompressionOptions {
  mode: CompressionMode;
  outputFormat: "original" | "auto";
  hardwareAcceleration: boolean;
  preserveMetadata: boolean;
  minSavingsPercent: number;
}

interface CompressionJob {
  id: string;
  inputPath: string;
  outputPath?: string;
  mediaType: MediaType;
  status: JobStatus;
  progress: number;
  originalBytes: number;
  outputBytes?: number;
  error?: string;
}
```

主流程：`queued → analyzing → compressing → validating → completed`；失敗、取消、收益不足則進入 `failed`、`cancelled`、`skipped`。Rust 負責狀態轉換，React 訂閱事件。初期採保守並行數，避免高解析影片大量占用記憶體。每個工作具有獨立 ID、暫存輸出、取消控制及錯誤資訊。相同輸出檔名必須安全處理，不能靜默覆蓋原檔。

## 8. 驗證與收益策略

輸出完成後至少驗證：可解碼、圖片尺寸／方向／透明度／必要色彩資訊、影片時長／音軌／必要串流／方向，以及檔案大小。建議初期將 **最低節省 10%** 當作可調整的候選預設值，實測後再定案；收益不足時標示 `skipped` 並保留原檔。品質指標僅供輔助，不能取代人工視覺檢查。

### 測試素材與指標

| 測試面向 | 內容 |
|---|---|
| 照片 | 人像、風景、細節、暗部、雜訊 |
| UI 截圖 | 小字、圖示、邊框、漸層、陰影 |
| 透明素材 | PNG 半透明邊緣、少色與多色 |
| 影片 | 1080p／4K、動靜場景、不同音軌、MOV／MP4 |
| 邊界案例 | 已壓縮素材、損毀檔、超大檔、非 ASCII 檔名、HDR |
| 品質 | 圖片 SSIM／Butteraugli；影片 VMAF／SSIM；人工檢視 |
| 效能 | 處理時間、峰值記憶體、CPU 使用率 |
| 相容性 | Preview、QuickTime、瀏覽器開啟驗證 |

與 Compresto、HandBrake／FFmpeg 的比較須使用同一批來源素材、相同輸出條件與相同硬體。尚未實測前不宣稱優劣；VMAF 等指標對小字、UI 錄影與 HDR 不一定能完整反映主觀品質。

## 9. 開發階段與驗收

### Phase 1 — Desktop Foundation
交付 Tauri 2 + React 專案、macOS 拖放、格式辨識、工作佇列、IPC 事件、取消機制。**驗收：** 多檔可加入、狀態可追蹤、取消清理暫存、不修改原檔。

### Phase 2 — Image Compression
交付 JPEG／PNG 壓縮，再加入 WebP／HEIC；容量比較與圖片 Before / After 預覽。**驗收：** 輸出可解碼，無損路徑符合定義，透明度／方向／色彩檢查通過，收益不足正確標示。

### Phase 3 — Video Compression
交付 FFmpeg、MP4／MOV、軟體及可選硬體編碼、進度與輸出驗證。**驗收：** 正常播放、必要音軌與顯示資訊保留、取消後無殘缺輸出。

### Phase 4 — Quality & Packaging
交付品質評估、收益判斷、錯誤復原、`.app`／`.dmg` 打包流程。**驗收：** 固定測試集通過、策略可重現、乾淨 macOS 環境可安裝執行。

## 10. 專案結構

```text
squeeze/
├── src/
│   ├── components/
│   ├── features/
│   │   ├── dropzone/
│   │   ├── compression/
│   │   ├── preview/
│   │   └── settings/
│   ├── hooks/
│   ├── lib/
│   └── types/
├── src-tauri/
│   ├── src/
│   │   ├── commands/
│   │   ├── engine/
│   │   │   ├── images/
│   │   │   ├── videos/
│   │   │   ├── analyzer.rs
│   │   │   ├── scheduler.rs
│   │   │   └── validator.rs
│   │   └── lib.rs
│   ├── binaries/
│   ├── capabilities/
│   └── tauri.conf.json
├── fixtures/
├── tests/
└── package.json
```

## 11. 待確認事項與風險

1. 第一版以 Apple Silicon 優先驗證，同時保留 Intel 編譯相容性。
2. 壓縮編碼器的授權、sidecar 方案及 macOS 簽署／notarization 尚待實作時確認。
3. metadata 保留預設政策與隱私權衡需明確定義。
4. HDR、色彩空間、透明度、影片多音軌的支援界線須逐項測試。
5. 品質預設參數、最低節省門檻及並行數需經基準測試調整。
6. 本文件為規格草案，不代表功能已實作或效能已驗證。

## 12. 下一個可交付版本

優先開始 **Phase 1 + Phase 2（JPEG／PNG）**，交付可建置的 Tauri + React 專案、拖放及佇列、實際壓縮引擎、容量比較、圖片預覽與 macOS 執行說明。原生 `.app` 的編譯與簽署仍需在 macOS 環境驗證。
