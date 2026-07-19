#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BIN_DIR="$REPO_ROOT/../.tools/ffmpeg/bin"

require_cmd() {
	if ! command -v "$1" >/dev/null 2>&1; then
		echo "required command not found: $1" >&2
		exit 1
	fi
}

require_cmd curl
require_cmd gzip
require_cmd shasum
require_cmd tar

platform="$(uname -s)"
architecture="$(uname -m)"
if [ "$platform" != "Darwin" ] || [ "$architecture" != "arm64" ]; then
	echo "unsupported chat media processor platform: $platform/$architecture" >&2
	echo "set MOBILE_CHAT_MEDIA_FFMPEG_BIN and MOBILE_CHAT_MEDIA_FFPROBE_BIN explicitly" >&2
	exit 2
fi

ffmpeg_url="https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-darwin-arm64.gz"
ffmpeg_sha256="a90e3db6a3fd35f6074b013f948b1aa45b31c6375489d39e572bea3f18336584"
ffprobe_url="https://registry.npmjs.org/@ffprobe-installer/darwin-arm64/-/darwin-arm64-5.0.1.tgz"
ffprobe_sha256="c846d5db9d3b5bc33f987725e21f3ea14953931221c191575918e907ad6c18ff"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/mini-rs-chat-media.XXXXXX")"
trap 'rm -rf -- "$work_dir"' EXIT

curl --fail --location --retry 3 --silent --show-error \
	"$ffmpeg_url" --output "$work_dir/ffmpeg.gz"
gzip -dc "$work_dir/ffmpeg.gz" >"$work_dir/ffmpeg"

curl --fail --location --retry 3 --silent --show-error \
	"$ffprobe_url" --output "$work_dir/ffprobe.tgz"
mkdir -p "$work_dir/ffprobe-package"
tar -xzf "$work_dir/ffprobe.tgz" -C "$work_dir/ffprobe-package"
cp "$work_dir/ffprobe-package/package/ffprobe" "$work_dir/ffprobe"

printf '%s  %s\n' "$ffmpeg_sha256" "$work_dir/ffmpeg" | shasum -a 256 -c -
printf '%s  %s\n' "$ffprobe_sha256" "$work_dir/ffprobe" | shasum -a 256 -c -

mkdir -p "$BIN_DIR"
install -m 0755 "$work_dir/ffmpeg" "$BIN_DIR/ffmpeg.next"
install -m 0755 "$work_dir/ffprobe" "$BIN_DIR/ffprobe.next"
mv "$BIN_DIR/ffmpeg.next" "$BIN_DIR/ffmpeg"
mv "$BIN_DIR/ffprobe.next" "$BIN_DIR/ffprobe"

if ! "$BIN_DIR/ffmpeg" -hide_banner -encoders 2>/dev/null | grep -Eq '[[:space:]]aac[[:space:]]'; then
	echo "installed ffmpeg does not provide the AAC encoder" >&2
	exit 1
fi
if ! "$BIN_DIR/ffmpeg" -hide_banner -filters 2>/dev/null | grep -Eq '[[:space:]]showwavespic[[:space:]]'; then
	echo "installed ffmpeg does not provide the showwavespic filter" >&2
	exit 1
fi
if ! "$BIN_DIR/ffprobe" -version >/dev/null 2>&1; then
	echo "installed ffprobe cannot be executed" >&2
	exit 1
fi

probe_report="$work_dir/probe.txt"
probe_output="$work_dir/probe.m4a"
"$BIN_DIR/ffmpeg" -nostdin -hide_banner -loglevel error -y \
	-f lavfi -i "sine=frequency=440:duration=0.2" \
	-c:a aac -b:a 64k -ar 48000 -ac 1 "$probe_output"
"$BIN_DIR/ffprobe" -v error -select_streams a:0 \
	-show_entries stream=codec_name,channels,sample_rate \
	-of default=noprint_wrappers=1 "$probe_output" >"$probe_report"
grep -Fxq 'codec_name=aac' "$probe_report"
grep -Fxq 'sample_rate=48000' "$probe_report"
grep -Fxq 'channels=1' "$probe_report"

echo "chat media processor installed: $BIN_DIR/ffmpeg / $BIN_DIR/ffprobe"
