#!/usr/bin/env bash
set -euo pipefail
: "${VERSION:?}"
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
out="$root/dist"
stage="$root/target/linux-root"
appdir="$root/target/Patina.AppDir"
rm -rf "$stage" "$appdir" target/linux
mkdir -p "$out" target/linux
(cd crates/iptv-app && "$NSH" package --target linux -o "$root/target/linux" .)
native="$root/target/linux/Patina"
mapfile -t executables < <(find "$native" -maxdepth 1 -type f -executable -printf '%f\n')
test "${#executables[@]}" -eq 1
exe=${executables[0]}
id=com.hantekllc.iptvrs
mkdir -p "$stage/opt/patina" "$stage/usr/bin" "$stage/usr/share/applications" "$stage/usr/share/icons/hicolor/1024x1024/apps"
cp -a "$native/." "$stage/opt/patina/"
printf '#!/bin/sh\nset -e\ncd /opt/patina\nexec "./%s" "$@"\n' "$exe" > "$stage/usr/bin/patina"
chmod +x "$stage/usr/bin/patina"
cp "$native/$id.png" "$stage/usr/share/icons/hicolor/1024x1024/apps/"
sed 's/^Exec=.*/Exec=patina/' "$native/$id.desktop" > "$stage/usr/share/applications/$id.desktop"
desktop-file-validate "$stage/usr/share/applications/$id.desktop"
for format in deb rpm pacman; do
  case "$format" in
    deb) deps=(--depends 'libc6 (>= 2.39)' --depends 'libgtk-4-1 (>= 4.14)' --depends libmpv2 --depends libgstreamer1.0-0 --depends libgstreamer-plugins-base1.0-0); extension=deb;;
    rpm) deps=(--depends 'glibc >= 2.39' --depends 'gtk4 >= 4.14' --depends mpv-libs --depends gstreamer1 --depends gstreamer1-plugins-base); extension=rpm;;
    pacman) deps=(--depends 'glibc>=2.39' --depends 'gtk4>=4.14' --depends mpv --depends gstreamer --depends gst-plugins-base-libs --pacman-compression zstd); extension=pkg.tar.zst;;
  esac
  fpm --force -s dir -t "$format" -n patina -v "$VERSION" \
    --description 'Patina IPTV player' --maintainer 'Patina maintainers' \
    "${deps[@]}" -C "$stage" -p "$out/Patina-$VERSION-linux-x86_64.$extension" .
done
mkdir -p "$appdir/usr/bin"
cp -a "$native/." "$appdir/usr/bin/"
cp "$native/$id.desktop" "$appdir/"
cp "$native/$id.png" "$appdir/"
cat > "$appdir/AppRun" <<'RUN'
#!/usr/bin/env bash
set -e
export APPDIR="$(dirname "$(readlink -f "$0")")"
export LD_LIBRARY_PATH="$APPDIR/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
for hook in "$APPDIR"/apprun-hooks/*.sh; do
  [ ! -f "$hook" ] || source "$hook"
done
cd "$APPDIR/usr/bin"
RUN
printf 'exec "./%s" "$@"\n' "$exe" >> "$appdir/AppRun"
chmod +x "$appdir/AppRun"
# GTK needs schemas, loaders and other resources beyond its linked libraries.
export DEPLOY_GTK_VERSION=4 OUTPUT="$out/Patina-$VERSION-linux-x86_64.AppImage"
linuxdeploy --appdir "$appdir" --executable "$appdir/usr/bin/$exe" --plugin gtk --output appimage
