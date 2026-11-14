#!/usr/bin/env bash
# Run only after signing credentials and the temporary keychain are removed.
set -euo pipefail
cd "$(dirname "$0")/../.."
: "${VERSION:?}"
for name in MACOS_CERTIFICATE MACOS_CERTIFICATE_PASSWORD APPLE_APP_PASSWORD; do
  if [ -n "${!name:-}" ]; then
    echo "Do not launch package code with signing credentials" >&2
    exit 1
  fi
done
mount=$(mktemp -d "${RUNNER_TEMP:-$PWD/target}/patina-smoke.XXXXXX")
hdiutil attach "$PWD/dist/Patina-$VERSION-macos-arm64.dmg" -readonly -nobrowse -mountpoint "$mount"
trap 'hdiutil detach "$mount"; rmdir "$mount"' EXIT
app="$mount/Patina.app"
codesign --verify --deep --strict "$app"
python3 tools/ci/check-notices.py "$app" mpv-runtime/components.json
python3 - "$app" <<'PY'
from pathlib import Path
import plistlib
import subprocess
import sys
app = Path(sys.argv[1])
info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
subprocess.run(["python3", "tools/ci/smoke-desktop.py", str(app / "Contents/MacOS" / info["CFBundleExecutable"])], check=True)
PY
