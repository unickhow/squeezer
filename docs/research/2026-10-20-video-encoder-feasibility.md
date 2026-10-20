# Squeezer video encoder feasibility results

Date: 2026-10-20
Host: macOS 26.0, arm64
Plan: [video compression plan](../superpowers/plans/2026-10-20-video-compression.md)
Tools: Swift 6.2, AVFoundation, local FFmpeg/ffprobe 8.1.1

## Decision

**Proceed with an Apple-native `AVAssetReader`/`AVAssetWriter` + VideoToolbox implementation, initially exposing H.264 output only. Defer HEVC output and exact target-size promises until more hardware/source samples validate rate control.** The native route encoded H.264 with track preservation, cancellation, and usable per-file bitrate control; the single 1080p synthetic fixture landed within 0.5–2.9% of two target sizes before any bounded retry. The tested HEVC encoder did not respond meaningfully to lower requested bitrates on that same fixture. Do not use the high-level export preset as the sole compressor: “Highest Quality” nearly passed through an already-encoded source, and the file-length cap exceeded its requested limit.

This is enough to select the initial implementation route, not enough to claim general quality or size behavior. Keep target size as an aspirational cap: measure the finished file, retry with a lower bitrate or smaller resolution when the target is missed, and tell the user when the target cannot be met without excessive quality loss. H.264 and HEVC output remain separately testable codec choices; only H.264 is approved for the first encoder integration based on these measurements.

## Fixture matrix

`tools/video-feasibility/fixtures.swift` generated five synthetic MP4s: 8-second 720p H.264 without audio, 12-second 1080p H.264/AAC with motion, 12-second portrait H.264/AAC, 90-second 720p HEVC/AAC, and 6-second 4K HEVC without audio. HEVC samples are tagged `hvc1` for Apple reader compatibility. The `testsrc2` pattern stresses moving detail; results do not represent natural camera footage, screen recordings, or subjective quality across scenes.

## Measurements

| Route and setting | Input → output | Time | Result |
|---|---:|---:|---|
| `AVAssetExportSession`, Highest Quality, 720p H.264 → MP4 | 8,727,723 → 8,727,625 bytes | 0.036 s | Same dimensions/codec; effectively passthrough |
| `AVAssetExportSession`, Medium Quality, 1080p H.264/AAC → MP4 | 24,943,801 → 1,231,015 bytes | 0.73 s | 568×320 H.264 + AAC; 95.1% smaller, but much lower than the requested 720p/1080p choices |
| `AVAssetExportSession`, 1280×720 preset, 1080p H.264/AAC → MP4 | 24,943,801 → 9,368,299 bytes | 1.29 s | 1280×720 H.264 + AAC; 62.4% smaller, 12-second duration retained |
| `AVAssetExportSession`, HEVC 1920×1080 preset, 1080p H.264/AAC → MP4 | 24,943,801 → 12,970,341 bytes | 2.27 s | 1920×1080 HEVC + AAC; 48.0% smaller |
| `AVAssetExportSession`, Medium Quality with 1,000,000-byte file-length cap | 24,943,801 → 1,134,291 bytes | 0.79 s | Exceeded the requested cap by 13.4% |
| Reader/Writer H.264, 2,000,000-byte target estimate | 24,943,801 → 2,010,395 bytes | 1.89 s | +0.52%; H.264 + original AAC; 12-second duration retained |
| Reader/Writer H.264, 1,500,000-byte target estimate | 24,943,801 → 1,542,724 bytes | 1.90 s | +2.85%; bitrate correction to 848 kbps still produced 1,536,510 bytes (+2.43%) |
| Reader/Writer HEVC, 2,000,000-byte target estimate | 24,943,801 → 2,629,646 bytes | 2.03 s | +31.5%; H.264 input’s AAC track and duration retained |
| Reader/Writer HEVC, requested 900 kbps and 400 kbps | 24,943,801 → 2,622,380 / 2,618,162 bytes | 2.39 / 2.06 s | Output stayed near 2.62 MB despite the requested bitrate change |

On the H.264/AAC target outputs, ffmpeg’s SSIM comparison against the synthetic source was 0.9627 for H.264 and 0.9643 for HEVC. This is a narrow metric on a generated clip, not a general visual-quality rating.

The Reader/Writer harness canceled a 90-second HEVC source after 0.2 seconds and removed its partial output. The first `hev1`-tagged generated HEVC sample failed in AVFoundation; retagging it as `hvc1` made it readable. The generator now emits that compatible tag. This is a fixture/container compatibility finding, not evidence that all `hev1` inputs should be rejected.

## Route assessment

### AVAssetExportSession

Fast and straightforward for a few fixed output presets. The preset determines both quality and often resolution; the Medium Quality result demonstrates why it cannot be presented as a user-selected 720p/1080p quality curve. The file-length limit is not a strict size guarantee. Use only if the UI wants fixed, empirically named presets and accepts actual result measurement.

### AVAssetReader/Writer + VideoToolbox

Supports explicit H.264 bitrate requests, audio passthrough, and cancellation in the harness. H.264 target misses were small on this clip, but not zero; lower bitrates eventually stopped shrinking the output enough to hit tighter targets. The observed HEVC behavior is not suitable for a first-release target-size control. Apple documents average bitrate as a desired long-term average rather than a hard limit and notes that some codecs may not support the setting; this matches the need to measure and validate each output. [Apple bitrate property](https://developer.apple.com/documentation/videotoolbox/kvtcompressionpropertykey_averagebitrate), [AVFoundation bitrate key](https://developer.apple.com/documentation/avfoundation/avvideoaveragebitratekey)

This route requires real per-frame decoding/re-encoding, muxing, audio timing, error cleanup, and metadata validation. Test image orientation and audio sync on phone clips before exposing quality levels. Do not claim hardware acceleration without checking the actual encoder session.

### FFmpeg

The local `/opt/homebrew/bin/ffmpeg` is version 8.1.1 and was built with `--enable-gpl`, `libx264`, and `libx265` among other options. It has no validated pinned-source provenance record and is therefore reference-only; the strict runner rejects it as a distribution candidate. No FFmpeg size/quality comparison is included in the decision data. A separately built, pinned configuration may be evaluated later, but it carries build, bundle, source/notice, and license review work. [FFmpeg download](https://www.ffmpeg.org/download.html), [FFmpeg legal notes](https://ffmpeg.org/legal.html)

## Integration constraints

- Register H.264 output first. Do not present HEVC output until a broader real-media matrix confirms bitrate responsiveness and device compatibility.
- Keep H.264/HEVC source inspection separate from output codec selection. At analysis time, report the real source tracks; reject only unsupported structures, not a tag the runtime can decode.
- Provide preserve-source, 1080p, and 720p sizing choices. The current low-level harness does not resize frames; Task 4 must add and validate real downscaling before the UI exposes those choices.
- A target size requires measured output and at least one correction attempt when oversize. If the encoder reaches its useful rate-control floor or the requested size implies unacceptable quality, return a target-missed result with the actual bytes and a resolution/codec suggestion; never report the target as achieved.
- Keep the high-level preset path as a measured comparison, not the production backend for target-size mode.

## Reproduction and scope

See [`tools/video-feasibility/README.md`](../../tools/video-feasibility/README.md) for fixture, preset, Reader/Writer, and FFmpeg runner commands. AVFoundation exports ran outside the restricted shell sandbox because the sandbox blocked a system virtualization query; outputs and synthetic sources stayed under `/private/tmp`. The local Homebrew FFmpeg was used only to generate fixtures and compute SSIM, not as the shipping encoder candidate.

The next implementation step is Task 2 in the approved plan, with Task 4 requiring a bounded downscale implementation and HEVC output deferred. Revisit the design if H.264-only output does not meet the actual user’s quality expectations on natural footage.
