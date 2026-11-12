#!/usr/bin/env bash
set -euo pipefail
: "${VERSION:?}"
root=$PWD
out="$root/dist"
mkdir -p "$out" target/linux
(cd crates/iptv-app && "$NSH" package --target linux -o "$root/target/linux" .)
stage="$root/target/linux-root"
mkdir -p "$stage/opt/patina" "$stage/usr/bin" "$stage/usr/share/applications" "$stage/usr/share/icons/hicolor/512x512/apps"
cp -a target/linux/Patina/. "$stage/opt/patina/"
exe=$(find "$stage/opt/patina" -maxdepth 1 -type f -executable -printf '%f\n')
test -n "$exe"
printf '#!/bin/sh\ncd /opt/patina\nexec "./%s" "$@"\n' "$exe" > "$stage/usr/bin/patina"
chmod +x "$stage/usr/bin/patina"
id=com.hantekllc.iptvrs
cp "$stage/opt/patina/$id.png" "$stage/usr/share/icons/hicolor/512x512/apps/"
sed 's/^Exec=.*/Exec=patina/' "$stage/opt/patina/$id.desktop" > "$stage/usr/share/applications/$id.desktop"
for format in deb rpm pacman; do
  case "$format" in
    deb) deps=(--depends libgtk-4-1 --depends libmpv2);;
    rpm) deps=(--depends gtk4 --depends mpv-libs);;
    pacman) deps=(--depends gtk4 --depends mpv);;
  esac
  fpm -s dir -t "$format" -n patina -v "$VERSION" --description 'Patina IPTV player' --maintainer 'Patina maintainers' "${deps[@]}" -C "$stage" -p "$out/" .
done
# FPM's pacman output uses xz; distribute the requested zstd package.
for package in "$out/"*.pkg.tar.xz; do
  xz -dc "$package" | zstd -o "${package%.xz}.zst"
  rm "$package"
done
appdir="$root/target/Patina.AppDir"
mkdir -p "$appdir/usr/bin"
cp -a target/linux/Patina/. "$appdir/usr/bin/"
cp "$stage/usr/share/applications/$id.desktop" "$appdir/"
sed -i "s/^Exec=.*/Exec=$exe/" "$appdir/$id.desktop"
cp "$stage/opt/patina/$id.png" "$appdir/"
printf '#!/bin/sh\ncd "$(dirname "$(readlink -f "$0")")/usr/bin"\nexec "./%s" "$@"\n' "$exe" > "$appdir/AppRun"
chmod +x "$appdir/AppRun"
# linuxdeploy bundles linked libraries, including GTK and mpv.
linuxdeploy --appdir "$appdir" --executable "$appdir/usr/bin/$exe" --output appimage
mv ./*.AppImage "$out/Patina-$VERSION-x86_64.AppImage"
