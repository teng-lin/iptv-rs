#!/usr/bin/env python3
"""Makes a macOS .app self-contained: copies the dylibs its executable
links outside the system (libmpv from Homebrew and everything it pulls in:
FFmpeg, libass, libplacebo, ...) into Contents/Frameworks, points every
reference at them (@rpath/<name>, the executable's rpath
@executable_path/../Frameworks, each dylib's @loader_path), drops the rpaths
that lead back to Homebrew, and signs it all again ad hoc (`codesign -s -`).

Usage (after `nsh package --target macos`):
  python3 -I tools/bundle-macos.py target/package/Patina.app

Needs Xcode's command line tools (otool, install_name_tool, codesign). It
fails when a dependency cannot be found, and checks at the end that no
reference outside the system and the bundle is left. Homebrew builds its
bottles for the macOS it runs on: the app's LSMinimumSystemVersion is raised
to the newest minimum of what it bundles, so an older Mac says the app needs
a newer macOS rather than failing at launch (a release builds libmpv for an
older deployment target instead).
"""

import os
import plistlib
import shutil
import subprocess
import sys

SYSTEM = ("/usr/lib/", "/System/")


def run(*args):
    return subprocess.run(args, check=True, capture_output=True, text=True).stdout


def refs(path):
    """The dylibs `path` links (otool -L), its own id aside."""
    lines = run("otool", "-L", path).splitlines()[1:]
    out = [line.strip().split(" (compatibility")[0] for line in lines]
    own = dylib_id(path)
    return [r for r in out if r != own]


def dylib_id(path):
    lines = run("otool", "-D", path).splitlines()[1:]
    return lines[0].strip() if lines else None


def rpaths(path):
    lines = run("otool", "-l", path).splitlines()
    return [
        lines[i + 2].strip().split(" ")[1]
        for i, line in enumerate(lines)
        if line.strip() == "cmd LC_RPATH"
    ]


def resolve(ref, loader, exe_dir):
    """The file `ref` names for `loader`, or None."""
    here = os.path.dirname(loader)
    if ref.startswith("@loader_path/"):
        candidates = [os.path.join(here, ref[len("@loader_path/"):])]
    elif ref.startswith("@executable_path/"):
        candidates = [os.path.join(exe_dir, ref[len("@executable_path/"):])]
    elif ref.startswith("@rpath/"):
        name = ref[len("@rpath/"):]
        candidates = [
            os.path.join(rp.replace("@loader_path", here).replace("@executable_path", exe_dir), name)
            for rp in rpaths(loader)
        ]
    else:
        candidates = [ref]
    for c in candidates:
        if os.path.exists(c):
            return os.path.realpath(c)
    return None


def minimum_os(path):
    """The macOS a Mach-O file needs (LC_BUILD_VERSION minos), as a tuple."""
    lines = run("otool", "-l", path).splitlines()
    for i, line in enumerate(lines):
        if line.strip() == "cmd LC_BUILD_VERSION":
            for follow in lines[i + 1 : i + 6]:
                parts = follow.split()
                if parts and parts[0] == "minos":
                    return tuple(int(x) for x in parts[1].split("."))
    return (0,)


def external(ref):
    return not ref.startswith(SYSTEM)


def main():
    if len(sys.argv) != 2 or not sys.argv[1].rstrip("/").endswith(".app"):
        sys.exit(__doc__)
    app = os.path.abspath(sys.argv[1].rstrip("/"))
    info = plistlib.load(open(os.path.join(app, "Contents", "Info.plist"), "rb"))
    exe = os.path.join(app, "Contents", "MacOS", info["CFBundleExecutable"])
    exe_dir = os.path.dirname(exe)
    frameworks = os.path.join(app, "Contents", "Frameworks")
    bundled = os.path.realpath(frameworks) + "/"

    # Every external dylib, by real path, with the name it gets in the bundle
    # (its install name's: libavcodec.62.dylib, not the versioned file).
    names = {}
    todo = [exe]
    seen = set()
    while todo:
        f = todo.pop()
        if f in seen:
            continue
        seen.add(f)
        for ref in refs(f):
            if not external(ref):
                continue
            real = resolve(ref, f, exe_dir)
            if real is None:
                sys.exit(f"cannot find {ref} (linked by {f})")
            if real.startswith(bundled):
                continue
            if real not in names:
                names[real] = os.path.basename(dylib_id(real) or real)
                todo.append(real)
    taken = {}
    for real, name in names.items():
        if taken.setdefault(name, real) != real:
            sys.exit(f"two dylibs named {name}: {taken[name]} and {real}")

    os.makedirs(frameworks, exist_ok=True)
    copies = {}
    for real, name in names.items():
        dest = os.path.join(frameworks, name)
        shutil.copyfile(real, dest)
        os.chmod(dest, 0o644)
        copies[real] = dest

    def rewrite(path, is_exe):
        args = []
        for ref in refs(path):
            if not external(ref):
                continue
            real = resolve(ref, path if is_exe else [k for k, v in copies.items() if v == path][0], exe_dir)
            if real in names:
                args += ["-change", ref, "@rpath/" + names[real]]
        for rp in rpaths(path):
            if rp not in ("@executable_path/../Frameworks", "@loader_path"):
                args += ["-delete_rpath", rp]
        if not is_exe:
            args += ["-id", "@rpath/" + os.path.basename(path)]
        if args:
            run("install_name_tool", *args, path)
        want = "@executable_path/../Frameworks" if is_exe else "@loader_path"
        if want not in rpaths(path):
            run("install_name_tool", "-add_rpath", want, path)

    # The references are resolved against the originals (their rpaths and
    # loader paths), before those are rewritten.
    for dest in copies.values():
        rewrite(dest, False)
    rewrite(exe, True)

    # The macOS the bundle needs, in its Info.plist.
    plist_path = os.path.join(app, "Contents", "Info.plist")
    declared = tuple(int(x) for x in info.get("LSMinimumSystemVersion", "0").split("."))
    needed = max(minimum_os(f) for f in [exe, *copies.values()])
    if needed > declared:
        info["LSMinimumSystemVersion"] = ".".join(str(x) for x in needed)
        with open(plist_path, "wb") as f:
            plistlib.dump(info, f)

    # Signatures: the dylibs, then the app around them.
    for dest in sorted(copies.values()):
        run("codesign", "--force", "--sign", "-", dest)
    run("codesign", "--force", "--sign", "-", app)

    # Nothing outside the system and the bundle is left.
    for f in [exe, *copies.values()]:
        for ref in refs(f):
            if external(ref):
                real = resolve(ref, f, exe_dir)
                if not ref.startswith("@rpath/") or real is None or not real.startswith(bundled):
                    sys.exit(f"{f} has an unbundled dependency: {ref}")
        for rp in rpaths(f):
            if not rp.startswith(("@executable_path", "@loader_path")):
                sys.exit(f"{f} still has rpath {rp}")
    size = sum(os.path.getsize(p) for p in copies.values())
    print(f"bundled {len(copies)} dylibs ({size / 2**20:.0f} MB) into {os.path.relpath(frameworks)}")
    print(f"needs macOS {info['LSMinimumSystemVersion']} (LSMinimumSystemVersion)")


if __name__ == "__main__":
    main()
