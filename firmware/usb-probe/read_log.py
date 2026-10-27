"""Reset through COM and capture a bounded USB enumeration log."""
import argparse
import time
import serial

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('port')
parser.add_argument('--seconds', type=int, default=25)
args = parser.parse_args()
with serial.Serial(port=None, baudrate=115200, timeout=0.5) as port:
    port.dtr = False
    port.rts = False
    port.port = args.port
    port.open()
    port.rts = True
    time.sleep(0.1)
    port.rts = False
    end = time.monotonic() + args.seconds
    while time.monotonic() < end:
        chunk = port.read(4096)
        if chunk:
            print(chunk.decode('utf-8', errors='replace'), end='', flush=True)
