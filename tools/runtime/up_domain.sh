#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

PUBLIC_HOSTNAME="${1:-${PUBLIC_HOSTNAME:-}}"
if [ -z "${PUBLIC_HOSTNAME:-}" ]; then
	echo "usage: $0 <hostname>" >&2
	echo "example: $0 mini-rs-erp-test.wspace.sbs" >&2
	exit 2
fi

case "$PUBLIC_HOSTNAME" in
	*://*|*/*|*:*|.*|*.)
		echo "invalid hostname: $PUBLIC_HOSTNAME" >&2
		exit 2
		;;
esac

if ! printf '%s' "$PUBLIC_HOSTNAME" | grep -Eq '^[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?(\.[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?)+$'; then
	echo "invalid hostname: $PUBLIC_HOSTNAME" >&2
	exit 2
fi

PORT="${PORT:-18081}"
CORE_URL="${CORE_URL:-http://127.0.0.1:$PORT}"
MOBILE_API_ADDR="${MOBILE_API_ADDR:-127.0.0.1:$PORT}"
RUST_LOG="${RUST_LOG:-info}"
BUILD_RELEASE="${BUILD_RELEASE:-1}"
REQUIRE_DATABASE_URL="${REQUIRE_DATABASE_URL:-0}"
ROUTE_DNS="${ROUTE_DNS:-1}"

configure_chat_media_processor() {
	local ffmpeg="${MOBILE_CHAT_MEDIA_FFMPEG_BIN:-}"
	local ffprobe="${MOBILE_CHAT_MEDIA_FFPROBE_BIN:-}"
	local bundled_bin="$REPO_ROOT/../.tools/ffmpeg/bin"

	if [ -z "$ffmpeg" ] && [ -x "$bundled_bin/ffmpeg" ]; then
		ffmpeg="$bundled_bin/ffmpeg"
	fi
	if [ -z "$ffprobe" ] && [ -x "$bundled_bin/ffprobe" ]; then
		ffprobe="$bundled_bin/ffprobe"
	fi
	if [ -z "$ffmpeg" ]; then
		ffmpeg="$(command -v ffmpeg 2>/dev/null || true)"
	fi
	if [ -z "$ffprobe" ]; then
		ffprobe="$(command -v ffprobe 2>/dev/null || true)"
	fi

	if [ -n "$ffmpeg" ] && [ ! -x "$ffmpeg" ]; then
		echo "configured ffmpeg is not executable: $ffmpeg" >&2
		exit 1
	fi
	if [ -n "$ffprobe" ] && [ ! -x "$ffprobe" ]; then
		echo "configured ffprobe is not executable: $ffprobe" >&2
		exit 1
	fi
	if [ -z "$ffmpeg" ] || [ -z "$ffprobe" ]; then
		echo "ffmpeg/ffprobe unavailable; chat video/audio processing cannot start" >&2
		echo "run: $REPO_ROOT/tools/runtime/setup_chat_media_processor.sh" >&2
		exit 1
	fi
	if ! "$ffmpeg" -version >/dev/null 2>&1; then
		echo "configured ffmpeg cannot be executed: $ffmpeg" >&2
		exit 1
	fi
	if ! "$ffprobe" -version >/dev/null 2>&1; then
		echo "configured ffprobe cannot be executed: $ffprobe" >&2
		exit 1
	fi

	local verification_dir
	verification_dir="$(mktemp -d "${TMPDIR:-/tmp}/mini-rs-chat-media.XXXXXX")"
	if ! "$ffmpeg" -hide_banner -encoders >"$verification_dir/encoders.txt" 2>&1 ||
		! grep -Eq '[[:space:]]aac[[:space:]]' "$verification_dir/encoders.txt"; then
		rm -rf -- "$verification_dir"
		echo "configured ffmpeg does not provide the AAC encoder: $ffmpeg" >&2
		exit 1
	fi
	if ! "$ffmpeg" -hide_banner -filters >"$verification_dir/filters.txt" 2>&1 ||
		! grep -Eq '[[:space:]]showwavespic[[:space:]]' "$verification_dir/filters.txt"; then
		rm -rf -- "$verification_dir"
		echo "configured ffmpeg does not provide the showwavespic filter: $ffmpeg" >&2
		exit 1
	fi
	if ! "$ffmpeg" -nostdin -hide_banner -loglevel error -y \
		-f lavfi -i "sine=frequency=440:duration=0.2" \
		-c:a aac -b:a 64k -ar 48000 -ac 1 "$verification_dir/probe.m4a" ||
		! "$ffprobe" -v error -select_streams a:0 \
			-show_entries stream=codec_name,channels,sample_rate \
			-of default=noprint_wrappers=1 "$verification_dir/probe.m4a" \
			>"$verification_dir/probe.txt" ||
		! grep -Fxq 'codec_name=aac' "$verification_dir/probe.txt" ||
		! grep -Fxq 'sample_rate=48000' "$verification_dir/probe.txt" ||
		! grep -Fxq 'channels=1' "$verification_dir/probe.txt"; then
		rm -rf -- "$verification_dir"
		echo "chat media processor probe failed: $ffmpeg / $ffprobe" >&2
		exit 1
	fi
	rm -rf -- "$verification_dir"

	export MOBILE_CHAT_MEDIA_FFMPEG_BIN="$ffmpeg"
	export MOBILE_CHAT_MEDIA_FFPROBE_BIN="$ffprobe"
	echo "chat media processor ready: $ffmpeg / $ffprobe"
}

HOSTNAME_SLUG="$(
	printf '%s' "$PUBLIC_HOSTNAME" |
		tr '[:upper:]' '[:lower:]' |
		tr -c 'a-z0-9-' '-' |
		sed 's/--*/-/g; s/^-//; s/-$//' |
		cut -c1-120
)"
TUNNEL_NAME="${TUNNEL_NAME:-mini-rs-erp-$HOSTNAME_SLUG}"

STATE_ROOT="${STATE_ROOT:-$REPO_ROOT/garbage/domain}"
STATE_DIR="$STATE_ROOT/$PUBLIC_HOSTNAME"
mkdir -p "$STATE_DIR"

APP_PID="$STATE_DIR/mini_rs_erp.pid"
APP_LOG="$STATE_DIR/mini_rs_erp.log"
TUNNEL_PID="$STATE_DIR/cloudflared.pid"
TUNNEL_LOG="$STATE_DIR/cloudflared.log"
TUNNEL_CONFIG="$STATE_DIR/cloudflared.yml"
URL_FILE="$STATE_DIR/url"

require_cmd() {
	if ! command -v "$1" >/dev/null 2>&1; then
		echo "required command not found: $1" >&2
		exit 1
	fi
}

spawn_detached() {
	local pid_file="$1"
	local log_file="$2"
	local cwd="$3"
	shift 3
	python3 - "$pid_file" "$log_file" "$cwd" "$@" <<'PY'
import os
import subprocess
import sys

pid_file, log_file, cwd = sys.argv[1:4]
cmd = sys.argv[4:]

with open(log_file, "ab", buffering=0) as log, open(os.devnull, "rb") as devnull:
    proc = subprocess.Popen(
        cmd,
        cwd=cwd,
        stdin=devnull,
        stdout=log,
        stderr=log,
        start_new_session=True,
    )

with open(pid_file, "w", encoding="utf-8") as fh:
    fh.write(str(proc.pid))
PY
}

has_database_url() {
	if [ -n "${MINI_ERP_DATABASE_URL:-}" ]; then
		return 0
	fi
	if [ -f "$REPO_ROOT/.env" ] && grep -Eq '^[[:space:]]*MINI_ERP_DATABASE_URL[[:space:]]*=[[:space:]]*[^[:space:]#]+' "$REPO_ROOT/.env"; then
		return 0
	fi
	return 1
}

stop_pid_file() {
	local file="$1"
	local label="$2"
	if [ ! -f "$file" ]; then
		return 0
	fi
	local pid
	pid="$(cat "$file" 2>/dev/null || true)"
	if [ -n "${pid:-}" ] && kill -0 "$pid" 2>/dev/null; then
		echo "stopping stale $label process: $pid"
		kill "$pid" 2>/dev/null || true
		sleep 1
		if kill -0 "$pid" 2>/dev/null; then
			kill -9 "$pid" 2>/dev/null || true
		fi
	fi
	rm -f "$file"
}

wait_for_health() {
	local url="$1"
	local label="$2"
	for _ in $(seq 1 80); do
		if curl -fsS "$url/healthz" >/dev/null 2>&1; then
			echo "$label ready: $url"
			return 0
		fi
		sleep 0.5
	done
	return 1
}

ensure_backend() {
	if curl -fsS "$CORE_URL/healthz" >/dev/null 2>&1; then
		echo "mini_rs_erp already running: $CORE_URL"
		return 0
	fi

	stop_pid_file "$APP_PID" "mini_rs_erp"

	if [ "$REQUIRE_DATABASE_URL" = "1" ] && ! has_database_url; then
		echo "MINI_ERP_DATABASE_URL is required when REQUIRE_DATABASE_URL=1" >&2
		exit 1
	fi

	if [ "$BUILD_RELEASE" = "1" ]; then
		(cd "$REPO_ROOT" && cargo build --release)
	fi

	local binary="$REPO_ROOT/target/release/mini_rs_erp"
	if [ ! -x "$binary" ]; then
		echo "release binary not found: $binary" >&2
		echo "set BUILD_RELEASE=1 or build the service first" >&2
		exit 1
	fi

	rm -f "$APP_LOG"
	export MOBILE_API_ADDR RUST_LOG
	spawn_detached "$APP_PID" "$APP_LOG" "$REPO_ROOT" "$binary"

	if ! wait_for_health "$CORE_URL" "mini_rs_erp"; then
		echo "mini_rs_erp failed to start; see $APP_LOG" >&2
		exit 1
	fi
}

find_tunnel_id() {
	cloudflared tunnel list 2>/dev/null | awk -v name="$TUNNEL_NAME" '$2 == name {print $1; exit}'
}

ensure_tunnel() {
	require_cmd cloudflared

	local tunnel_id
	tunnel_id="$(find_tunnel_id)"
	if [ -z "${tunnel_id:-}" ]; then
		cloudflared tunnel create "$TUNNEL_NAME" >/dev/null
		tunnel_id="$(find_tunnel_id)"
	fi
	if [ -z "${tunnel_id:-}" ]; then
		echo "failed to resolve tunnel id for $TUNNEL_NAME" >&2
		exit 1
	fi

	local credentials_file="$HOME/.cloudflared/$tunnel_id.json"
	if [ ! -f "$credentials_file" ]; then
		echo "tunnel credentials not found: $credentials_file" >&2
		exit 1
	fi

	if [ "$ROUTE_DNS" = "1" ]; then
		cloudflared tunnel route dns --overwrite-dns "$TUNNEL_NAME" "$PUBLIC_HOSTNAME" >/dev/null
	fi

	if curl -fsS "https://$PUBLIC_HOSTNAME/healthz" >/dev/null 2>&1; then
		printf 'https://%s\n' "$PUBLIC_HOSTNAME" >"$URL_FILE"
		echo "public endpoint ready: https://$PUBLIC_HOSTNAME"
		return 0
	fi

	stop_pid_file "$TUNNEL_PID" "cloudflared"

	cat >"$TUNNEL_CONFIG" <<EOF
tunnel: $tunnel_id
credentials-file: $credentials_file

ingress:
  - hostname: $PUBLIC_HOSTNAME
    service: $CORE_URL
  - service: http_status:404
EOF

	rm -f "$TUNNEL_LOG"
	spawn_detached "$TUNNEL_PID" "$TUNNEL_LOG" "$REPO_ROOT" cloudflared tunnel --config "$TUNNEL_CONFIG" run "$TUNNEL_NAME"

	if ! wait_for_health "https://$PUBLIC_HOSTNAME" "public endpoint"; then
		echo "cloudflared tunnel failed; see $TUNNEL_LOG" >&2
		exit 1
	fi

	printf 'https://%s\n' "$PUBLIC_HOSTNAME" >"$URL_FILE"
}

require_cmd curl
configure_chat_media_processor
ensure_backend
ensure_tunnel

cat "$URL_FILE"
