#!/usr/bin/env python3
"""Test screenless firmware through the board's COM USB-to-UART port."""
import argparse
import struct
import time
import zlib

import serial
from host import Client, FRAME_BYTES, WIDTH, HEIGHT, packet


class SerialTransport:
    def __init__(self, port):
        self.port = port

    def settimeout(self, value):
        self.port.timeout = value

    def recv(self, count):
        data = self.port.read(count)
        if not data:
            raise TimeoutError("No firmware reply on COM port")
        return data

    def sendall(self, data):
        if self.port.write(data) != len(data):
            raise IOError("Incomplete serial write")
        self.port.flush()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("port")
    args = parser.parse_args()
    with serial.Serial(port=None, baudrate=115200, timeout=5, write_timeout=10) as port:
        # Avoid intentionally asserting boot/reset controls when opening COM.
        port.dtr = False
        port.rts = False
        port.port = args.port
        port.open()
        time.sleep(2)
        port.reset_input_buffer()
        client = Client(SerialTransport(port))
        assert struct.unpack("<HHHB", client.request(1)) == (WIDTH, HEIGHT, 4096, 1)
        print("PASS: physical board protocol handshake", flush=True)
        client.request(5)
        assert struct.unpack("<I", client.request(3))[0] == zlib.crc32(bytes(FRAME_BYTES))
        frame = bytes(range(256)) * (FRAME_BYTES // 256)
        client.upload(frame)
        print("PASS: full frame stored in PSRAM; CRC matches host", flush=True)
        baseline = client.request(3)
        client.request(2, struct.pack("<4H", 479, 0, 2, 1) + b"\0" * 4, status=1)
        client.seq += 1
        corrupt = bytearray(packet(5, client.seq))
        corrupt[12] ^= 1
        client.sock.sendall(corrupt)
        client.response(5, client.seq, status=2)
        assert client.request(3) == baseline
        client.sock.sendall(packet(2, 10000, b"\0" * 12)[:18])
        time.sleep(1.5)
        client.request(1)
        assert client.request(3) == baseline
        print("PASS: invalid rectangle/CRC rejected; partial packet timeout recovery", flush=True)
        print("Screenless hardware test complete. LCD and touch are not tested.")


if __name__ == "__main__":
    main()
