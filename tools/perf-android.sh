#!/usr/bin/env bash
# Measures the product plan's budgets on an Android device or emulator with
# the app installed and a playlist already added (synthetic data only; the
# large fixture: tools/make-iptv-fixtures.py --channels 10000 --days-back 7).
#
#   tools/perf-android.sh [serial]      (default: the only device)
#
# Reports: cold start to the stored guide on screen, zap to first picture
# (channel down in full screen), guide key handling and frame times during
# a burst of Down keys, and memory. Mutes the device first. Emulator numbers
# are not the budget: run it on a real low-end TV box for that.
set -uo pipefail
export PATH="${ANDROID_HOME:-$HOME/Android/Sdk}/platform-tools:$PATH"
serial=${1:-}
adb="adb${serial:+ -s $serial}"
pkg=com.hantekllc.iptvrs
act=$pkg/.host.TvActivity
pause() { timeout "$1" tail -f /dev/null; }
focused() { $adb shell dumpsys window | grep -m1 mCurrentFocus | grep -q "$pkg/"; }
key() { focused || { echo "ABORT: the app does not have the focus" >&2; exit 2; }; $adb shell input keyevent "$@"; }
# Milliseconds of a logcat `-v epoch` line's first field.
ms() { awk '{ split($1, t, "."); print t[1] * 1000 + substr(t[2] "000", 1, 3) }'; }
median() { sort -n | awk '{ a[NR] = $1 } END { if (NR) print a[int((NR + 1) / 2)]; else print "-" }'; }

# Mute: music stream at 0 and muted (the mute key toggles).
for _ in $(seq 1 16); do $adb shell input keyevent 25; done
$adb shell dumpsys audio | grep -E "^- STREAM_MUSIC" -A1 | grep -q "Muted: false" && $adb shell input keyevent 164

echo "== cold start (3 runs)"
starts=()
for run in 1 2 3; do
  $adb shell am force-stop $pkg
  pause 1
  $adb logcat -c
  t0=$($adb shell date +%s%3N)
  total=$($adb shell am start -W -n $act | sed -n 's/^TotalTime: //p' | tr -d '\r')
  for _ in $(seq 1 40); do
    $adb logcat -d | grep -q 'iptv data: guide switched' && break
    pause 0.25
  done
  guide=$($adb logcat -v epoch -d | grep -m1 'iptv data: guide switched' | ms)
  played=$($adb logcat -v epoch -d | grep -m1 'iptv player state: ready' | ms)
  echo "run $run: first frame ${total} ms, guide on screen $((guide - t0)) ms${played:+, resumed channel ready $((played - t0)) ms}"
  starts+=($((guide - t0)))
  pause 3
done
echo "median guide on screen: $(printf '%s\n' "${starts[@]}" | median) ms (budget 2000)"

echo "== zap to picture (channel down x5, full screen)"
pause 5
zaps=()
for z in 1 2 3 4 5; do
  $adb logcat -c
  key 167
  for _ in $(seq 1 40); do
    $adb logcat -d | grep -q 'iptv player size\|iptv player state: ready' && break
    pause 0.2
  done
  down=$($adb logcat -v epoch -d | grep -m1 'iptv key=channel_down\|iptv key=channeldown' | ms)
  ready=$($adb logcat -v epoch -d | grep -m1 'iptv player state: ready' | ms)
  if [ -n "$down" ] && [ -n "$ready" ]; then zaps+=($((ready - down))); echo "zap $z: $((ready - down)) ms"; fi
  pause 3
done
echo "median zap: $(printf '%s\n' "${zaps[@]}" | median) ms (budget 1500)"

echo "== guide keys (30 x Down)"
key 4; pause 2
$adb shell dumpsys gfxinfo $pkg reset >/dev/null
$adb logcat -c
for _ in $(seq 1 30); do $adb shell input keyevent 20; pause 0.1; done
pause 2
handler=$($adb logcat -d | sed -n 's/.*iptv key=down phase=Down.*handler_us=\([0-9]*\).*/\1/p')
echo "key handler: median $(printf '%s\n' $handler | median) us, max $(printf '%s\n' $handler | sort -n | tail -1) us"
$adb shell dumpsys gfxinfo $pkg | grep -E "Total frames rendered|Janky frames|50th percentile|90th percentile|99th percentile" | sed 's/^ *//'
echo "(budget: key to frame under 50 ms)"

echo "== memory"
$adb shell dumpsys meminfo $pkg | grep -E "TOTAL PSS|Native Heap:" | sed 's/^ *//' | head -2
