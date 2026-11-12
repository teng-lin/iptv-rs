#!/usr/bin/env bash
# Run only in an ephemeral Ubuntu runner/container: installs the generated DEB.
set -euo pipefail
cd "$(dirname "$0")/../.."
: "${VERSION:?}"
prefix="$PWD/dist/Patina-$VERSION-linux-x86_64"
dpkg-deb --info "$prefix.deb"
rpm --query --package --requires "$prefix.rpm"
tar --zstd -tf "$prefix.pkg.tar.zst" > target/pacman-contents.txt
tar --zstd -xOf "$prefix.pkg.tar.zst" .PKGINFO
if [ "$(id -u)" -eq 0 ]; then
  apt-get install -y "$prefix.deb"
else
  sudo apt-get install -y "$prefix.deb"
fi
python3 tools/ci/check-notices.py /opt/patina
xvfb-run -a dbus-run-session -- python3 tools/ci/smoke-desktop.py /usr/bin/patina
chmod +x "$prefix.AppImage"
APPIMAGE_EXTRACT_AND_RUN=1 xvfb-run -a dbus-run-session -- python3 tools/ci/smoke-desktop.py "$prefix.AppImage"
docker build -t patina-appimage-smoke -f tools/ci/Dockerfile.appimage-smoke tools/ci
docker run --rm --init -v "$PWD/dist:/packages:ro" patina-appimage-smoke \
  "/packages/Patina-$VERSION-linux-x86_64.AppImage"
for format in rpm pacman; do
  docker build -t "patina-$format-smoke" -f "tools/ci/Dockerfile.$format-smoke" tools/ci
  docker run --rm --init -v "$PWD/dist:/packages:ro" "patina-$format-smoke"
done
