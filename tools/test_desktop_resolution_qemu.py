#!/usr/bin/env python3
"""Claude hook + broker + real Xtensa board: native answer before any touch.
All status/transcript inputs are synthetic and isolated from real sessions.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from test_codex_qemu import Case
from test_protocol_v2_qemu import emulator, pb, ROOT, OUT


def run(device, qmp, process):
    results = []
    for name, error, decision in [('approved', False, 1), ('error-or-denial', True, 0), ('continued', None, 0)]:
        case = Case(device)
        hook = None
        try:
            with tempfile.TemporaryDirectory(prefix='tc-native-qa-', dir='/tmp') as directory:
                home = Path(directory)
                (home / '.token-cafe').mkdir()
                (home / '.token-cafe/approval.sock').symlink_to(case.socket)
                sessions = home / '.claude/sessions'
                sessions.mkdir(parents=True)
                status = sessions / '42.json'
                status.write_text(json.dumps({'sessionId':'tc-claude-qa','status':'waiting','waitingFor':'permission prompt'}))
                transcript = home / 'qa.jsonl'
                call = {'type':'tool_use','id':'current','name':'Bash','input':{'command':'echo synthetic only'}}
                transcript.write_text(json.dumps({'message':{'content':[call]}}) + '\n')
                request = {'hook_event_name':'PermissionRequest','session_id':'tc-claude-qa',
                           'transcript_path':str(transcript),'cwd':'/tmp','tool_name':'Bash','tool_input':call['input']}
                hook = subprocess.Popen([sys.executable,str(ROOT / 'tools/approval/claude_hook.py')],
                    stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,
                    env={**os.environ,'HOME':directory,'CLAUDE_CONFIG_DIR':str(home / '.claude')})
                hook.stdin.write(json.dumps(request).encode()); hook.stdin.close()
                case.publish()
                assert not case.host.actions, 'board must remain untouched before native answer'
                if error is None:
                    status.write_text(json.dumps({'sessionId':'tc-claude-qa','status':'busy'}))
                else:
                    with transcript.open('a') as output:
                        output.write(json.dumps({'message':{'content':[{'type':'tool_result','tool_use_id':'current','is_error':error}]}}) + '\n')
                hook.wait(timeout=4)
                assert hook.stdout.read() == b'', 'native outcome must never return a hook permission grant'
                assert hook.returncode == 0, hook.stderr.read()
                until = time.monotonic() + 4
                while True:
                    model = case.call(op='settle')
                    packets = [pb.Envelope.FromString(bytes.fromhex(p)) for p in model['packets']]
                    ss = [s for p in packets if p.HasField('snapshot_part') for s in p.snapshot_part.sessions]
                    if ss[0].approval.state == 6:
                        break
                    assert time.monotonic() < until, (name, ss[0].approval.state, (home / '.token-cafe/approval-hook-status.json').read_text())
                    time.sleep(.02)
                assert ss[0].approval.delivered_decision == decision
                # Collector sees Claude continuing, not waiting for a human.
                ss[0].status = 1; ss[0].waiting_reason = 0
                case.host.snapshot(ss, packets[0].snapshot_begin.view, packets[0].snapshot_begin.revision)
                case.host.advance(10000)
                case.host.present('desktop-' + name + '-cafe',qmp,process)
                # First ever board interaction happens after the Desktop resolved it.
                case.host.tap(60,90); case.actions()
                for body in case.bodies:
                    case.host.body(body)
                case.host.advance(600)
                case.host.tap(200,240); case.host.advance(100); case.host.tap(200,240)
                case.actions(); case.host.present('desktop-' + name + '-review',qmp,process)
                case.host.advance(600)
                case.host.tap(250,240); case.host.advance(600); case.host.tap(50,240)
                assert not [a for a, _ in case.actions() if a.kind == 7], 'resolved request exposed approval controls'
                results.append({'case':name,'result':'PASS'})
        finally:
            if hook and hook.poll() is None:
                hook.terminate(); hook.wait(timeout=5)
            case.close()
    (OUT / 'desktop-resolution-results.json').write_text(json.dumps(results,indent=2))
    print(json.dumps(results,indent=2))


if __name__ == '__main__':
    with emulator() as args:
        run(*args)
