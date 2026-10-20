# Squeezer 影片壓縮設計

## 意圖

讓使用者在同一個 Squeezer 工作區，安全地整理並壓縮常見影片，涵蓋兩種核心需求：日常省空間，以及分享前符合指定容量限制。清單能直接辨認片段、進度和節省結果，檢視器可播放原片與輸出。影片匯出不得阻塞介面，也不得用估算數字冒充實際成果。

## 背景

- Squeezer 是 macOS-only 的 Tauri 2、React/TypeScript、Rust 應用。現有佇列、取消、重試、結果摘要與圖片壓縮引擎皆在 Rust 管理。
- 現有 `CompressionJob` 與 `CompressionOptions` 是圖片專用；圖片上限 512 MiB，不適合作為影片限制。
- 目前只支援 JPEG/PNG；影片、HDR、多音訊軌尚未支援。參考：[README](../../README.md)、[原生工作區規格](2026-10-20-squeezer-native-workspace-design.md)。
- 影片比較預覽不可將整部影片讀成 base64；必須以串流方式提供播放，並把 WebView 可讀取的檔案範圍限制在目前已加入的來源與輸出檔。Tauri Asset Protocol 預設停用，且必須設定允許範圍；不得為了播放而開放整個使用者家目錄。[Tauri Asset Protocol](https://v2.tauri.app/security/asset-protocol/)

## 目標

1. 從原生選檔與拖放加入常見影片，並與圖片共用工作區、佇列順序和主要操作。
2. 支援本機串列壓縮、真實階段／進度、取消、錯誤重試和無覆寫輸出。
3. 讓使用者在清單看到影片縮圖、時長、解析度、容量與狀態；完成後看到實際輸出容量和節省比例。
4. 可在檢視器播放原片和輸出，切換比較時保留播放位置，並清楚標示目前檢視哪個版本。
5. 圖片流程和設定維持原行為；圖片與影片的整批設定彼此獨立，逐檔覆寫只影響所選媒體。
6. 僅使用實際產生並通過驗證的輸出呈現「已完成」或節省量；原始檔永不修改。

## MVP 建議範圍

以下是參考 Compresto 後的規劃建議，須由使用者檢視後確認。Compresto 的產品內容顯示它同時處理社群／訊息平台上傳限制與一般儲存最佳化，提供快速品質 preset、手動解析度與 bitrate 控制、指定目標容量及逐檔覆寫；這些可作為 Squeezer 的需求參照，不代表其宣稱的壓縮率或品質保證可直接套用。[Compresto 影片操作指南](https://compresto.app/blog/how-to-compress-a-video)、[Compresto 工作情境](https://compresto.app/use-case)、[逐檔覆寫說明](https://compresto.app/blog/compresto-1-28)。

- 入口副檔名：`.mp4`、`.mov`、`.m4v`。副檔名只用於初步篩選，接著以 AVFoundation 實際檢查可否讀取、視訊軌／音訊軌與匯出相容性。
- 第一階段以一般 SDR 影片為目標；HDR、Dolby Vision、空間／多視角影片、ProRes、字幕、章節與替代音軌先明確拒絕或標為不支援，不可靜默丟棄軌道或變更色彩資訊。
- 支援一般 H.264／HEVC 視訊，以及單一 AAC 音訊軌或無音訊來源。只接受通過執行期 track 與輸出 preset 相容性檢查的素材。
- 預設保留來源解析度、方向、影格速率與主音訊；提供明確的 1080p／720p 等縮放選擇，未選擇縮放時不自行降解析度。來源音訊／方向／影格率需由代表性樣本驗證保留情形。
- 輸出放在來源資料夾，以 `.squeezed` 後綴建立新 MP4；檔名衝突時使用不覆寫的遞增名稱。
- 影片整批設定與圖片設定分開呈現；快速設定先提供少量清楚命名的品質選擇，進階調整包含 codec 與解析度。若可行，另提供指定目標容量模式，處理分享平台的容量上限。顯示容量預估時必須標為預估，不可當作實際結果。
- 設定在逐檔覆寫和影片批次預設之間有清楚關係；個別調整只影響該影片。允許壓縮期間繼續加入後續項目，但不要求第一版平行編碼。
- 若輸出未比原檔至少小 10%（沿用圖片目前的初始門檻），刪除暫存輸出並標成略過，附上原因。實際素材測試若顯示 10% 不適合作為影片門檻，先回報結果再調整。

## 編碼器方向

尚未定案 encoder。Compresto 的產品參照包含目標容量和更直接的解析度／bitrate 控制；先前研究顯示 Apple 高階 export preset 的選項較固定。因此第一個技術驗證須比較 `AVAssetExportSession`、`AVAssetReader`／`AVAssetWriter`／VideoToolbox，以及受控建置的 FFmpeg 路線，檢查三者能否支援目標容量、品質控制、H.264／HEVC、實際節省、輸出相容性、音訊／方向保留、取消清理與發佈成本。不要在驗證前假定原生路線足以達到 Compresto 參照的操作控制，也不要預先排除 FFmpeg。

後續實作計畫建議先安排一個限定素材與指標的 encoder feasibility task，並產生三種路線的比較報告，不直接綁定正式架構；若任一路線未達品質、操作控制或發佈要求，先回到此規格確認範圍，再開始 encoder integration。

Apple 文件指出 `AVAssetExportSession` 以 preset 與輸出類型匯出，且應先查詢來源／輸出相容性；VideoToolbox 可使用硬體編碼，但硬體可用性取決於機型與設定。[替代格式匯出](https://developer.apple.com/documentation/avfoundation/exporting-video-to-alternative-formats)、[Export Presets](https://developer.apple.com/documentation/avfoundation/export-presets)、[VideoToolbox](https://developer.apple.com/documentation/videotoolbox)。不要在 UI 保證所有裝置皆硬體加速。

Compresto 公開 README 說明其使用 FFmpeg；這可作為其 codec 與控制範圍的技術參照，但 Squeezer 是否採用仍由原型結果及發佈需求決定。[Compresto 公開 README](https://github.com/dqhieu/ComprestoApp#features)。FFmpeg 官方只提供原始碼，建置選項會影響 GPL/LGPL 與再散佈義務；任何打包方案必須先固定 build 組態並完成來源碼、license、notice 與二進位稽核。[FFmpeg 官方下載](https://www.ffmpeg.org/download.html)、[FFmpeg 法律說明](https://ffmpeg.org/legal.html)。本規格不構成法律意見。

## 操作流程

```text
加入 MP4 / MOV / M4V
  → 立即建立佇列列，顯示可取得的檔名與容量
  → 分析來源軌道、時長、解析度並擷取代表性縮圖
  → 選取影片，在檢視器檢視資料與影片專用壓縮方式
  → 套用品質快速選項，或選目標容量／解析度／codec 等進階控制
  → 個別影片可覆寫批次設定
  → 開始後顯示匯出階段與可信的進度；可取消或繼續處理其他佇列項目
  → 驗證實際輸出，再顯示節省結果
  → 以同一播放器檢視原片／輸出版本，或於 Finder 顯示輸出
```

## 互動與回饋

- 影片列直接顯示 poster 縮圖，疊加時長；播放只在明確操作後開始，不在清單自動播放。
- 分析中顯示「分析中」和不定進度；匯出進度以編碼器可提供的狀態／媒體時間計算，不得以經過時間製造假百分比或 ETA；驗證中顯示獨立階段。
- 取消時先停止匯出，再移除暫存檔；確認輸出完整前不得搬到正式檔名。
- 失敗時保留檔案列與錯誤原因；重試前重新讀取當前來源 metadata 和相容性，再用同一 job ID 回到佇列。圖片逐檔覆寫不得遺失。
- 預估容量與已完成容量使用不同視覺層級與標籤；若估算不可靠，隱藏數字並說明需要完成編碼才能得知。不要把品質 preset 標示成固定節省百分比。
- 比較檢視器提供播放／暫停、播放位置與原片／輸出切換。切換來源時盡可能保留播放位置；每個播放器不得同時發聲。支援 Escape、明確關閉按鈕、焦點回復及減少動態偏好。
- 重複加入、匯入不支援、匯出失敗、取消、略過與成功都提供靠近佇列的可見回饋和適當 VoiceOver 宣告。

## 非目標

- 影片剪輯、裁切片段、濾鏡、字幕製作、音軌混音、雲端處理、批次排程、暫停／續傳。
- 第一階段不承諾 HDR/Dolby Vision、空間影片、多視角、ProRes、多音訊、字幕或章節保留。
- 不覆蓋原片，不自動選用第三方 FFmpeg binary，不廣泛開放 WebView 對使用者檔案系統的讀取。
- 不把圖片品質選項直接套在影片上；影片編碼選項必須由實測與媒體能力決定。

## 驗收條件

1. 有效 MP4、MOV、M4V 可從原生選檔和拖放加入，錯誤檔案不阻塞同批有效檔案；副檔名偽裝、毀損、不支援 codec 或缺少視訊軌都有具體錯誤。
2. 含音訊／無音訊、H.264／HEVC、SD／HD／4K、直式方向與長時影片樣本驗證時長、方向、尺寸和音訊保留；HDR/多軌等邊界按規格明確拒絕。
3. 壓縮、略過、失敗、取消和重試都保持原圖工作流隔離且保留來源；暫存檔在取消／失敗時清理，輸出永不覆寫。
4. 輸出要可由 AVFoundation 重新讀取，且視訊軌／可預期的音訊軌存在；只有此時列為完成。
5. 輸入／輸出串流預覽不載入整部影片至記憶體，WebView 只能讀取當前工作已授權的精確路徑；不擴大為家目錄全開。
6. 至少涵蓋 800 × 600 和 1120 × 780：排隊資料、開始／取消、搜尋、選取、預覽與主行動均可達；讀屏和減少動態行為可用。
7. 以固定測試素材記錄輸出格式、解析度、實際 bytes、節省比例、耗時、音訊與方向結果；依此校正影片 preset 名稱，不能只測「命令成功」。

## 待使用者確認

1. 主要使用情境：手機影片歸檔省空間，還是分享前縮檔？這會影響預設編碼、相容性和縮放選項。
2. 使用者是否接受輸出 HEVC 才能換取較小檔案，或第一版只要更廣泛相容的 H.264？建議先提供 H.264 預設並把 HEVC 作為明確的小檔選項，前提是實測可行。
3. 是否需要影片預設縮到 1080p，還是預設保留解析度且只由使用者主動選擇縮小？建議保留來源解析度作預設。
4. 首版是否接受先不處理 HDR、Dolby Vision、多音軌和字幕？建議拒絕並清楚說明，不靜默轉成 SDR 或丟資料。
5. 是否同意實作計畫先做 encoder feasibility task，比較 Apple 原生路線與 FFmpeg 控制能力及發佈成本，再決定正式 integration？建議同意，因為目標容量與細緻品質控制可能超出 AVAssetExportSession presets 能提供的範圍。

## 研究記錄

編碼器比較、FFmpeg 發佈風險、Tauri sidecar 與媒體串流範圍整理於 [影片編碼器選項研究](../../research/2026-10-20-video-encoder-options.md)。研究尚未在目標 Mac 上量測 AVFoundation 輸出品質、媒體相容性或能耗；技術驗證結果應更新此規格後再進入實作計畫。

Compresto 參照補充於 2026-10-20：本規格採用其「快速品質選擇 + 需要時深入控制 + 逐檔覆寫」的產品分層作為方向；不複製其全部格式、轉 GIF、資料夾監控或目標平台 presets。Compresto 官網、公開 changelog 和工作情境頁列出目標容量、解析度調整、H.264／HEVC、影片/GIF 預覽與批次流程等功能。[官方網站](https://compresto.app/)、[官方 changelog](https://compresto.app/changelog)、[官方工作情境](https://compresto.app/use-case)。
