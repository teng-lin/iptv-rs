#!/usr/bin/env python3
"""Local mock IPTV server for development (synthetic data only).

Serves the synthetic HLS channels from fixtures/hls (see
make-hls-fixtures.sh) and answers catch-up requests: any path under
/archive/ is served as the same path without the prefix, whatever the
query string, so the app can play "archived" programmes. Every request is
logged with its full query to stdout, so catch-up URL construction can be
checked against what the app actually sent.

Usage: tools/mock-server.py [port] [directory]
The Android emulator reaches the host at http://10.0.2.2:<port>/.
"""

import http.server
import os
import sys
import urllib.parse


class Handler(http.server.SimpleHTTPRequestHandler):
    def translate_path(self, path):
        parsed = urllib.parse.urlsplit(path)
        p = parsed.path
        if p.startswith("/archive/"):
            p = p[len("/archive"):]
        return super().translate_path(p)

    def log_message(self, fmt, *args):
        sys.stdout.write("%s %s\n" % (self.log_date_time_string(), fmt % args))
        sys.stdout.flush()


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8089
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    directory = sys.argv[2] if len(sys.argv) > 2 else os.path.join(root, "fixtures", "hls")

    def handler(*args, **kwargs):
        return Handler(*args, directory=directory, **kwargs)

    server = http.server.ThreadingHTTPServer(("0.0.0.0", port), handler)
    print(f"mock server on :{port} serving {directory} (/archive/* -> /*)", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
