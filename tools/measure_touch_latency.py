#!/usr/bin/env python3
"""Correlate board touch timestamps with acknowledged desktop input generations.

Capture board serial output and desktop stderr from the same uninterrupted run.
Run: python3 tools/measure_touch_latency.py BOARD_LOG APP_LOG
Times use only the board clock, avoiding USB delivery delay and clock skew.
"""
import argparse
import re
import statistics
from pathlib import Path


def measure(board, app):
    taps, frames = [], []
    down = None
    for line in board.splitlines():
        match = re.search(r'\((\d+)\) TOUCH: (DOWN|TAP) (\d+),(\d+)', line)
        if match:
            time, kind, x, y = match.groups()
            point = (int(x), int(y))
            if kind == 'DOWN':
                down = (int(time), point)
            else:
                taps.append((int(time), point, down[0] if down and down[1] == point else None))
                down = None
        match = re.search(r'\((\d+)\) BLE: FRAME \d+ CRC=([0-9a-f]+)', line)
        if match:
            frames.append((int(match[1]), match[2]))
    applied, completed = [], {}
    frame_index = 0
    for line in app.splitlines():
        match = re.search(r'applied input=(\d+) Tap\((\d+), (\d+)\)', line)
        if match:
            applied.append(tuple(map(int, match.groups())))
        match = re.search(r'verified frame CRC=([0-9a-f]+).* input=(\d+)', line)
        if match:
            # Match in stream order; repeated CRCs must not select an earlier frame.
            while frame_index < len(frames) and frames[frame_index][1] != match[1]:
                frame_index += 1
            if frame_index == len(frames):
                break  # Serial capture has not caught up yet.
            completed.setdefault(int(match[2]), frames[frame_index][0])
            frame_index += 1
    samples = []
    for tap, event in zip(taps, applied):
        released, point, started = tap
        generation, x, y = event
        if (point[0], point[1]) != (x, y):
            raise ValueError('Touch/app event streams do not match; start a fresh capture.')
        finished = completed.get(generation)
        if finished is not None and finished >= released:
            samples.append((generation, finished - started if started is not None else None,
                            finished - released, released - started if started is not None else None))
    return samples, len(taps), len(applied)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('board_log', type=Path)
    parser.add_argument('app_log', type=Path)
    args = parser.parse_args()
    samples, touches, applied = measure(args.board_log.read_text(errors='replace'), args.app_log.read_text(errors='replace'))
    print(f'Taps recorded: {touches}; applied: {applied}; matched displayed responses: {len(samples)}')
    print('input  touch-down→LCD ms  release→LCD ms  finger-held ms')
    for sample in samples:
        print('  '.join(str(n) if n is not None else 'unknown' for n in sample))
    for title, index in [('Touch-down to LCD', 1), ('Release to LCD', 2)]:
        values = [sample[index] for sample in samples if sample[index] is not None]
        if values:
            print(f'{title}: median {statistics.median(values):.0f} ms, min {min(values)} ms, max {max(values)} ms (n={len(values)})')
    print('LCD endpoint is the board frame-completion log, not measured photon response. Touch polling adds up to about 20 ms before DOWN.')


if __name__ == '__main__':
    main()
