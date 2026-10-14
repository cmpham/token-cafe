#!/usr/bin/env python3
"""Claude PermissionRequest hook. Failure leaves the original prompt intact."""
import json
import os
import socket
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


def main():
    raw = sys.stdin.buffer.read(65537)
    if len(raw) > 65536:
        return
    request = json.loads(raw)
    if request.get('hook_event_name') != 'PermissionRequest':
        return
    record("request_received")
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(595)
        connection.connect(os.path.expanduser('~/.token-cafe/approval.sock'))
        connection.sendall(json.dumps(request).encode() + b'\n')
        record("waiting_for_board")
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
