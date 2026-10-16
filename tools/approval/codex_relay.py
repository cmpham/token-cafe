#!/usr/bin/env python3
"""Stdio app-server relay. Connect the owning client here; existing Desktop
sessions are not attachable. No model calls or approvals occur at startup.

Usage: configure an app-server client to launch:
  python3 /path/to/codex_relay.py -- codex app-server --listen stdio://
"""
import argparse
import json
from pathlib import Path
import socket
import subprocess
import sys
import threading

MAX_MESSAGE = 4 * 1024 * 1024


def bridge_requests(message, items):
    """Return bounded board questions plus a response encoder, or native-only."""
    method, p = message.get('method'), message.get('params', {})
    if not isinstance(p, dict) or not all(isinstance(p.get(k), str) and p[k] for k in ('threadId', 'turnId', 'itemId')):
        return None
    base = {'hook_event_name': 'TokenCafeCodexRequest', 'session_id': p['threadId'],
            'cwd': p.get('cwd', 'Unknown'), 'tool_input': {}}
    if method in ('item/commandExecution/requestApproval', 'item/fileChange/requestApproval'):
        if p.get('networkApprovalContext') or p.get('additionalPermissions'):
            return None  # Do not flatten a scoped permission grant into a command approval.
        allowed = p.get('availableDecisions', ['accept', 'decline'])
        if allowed is None:
            allowed = ['accept', 'decline']
        if not isinstance(allowed, list) or 'accept' not in allowed or 'decline' not in allowed:
            return None
        if method == 'item/commandExecution/requestApproval':
            command = p.get('command')
            if not isinstance(command, str) or not command:
                return None
            body = {'command': command, 'reason': p.get('reason', '')}
            tool = 'Codex command'
        else:
            if p.get('grantRoot'):
                return None  # A persistent filesystem grant is not approve-once.
            item = items.get((p['threadId'], p['turnId'], p['itemId']), {})
            if not item.get('changes'):
                return None  # Full diff must have arrived before offering approval.
            body = {'changes': item['changes'], 'reason': p.get('reason', ''), 'grantRoot': p.get('grantRoot')}
            tool = 'Codex file change'
        if len(json.dumps(body).encode()) > 24000:
            return None
        def encode(replies):
            if len(replies) != 1:
                return None
            d = replies[0].get('decision')
            return {'decision': d} if d in ('accept', 'decline') else None
        return [{**base, 'tool_name': tool, 'tool_input': body}], encode
    if method != 'item/tool/requestUserInput':
        return None
    questions = p.get('questions')
    if not isinstance(questions, list) or not 1 <= len(questions) <= 3:
        return None
    requests, ids, labels = [], [], []
    for q in questions:
        if not isinstance(q, dict) or not isinstance(q.get('id'), str) or not q['id'] or q['id'] in ids:
            return None
        options = q.get('options')
        if q.get('isSecret') or q.get('isOther') or q.get('multiSelect') or not isinstance(options, list) or not 2 <= len(options) <= 4:
            return None
        names = [o.get('label') if isinstance(o, dict) else None for o in options]
        if any(not isinstance(n, str) or not n for n in names) or len(set(names)) != len(names):
            return None
        if not isinstance(q.get('question'), str) or not q['question']:
            return None
        text = f"Question {len(ids) + 1} of {len(questions)}\n{q['question']}\n"
        text += '\n'.join(f"OPTION {i + 1}: {o['label']}" for i, o in enumerate(options))
        text += '\n\n' + '\n\n'.join(f"OPTION {i + 1}: {o.get('description', '')}" for i, o in enumerate(options) if o.get('description'))
        if len(text.encode()) > 24000:
            return None
        ids.append(q['id']); labels.append(names)
        requests.append({**base, 'tool_name': 'Codex question', 'choices': len(options), 'question_text': text})
    def encode(replies):
        if len(replies) != len(ids):
            return None
        answers = {}
        for ident, names, reply in zip(ids, labels, replies):
            n = reply.get('choice')
            if type(n) is not int or not 0 <= n < len(names):
                return None
            answers[ident] = {'answers': [names[n]]}
        return {'answers': answers}
    return requests, encode


class Relay:
    def __init__(self, server, client, broker):
        self.server, self.client, self.broker = server, client, broker
        self.lock = threading.RLock()
        self.pending = {}
        self.resolved = set()
        self.items = {}
        self.board_disabled = False

    @staticmethod
    def key(value):
        return json.dumps(value, sort_keys=True)

    @staticmethod
    def write(stream, message):
        stream.write(json.dumps(message, ensure_ascii=False).encode() + b'\n')
        stream.flush()

    def cancel(self, key):
        pending = self.pending.pop(key, None)
        if pending and pending.get('socket'):
            try:
                pending['socket'].shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
        # Keep tombstones for every bridged request; never evict and risk replay.
        # After this connection's budget, further requests stay native-only.
        self.resolved.add(key)
        if len(self.resolved) >= 10000:
            self.board_disabled = True

    def from_client(self, message):
        with self.lock:
            if 'id' in message and 'method' not in message:
                key = self.key(message['id'])
                if key in self.resolved:
                    return
                if key in self.pending:
                    self.cancel(key)
            self.write(self.server, message)

    def from_server(self, message):
        with self.lock:
            method, p = message.get('method'), message.get('params', {})
            if method == 'item/started' and isinstance(p, dict):
                item = p.get('item', {})
                if item.get('type') == 'fileChange':
                    if len(self.items) >= 128:
                        self.items.clear()
                    self.items[(p.get('threadId'), p.get('turnId'), item.get('id'))] = item
            if method == 'serverRequest/resolved':
                key = self.key(p.get('requestId'))
                if not self.board_disabled or key in self.pending:
                    self.cancel(key)
            if method in ('turn/completed', 'turn/started'):
                for key, pending in list(self.pending.items()):
                    if pending['thread'] == p.get('threadId'):
                        self.cancel(key)
                self.items = {k: v for k, v in self.items.items() if k[0] != p.get('threadId')}
            self.write(self.client, message)
            if self.board_disabled or 'id' not in message or 'method' not in message:
                return
            key = self.key(message['id'])
            if key in self.pending or key in self.resolved:
                return
            converted = bridge_requests(message, self.items)
            if converted is None or len(self.pending) >= 32:
                return
            pending = {'thread': p['threadId'], 'socket': None}
            self.pending[key] = pending
            threading.Thread(target=self.answer, args=(message['id'], key, pending, converted), daemon=True).start()

    def answer(self, request_id, key, pending, converted):
        requests, encode = converted
        replies = []
        try:
            for request in requests:
                raw = json.dumps(request).encode() + b'\n'
                if len(raw) > 65536:
                    return
                with socket.socket(socket.AF_UNIX) as connection:
                    connection.settimeout(595)
                    with self.lock:
                        if self.pending.get(key) is not pending:
                            return
                        pending['socket'] = connection
                    connection.connect(self.broker)
                    with self.lock:
                        if self.pending.get(key) is not pending:
                            return
                        connection.sendall(raw)
                    reply = json.loads(connection.makefile('rb').readline(65537))
                    if not isinstance(reply, dict) or not reply:
                        return
                    replies.append(reply)
            result = encode(replies)
            with self.lock:
                if result is not None and self.pending.get(key) is pending:
                    # First answer wins, including a simultaneous answer from the native client.
                    self.write(self.server, {'id': request_id, 'result': result})
                    self.cancel(key)
        except (OSError, ValueError, TypeError):
            pass  # The original request remains available in the owning client.
        finally:
            with self.lock:
                if self.pending.get(key) is pending:
                    self.pending.pop(key, None)

    def close(self):
        with self.lock:
            self.board_disabled = True
            for key in list(self.pending):
                self.cancel(key)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        parser.error('provide the Codex app-server command after --')
    process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=sys.stderr)
    relay = Relay(process.stdin, sys.stdout.buffer, str(Path.home() / '.token-cafe/approval.sock'))
    def messages(stream):
        while raw := stream.readline(MAX_MESSAGE + 1):
            if len(raw) > MAX_MESSAGE or not raw.endswith(b'\n'):
                raise ValueError('oversized or incomplete app-server message')
            message = json.loads(raw)
            if not isinstance(message, dict):
                raise ValueError('app-server message must be an object')
            yield message
    def forward_client():
        try:
            for message in messages(sys.stdin.buffer):
                relay.from_client(message)
        except (OSError, ValueError):
            pass
        finally:
            relay.close()
            process.terminate()
    threading.Thread(target=forward_client, daemon=True).start()
    try:
        for message in messages(process.stdout):
            relay.from_server(message)
    finally:
        relay.close()
        process.terminate()
        process.wait(timeout=5)


if __name__ == '__main__':
    main()
