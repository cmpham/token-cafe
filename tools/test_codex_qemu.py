#!/usr/bin/env python3
"""Synthetic Codex RPC -> relay -> Rust broker/store -> real Xtensa UI ->
Rust authority/broker -> relay RPC reply. No real provider or hardware actions."""
import io
import json
import pathlib
import subprocess
import sys
import time
from test_protocol_v2_qemu import Host, emulator, pb, OUT, ROOT, sha
sys.path.insert(0, str(ROOT / 'tools/approval'))
from codex_relay import Relay


def rpc(method, case, **extra):
    if method == 'item/tool/requestUserInput':
        extra.setdefault('isBlocking', True)
        for question in extra.get('questions', []):
            question.setdefault('header', 'QEMU QA')
    return {'id': case, 'method': method, 'params': {
        'threadId': 'tc-codex-qa-' + str(case), 'turnId': 'turn', 'itemId': 'item',
        'cwd': '/tmp', **extra}}


class Case:
    def __init__(self, device):
        self.process = subprocess.Popen([str(ROOT / 'target/release/examples/codex_bridge_qa')],
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        self.socket = json.loads(self.process.stdout.readline())['socket']
        self.server, self.client = io.BytesIO(), io.BytesIO()
        self.relay = Relay(self.server, self.client, self.socket)
        self.host = Host(device, 244)
        self.action_index = 0

    def call(self, **message):
        self.process.stdin.write(json.dumps(message).encode() + b'\n'); self.process.stdin.flush()
        raw = self.process.stdout.readline()
        assert raw, 'Rust fixture exited'
        return json.loads(raw)

    def publish(self, expected=True, previous=None):
        until = time.monotonic() + 4
        while True:
            model = self.call(op='snapshot')
            packets = [pb.Envelope.FromString(bytes.fromhex(p)) for p in model['packets']]
            sessions = [s for p in packets if p.HasField('snapshot_part') for s in p.snapshot_part.sessions]
            if bool(sessions) == expected and (previous is None or sessions[0].approval.id != previous):
                break
            assert time.monotonic() < until, 'broker request not reconciled'
            time.sleep(.02)
        begin = packets[0].snapshot_begin
        self.host.snapshot(sessions, begin.view, begin.revision)
        self.bodies = [b.encode() for b in model['bodies']]
        return sessions

    def actions(self):
        out = []
        for action in self.host.actions[self.action_index:]:
            result = self.call(op='action', time=self.host.time,
                               hex=self.host.env('action', action).SerializeToString().hex())
            out.append((action, result))
            if result['delivered']:
                until = time.monotonic() + 4
                while True:
                    settled = self.call(op='settle')
                    packets = [pb.Envelope.FromString(bytes.fromhex(p)) for p in settled['packets']]
                    sessions = [s for p in packets if p.HasField('snapshot_part') for s in p.snapshot_part.sessions]
                    if any(s.approval.id == action.request_id and s.approval.state == 3 for s in sessions):
                        break
                    assert time.monotonic() < until, 'broker delivery not settled'
                    time.sleep(.02)
                self.host.snapshot(sessions, packets[0].snapshot_begin.view, packets[0].snapshot_begin.revision)
                self.host.advance(100)
        self.action_index = len(self.host.actions)
        return out

    def open(self, name, qmp, process):
        self.host.advance(10000)
        self.host.tap(300, 90); self.actions()
        for body in self.bodies:
            self.host.body(body)
        self.host.advance(600)
        self.host.tap(100, 260); self.actions()
        self.host.present(name, qmp, process)
        self.host.advance(600)

    def reply(self):
        until = time.monotonic() + 4
        while not self.server.getvalue():
            assert time.monotonic() < until, 'relay did not deliver a reply'
            time.sleep(.02)
        return [json.loads(line) for line in self.server.getvalue().splitlines()]

    def close(self):
        self.relay.close()
        self.process.stdin.close()
        self.process.wait(timeout=5)


def run(d, qmp, process):
    results = []
    for number, (label, decision, x) in enumerate([('approve', 'accept', 250), ('deny', 'decline', 50), ('file-approve', 'accept', 250)], 1):
        case = Case(d)
        try:
            if label == 'file-approve':
                case.relay.from_server({'method':'item/started','params':{'threadId':'tc-codex-qa-3','turnId':'turn',
                    'item':{'type':'fileChange','id':'item','status':'inProgress','changes':[{'path':'/tmp/qa-only','kind':{'type':'add'},'diff':'+synthetic-only'}]}}})
                case.relay.from_server(rpc('item/fileChange/requestApproval', number))
            else:
                case.relay.from_server(rpc('item/commandExecution/requestApproval', number,
                                           command='echo synthetic-only', reason='QEMU QA only'))
            case.publish(); case.open('codex-' + label, qmp, process)
            case.host.tap(x, 240)
            actions = case.actions()
            assert len(actions) == 1 and actions[0][0].kind == 7 and actions[0][1]['delivered'], actions
            assert case.reply() == [{'id': number, 'result': {'decision': decision}}]
            case.host.advance(600); case.host.tap(x, 240)
            assert not case.actions(), 'duplicate touch sent another decision'
            case.relay.from_client({'id': number, 'result': {'decision': 'cancel'}})
            assert len(case.reply()) == 1, 'late native response was forwarded'
            results.append({'case': label, 'result': 'PASS'})
        finally:
            case.close()
    # Each supported option, with actual presentation/quiet-interval safeguards.
    for selected in range(4):
        case = Case(d)
        ident = 10 + selected
        try:
            case.relay.from_server(rpc('item/tool/requestUserInput', ident, questions=[{
                'id': 'environment', 'question': 'Choose a synthetic environment.',
                'options': [{'label': name, 'description': 'Synthetic QA choice'} for name in ['Local', 'Dev', 'Staging', 'Preview']]}]))
            sessions = case.publish()
            assert list(sessions[0].approval.allowed_decisions) == [3, 4, 5, 6]
            case.open(f'codex-choice-{selected+1}', qmp, process)
            case.host.tap(250, 240)
            assert not case.actions(), 'submit before selection must be inert'
            case.host.tap(35 + selected * 104, 190)
            assert not case.actions(), 'selection must not submit'
            case.host.tap(250, 240)
            assert not case.actions(), 'selection must require a fresh quiet interval'
            case.host.present(f'codex-choice-{selected+1}-selected', qmp, process)
            case.host.advance(600); case.host.tap(250, 240)
            sent = case.actions()
            assert len(sent) == 1 and sent[0][0].decision == 3 + selected and sent[0][1]['delivered'], sent
            assert case.reply() == [{'id': ident, 'result': {'answers': {'environment': {'answers': [['Local','Dev','Staging','Preview'][selected]]}}}}]
            results.append({'case': 'choice-' + str(selected+1), 'result': 'PASS'})
        finally:
            case.close()
    # Two questions share one Codex RPC: no partial answer is sent upstream.
    case = Case(d)
    try:
        case.relay.from_server(rpc('item/tool/requestUserInput', 25, questions=[
            {'id': ident, 'question': 'Choose a synthetic color.', 'options': [
                {'label':'Blue','description':'Test A'}, {'label':'Green','description':'Test B'}]}
            for ident in ['first','second']]))
        previous = None
        for n in range(2):
            sessions = case.publish(previous=previous)
            previous = sessions[0].approval.id
            case.open('codex-multi-' + str(n+1), qmp, process)
            case.host.tap(35 + n * 104, 190)
            case.host.present(); case.host.advance(600); case.host.tap(250,240)
            assert case.actions()[-1][1]['delivered']
            if n == 0:
                assert case.server.getvalue() == b'', 'partial multi-question reply escaped'
        assert case.reply() == [{'id':25,'result':{'answers':{'first':{'answers':['Blue']},'second':{'answers':['Green']}}}}]
        results.append({'case':'multi-question','result':'PASS'})
    finally:
        case.close()
    # A stale digest must never dispatch; reconnection must not submit a choice.
    case = Case(d)
    try:
        case.relay.from_server(rpc('item/commandExecution/requestApproval', 26, command='echo stale-digest'))
        sessions = case.publish(); request = sessions[0].approval
        stale = pb.Action(id=1, observed_revision=1, kind=7, session_id=sessions[0].id,
                          request_id=request.id, request_digest=b'x'*32, decision=1,
                          displayed_body_sha256=request.body_blob.sha256)
        result = case.call(op='action',time=case.host.time,hex=case.host.env('action',stale).SerializeToString().hex())
        assert result['status'] == 3 and not result['delivered']
        case.host = Host(d,244); case.action_index = 0; case.call(op='reset_peer')
        case.publish(); case.host.advance(10000); case.host.present('codex-reconnected',qmp,process)
        assert not case.actions() and case.server.getvalue() == b''
        case.relay.from_server({'method':'turn/completed','params':{'threadId':'tc-codex-qa-26','turn':{'id':'turn'}}})
        case.publish(expected=False)
        assert case.server.getvalue() == b''
        results.append({'case':'stale-digest-reconnect-turn-cancel','result':'PASS'})
    finally:
        case.close()
    for native in (False, True):
        case = Case(d)
        try:
            ident = 30 + int(native)
            case.relay.from_server(rpc('item/commandExecution/requestApproval', ident, command='echo stale-test'))
            case.publish(); case.open('codex-before-cancel', qmp, process)
            if native:
                case.relay.from_client({'id': ident, 'result': {'decision': 'decline'}})
            else:
                case.relay.from_server({'method': 'serverRequest/resolved', 'params': {'requestId': ident}})
            case.publish(expected=False)
            case.host.tap(250, 240)
            assert not [a for a, _ in case.actions() if a.kind == 7]
            case.host.present('codex-native-answer' if native else 'codex-cancelled', qmp, process)
            if native:
                assert case.reply() == [{'id': ident, 'result': {'decision': 'decline'}}]
            else:
                assert case.server.getvalue() == b''
            results.append({'case': 'native-answer' if native else 'cancelled', 'result': 'PASS'})
        finally:
            case.close()
    (OUT / 'codex-results.json').write_text(json.dumps(results, indent=2))
    print(json.dumps(results, indent=2))


if __name__ == '__main__':
    with emulator() as args:
        run(*args)
