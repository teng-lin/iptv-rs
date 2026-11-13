"""Exercise the exact Windows libmpv DLL with generated MPEG-TS and local HLS."""

import argparse
import array
import ctypes as C
import functools
import hashlib
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import math
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time


WIDTH, HEIGHT, FPS, SECONDS = 160, 96, 25, 8


class RenderParam(C.Structure):
    _fields_ = [("type", C.c_int), ("data", C.c_void_p)]


class Event(C.Structure):
    _fields_ = [("event_id", C.c_int), ("error", C.c_int), ("reply_userdata", C.c_uint64), ("data", C.c_void_p)]


class EndFile(C.Structure):
    _fields_ = [("reason", C.c_int), ("error", C.c_int)]


def generate(ffmpeg, directory):
    video, audio = directory / "generated.rgb", directory / "generated.pcm"
    with video.open("wb") as stream:
        for frame in range(FPS * SECONDS):
            # Moving, colorful bands: no external fixture or encoder filter required.
            stream.write(bytes(channel for y in range(HEIGHT) for x in range(WIDTH)
                               for channel in ((x * 3 + frame * 9) % 256,
                                               (y * 5 + frame * 13) % 256,
                                               50 + (x + y + frame * 7) % 200)))
    samples = array.array("h", (int(5000 * math.sin(2 * math.pi * 440 * i / 48000))
                              for i in range(48000 * SECONDS)))
    if sys.byteorder != "little":
        samples.byteswap()
    audio.write_bytes(samples.tobytes())

    def run(arguments):
        result = subprocess.run([str(ffmpeg), "-hide_banner", "-loglevel", "error", "-nostdin", "-y", *arguments],
                                cwd=directory, capture_output=True, text=True, timeout=120)
        if result.returncode:
            raise RuntimeError(f"Synthetic ffmpeg generation failed: {result.stderr[-4000:]}")

    run(["-f", "rawvideo", "-pixel_format", "rgb24", "-video_size", f"{WIDTH}x{HEIGHT}",
         "-framerate", str(FPS), "-i", str(video), "-f", "s16le", "-ar", "48000", "-ac", "1", "-i", str(audio),
         "-t", str(SECONDS), "-c:v", "mpeg2video", "-pix_fmt", "yuv420p", "-g", str(FPS), "-bf", "0",
         "-c:a", "mp2", "-b:a", "128k", "-f", "mpegts", "generated.ts"])
    run(["-i", "generated.ts", "-map", "0", "-c", "copy", "-f", "hls", "-hls_time", "1",
         "-hls_list_size", "0", "-hls_playlist_type", "vod", "-hls_segment_filename", "segment-%03d.ts", "generated.m3u8"])
    if not (directory / "generated.m3u8").read_text().startswith("#EXTM3U"):
        raise RuntimeError("ffmpeg did not generate an HLS playlist")


class Runtime:
    def __init__(self, path):
        self.dll = C.CDLL(str(path.resolve()))
        signatures = {
            "mpv_create": (C.c_void_p, []),
            "mpv_initialize": (C.c_int, [C.c_void_p]),
            "mpv_terminate_destroy": (None, [C.c_void_p]),
            "mpv_set_option_string": (C.c_int, [C.c_void_p, C.c_char_p, C.c_char_p]),
            "mpv_command": (C.c_int, [C.c_void_p, C.POINTER(C.c_char_p)]),
            "mpv_get_property": (C.c_int, [C.c_void_p, C.c_char_p, C.c_int, C.c_void_p]),
            "mpv_get_property_string": (C.c_void_p, [C.c_void_p, C.c_char_p]),
            "mpv_free": (None, [C.c_void_p]),
            "mpv_wait_event": (C.POINTER(Event), [C.c_void_p, C.c_double]),
            "mpv_render_context_create": (C.c_int, [C.POINTER(C.c_void_p), C.c_void_p, C.POINTER(RenderParam)]),
            "mpv_render_context_update": (C.c_uint64, [C.c_void_p]),
            "mpv_render_context_render": (C.c_int, [C.c_void_p, C.POINTER(RenderParam)]),
            "mpv_render_context_free": (None, [C.c_void_p]),
        }
        for name, (result, arguments) in signatures.items():
            function = getattr(self.dll, name)
            function.restype, function.argtypes = result, arguments

    @staticmethod
    def check(result, operation):
        if result < 0:
            raise RuntimeError(f"{operation} failed with libmpv error {result}")

    def text(self, handle, name):
        pointer = self.dll.mpv_get_property_string(handle, name.encode())
        if not pointer:
            return None
        try:
            return C.string_at(pointer).decode("utf-8", errors="replace")
        finally:
            self.dll.mpv_free(pointer)

    def position(self, handle):
        value = C.c_double()
        if self.dll.mpv_get_property(handle, b"time-pos", 5, C.byref(value)) < 0:
            return None
        return value.value

    def play(self, url):
        handle, context = self.dll.mpv_create(), C.c_void_p()
        if not handle:
            raise RuntimeError("mpv_create returned null")
        try:
            for key, value in {"config": "no", "load-scripts": "no", "input-default-bindings": "no",
                               "input-terminal": "no", "terminal": "no", "vo": "libmpv", "ao": "null",
                               "hwdec": "no", "idle": "yes", "keep-open": "yes", "audio-display": "no"}.items():
                result = self.dll.mpv_set_option_string(handle, key.encode(), value.encode())
                # Builds without Lua/JavaScript expose no load-scripts option.
                if key != 'load-scripts' or result != -5:
                    self.check(result, key)
            self.check(self.dll.mpv_initialize(handle), "initialize")
            api = C.create_string_buffer(b"sw")
            parameters = (RenderParam * 2)(RenderParam(1, C.cast(api, C.c_void_p)), RenderParam(0, None))
            self.check(self.dll.mpv_render_context_create(C.byref(context), handle, parameters), "software render context")
            arguments = (C.c_char_p * 4)(b"loadfile", url.encode(), b"replace", None)
            self.check(self.dll.mpv_command(handle, arguments), "load synthetic media")
            stride = C.c_size_t((WIDTH * 4 + 63) & ~63)
            memory = C.create_string_buffer(stride.value * HEIGHT + 63)
            pixels = (C.addressof(memory) + 63) & ~63
            dimensions = (C.c_int * 2)(WIDTH, HEIGHT)
            format_name = C.create_string_buffer(b"bgr0")
            block = C.c_int(0)
            render = (RenderParam * 6)(
                RenderParam(17, C.cast(dimensions, C.c_void_p)),
                RenderParam(18, C.cast(format_name, C.c_void_p)),
                RenderParam(19, C.cast(C.byref(stride), C.c_void_p)),
                RenderParam(20, pixels), RenderParam(12, C.cast(C.byref(block), C.c_void_p)), RenderParam(0, None))
            positions, frames, tracks = [], set(), set()
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                while True:
                    event = self.dll.mpv_wait_event(handle, 0).contents
                    if event.event_id == 0:
                        break
                    if event.error < 0:
                        raise RuntimeError(f"libmpv event failed: {event.error}")
                    if event.event_id == 7 and event.data:
                        ending = C.cast(event.data, C.POINTER(EndFile)).contents
                        if ending.error < 0:
                            raise RuntimeError(f"Synthetic playback failed: {ending.error}")
                if self.dll.mpv_render_context_update(context) & 1:
                    self.check(self.dll.mpv_render_context_render(context, render), "software frame render")
                    frame = C.string_at(pixels, stride.value * HEIGHT)
                    # Ignore the unused fourth byte of each bgr0 pixel.
                    if max(frame[0::4]) > 20 or max(frame[1::4]) > 20 or max(frame[2::4]) > 20:
                        frames.add(hashlib.sha256(frame[0::4] + frame[1::4] + frame[2::4]).hexdigest())
                position = self.position(handle)
                if position is not None:
                    positions.append(position)
                count = self.text(handle, "track-list/count")
                if count:
                    tracks = {self.text(handle, f"track-list/{index}/type") for index in range(int(count))}
                audio_ready = self.text(handle, "audio-params/samplerate") == "48000"
                if len(frames) >= 3 and positions and max(positions) - min(positions) >= 1.0 and {"audio", "video"} <= tracks and audio_ready:
                    return {"distinct_nonblack_frames": len(frames), "advanced_seconds": round(max(positions) - min(positions), 2), "tracks": sorted(tracks)}
                time.sleep(0.01)
            raise RuntimeError(f"Playback did not produce advancing video/audio: frames={len(frames)}, positions={positions[-3:]}, tracks={tracks}")
        finally:
            if context:
                self.dll.mpv_render_context_free(context)
            self.dll.mpv_terminate_destroy(handle)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dll", type=Path, required=True)
    parser.add_argument("--ffmpeg", type=Path, required=True)
    args = parser.parse_args()
    if sys.platform != "win32":
        raise RuntimeError("This smoke test loads the Windows DLL and must run on Windows")
    runtime = Runtime(args.dll)
    with tempfile.TemporaryDirectory(prefix="patina-synthetic-mpv-") as temporary:
        directory = Path(temporary)
        generate(args.ffmpeg.resolve(), directory)
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
                result = runtime.play(f"{base}/{filename}")
                print(f"PASS {filename}: {result}")
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
