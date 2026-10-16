#!/usr/bin/env python3
import io
import json
from pathlib import Path
import socket
import tempfile
import threading
import time
import unittest
from codex_relay import Relay, bridge_requests


def request(method='item/tool/requestUserInput', **params):
    if method == 'item/tool/requestUserInput':
        params.setdefault('isBlocking', True)
    return {'id': 17, 'method': method, 'params': {'threadId': 'test', 'turnId': 'turn', 'itemId': 'item', **params}}


def question(ident='q'):
    return {'id': ident, 'header': 'Color', 'question': 'Choose a color', 'options': [
        {'label': 'Blue', 'description': 'First'}, {'label': 'Green', 'description': 'Second'}]}


class CodexRelayTests(unittest.TestCase):
    def test_exact_choice_ids_and_labels(self):
        requests, encode = bridge_requests(request(questions=[question('one'), question('two')]), {})
        self.assertEqual([r['choices'] for r in requests], [2, 2])
        self.assertIn('OPTION 2: Green', requests[0]['question_text'])
        self.assertEqual(encode([{'choice': 1}, {'choice': 0}]), {'answers': {
            'one': {'answers': ['Green']}, 'two': {'answers': ['Blue']}}})
        self.assertIsNone(encode([{'choice': 2}, {'choice': 0}]))
        self.assertIsNone(encode([{'choice': True}, {'choice': 0}]))
        self.assertIsNone(encode([{'choice': 0}]))

    def test_unsupported_questions_remain_native(self):
        for q in [dict(question(), isOther=True), dict(question(), isSecret=True),
                  dict(question(), multiSelect=True), dict(question(), options=[]),
                  dict(question(), options=question()['options'] * 3)]:
            self.assertIsNone(bridge_requests(request(questions=[q]), {}))
        self.assertIsNone(bridge_requests(request(questions=[question(), question()]), {}))

    def test_approvals_require_full_content_and_allow_once(self):
        self.assertIsNone(bridge_requests(request('item/commandExecution/requestApproval'), {}))
        self.assertIsNone(bridge_requests(request('item/fileChange/requestApproval'), {}))
        self.assertIsNone(bridge_requests(request('item/commandExecution/requestApproval', command='true',
                                                 availableDecisions=['acceptForSession', 'decline']), {}))
        item = {('test','turn','item'): {'changes': [{'path': '/tmp/test', 'diff': '+synthetic'}]}}
        requests, encode = bridge_requests(request('item/fileChange/requestApproval'), item)
        self.assertEqual(requests[0]['tool_input']['changes'], item[('test','turn','item')]['changes'])
        self.assertEqual(encode([{'decision':'accept'}]), {'decision':'accept'})
        self.assertIsNone(encode([{'decision':'acceptForSession'}]))
        self.assertIsNone(bridge_requests(request('item/fileChange/requestApproval', grantRoot='/tmp'), item))
        self.assertIsNone(bridge_requests(request('item/commandExecution/requestApproval', command='true',
                                                 additionalPermissions={'network': True}), {}))

    def test_unavailable_broker_preserves_native_answer(self):
        target, client = io.BytesIO(), io.BytesIO()
        relay = Relay(target, client, '/nonexistent/tc-qa.sock')
        msg = request(questions=[question()])
        relay.from_server(msg)
        until = time.monotonic() + 3
        while relay.pending and time.monotonic() < until:
            time.sleep(.01)
        self.assertFalse(relay.pending)
        self.assertEqual(json.loads(client.getvalue()), msg)
        self.assertEqual(target.getvalue(), b'')
        native = {'id':17,'result':{'answers':{'q':{'answers':['Blue']}}}}
        relay.from_client(native)
        self.assertEqual(json.loads(target.getvalue()), native)

    def test_native_answer_cancels_inflight_board_request(self):
        with tempfile.TemporaryDirectory(dir='/tmp', prefix='tc-relay-') as directory:
            path = str(Path(directory) / 'broker')
            received, released = threading.Event(), threading.Event()
            with socket.socket(socket.AF_UNIX) as server:
                server.bind(path); server.listen(1); server.settimeout(3)
                def serve():
                    connection, _ = server.accept()
                    with connection:
                        connection.makefile('rb').readline()
                        received.set()
                        released.wait(3)
                        try:
                            connection.sendall(b'{"choice":0}\n')
                        except OSError:
                            pass
                worker = threading.Thread(target=serve); worker.start()
                target, client = io.BytesIO(), io.BytesIO()
                relay = Relay(target, client, path)
                msg = request(questions=[question()]); relay.from_server(msg)
                self.assertTrue(received.wait(3))
                native = {'id': 17, 'result': {'answers': {'q': {'answers': ['Green']}}}}
                relay.from_client(native)
                released.set(); worker.join(3)
                self.assertEqual([json.loads(x) for x in target.getvalue().splitlines()], [native])
                relay.from_client(native)
                self.assertEqual(len(target.getvalue().splitlines()), 1)
                relay.close()

    def test_cancelled_request_does_not_reappear(self):
        target, client = io.BytesIO(), io.BytesIO()
        relay = Relay(target, client, '/nonexistent/tc-qa.sock')
        relay.from_server({'method':'serverRequest/resolved','params':{'requestId':17}})
        relay.from_server(request(questions=[question()]))
        self.assertFalse(relay.pending)
        self.assertEqual(target.getvalue(), b'')


if __name__ == '__main__':
    unittest.main()
