#!/usr/bin/env python3
"""Two synthetic playback origins for the recorded Android journey.

Usage: python tools/test-playback-origins.py fixtures/hls/ch0
Import http://10.0.2.2:8091/fixture.m3u in a synthetic development build.
Only header *names*, never values, are recorded. No upstream requests occur.
"""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit
import json
import sys
import threading

ROOT = Path(sys.argv[1]).resolve()
IDENTITY = ("Authorization", "Cookie", "SN", "X-User-Agent")


class Fixture(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        path = urlsplit(self.path).path
        print(json.dumps({"origin": self.server.server_port, "path": path,
                          "identity": [key for key in IDENTITY if key in self.headers]}), flush=True)
        if self.server.server_port == 8091:
            if path == "/fixture.m3u":
                body = ("#EXTM3U\n#EXTINF:-1,Synthetic Relay Channel\n"
                        "#KODIPROP:inputstream.adaptive.stream_headers="
                        "Authorization=synthetic-c-identity&Cookie=synthetic-c-identity&"
                        "SN=synthetic-c-identity&X-User-Agent=synthetic-c-identity\n"
                        "http://10.0.2.2:8091/start.m3u8\n").encode()
            elif path == "/start.m3u8":
                self.send_response(302)
                self.send_header("Location", "/manifest.m3u8")
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            elif path == "/manifest.m3u8":
                lines = (ROOT / "index.m3u8").read_text().splitlines()
                body = "\n".join(line if not line or line.startswith("#")
                                 else f"http://10.0.2.2:8092/{line}" for line in lines).encode()
            else:
                self.send_error(404)
                return
        else:
            leaf = path.removeprefix("/")
            if "/" in leaf or not leaf.endswith(".ts") or not (ROOT / leaf).is_file():
                self.send_error(404)
                return
            body = (ROOT / leaf).read_bytes()
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Content-Type", "application/vnd.apple.mpegurl" if path.endswith(".m3u8") else "application/octet-stream")
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass


servers = [ThreadingHTTPServer(("127.0.0.1", port), Fixture) for port in (8091, 8092)]
for server in servers:
    threading.Thread(target=server.serve_forever, daemon=True).start()
try:
    threading.Event().wait()
except KeyboardInterrupt:
    for server in servers:
        server.shutdown()
