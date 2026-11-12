#!/usr/bin/env python3
"""Copy Ubuntu copyright notices for native libraries deployed in an AppImage."""
from pathlib import Path
import json
import shutil
import subprocess
import sys

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


def main():
    appdir = Path(sys.argv[1])
    output = appdir / "usr/bin/notices/native"
    output.mkdir(parents=True, exist_ok=True)
    inventory = []
    for library in sorted((appdir / "usr/lib").rglob("*.so*")):
        if not library.is_file() or library.is_symlink():
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
        inventory.append({"library": str(library.relative_to(appdir)), "package": package, "version": version})
    (output / "components.json").write_text(json.dumps(inventory, indent=2) + "\n")


if __name__ == "__main__":
    main()
