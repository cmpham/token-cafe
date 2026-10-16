#!/usr/bin/env python3
"""Attention lifecycle on the actual Xtensa engine; QMP screenshots, synthetic data only."""
import json
import struct
from test_protocol_v2_qemu import Host, fixture, emulator, pb, OUT, sha, lp


def run(device, qmp, process):
    host = Host(device, 244)
    original, _ = fixture(6100)
    sessions = [original[0], original[4]]
    body = b"May I run the test suite?\n\ncargo test --workspace\n\nSynthetic QA fixture. No command will be executed."
    question = b"Which environment should I use: staging or development?"
    blob = pb.BlobRef(sha256=sha(body), byte_length=len(body))
    question_blob = pb.BlobRef(sha256=sha(question), byte_length=len(question))
    for session, title in zip(sessions, ['CHECKOUT-FLOW', 'INFRA']):
        session.title = title
        session.display_name = title
        session.status = 1
        session.waiting_reason = 0
        session.ClearField('approval')
        session.task_blob.CopyFrom(blob if session.provider == 1 else question_blob)
    revision = 0
    captures = []

    def status():
        return struct.unpack('<I', device.request(8))[0]

    def asleep():
        return bool(status() & 4)

    def publish(view=None):
        nonlocal revision
        revision += 1
        host.snapshot(sessions, view or pb.View(page=1), revision)

    def capture(name):
        filename = 'attention-' + name
        host.present(filename, qmp, process)
        captures.append({'state': name, 'time_ms': host.time, 'sleeping': asleep(), 'file': filename + '.ppm'})
        print('CAPTURE', name, 'sleeping=' + str(asleep()), flush=True)
        return (OUT / (filename + '.ppm')).read_bytes().split(b'255\n', 1)[1]

    def idle_until(end):
        # Keep the authenticated link alive without generating a touch or changing state.
        while host.time < end:
            host.send('heartbeat', pb.Heartbeat())
            host.time = min(host.time + 1000, end)
            device.request(5, struct.pack('<Q', host.time))
            host.poll()

    def pixel(pixels, x, y):
        return pixels[(y * 480 + x) * 3:(y * 480 + x + 1) * 3]

    def approval(request_id):
        title = 'May I run the test suite?'
        digest = sha(b'tc-req-v2' + lp(request_id) + lp(sessions[0].id) + b'\x01' + lp(title) + lp('Bash')
                     + sha(body) + struct.pack('<I', len(body)) + b'\x02\x01\x02')
        return pb.Request(id=request_id, digest=digest, title=title, tool_name='Bash', body_blob=blob,
                          state=1, allowed_decisions=[1, 2])

    publish()
    host.advance(10000)
    capture('01-working')
    idle_until(600000)
    assert asleep(), 'ten-minute idle did not sleep'
    black = capture('02-sleeping')
    assert black == bytes(480 * 272 * 3), 'QEMU panel must be black while asleep'

    sessions[0].status = 3
    sessions[0].waiting_reason = 1
    sessions[0].approval.CopyFrom(approval('attention-1'))
    sessions[1].status = 3
    sessions[1].waiting_reason = 2
    publish()
    assert not asleep(), 'new waiting request did not wake the engine'
    bright = capture('03-new-request-bright')
    host.advance(600)
    dim = capture('04-new-request-dim')
    assert pixel(bright, 2, 213) != pixel(dim, 2, 213), 'footer accent did not pulse'
    # A large surface must change, not just the old two-pixel accent.
    changed = sum(pixel(bright, x, y) != pixel(dim, x, y)
                  for y in range(212, 272) for x in range(480))
    assert changed > 14000, f'footer pulse too small: {changed} pixels'
    # The question's white glyphs stay in exactly the same positions in both phases.
    white = bytes((247, 247, 255))  # TEXT after RGB565 conversion
    for y in range(234, 248):
        for x in range(50, 430):
            assert (pixel(bright, x, y) == white) == (pixel(dim, x, y) == white)
    host.advance(600)
    host.present()
    phase2 = struct.unpack('<H', device.request(10, struct.pack('<I', (213 * 480 + 2) * 2))[:2])[0]
    assert phase2 == 0xf79d, hex(phase2)  # BASE.W quantized to RGB565
    for phase in range(3, 50):
        host.advance(600)
        host.present()
        color = struct.unpack('<H', device.request(10, struct.pack('<I', (213 * 480 + 2) * 2))[:2])[0]
        assert (color == 0xf79d) == (phase % 2 == 0), 'unexpected blink phase'
    host.advance(600)
    steady = capture('05-steady-reminder')
    assert pixel(steady, 2, 213) == pixel(dim, 2, 213), 'pulse did not stop after 30 seconds'

    # Acknowledgement is selection only; no action may grant permission.
    host.tap(100, 237)
    assert host.actions[-1].kind == 1
    host.body(body)
    acknowledged = capture('06-acknowledged')
    host.tap(450, 240)
    assert host.actions[-1].kind == 1 and host.actions[-1].session_id == sessions[1].id
    host.body(question)
    next_question = capture('07-next-question')
    assert pixel(acknowledged, 60, 53) != pixel(next_question, 60, 53), 'active frame did not follow Next'
    assert pixel(acknowledged, 300, 53) != pixel(next_question, 300, 53), 'next patron was not highlighted'
    host.tap(450, 240)
    host.advance(500)
    before = len(host.actions)
    host.tap(100, 260)
    assert len(host.actions) == before + 1 and host.actions[-1].kind == 2
    # The verified body was cached on acknowledgement; do not send unsolicited chunks.
    review = capture('08-review')
    for y in range(36):
        assert acknowledged[((227+y)*480+8)*3:((227+y)*480+42)*3] == review[((9+y)*480+104)*3:((9+y)*480+138)*3], 'patron portrait changed in review'
    host.advance(600)
    ready = capture('08b-review-ready')
    assert ready != review, 'review controls did not become ready'
    assert all(action.kind != 7 for action in host.actions), 'view/next/acknowledge dispatched a decision'
    host.tap(30, 20)
    assert host.actions[-1].kind == 3
    # Acknowledge the last local action with a newer revision before retiring requests.
    revision += 1
    host.send('action_result', pb.ActionResult(action_id=host.actions[-1].id, status=1, applied_revision=revision))
    for session in sessions:
        session.status = 1
        session.waiting_reason = 0
        session.ClearField('approval')
    publish()
    resolved = capture('09-resolved')
    assert pixel(resolved, 14, 53) != pixel(steady, 14, 53), 'desk marker survived resolution'

    host.advance(4100)
    capture('09b-last-request')
    progress = b'Implementing checkout validation.'
    sessions[0].task_blob.CopyFrom(pb.BlobRef(sha256=sha(progress), byte_length=len(progress)))
    publish(pb.View(page=1, selected_session_id=sessions[0].id))
    host.body(progress)
    capture('09c-current-task')

    # New request wakes, survives 59,999ms, then sleeps without touch. Repeated snapshots do not renew it.
    idle_until(host.time + 601000)
    assert asleep()
    sessions[0].status = 3
    sessions[0].waiting_reason = 1
    sessions[0].approval.CopyFrom(approval('attention-2'))
    publish()
    assert not asleep()
    woke = host.time
    idle_until(woke + 59999)
    assert not asleep(), 'wake hold ended early'
    publish()
    idle_until(woke + 60000)
    assert asleep(), 'same snapshot extended the wake hold'
    publish()
    assert asleep(), 'duplicate pending request woke a sleeping engine'
    sessions[0].approval.CopyFrom(approval('attention-3'))
    publish()
    assert not asleep(), 'replacement request on the same waiting patron did not wake'
    capture('10-replacement-wakes')
    before = len(host.actions)
    host.tap(450, 240)
    assert len(host.actions) == before, 'hidden Next emitted an action for one waiter'
    host.tap(100, 237)
    early = capture('11-early-acknowledgement')
    assert pixel(early, 2, 213) == pixel(dim, 2, 213), 'acknowledgement did not stop active pulse'
    host.advance(1200)
    host.present()
    color = struct.unpack('<H', device.request(10, struct.pack('<I', (213 * 480 + 2) * 2))[:2])[0]
    assert color != 0xf79d, 'acknowledged request resumed blinking'
    assert all(action.kind != 7 for action in host.actions)
    result = {'result': 'PASS', 'checks': ['idle sleep', 'automatic wake', '30-second pulse window', 'steady text',
              'acknowledgement', 'next waiting patron', 'view without approval', 'resolved markers',
              '60-second wake hold', 'duplicate suppression', 'replacement request wake', 'acknowledge during active pulse', 'active patron highlight', 'matching review portrait', 'single-waiter Next disabled'],
              'captures': captures, 'actions': [{'kind': a.kind, 'session': a.session_id} for a in host.actions],
              'sleep_display': 'QEMU-only adapter blacks out the emulated panel from the shared engine sleep flag'}
    (OUT / 'attention-results.json').write_text(json.dumps(result, indent=2))
    print(json.dumps(result, indent=2), flush=True)


if __name__ == '__main__':
    with emulator() as args:
        run(*args)
