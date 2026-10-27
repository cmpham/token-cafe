#!/usr/bin/env python3
"""Exercise real Xtensa creature rendering, roster deltas, and seat assignments."""
from test_protocol_v2_qemu import Host, fixture, emulator, pb, OUT


def run(device, qmp, process):
    rosters = {'pokemon': bytes(range(9, 17)), 'dragons': bytes(range(1, 9)),
               'mixed': bytes([1, 9, 2, 10, 3, 11, 6, 16]), 'humans': bytes(8)}
    for name, ids in rosters.items():
        host = Host(device, 244)
        sessions, _ = fixture(244)
        host.snapshot([], pb.View(page=1, patron_ids=ids))
        host.snapshot(sessions, pb.View(page=1, patron_ids=ids), revision=2)
        for step in range(12):
            host.advance(500)
            host.present(f'patrons-{name}-arrival-{step:02}', qmp, process)
        host.advance(4000)
        host.present(f'patrons-{name}-seated', qmp, process)
        for step in range(8):
            host.advance(250)
            host.present(f'patrons-{name}-pose-{step:02}', qmp, process)
        if name != 'humans':
            # A view-only delta swaps characters with no session or seat change.
            swapped = ids[1:2] + ids[:1] + ids[2:]
            host.send('delta', pb.Delta(base_revision=2, revision=3,
                      view=pb.View(page=1, patron_ids=swapped)))
            assert any(e.HasField('applied') for e in host.poll())
            host.present(f'patrons-{name}-swapped', qmp, process)
            assert (OUT / f'patrons-{name}-swapped.ppm').read_bytes() != (OUT / f'patrons-{name}-pose-07.ppm').read_bytes()
            # Tap still identifies the real session, independent of appearance.
            host.tap(60, 90)
            assert host.actions[-1].session_id == 'claude:qa'
        host.snapshot([], pb.View(page=1, patron_ids=ids), revision=4)
        for step in range(12):
            host.advance(500)
            host.present(f'patrons-{name}-departure-{step:02}', qmp, process)
    print('PASS creature rosters: snapshot, animation, view-only seat swap, touch identity, reconnect', flush=True)


if __name__ == '__main__':
    with emulator() as args:
        run(*args)
