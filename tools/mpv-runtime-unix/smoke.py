"""Play generated MPEG-TS and local HLS through an exact Linux or macOS libmpv.

The Windows smoke test (`tools/mpv-runtime/smoke.py`) does the work: it makes
its own video and audio with the runtime's test ffmpeg, serves them on
loopback and checks advancing playback, both tracks and distinct nonblack
frames through libmpv's software render API. This runs it against the shared
library given, reading no user profile or real provider media.
"""

import argparse
import functools
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import sys
import tempfile
import threading

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "mpv-runtime"))
import smoke  # noqa: E402


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", type=Path, required=True)
    parser.add_argument("--ffmpeg", type=Path, required=True)
    args = parser.parse_args()
    runtime = smoke.Runtime(args.library)
    version = runtime.dll.mpv_client_api_version
    version.restype = smoke.C.c_ulong
    print(f"libmpv client API {version() >> 16}.{version() & 0xffff} from {args.library.resolve()}")
    with tempfile.TemporaryDirectory(prefix="patina-synthetic-mpv-") as temporary:
        directory = Path(temporary)
        smoke.generate(args.ffmpeg.resolve(), directory)
        requests = []

        class Handler(SimpleHTTPRequestHandler):
            def do_GET(self):
                requests.append(self.path)
                super().do_GET()

            def log_message(self, *_):
                pass

        server = ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Handler, directory=str(directory)))
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            base = f"http://127.0.0.1:{server.server_port}"
            for filename in ("generated.ts", "generated.m3u8"):
                print(f"PASS {filename}: {runtime.play(f'{base}/{filename}')}")
            if "/generated.m3u8" not in requests or not any(path.startswith("/segment-") for path in requests):
                raise RuntimeError("HLS playback did not request its playlist and segments")
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"FAIL: {error}", file=sys.stderr)
        sys.exit(1)
