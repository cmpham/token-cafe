#!/usr/bin/env python3
"""Claude PermissionRequest hook. Failure leaves the original prompt intact."""
import json
import os
import socket
import select
import sys
import time
from pathlib import Path


def record(event):
    """Record bridge health, never commands, arguments or session contents."""
    try:
        directory = Path.home() / ".token-cafe"
        directory.mkdir(mode=0o700, exist_ok=True)
        path = directory / "approval-hook-status.json"
        temporary = directory / f"approval-hook-status.{os.getpid()}.tmp"
        fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w") as output:
            json.dump({"event": event, "time": time.time()}, output)
        temporary.replace(path)
    except OSError:
        pass


class RequestWatch:
    """Observe provider lifecycle only; never infer or send a permission grant."""
    def __init__(self, request):
        self.request = request
        self.parent = os.getppid()
        self.saw_waiting = False
        self.tool_id = None
        self.transcript = None
        self.offset = 0
        self.partial = b''
        path = request.get('transcript_path')
        self.sessions = Path(os.environ.get('CLAUDE_CONFIG_DIR') or Path.home() / '.claude') / 'sessions'
        if isinstance(path, str) and path:
            self.transcript = Path(path)
            # A transcript's own config root also handles switched Claude accounts.
            if self.transcript.parent.parent.name == 'projects':
                self.sessions = self.transcript.parent.parent.parent / 'sessions'
            try:
                with self.transcript.open('rb') as source:
                    size = source.seek(0, 2)
                    source.seek(max(0, size - 2 * 1024 * 1024))
                    if source.tell():
                        source.readline()  # Never parse a truncated JSON record.
                    records = source.read().splitlines()
                    self.offset = size
                calls = set()
                for raw in records:
                    try:
                        record = json.loads(raw)
                        for block in self.blocks(record):
                            if self.matches(block):
                                calls.add(block['id'])
                            elif block.get('type') == 'tool_result':
                                calls.discard(block.get('tool_use_id'))
                    except (ValueError, TypeError, KeyError):
                        continue
                if len(calls) == 1:
                    self.tool_id = calls.pop()
            except OSError:
                pass
        self.status_changed()  # Capture an already-visible permission prompt.

    @staticmethod
    def blocks(record):
        message = record.get('message', {}) if isinstance(record, dict) else {}
        content = message.get('content', []) if isinstance(message, dict) else []
        return [b for b in content if isinstance(b, dict)] if isinstance(content, list) else []

    def matches(self, block):
        return (block.get('type') == 'tool_use' and isinstance(block.get('id'), str)
                and block.get('name') == self.request.get('tool_name')
                and block.get('input') == self.request.get('tool_input'))

    def status_changed(self):
        try:
            for path in self.sessions.glob('*.json'):
                try:
                    status = json.loads(path.read_bytes())
                    if status.get('sessionId') != self.request.get('session_id'):
                        continue
                    waiting = (status.get('status') == 'waiting'
                               and status.get('waitingFor', 'permission prompt') == 'permission prompt')
                    if waiting:
                        self.saw_waiting = True
                    elif status.get('status') in ('busy', 'idle') and self.saw_waiting:
                        return True
                except (OSError, ValueError, TypeError, AttributeError):
                    continue
        except OSError:
            pass
        return False

    def poll(self):
        if self.transcript and self.tool_id:
            try:
                with self.transcript.open('rb') as source:
                    if source.seek(0, 2) < self.offset:
                        return {'resolved': True}  # Rotation invalidates the captured identity.
                    source.seek(self.offset)
                    raw = source.read(2 * 1024 * 1024)
                    self.offset = source.tell()
                records = (self.partial + raw).split(b'\n')
                self.partial = records.pop()
                if len(self.partial) > 2 * 1024 * 1024:
                    return {'resolved': True}
                for raw in records:
                    try:
                        for block in self.blocks(json.loads(raw)):
                            if block.get('type') == 'tool_result' and block.get('tool_use_id') == self.tool_id:
                                # An error can be denial, interruption, or execution failure.
                                # Never label it Denied by guessing from prose.
                                return {'resolved': True, 'decision': None if block.get('is_error') else 'allow'}
                    except (ValueError, TypeError):
                        continue
            except OSError:
                pass
        if os.getppid() != self.parent or self.status_changed():
            return {'resolved': True}
        return None


def main():
    raw = sys.stdin.buffer.read(65537)
    if len(raw) > 65536:
        return
    request = json.loads(raw)
    if request.get('hook_event_name') != 'PermissionRequest':
        return
    watch = RequestWatch(request)
    record("request_received")
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(595)
        connection.connect(os.path.expanduser('~/.token-cafe/approval.sock'))
        connection.sendall(json.dumps(request).encode() + b'\n')
        record("waiting_for_board")
        deadline = time.monotonic() + 590
        while time.monotonic() < deadline:
            resolved = watch.poll()
            if resolved:
                connection.sendall(json.dumps(resolved).encode() + b'\n')
                record("resolved_on_desktop")
                return  # No hook decision: the Desktop already owns the outcome.
            if select.select([connection], [], [], .1)[0]:
                break
        else:
            return
        result = connection.makefile('rb').readline(65537)
        response = json.loads(result)
        decision = response.get('hookSpecificOutput', {}).get('decision', {})
        if decision.get('behavior') in ('allow', 'deny'):
            print(json.dumps(response))
            record("decision_returned")
        else:
            record("returned_to_desktop")


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, TypeError):
        record("bridge_unavailable")
