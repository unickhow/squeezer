# Video encoder feasibility tools

These tools compare local encoder behavior before choosing a production backend. They are development tools only and are not included in the Squeezer bundle.

## Generate repeatable inputs

```sh
swift tools/video-feasibility/fixtures.swift --output-dir /tmp/squeezer-video-fixtures
```

The generator uses FFmpeg found in `PATH` only to create test inputs. It emits five synthetic clips: 720p H.264 with no audio, 1080p H.264/AAC with motion, portrait H.264/AAC, a 90-second 720p HEVC/AAC clip, and short 4K HEVC without audio. These clips are deterministic in content and settings, though encoded bytes can vary with the local FFmpeg build.

## AVFoundation export presets

```sh
swift tools/video-feasibility/avfoundation-export.swift --list-presets
swift tools/video-feasibility/avfoundation-export.swift \
  --input /tmp/squeezer-video-fixtures/short-h264-aac-motion.mp4 \
  --output /tmp/squeezer-video-fixtures/apple-medium.mp4 \
  --preset AVAssetExportPresetMediumQuality
```

The harness checks the selected session's supported file types, exports MP4, and reports actual bytes and elapsed time. Use `ffprobe` to verify output codec, dimensions, duration, and audio tracks.

## AVAssetReader/Writer + VideoToolbox

```sh
swift tools/video-feasibility/avfoundation-reader-writer.swift \
  --input /tmp/squeezer-video-fixtures/short-h264-aac-motion.mp4 \
  --output /tmp/squeezer-video-fixtures/apple-writer-hevc.mp4 \
  --codec hevc --target-bytes 2000000
```

The harness decodes video frames, re-encodes video with an explicit average bitrate, and passes through the first audio track. `--target-bytes` reserves 128 kbps for an audio track; actual container overhead means the requested byte count is an aim, not a guarantee. `--cancel-after SECONDS` exercises cancellation and removes the partial output. The current harness preserves the source dimensions and preferred transform; it does not implement resizing or multi-track audio.

## FFmpeg candidate runs

`run-ffmpeg.sh` requires an explicit executable and provenance file for a build from the official FFmpeg source repository. The provenance file must contain exactly these required fields (additional lines may describe licenses and notices):

```text
source=https://git.ffmpeg.org/ffmpeg.git
commit=<40-character-source-commit>
version=<exact first line of ffmpeg -version>
configuration=<exact configuration line without the `configuration: ` prefix>
```

The script verifies the commit format, version, and configure string before invoking an encoder. It does not decide whether a given FFmpeg configuration is legally suitable for distribution; record every enabled library and review the applicable license obligations separately. Do not use third-party prebuilt binaries as a distribution candidate.

Example:

```sh
tools/video-feasibility/run-ffmpeg.sh \
  --ffmpeg /path/to/pinned-build/bin/ffmpeg \
  --provenance /path/to/pinned-build/provenance.txt \
  --input /tmp/squeezer-video-fixtures/short-h264-aac-motion.mp4 \
  --output /tmp/squeezer-video-fixtures/ffmpeg-h264.mp4 \
  --encoder libx264 --bitrate 1200000
```

## Comparison criteria

For every route, record: input fixture and build/version, source and output bytes, output codec and dimensions, duration, audio/orientation preservation, elapsed time, cancellation cleanup, target-size miss, and image quality. Use objective metrics such as SSIM only as a supplement to visual inspection; compare resized outputs against a reference at the same dimensions. Record unavailable capabilities and licensing/packaging work rather than implying they passed.
