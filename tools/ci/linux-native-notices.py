#!/usr/bin/env python3
"""Copy Ubuntu copyright notices for native libraries deployed in an AppImage.

Also records which Ubuntu source package version built each library
(sources.json) and where a user finds that source (SOURCE.txt); the release
job downloads exactly that list with tools/ci/native-sources.py.

The pinned libmpv runtime (tools/mpv-runtime-unix) is not an Ubuntu build:
with RUNTIME given, its library is left to the runtime's own notices and
source archive (notices/mpv-runtime) and every other library is recorded.

Usage: linux-native-notices.py APPDIR VERSION [RUNTIME]
"""
import importlib.util
from pathlib import Path
import json
import shutil
import subprocess
import sys

spec = importlib.util.spec_from_file_location("native_sources", Path(__file__).with_name("native-sources.py"))
native_sources = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native_sources)


def installed_owners(query_output, architecture):
    """Only system library paths can own linuxdeploy's system dependencies.

    Runner images also install browser-private copies under /opt. Their matching
    SONAMEs do not make Chrome/Edge owners of /usr/lib's Vulkan loader.
    """
    owners = set()
    for line in query_output.splitlines():
        if ": /" not in line or line.startswith(("diversion ", "local diversion ")):
            continue
        owner, installed = line.split(": /", 1)
        installed = Path("/" + installed)
        if not any(installed.is_relative_to(root) for root in ("/usr/lib", "/lib")):
            continue
        if ":" not in owner or owner.endswith(":" + architecture):
            owners.add(owner)
    return owners


def select_owner(query_output, architecture, library):
    owners = installed_owners(query_output, architecture)
    if not owners:
        raise SystemExit(f"no installed system package owns {library}")
    if len(owners) != 1:
        raise SystemExit(f"ambiguous installed ownership of {library}: {sorted(owners)}")
    return owners.pop()


def runtime_libraries(runtime):
    """The file names of the pinned runtime's libraries (its manifest's)."""
    if runtime is None:
        return set()
    manifest = json.loads((Path(runtime) / "runtime.json").read_text())
    return {manifest["library"]}


def main():
    if len(sys.argv) not in (3, 4):
        raise SystemExit(__doc__)
    appdir, release = Path(sys.argv[1]), sys.argv[2]
    pinned = runtime_libraries(sys.argv[3] if len(sys.argv) == 4 else None)
    output = appdir / "usr/bin/notices/native"
    output.mkdir(parents=True, exist_ok=True)
    inventory = []
    for library in sorted((appdir / "usr/lib").rglob("*.so*")):
        if not library.is_file() or library.is_symlink() or library.name in pinned:
            continue
        # SONAME paths are package-owned even when the bundled file has been rewritten.
        result = subprocess.run(["dpkg-query", "-S", "*/" + library.relative_to(appdir / "usr/lib").as_posix()], capture_output=True, text=True)
        if result.returncode:
            raise SystemExit(f"no installed package owns {library.name}")
        architecture = subprocess.check_output(["dpkg", "--print-architecture"], text=True).strip()
        package = select_owner(result.stdout, architecture, library.name)
        version = subprocess.check_output(["dpkg-query", "-W", "-f=${Version}", package], text=True)
        doc = Path("/usr/share/doc") / package.split(":")[0] / "copyright"
        if not doc.is_file():
            raise SystemExit(f"missing copyright text for {package}")
        shutil.copyfile(doc, output / (package.replace(":", "_") + "-copyright"))
        source = subprocess.check_output(
            ["dpkg-query", "-W", "-f=${source:Package} ${source:Version}", package], text=True)
        inventory.append({"library": str(library.relative_to(appdir)), "package": package, "version": version,
                          **native_sources.source_package(source, package)})
    (output / "components.json").write_text(json.dumps(inventory, indent=2) + "\n")
    description = native_sources.manifest("linux", release, native_sources.ubuntu_components(inventory))
    (output / "sources.json").write_text(json.dumps(description, indent=2) + "\n")
    (output / "SOURCE.txt").write_text(native_sources.source_note("linux", release))


if __name__ == "__main__":
    main()
