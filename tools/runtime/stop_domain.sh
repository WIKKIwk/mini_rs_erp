#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

PUBLIC_HOSTNAME="${1:-${PUBLIC_HOSTNAME:-}}"
if [ -z "${PUBLIC_HOSTNAME:-}" ]; then
	echo "usage: $0 <hostname>" >&2
	exit 2
fi

STATE_ROOT="${STATE_ROOT:-$REPO_ROOT/garbage/domain}"
STATE_DIR="$STATE_ROOT/$PUBLIC_HOSTNAME"

stop_pid_file() {
	local file="$1"
	local label="$2"
	if [ ! -f "$file" ]; then
		return 0
	fi
	local pid
	pid="$(cat "$file" 2>/dev/null || true)"
	if [ -n "${pid:-}" ] && kill -0 "$pid" 2>/dev/null; then
		echo "stopping $label process: $pid"
		kill "$pid" 2>/dev/null || true
		sleep 1
		if kill -0 "$pid" 2>/dev/null; then
			kill -9 "$pid" 2>/dev/null || true
		fi
	fi
	rm -f "$file"
}

if [ ! -d "$STATE_DIR" ]; then
	echo "no runtime state found for $PUBLIC_HOSTNAME"
	exit 0
fi

stop_pid_file "$STATE_DIR/cloudflared.pid" "cloudflared"
stop_pid_file "$STATE_DIR/mini_rs_erp.pid" "mini_rs_erp"
rm -f "$STATE_DIR/url"

echo "stopped local runtime for $PUBLIC_HOSTNAME"
