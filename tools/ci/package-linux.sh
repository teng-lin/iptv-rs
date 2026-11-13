#!/usr/bin/env bash
set -euo pipefail
: "${VERSION:?}"
root=$(cd "$(dirname "$0")/../.." && pwd)
source "$root/tools/ci/locked-build-env.sh"
cd "$root"
out="$root/dist"
stage="$root/target/linux-root"
appdir="$root/target/Patina.AppDir"
rm -rf "$stage" "$appdir" target/linux
mkdir -p "$out" target/linux
(cd crates/iptv-app && "$NSH" package --target linux -o "$root/target/linux" .)
bash "$root/tools/ci/notices.sh"
native="$root/target/linux/Patina"
cp -R target/notices "$native/notices"
cp LICENSE "$native/LICENSE"
cp crates/iptv-app/fonts/OFL.txt "$native/OFL.txt"
python3 tools/ci/check-notices.py "$native"
license=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["license"])')
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
# GLES is loaded dynamically by the renderer, so ELF dependency scanning does
# not add its dispatch library to native package requirements.
for format in deb rpm pacman; do
  case "$format" in
    deb) deps=(--depends 'libc6 (>= 2.39)' --depends 'libgtk-4-1 (>= 4.14)' --depends libdbus-1-3 --depends libmpv2 --depends libgles2 --depends libgstreamer1.0-0 --depends libgstreamer-plugins-base1.0-0); extension=deb;;
    rpm) deps=(--depends 'glibc >= 2.39' --depends 'gtk4 >= 4.14' --depends dbus-libs --depends mpv-libs --depends libglvnd-gles --depends gstreamer1 --depends gstreamer1-plugins-base); extension=rpm;;
    pacman) deps=(--depends 'glibc>=2.39' --depends 'gtk4>=4.14' --depends dbus --depends mpv --depends libglvnd --depends gstreamer --depends gst-plugins-base-libs --pacman-compression zstd); extension=pkg.tar.zst;;
  esac
  fpm --force -s dir -t "$format" -n patina -v "$VERSION" --license "$license" \
    --url https://github.com/teng-lin/patina-iptv \
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
# linuxdeploy excludes these common desktop libraries by default. They are
# direct/transitive dependencies here and may be absent on a minimal desktop.
# Native notice attribution assumes distro system-library provenance. Browser
# private libraries must not enter the deployment through caller environment.
unset LD_LIBRARY_PATH LD_PRELOAD
libraries=()
for library in libharfbuzz.so.0 libfribidi.so.0 libfontconfig.so.1 libfreetype.so.6 \
  libasound.so.2 libjack.so.0 libpipewire-0.3.so.0 libusb-1.0.so.0; do
  path=$(ldconfig -p | awk -v name="$library" '$1 == name { print $NF }' | head -n 1)
  test -n "$path"
  canonical=$(readlink -f "$path")
  case "$canonical" in
    /usr/lib/*|/lib/*) ;;
    *) echo "refusing non-system deployment library: $library" >&2; exit 1 ;;
  esac
  # Preserve the SONAME filename: linuxdeploy uses the supplied basename for
  # its destination. Passing the resolved versioned file drops the loader alias.
  libraries+=(--library "$path")
done
linuxdeploy --appdir "$appdir" --executable "$appdir/usr/bin/$exe" "${libraries[@]}" --plugin gtk
for ((i=1; i<${#libraries[@]}; i+=2)); do
  test -f "$appdir/usr/lib/$(basename "${libraries[i]}")"
done
python3 tools/ci/linux-native-notices.py "$appdir"
python3 tools/ci/check-notices.py "$appdir" native/components.json
linuxdeploy --appdir "$appdir" --output appimage
