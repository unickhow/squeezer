# Squeezer 影片編碼器選項研究

日期：2026-10-20  
範圍：目前是 macOS-only 的 Tauri 2 / Rust 桌面程式；此筆記聚焦編碼器與發佈，不是完整產品規格。  
現況依據：[README.md](../../README.md) 指出目前只處理 JPEG/PNG，影片、HDR、多音訊軌都不在 MVP 內；[Tauri 設定](../../src-tauri/tauri.conf.json) 目前只打包 app 與 dmg，沒有 sidecar。

## 摘要與建議

**建議第一版走 Apple AVFoundation 的 `AVAssetExportSession`，由小型 Swift helper 執行匯出；先限定輸入 MP4/MOV 中常見的 H.264/HEVC 視訊與 AAC 音訊，輸出 MP4/H.264 或 HEVC，預設保留音訊。** 這符合現有 macOS-only 定位，不需攜帶 FFmpeg 發佈物，也可先用系統 export preset 上線。匯出 preset、容器及來源資產是否相容都要在執行期查詢；不能把副檔名當成真正格式，也不能承諾固定壓縮率。[Apple：替代格式匯出](https://developer.apple.com/documentation/avfoundation/exporting-video-to-alternative-formats)、[export presets](https://developer.apple.com/documentation/avfoundation/export-presets)

如果產品需求是精確 bitrate、音訊逐軌控制、更多容器/編碼器、非 Apple 平台，或必須有可預期的品質旋鈕，再評估 FFmpeg sidecar。可用 FFmpeg 的 VideoToolbox H.264/HEVC encoder 取得 Apple 硬體編碼路徑，但這仍要自行建置、驗證、打包及履行 FFmpeg 與外部元件授權義務；硬體能力不是每台 Mac、每種設定都保證可用。[FFmpeg VideoToolbox encoder source](https://ffmpeg.org/doxygen/trunk/videotoolboxenc_8c.html)、[FFmpeg 授權說明](https://ffmpeg.org/legal.html)

## 路線比較

| 路線 | 優點 | 取捨與責任 | 初步適配 |
|---|---|---|---|
| AVFoundation `AVAssetExportSession` | macOS 系統 API；高階匯出流程；Apple 提供 H.264/HEVC 與解析度 presets，且可查 preset/container 相容性 | preset 能控制的選項較少；相容性和支援格式隨來源、作業系統能力而異；對多音訊/字幕/特殊 metadata 的保留行為需實測 | **第一版首選**：本專案目前只支援 macOS，先建立清楚且有限的影片工作流程 |
| AVFoundation `AVAssetReader` + `AVAssetWriter` / `VideoToolbox` | 可逐軌讀取、設定輸出、建立 MOV/MP4；VideoToolbox 可要求硬體 encoder；更適合自訂品質與媒體處理 | 需要處理 sample buffer、時間戳、音訊同步、muxing、失敗與取消；工作量明顯高於 export session | 只有當 preset 不足以支撐產品控制時才升級 |
| FFmpeg sidecar | 編碼器與封裝格式選擇廣；CLI 進度可被包裝成 app 事件；可維持 Tauri/Rust app 主體不依賴 FFmpeg Rust bindings | 官方只提供 FFmpeg source；需要為 Apple Silicon / Intel 建置與簽署可執行檔、管理更新及完整第三方授權清單。選用 GPL 元件會改變 FFmpeg 的授權；`--enable-nonfree` 產物不可再散佈 | 可作後續擴充或跨平台需求，但不是最低風險的 macOS MVP |

## FFmpeg 發佈、授權與打包

- FFmpeg 官方網站提供原始碼，預編譯可執行檔連結是第三方提供，並非 FFmpeg 官方建置服務。[FFmpeg Download](https://www.ffmpeg.org/download.html)
- FFmpeg 預設為 LGPL 2.1 或更新版本；啟用部分 GPL 程式碼/最佳化後，整個 FFmpeg 需按 GPL 發佈。FFmpeg 官方特別列出 `--enable-gpl`、`--enable-nonfree`、動態連結、對應原始碼、建置設定與版權告示等注意事項；使用 `--enable-nonfree` 的二進位不可散佈。用 `libx264` 是常見 GPL 觸發項，FFmpeg 官方 checklist 明確提醒不可把 GPL library 混入 LGPL build。[FFmpeg Legal](https://ffmpeg.org/legal.html)
- 上述 FFmpeg checklist 明確以「linking against FFmpeg libraries」為主；把 FFmpeg CLI 作為獨立 sidecar 的具體產品授權結構並未由該清單完整裁定。**不能假設把 CLI 放在另一個 process 就免掉 FFmpeg 本身的散佈義務**；應保留精確 source、build config、license texts、元件清單與 notices，發布前請法務依實際組態確認。此處不是法律意見。[FFmpeg Legal](https://ffmpeg.org/legal.html)
- Tauri v2 可用 `bundle.externalBin` 打包外部可執行檔；每個目標需要帶 Rust target triple 後綴的 binary。若 sidecar 由 Rust 呼叫，可使用 Tauri Shell plugin 的 sidecar API；若由前端呼叫，需在 capability 明確授權允許的命令及參數。這會增加授權面與輸入驗證責任，應限制可執行名稱及參數，不接受任意 shell 字串。[Tauri：Embedding External Binaries](https://v2.tauri.app/develop/sidecar/)
- 若採 FFmpeg，建議自己固定版本與 configure flags，先限定 LGPL 組態、不啟用 GPL / nonfree library，並以 `-buildconf`、`-encoders`、`-muxers` 對發佈產物做稽核；但實際可用 encoder 仍以所建置的 binary 為準。FFmpeg 官方下載頁指出只有 source 是官方發佈物，因此不應把任意第三方 static build 當成可直接商用再散佈的無責任依賴。[FFmpeg Download](https://www.ffmpeg.org/download.html)、[FFmpeg Legal](https://ffmpeg.org/legal.html)
- H.264 / HEVC 的實作授權與 codec 專利是不同議題；FFmpeg 官方法律頁亦指出專利風險取決於司法管轄區與商業用途。不能因為使用 Apple API 或 LGPL build，就推定所有分發情境均免於專利授權評估。[FFmpeg Legal](https://ffmpeg.org/legal.html)

## Apple API 與硬體編碼

- `AVAssetExportSession` 以來源 asset、preset、output file type 與 URL 設定輸出。Apple 要求先查詢 preset 對來源與目標檔案類型的相容性；export session 也提供 `supportedFileTypes`。因此應在加入工作、顯示選項或開始處理前檢查相容性，並對不相容來源顯示明確原因。[Apple：替代格式匯出](https://developer.apple.com/documentation/avfoundation/exporting-video-to-alternative-formats)、[AVAssetExportSession](https://developer.apple.com/documentation/avfoundation/avassetexportsession)
- Apple presets 列出 H.264 + AAC 的 SD/HD/UHD 選項、HEVC + AAC 1080p/4K 選項，也有「最高品質 HEVC」及 passthrough preset。它們是固定 preset 模板，並非自由 bitrate/quality 曲線；壓縮後可能變大，應顯示實際輸入/輸出大小並允許設定「節省低於門檻時略過」。[Apple：export presets](https://developer.apple.com/documentation/avfoundation/export-presets)
- VideoToolbox 是低階硬體/軟體壓縮解壓框架。建立 `VTCompressionSession` 可指定 codec、尺寸與屬性；設定 `RequireHardwareAcceleratedVideoEncoder` 會在硬體編碼不可用時失敗。Apple 說明不可用情況包含硬體缺席、codec/configuration 不支援及硬體資源忙碌。因此只能「要求或偏好硬體加速」，不能在 UI 保證所有機器都會走硬體路徑；需讀取實際 session property 或採容錯 fallback。[Apple：VideoToolbox](https://developer.apple.com/documentation/videotoolbox)、[Require hardware encoder](https://developer.apple.com/documentation/videotoolbox/kvtvideoencoderspecification_requirehardwareacceleratedvideoencoder)
- `AVAssetWriter` 可輸出 QuickTime `.mov` 與 MPEG-4 `.mp4`，並可重新編碼寫入的 media samples；`AVAssetWriterInput` 可按 media track 建立輸入，`outputSettings: nil` 則表示不重新編碼、直接 pass through samples。要自己掌控編碼流程時，通常由 `AVAssetReader` 取樣、`AVAssetWriter` 封裝；此路線更能控制 track，但 app 要自行確保音訊/視訊時間軸與 metadata 行為。[Apple：AVAssetWriter](https://developer.apple.com/documentation/avfoundation/avassetwriter)、[Writer input settings](https://developer.apple.com/documentation/avfoundation/avassetwriterinput/outputsettings)、[AVAssetReader](https://developer.apple.com/documentation/avfoundation/avassetreader)

## 容器與轉碼邊界

- `.mp4` 是 MPEG-4 file type，`.mov` 是 QuickTime movie file type；容器和 codec 是不同層，副檔名本身不代表可解碼的視訊/音訊 codec。Apple API 有分開的 `AVFileType.mp4` / `.mov`，並要求以 `AVAssetExportSession` 動態判斷來源與目標組合是否可匯出。[Apple：AVFileType](https://developer.apple.com/documentation/avfoundation/avfiletype)、[相容性流程](https://developer.apple.com/documentation/avfoundation/exporting-video-to-alternative-formats)
- 建議 MVP 對輸入副檔名採常見 `.mp4`, `.mov`, `.m4v`，但匯入後仍以 AVFoundation 能否讀取、是否有可處理 video track、輸出 preset/容器是否相容為準。`.mkv`, `.webm`, AVI 與專業/相機格式暫不承諾支援；只有在實機素材測試證明支援面和錯誤提示足夠後再擴大格式表。這是產品範圍建議，不是 Apple 對這些副檔名的完整相容性保證。
- 將 H.264 轉成 HEVC、改解析度或改 bitrate 都是重新編碼；有損 codec 轉碼通常會再次損失畫質。僅做 stream copy 可以避免重新編碼，但不會達成大幅縮小檔案的目標，而且目標容器是否能裝入原始 tracks 仍要驗證。Apple 將格式轉換描述為基於原片內容產生新 movie，並提供 passthrough 作為獨立 preset。[Apple：替代格式匯出](https://developer.apple.com/documentation/avfoundation/exporting-video-to-alternative-formats)、[export presets](https://developer.apple.com/documentation/avfoundation/export-presets)
- 第一版建議拒絕或明確標示暫不支援 HDR/Dolby Vision、空間影片、多視角、字幕/章節、替代音軌、外掛 metadata、ProRes 等複雜內容，不默默丟軌或錯誤改寫色彩資訊。現行 README 已將 HDR、多音訊列為非 MVP；Apple 的媒體 API 支援更複雜的 track 和 color metadata，表示之後可獨立規劃驗證而非一開始隱性承諾。[現有範圍](../../README.md)、[AVAssetTrack](https://developer.apple.com/documentation/avfoundation/avassettrack)、[Apple：HDR video](https://developer.apple.com/av-foundation/Incorporating-HDR-video-with-Dolby-Vision-into-your-apps.pdf)

## 待產品規格決定

1. 第一版只輸出 MP4/H.264，還是提供 MP4/HEVC？H.264 相容性較廣；HEVC 較有效率但不能保證所有目標設備相容。Apple 提供兩者 presets，但具體預設應由目標使用場景決定。
2. 音訊保留策略：原音訊 pass-through、重編 AAC，或第一版只支援單一音軌？多音訊/替代音軌目前明確不在範圍。
3. 品質控制呈現為少數「較小 / 平衡 / 較清晰」preset，還是進階 bitrate？AVAssetExportSession 的 preset 不等同可承諾的品質曲線；若需求為自由 bitrate，更應先做 `AVAssetWriter`/VideoToolbox 原型或選 FFmpeg。
4. 轉碼結果少於 10% 節省時是否略過、保留原檔？圖片目前採 10% 門檻，可沿用作起點但影片需用代表性素材校準。
5. MVP 可否接受逐一串列處理、不可 pause/resume？需有可取消的匯出 task、進度、殘留暫存檔清理與輸出完整性驗證；AVAssetExportSession 支援進度與取消。[Apple：export session](https://developer.apple.com/documentation/avfoundation/avassetexportsession)、[async export/cancel](https://developer.apple.com/documentation/avfoundation/avassetexportsession/export%28to%3Aas%3Aisolation%3A)

## 尚未驗證

- 沒有在專案上實作或量測 Swift helper、AVAssetExportSession preset 的實際節省量、CPU/電池、進度粒度、失敗恢復，以及各種輸入樣本的 track/metadata 保留情形。
- Apple 文件會隨 macOS 版本變動；支援格式、export presets 和硬體 encoder 能力要以最低支援 macOS 版本與目標機種實測確認。
- FFmpeg 官方法律頁提供專案自身的合規說明，不代替針對商用發佈、codec patent pool、各地法規或具體第三方 build 的法律審查。
