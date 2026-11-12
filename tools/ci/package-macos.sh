#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
: "${VERSION:?}"
bash tools/package-macos.sh
rm -rf target/dmg
mkdir -p dist target/dmg
cp -R target/package/Patina.app target/dmg/
ln -s /Applications target/dmg/Applications
dmg="$PWD/dist/Patina-$VERSION-macos-arm64.dmg"
hdiutil create -volname Patina -srcfolder target/dmg -ov -format UDZO "$dmg"
hdiutil verify "$dmg"
mount="$PWD/target/dmg-mounted"
mkdir -p "$mount"
hdiutil attach "$dmg" -readonly -nobrowse -mountpoint "$mount"
trap 'hdiutil detach "$mount"' EXIT
app="$mount/Patina.app"
codesign --verify --deep --strict "$app"
python3 - "$app" <<'PY'
from pathlib import Path
import plistlib
import subprocess
import sys
app = Path(sys.argv[1])
info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
executable = app / "Contents/MacOS" / info["CFBundleExecutable"]
subprocess.run(["lipo", str(executable), "-verify_arch", "arm64"], check=True)
print("Minimum macOS:", info["LSMinimumSystemVersion"])
subprocess.run(["python3", "tools/ci/smoke-desktop.py", str(executable)], check=True)
PY
