#!/usr/bin/env python3
"""QEMU launcher, reference wire client, and integration tests (standard library only)."""
import argparse
import contextlib
import json
import os
import pathlib
import shutil
import socket
import struct
import subprocess
import tempfile
import time
import zlib

ROOT = pathlib.Path(__file__).resolve().parents[1]
WIDTH, HEIGHT = 480, 272
FRAME_BYTES = WIDTH * HEIGHT * 2


def packet(op, seq, payload=b"", version=1):
    header = struct.pack("<4sBBHI", b"TCF1", op, version, len(payload), seq)
    crc = zlib.crc32(payload, zlib.crc32(header))
    return header + struct.pack("<I", crc) + payload


class Client:
    def __init__(self, sock):
        self.sock = sock
        self.sock.settimeout(5)
        self.seq = 0

    def read(self, count):
        data = bytearray()
        while len(data) < count:
            part = self.sock.recv(count - len(data))
            if not part:
                raise ConnectionError("firmware disconnected")
            data.extend(part)
        return bytes(data)

    def response(self, op, seq, status=0):
        # Boot logs can precede the first binary reply.
        magic = bytearray()
        deadline = time.monotonic() + 10
        while bytes(magic[-4:]) != b"TCF1":
            if time.monotonic() > deadline:
                raise TimeoutError("no protocol reply (firmware may have failed to boot)")
            magic.extend(self.read(1))
            if len(magic) > 4:
                del magic[:-4]
        header = b"TCF1" + self.read(12)
        _, got_op, version, size, got_seq, crc = struct.unpack("<4sBBHII", header)
        assert 1 <= size <= 16, f"invalid reply length {size}"
        body = self.read(size)
        assert zlib.crc32(body, zlib.crc32(header[:12])) == crc, "reply CRC"
        assert (got_op, version, got_seq, body[0]) == (op | 0x80, 1, seq, status), (header, body)
        return body[1:]

    def request(self, op, payload=b"", status=0):
        self.seq += 1
        self.sock.sendall(packet(op, self.seq, payload))
        return self.response(op, self.seq, status)

    def upload(self, frame):
        if len(frame) != FRAME_BYTES:
            raise ValueError(f"expected {FRAME_BYTES} bytes of RGB565 LE")
        for y in range(0, HEIGHT, 4):
            self.request(2, struct.pack("<4H", 0, y, WIDTH, 4) + frame[y * WIDTH * 2:(y + 4) * WIDTH * 2])
        crc, = struct.unpack("<I", self.request(3))
        assert crc == zlib.crc32(frame), "firmware framebuffer differs from host"


def connect(path, process):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError("QEMU exited; see target/esp/qemu.log")
        sock = socket.socket(socket.AF_UNIX)
        try:
            sock.connect(str(path))
            return sock
        except (FileNotFoundError, ConnectionRefusedError):
            sock.close()
            time.sleep(.05)
    raise TimeoutError(f"QEMU socket not ready: {path}")


@contextlib.contextmanager
def emulator(graphics=False):
    qemu = shutil.which("qemu-system-xtensa")
    if not qemu:
        raise RuntimeError("Use bash tools/firmware.sh to activate Espressif QEMU")
    flash = ROOT / "firmware/build/merged-binary.bin"
    if not flash.exists():
        raise RuntimeError("Build first: bash tools/firmware.sh build")
    log_path = ROOT / "target/esp/qemu.log"
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="tc-qemu-", dir="/tmp") as tmp, log_path.open("wb") as log:
        serial = pathlib.Path(tmp) / "uart"
        qmp = pathlib.Path(tmp) / "qmp"
        command = [qemu, "-machine", "esp32s3", "-m", "8M",
                   "-global", "driver=ssi_psram,property=is_octal,value=true",
                   "-global", "driver=esp32s3.gpio,property=strap_mode,value=0x04",
                   "-drive", f"file={flash},if=mtd,format=raw", "-snapshot",
                   "-display", "sdl",
                   "-serial", f"unix:{serial},server=on,wait=on",
                   "-qmp", f"unix:{qmp},server=on,wait=off", "-monitor", "none"]
        # The virtual panel waits for a display refresh. `-display none` hangs
        # its driver; SDL's dummy backend exercises the panel without a window.
        env = os.environ.copy()
        if not graphics:
            env["SDL_VIDEODRIVER"] = "dummy"
        process = subprocess.Popen(command, stdout=log, stderr=log, env=env)
        try:
            with connect(serial, process) as sock:
                client = Client(sock)
                # Explicit readiness avoids sending into the ROM/bootloader UART.
                boot = bytearray()
                sock.settimeout(30)
                deadline = time.monotonic() + 20
                try:
                    while not boot.endswith(b"TC_READY\n"):
                        if time.monotonic() > deadline or len(boot) > 65536:
                            raise TimeoutError("firmware never became ready")
                        boot.extend(client.read(1))
                finally:
                    (ROOT / "target/esp/boot.log").write_bytes(boot)
                sock.settimeout(5)
                hello = client.request(1)
                assert struct.unpack("<HHHB", hello) == (WIDTH, HEIGHT, 4096, 1)
                yield client, process, serial, qmp
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


def screenshot(qmp_path, process, output):
    with connect(qmp_path, process) as sock:
        sock.settimeout(5)
        stream = sock.makefile("rwb")
        json.loads(stream.readline())  # greeting
        def command(name, arguments=None):
            stream.write(json.dumps({"execute": name, "arguments": arguments or {}}).encode() + b"\n")
            stream.flush()
            while True:
                reply = json.loads(stream.readline())
                if "error" in reply:
                    raise RuntimeError(reply)
                if "return" in reply:
                    return reply
        command("qmp_capabilities")
        command("screendump", {"filename": str(output.resolve())})
        stream.close()


def integration(client, process, serial, qmp, frame):
    print("PASS: firmware booted with 8 MB PSRAM; protocol handshake", flush=True)
    # Host CRC is an independent implementation (zlib), not copied firmware code.
    client.request(5)
    assert struct.unpack("<I", client.request(3))[0] == zlib.crc32(bytes(FRAME_BYTES))
    client.upload(frame)
    print("PASS: full café frame transferred; framebuffer CRC matches", flush=True)
    baseline = client.request(3)
    for rect in [(479, 0, 2, 1), (0, HEIGHT - 1, 1, 2), (0, 0, 0, 1), (65535, 0, 2, 1)]:
        client.request(2, struct.pack("<4H", *rect) + b"\0\0", status=1)
    client.request(2, struct.pack("<4H", 0, 0, 2, 1) + b"\0\0", status=1)
    client.seq += 1
    corrupt = bytearray(packet(5, client.seq))
    corrupt[12] ^= 1
    client.sock.sendall(corrupt)
    client.response(5, client.seq, status=2)
    assert client.request(3) == baseline, "invalid messages mutated the frame"
    client.request(99, status=3)
    client.request(1, b"unexpected", status=1)
    for version, size in [(2, 0), (1, 4097)]:
        client.seq += 1
        msg = packet(1, client.seq, b"\0" * size, version)
        client.sock.sendall(msg[:16])
        client.response(1, client.seq, status=1)
    client.sock.sendall(b"noiseTTC")
    client.seq += 1
    for byte in packet(1, client.seq):
        client.sock.sendall(bytes([byte]))
    client.response(1, client.seq)
    print("PASS: invalid requests, fragmentation, and noise recovery", flush=True)
    # Disconnect during a rectangle. Idle timeout must discard the partial packet.
    client.sock.sendall(packet(2, 10000, struct.pack("<4H", 0, 0, 2, 1) + b"\0" * 4)[:18])
    client.sock.close()
    time.sleep(1.5)
    with connect(serial, process) as sock:
        resumed = Client(sock)
        resumed.request(1)
        assert resumed.request(3) == baseline
        for x, y, expected in [(0, 0, (0, 0)), (479, 271, (479, 271)), (240, 136, (240, 136))]:
            assert struct.unpack("<HH", resumed.request(4, struct.pack("<HH", x, y))) == expected
        for x, y in [(480, 0), (0, 272), (65535, 0), (0, 65535)]:
            resumed.request(4, struct.pack("<HH", x, y), status=1)
        for n in range(30):
            patch = struct.pack("<4H", 0, 0, 1, 1) + struct.pack("<H", n)
            resumed.request(2, patch)
            expected = struct.pack("<H", n) + frame[2:]
            assert struct.unpack("<I", resumed.request(3))[0] == zlib.crc32(expected)
        print("PASS: reconnect, simulated taps, and 30 partial updates", flush=True)
        resumed.upload(frame)
        output = ROOT / "target/esp/cafe-qemu.ppm"
        screenshot(qmp, process, output)
        # Validate the virtual panel itself, not only its staging buffer.
        with output.open("rb") as image:
            assert image.readline().strip() == b"P6"
            assert image.readline().strip() == f"{WIDTH} {HEIGHT}".encode()
            assert image.readline().strip() == b"255"
            actual = image.read()
        expected_rgb = bytearray()
        for (pixel,) in struct.iter_unpack("<H", frame):
            r, g, b = pixel >> 11, (pixel >> 5) & 63, pixel & 31
            expected_rgb.extend(((r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)))
        assert actual == expected_rgb, "virtual display pixels differ from RGB565 frame"
    print("PASS: boot/PSRAM, frame CRC, bounds, bad lengths/version/CRC, fragmentation, reconnect,")
    print("      simulated taps, 30 updates, and pixel-exact QEMU display screenshot.")
    print(output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["run", "test"])
    parser.add_argument("--frame", type=pathlib.Path, default=ROOT / "target/cafe.rgb565")
    args = parser.parse_args()
    frame = args.frame.read_bytes()
    if len(frame) != FRAME_BYTES:
        parser.error(f"frame must contain exactly {FRAME_BYTES} bytes")
    with emulator(graphics=args.mode == "run") as (client, process, serial, qmp):
        if args.mode == "test":
            integration(client, process, serial, qmp, frame)
        else:
            client.upload(frame)
            print("Café frame verified on QEMU. Close the display or press Ctrl-C to stop.", flush=True)
            try:
                process.wait()
            except KeyboardInterrupt:
                pass


if __name__ == "__main__":
    main()
