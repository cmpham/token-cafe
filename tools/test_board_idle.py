#!/usr/bin/env python3
"""Exercise the production idle clock/backlight path using QA-only clock aging."""
import argparse
import time
import serial


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--port', required=True)
    args = parser.parse_args()
    lines = []
    sequence = 0
    with serial.Serial(args.port, 115200, timeout=.1) as port:
        def wait(marker, timeout=3):
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                line = port.readline().decode(errors='replace').strip()
                if line:
                    lines.append(line)
                    if marker in line:
                        return line
            raise AssertionError(f'Missing {marker}: {lines[-12:]}')

        def command(kind, values):
            nonlocal sequence
            sequence += 1
            port.write(f'QA {kind} {sequence} {values}\n'.encode())
            port.flush()
            return wait(f'@QA {"IDLE" if kind == "IDLE" else "INPUT"} {sequence} ')

        try:
            command('MODE', '1')
            # Begin with a complete contact to reset the inactivity clock.
            command('IN', '1 240 250')
            command('IN', '0 240 250')
            assert command('IDLE', '599000').endswith(' 1')
            wait('BACKLIGHT OFF', 2)
            print('PASS sleeps at 10-minute boundary (clock aged to 9m59s)', flush=True)
            # Desktop frames still arrive, but must not wake the backlight.
            wait('FRAME ', 10)
            assert command('IDLE', '600000').endswith(' 0')
            start = len(lines)
            command('IN', '1 420 90')
            command('IN', '1 420 90')
            command('IN', '0 420 90')
            assert any('BACKLIGHT ON' in line for line in lines[start:])
            assert not any('TOUCH:' in line and ('TAP' in line or 'SCROLL' in line) for line in lines[start:])
            print('PASS touch wakes without emitting a patron gesture', flush=True)
            start = len(lines)
            command('IN', '1 240 250')
            command('IN', '0 240 250')
            assert any('TOUCH: TAP' in line for line in lines[start:])
            print('PASS next touch works normally', flush=True)
            command('IN', '1 240 250')
            assert command('IDLE', '600000').endswith(' 1')
            command('IN', '0 240 250')
            assert command('IDLE', '600000').endswith(' 0')
            command('IN', '1 240 250')
            command('IN', '0 240 250')
            print('PASS held touch resets idle timer; repeated sleep/wake works', flush=True)
        finally:
            command('MODE', '0')


if __name__ == '__main__':
    main()
