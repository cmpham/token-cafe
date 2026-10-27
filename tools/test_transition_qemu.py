#!/usr/bin/env python3
"""Delayed desktop navigation must never override a newer review (actual Xtensa engine)."""
import json, struct, time
from test_protocol_v2_qemu import Host, fixture, emulator, pb, OUT

def review_visible(d):
    # Top bevel exists only in the full-screen review; inspect actual framebuffer.
    pixel = struct.unpack('<H', d.request(10, struct.pack('<I', (480 + 2) * 2))[:2])[0]
    return pixel == ((0xef >> 3) << 11 | (0xeb >> 2) << 5 | (0xf7 >> 3))

def run(d, qmp, process):
    h = Host(d, 244)
    sessions, body = fixture(4242)
    h.snapshot(sessions, pb.View(page=1)); h.advance(10000)
    h.tap(60,90); h.body(body); h.advance(100); h.tap(60,90)
    select, navigate = h.actions[-2:]
    for action, revision, status in [(select,2,1),(navigate,3,2)]:
        h.send('action_result',pb.ActionResult(action_id=action.id,status=status,applied_revision=revision))
    h.snapshot(sessions,pb.View(page=1,selected_session_id=sessions[0].id),3)
    h.advance(500); h.tap(200,240); h.advance(100); h.tap(200,240)
    opened=h.actions[-1]; assert opened.kind==2
    h.present('transition-optimistic-review',qmp,process)
    assert review_visible(d)
    h.send('action_result',pb.ActionResult(action_id=opened.id,status=1,applied_revision=4))
    h.snapshot(sessions,pb.View(page=2,selected_session_id=sessions[0].id,review_kind=1,review_request_id=sessions[0].approval.id,review_blob=sessions[0].approval.body_blob),4)
    h.present('transition-acknowledged-review',qmp,process)
    assert review_visible(d), 'delayed desktop handoff closed the newer review'
    h.send('action_result',pb.ActionResult(action_id=navigate.id,status=1,applied_revision=4))
    h.poll(); h.present(); assert review_visible(d)
    timings=[]
    for _ in range(10):
        start=time.monotonic();h.present();timings.append(round((time.monotonic()-start)*1000,2))
    h.tap(30,20);h.present('transition-back',qmp,process);assert not review_visible(d)
    result={'result':'PASS','review_render_roundtrip_ms':timings,'delayed_handoff':'PASS','local_back':'PASS'}
    (OUT/'transitions.json').write_text(json.dumps(result,indent=2));print(result,flush=True)

if __name__=='__main__':
    with emulator() as args:run(*args)
