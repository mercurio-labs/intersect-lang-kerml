"""Give a copied Windows x64 measurement executable a 64 MiB main stack.

Oracle-only host workaround for unoptimized recursive parser measurements.
Never modifies the input executable or a production build.
Usage: python set_exe_stack.py INPUT.exe OUTPUT.exe
"""
import struct
import sys
from pathlib import Path


def copy_with_stack(source, destination):
    source, destination = Path(source), Path(destination)
    if source.resolve() == destination.resolve():
        raise ValueError("Output must differ from the original executable")
    data = bytearray(source.read_bytes())
    if len(data) < 64 or data[:2] != b"MZ":
        raise ValueError("Expected a Windows PE executable")
    pe = struct.unpack_from("<I", data, 60)[0]
    optional = pe + 24
    if optional + 88 > len(data) or data[pe:pe + 4] != b"PE\0\0":
        raise ValueError("Invalid PE header")
    if struct.unpack_from("<H", data, optional)[0] != 0x20B:
        raise ValueError("Only PE32+ (Windows x64) measurement executables are supported")
    offset = optional + 72
    reserve, commit = struct.unpack_from("<QQ", data, offset)
    enlarged = max(reserve, 64 * 1024 * 1024)
    if commit > enlarged:
        raise ValueError("Invalid stack commit/reserve sizes")
    struct.pack_into("<Q", data, offset, enlarged)
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(data)
    print(f"Copied {source} to {destination}; stack reserve {reserve} -> {enlarged} bytes")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit("Usage: set_exe_stack.py INPUT.exe OUTPUT.exe")
    copy_with_stack(*sys.argv[1:])
