#!/usr/bin/env python3
"""Check native packaging, dependency names and 16 KiB alignment in an APK."""
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import tempfile
import zipfile


def main():
    apk_path = Path(sys.argv[1])
    ndk = Path(os.environ["ANDROID_NDK_HOME"])
    reader = next((ndk / "toolchains/llvm/prebuilt").glob("*/bin/llvm-readelf"))
    expected = {
        f"lib/{abi}/lib{name}.so"
        for abi in ("arm64-v8a", "x86_64")
        for name in ("token_cafe_android", "token_cafe_jni")
    }
    with zipfile.ZipFile(apk_path) as apk, apk_path.open("rb") as raw, tempfile.TemporaryDirectory() as temp:
        libraries = {name for name in apk.namelist() if name.startswith("lib/") and name.endswith(".so")}
        if libraries != expected:
            raise ValueError(f"Unexpected native libraries: {libraries}; expected {expected}")
        for name in sorted(libraries):
            info = apk.getinfo(name)
            raw.seek(info.header_offset + 26)
            name_len, extra_len = struct.unpack("<HH", raw.read(4))
            offset = info.header_offset + 30 + name_len + extra_len
            if info.compress_type != zipfile.ZIP_STORED or offset % 16384:
                raise ValueError(f"{name}: not uncompressed and 16 KiB ZIP aligned")
            path = apk.extract(name, temp)
            headers = subprocess.check_output([str(reader), "-l", path], text=True)
            loads = [line for line in headers.splitlines() if line.strip().startswith("LOAD")]
            if not loads or any(int(line.split()[-1], 16) < 16384 for line in loads):
                raise ValueError(f"{name}: ELF load alignment below 16 KiB")
            dynamic = subprocess.check_output([str(reader), "-d", path], text=True)
            needed = re.findall(r"Shared library: \[([^\]]+)\]", dynamic)
            if any("/" in dependency for dependency in needed):
                raise ValueError(f"{name}: absolute build path in dependencies: {needed}")
            if name.endswith("libtoken_cafe_jni.so") and "libtoken_cafe_android.so" not in needed:
                raise ValueError(f"{name}: missing Rust renderer dependency")
            print(f"Verified {name}")


if __name__ == "__main__":
    main()
