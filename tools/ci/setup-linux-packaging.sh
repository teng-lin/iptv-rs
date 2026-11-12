#!/usr/bin/env bash
# Pinned packaging tools, verified before execution.
set -euo pipefail
out="${RUNNER_TEMP:?}/linux-packaging"
mkdir -p "$out"
curl --retry 3 -fL https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20250213-2/linuxdeploy-x86_64.AppImage -o "$out/linuxdeploy"
curl --retry 3 -fL https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/7a3fbc31a9e5075073ff8790f26effbac5f84453/linuxdeploy-plugin-gtk.sh -o "$out/linuxdeploy-plugin-gtk.sh"
(
  cd "$out"
  sha256sum --check <<'SUMS'
4648f278ab3ef31f819e67c30d50f462640e5365a77637d7e6f2ad9fd0b4522a  linuxdeploy
b0f4cbc684a0103a9651f0955b635eaea0096b3a66c0f5a2c2aa337960375171  linuxdeploy-plugin-gtk.sh
SUMS
)
chmod +x "$out/linuxdeploy" "$out/linuxdeploy-plugin-gtk.sh"
