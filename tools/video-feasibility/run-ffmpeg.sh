#!/bin/sh
set -eu

usage() {
  cat <<'USAGE'
Usage: run-ffmpeg.sh --ffmpeg PATH --provenance FILE --input FILE --output FILE --encoder NAME --bitrate BPS
Accepted encoders: libx264, libx265, h264_videotoolbox, hevc_videotoolbox
USAGE
}

ffmpeg_path=
provenance_path=
input_path=
output_path=
encoder=
bitrate=

while [ "$#" -gt 0 ]; do
  case "$1" in
    --ffmpeg) ffmpeg_path=${2-}; shift 2 ;;
    --provenance) provenance_path=${2-}; shift 2 ;;
    --input) input_path=${2-}; shift 2 ;;
    --output) output_path=${2-}; shift 2 ;;
    --encoder) encoder=${2-}; shift 2 ;;
    --bitrate) bitrate=${2-}; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done

if [ -z "$ffmpeg_path" ] || [ -z "$provenance_path" ] || [ -z "$input_path" ] || [ -z "$output_path" ] || [ -z "$encoder" ] || [ -z "$bitrate" ]; then
  usage >&2
  exit 2
fi
if [ ! -x "$ffmpeg_path" ] || [ ! -f "$provenance_path" ] || [ ! -f "$input_path" ] || [ -e "$output_path" ]; then
  echo "FFmpeg, provenance, or input is missing, or output already exists." >&2
  exit 2
fi
case "$encoder" in
  libx264|libx265|h264_videotoolbox|hevc_videotoolbox) ;;
  *) echo "Unsupported encoder: $encoder" >&2; exit 2 ;;
esac
case "$bitrate" in
  ''|*[!0-9]*) echo "Bitrate must be a positive integer." >&2; exit 2 ;;
esac
if [ "$bitrate" -le 0 ]; then
  echo "Bitrate must be a positive integer." >&2
  exit 2
fi

grep -Eq '^source=https://git\.ffmpeg\.org/ffmpeg\.git$' "$provenance_path" || {
  echo "Provenance must name the official FFmpeg source repository." >&2
  exit 2
}
grep -Eq '^commit=[0-9a-f]{40}$' "$provenance_path" || {
  echo "Provenance must contain a 40-character source commit." >&2
  exit 2
}
version_line=$("$ffmpeg_path" -version | sed -n '1p')
configuration_line=$("$ffmpeg_path" -version | sed -n 's/^configuration: //p')
grep -Fqx "version=$version_line" "$provenance_path" || {
  echo "FFmpeg version does not match provenance." >&2
  exit 2
}
grep -Fqx "configuration=$configuration_line" "$provenance_path" || {
  echo "FFmpeg configure string does not match provenance." >&2
  exit 2
}
"$ffmpeg_path" -encoders 2>/dev/null | grep -Eq "[[:space:]]${encoder}[[:space:]]" || {
  echo "Configured FFmpeg does not expose encoder $encoder." >&2
  exit 2
}

start_seconds=$(date +%s)
"$ffmpeg_path" -hide_banner -nostdin -n -i "$input_path" \
  -map 0:v:0 -map '0:a?' -c:v "$encoder" -b:v "$bitrate" \
  -maxrate "$bitrate" -bufsize "$((bitrate * 2))" -c:a copy \
  -movflags +faststart "$output_path"
end_seconds=$(date +%s)
output_bytes=$(stat -f %z "$output_path")
echo "encoder=$encoder"
echo "requested_video_bitrate=$bitrate"
echo "output_bytes=$output_bytes"
echo "elapsed_seconds=$((end_seconds - start_seconds))"
"${ffmpeg_path%/*}/ffprobe" -v error \
  -show_entries format=duration,size \
  -show_entries stream=codec_type,codec_name,width,height,avg_frame_rate,channels \
  -of json "$output_path"
