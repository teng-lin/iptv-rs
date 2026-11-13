"""Verify that the built executable embeds the multi-resolution app icon."""
import ctypes
from ctypes import wintypes
from pathlib import Path
import struct
import sys

kernel = ctypes.WinDLL("kernel32", use_last_error=True)
kernel.LoadLibraryExW.argtypes = [wintypes.LPCWSTR, wintypes.HANDLE, wintypes.DWORD]
kernel.LoadLibraryExW.restype = wintypes.HMODULE
kernel.FindResourceW.argtypes = [wintypes.HMODULE, ctypes.c_void_p, ctypes.c_void_p]
kernel.FindResourceW.restype = ctypes.c_void_p
kernel.LoadResource.argtypes = [wintypes.HMODULE, ctypes.c_void_p]
kernel.LoadResource.restype = ctypes.c_void_p
kernel.LockResource.argtypes = [ctypes.c_void_p]
kernel.LockResource.restype = ctypes.c_void_p
kernel.SizeofResource.argtypes = [wintypes.HMODULE, ctypes.c_void_p]
kernel.SizeofResource.restype = wintypes.DWORD
kernel.FreeLibrary.argtypes = [wintypes.HMODULE]
module = kernel.LoadLibraryExW(str(Path(sys.argv[1]).resolve()), None, 0x22)
if not module:
    raise ctypes.WinError(ctypes.get_last_error())
try:
    def resource(kind, identifier):
        found = kernel.FindResourceW(module, identifier, kind)
        if not found:
            raise RuntimeError(f"missing resource {kind}/{identifier}")
        data = kernel.LockResource(kernel.LoadResource(module, found))
        return ctypes.string_at(data, kernel.SizeofResource(module, found))
    group = resource(14, 1)
    reserved, kind, count = struct.unpack_from("<HHH", group)
    assert reserved == 0 and kind == 1 and count == 7
    ico = (Path(__file__).resolve().parents[2] / "crates/iptv-app/icon.ico").read_bytes()
    expected = {}
    for i in range(struct.unpack_from("<H", ico, 4)[0]):
        width, height, colors, reserved, planes, bits, length, offset = struct.unpack_from("<BBBBHHII", ico, 6 + 16 * i)
        expected[width or 256] = ico[offset:offset + length]
    sizes = set()
    for i in range(count):
        width, height, colors, reserved, planes, bits, length, identifier = struct.unpack_from("<BBBBHHIH", group, 6 + 14 * i)
        sizes.add(width or 256)
        image = resource(3, identifier)
        assert width == height and len(image) == length
        assert image == expected[width or 256], "executable icon differs from Patina artwork"
    assert sizes == {16, 24, 32, 48, 64, 128, 256}
    print("Windows executable contains all seven Patina icon sizes.")
finally:
    kernel.FreeLibrary(module)
