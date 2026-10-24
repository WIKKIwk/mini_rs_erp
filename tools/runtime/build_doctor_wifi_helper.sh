#!/bin/sh
# Explicit build only: this script does not scan or change any network settings.
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
output=${1:-"$script_dir/doctor_wifi"}
case "$(uname -s)" in
    Darwin) ;;
    *) echo "The Doctor CoreWLAN helper can only be built on macOS." >&2; exit 1 ;;
esac

xcrun swiftc -O -framework CoreWLAN -framework CoreLocation \
    "$script_dir/doctor_wifi.swift" -o "$output"
echo "Built $output; no Wi-Fi commands were executed."
