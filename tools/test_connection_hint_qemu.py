#!/usr/bin/env python3
"""Desktop startup/loss/recovery on the actual board engine; synthetic data only."""
import struct
from test_protocol_v2_qemu import Host, fixture, emulator, pb, OUT, screenshot


def run(device, qmp, process):
    def capture(name):
        device.request(7)
        path = OUT / ('connection-' + name + '.ppm')
        screenshot(qmp, process, path)
        return path.read_bytes()

    device.request(9, struct.pack('<H', 244))
    boot = capture('01-start-desktop')
    device.request(5, struct.pack('<Q', 30_000))
    assert capture('02-no-desktop') == boot, 'startup instruction must remain visible'

    host = Host(device, 244)
    assert capture('03-waiting-for-snapshot') == boot, 'authentication alone must not hide the instruction'
    sessions, _ = fixture(7100)
    host.snapshot(sessions, pb.View(page=1))
    host.advance(1000)
    assert capture('04-synced') != boot, 'valid snapshot must replace the instruction'

    # Heartbeats keep the connection alive while the existing idle timer sleeps.
    while host.time < 601_000:
        host.send('heartbeat', pb.Heartbeat())
        host.time += 1000
        device.request(5, struct.pack('<Q', host.time))
        host.poll()
    assert struct.unpack('<I', device.request(8))[0] & 4

    # A replay fails the live link, exercising the same engine fail/disconnect
    # path as physical BLE loss, including waking a previously sleeping panel.
    wire = next(host.fragments(host.env('heartbeat', pb.Heartbeat())))
    device.request(3, wire)
    device.request(3, wire, ok=False)
    status = struct.unpack('<I', device.request(8))[0]
    assert not status & (1 | 4), 'disconnect must clear connected and sleeping state'
    assert capture('05-disconnected') == boot
    device.request(6, struct.pack('<BHH', 1, 250, 240))
    device.request(6, struct.pack('<BHH', 0, 250, 240))
    assert capture('06-disconnected-touch') == boot

    # Authenticate again on the same engine, without resetting/rebooting it.
    host = Host(device, 244, reset=False, now=host.time)
    assert capture('07-reconnecting') == boot
    host.snapshot(sessions, pb.View(page=1), revision=2)
    host.advance(1000)
    assert capture('08-recovered') != boot
    assert not host.actions, 'the instruction screen must not dispatch actions'
    print('PASS desktop instruction: cold boot, absent app, snapshot gating, sleeping disconnect, touch, reconnect', flush=True)


if __name__ == '__main__':
    with emulator() as (device, qmp, process):
        run(device, qmp, process)
