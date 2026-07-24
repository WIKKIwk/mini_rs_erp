#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 --apk PATH --version-code N --version-name NAME [options]"
  echo
  echo "Options:"
  echo "  --release-dir PATH          Default: data/mobile_releases"
  echo "  --minimum-version-code N    Force clients below N to update (default: 0)"
  echo "  --mandatory                 Force the update for every older build"
  echo "  --notes TEXT                Release notes"
  echo "  --notes-file PATH           Read release notes from a UTF-8 file"
}

apk_path=""
version_code=""
version_name=""
release_dir="${MOBILE_APP_RELEASE_DIR:-data/mobile_releases}"
minimum_version_code=""
mandatory="false"
release_notes=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --apk)
      apk_path="${2:-}"
      shift 2
      ;;
    --version-code)
      version_code="${2:-}"
      shift 2
      ;;
    --version-name)
      version_name="${2:-}"
      shift 2
      ;;
    --release-dir)
      release_dir="${2:-}"
      shift 2
      ;;
    --minimum-version-code)
      minimum_version_code="${2:-}"
      shift 2
      ;;
    --mandatory)
      mandatory="true"
      shift
      ;;
    --notes)
      release_notes="${2:-}"
      shift 2
      ;;
    --notes-file)
      release_notes="$(<"${2:-}")"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$apk_path" || ! -f "$apk_path" ]]; then
  echo "APK file not found: $apk_path" >&2
  exit 2
fi
if [[ ! "$version_code" =~ ^[1-9][0-9]*$ ]]; then
  echo "version-code must be a positive integer" >&2
  exit 2
fi
if [[ -z "$version_name" ]]; then
  echo "version-name is required" >&2
  exit 2
fi
minimum_version_code="${minimum_version_code:-0}"
if [[ ! "$minimum_version_code" =~ ^[0-9]+$ ]]; then
  echo "minimum-version-code must be a non-negative integer" >&2
  exit 2
fi
if (( minimum_version_code > version_code )); then
  echo "minimum-version-code cannot exceed version-code" >&2
  exit 2
fi

mkdir -p "$release_dir"
safe_version_name="$(printf '%s' "$version_name" | tr -c 'A-Za-z0-9._-' '-')"
apk_tmp="$release_dir/.android.apk.tmp.$$"
manifest_tmp="$release_dir/.android.json.tmp.$$"

cleanup() {
  rm -f "$apk_tmp" "$manifest_tmp"
}
trap cleanup EXIT

cp "$apk_path" "$apk_tmp"
if command -v shasum >/dev/null 2>&1; then
  sha256="$(shasum -a 256 "$apk_tmp" | awk '{print $1}')"
elif command -v sha256sum >/dev/null 2>&1; then
  sha256="$(sha256sum "$apk_tmp" | awk '{print $1}')"
else
  echo "shasum or sha256sum is required" >&2
  exit 2
fi
apk_name="accord-mobile-android-${safe_version_name}-${version_code}-${sha256:0:12}.apk"

if stat -f '%z' "$apk_tmp" >/dev/null 2>&1; then
  size_bytes="$(stat -f '%z' "$apk_tmp")"
else
  size_bytes="$(stat -c '%s' "$apk_tmp")"
fi
published_at="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

python3 - \
  "$manifest_tmp" \
  "$version_code" \
  "$version_name" \
  "$minimum_version_code" \
  "$mandatory" \
  "$apk_name" \
  "$sha256" \
  "$size_bytes" \
  "$published_at" \
  "$release_notes" <<'PY'
import json
import pathlib
import sys

(
    manifest_path,
    version_code,
    version_name,
    minimum_version_code,
    mandatory,
    apk_name,
    sha256,
    size_bytes,
    published_at,
    release_notes,
) = sys.argv[1:]
path = pathlib.Path(manifest_path)
payload = {
    "version_code": int(version_code),
    "version_name": version_name,
    "minimum_supported_version_code": int(minimum_version_code),
    "mandatory": mandatory == "true",
    "apk_file": apk_name,
    "sha256": sha256,
    "size_bytes": int(size_bytes),
    "release_notes": release_notes,
    "published_at": published_at,
}
path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
PY

mv "$apk_tmp" "$release_dir/$apk_name"
mv "$manifest_tmp" "$release_dir/android.json"
trap - EXIT

echo "Published Android release:"
echo "  APK: $release_dir/$apk_name"
echo "  Manifest: $release_dir/android.json"
echo "  Version: $version_name ($version_code)"
echo "  SHA-256: $sha256"
echo "  Size: $size_bytes bytes"
