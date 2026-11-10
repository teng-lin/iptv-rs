#!/usr/bin/env bash
# Sends a burst of D-pad keys to the running spike and reports whether every
# key reached the app, the handler times it logged, frame jank and a
# screenshot. Usage: tools/burst.sh <label> <keycode> <count> [out-dir]
#   e.g. tools/burst.sh down 20 200
set -euo pipefail
label=${1:?label}; code=${2:?keycode}; count=${3:?count}
out=${4:-$(dirname "$0")/../results}
pkg=com.hantekllc.iptvrs
adb=${ADB:-adb}
mkdir -p "$out"

$adb logcat -c
$adb shell dumpsys gfxinfo "$pkg" reset >/dev/null
keys=$(printf "$code %.0s" $(seq "$count"))
start=$(date +%s%N)
# shellcheck disable=SC2086
$adb shell input keyevent $keys
sent_ms=$(( ($(date +%s%N) - start) / 1000000 ))
sleep 1.5

log="$out/$label.logcat.txt"
$adb logcat -d | grep 'iptv key=' > "$log" || true
got=$(wc -l < "$log")
$adb shell dumpsys gfxinfo "$pkg" > "$out/$label.gfxinfo.txt"
$adb exec-out screencap -p > "$out/$label.png"

echo "== $label: keycode $code x $count, sent in ${sent_ms} ms"
echo "keys logged by app: $got / $count"
if [ "$got" -gt 0 ]; then
  grep -oE 'handler_us=[0-9]+' "$log" | cut -d= -f2 | sort -n | awk '
    function at(q,  i) { i = int(NR * q) + 1; if (i > NR) i = NR; return v[i] }
    { v[NR] = $1 }
    END { printf "handler_us p50=%d p95=%d max=%d\n", at(0.5), at(0.95), v[NR] }'
fi
grep -E 'Total frames rendered|Janky frames|50th percentile|90th percentile|99th percentile' "$out/$label.gfxinfo.txt" | head -5
echo "screenshot: $out/$label.png"
