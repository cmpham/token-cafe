#!/usr/bin/env python3
"""Isolated installation and hook IPC regressions; never contact a real Claude session."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch
from claude_hook import RequestWatch

ROOT = Path(__file__).resolve().parent


class BridgeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='tc-hook-', dir='/tmp')
        self.home = Path(self.temp.name)
        self.env = {**os.environ, 'HOME': str(self.home), 'CLAUDE_CONFIG_DIR': str(self.home / 'custom-claude')}
        self.settings = self.home / 'custom-claude/settings.json'
        self.settings.parent.mkdir()

    def tearDown(self):
        self.temp.cleanup()

    def install(self, *args):
        return subprocess.run([sys.executable, str(ROOT / 'install.py'), *args], env=self.env,
                              capture_output=True, text=True, check=True)

    def test_migrates_missing_worktree_and_preserves_other_hooks(self):
        other = {'type': 'command', 'command': 'echo other'}
        old = {'type': 'command', 'command': '/old/python3 /deleted/token-cafe/tools/approval/claude_hook.py'}
        self.settings.write_text(json.dumps({'theme': 'dark', 'hooks': {'PermissionRequest': [
            {'matcher': 'Bash', 'hooks': [old, other]}], 'Stop': [{'hooks': [other]}]}}))
        self.install()
        self.install()
        self.install('--check')
        data = json.loads(self.settings.read_text())
        self.assertEqual(data['theme'], 'dark')
        entries = data['hooks']['PermissionRequest']
        self.assertEqual(len(entries), 2)
        self.assertEqual(entries[0], {'matcher': 'Bash', 'hooks': [other]})
        self.assertEqual(data['hooks']['Stop'], [{'hooks': [other]}])
        self.assertTrue((self.home / '.token-cafe/claude_hook.py').is_file())
        self.install('--remove')
        self.assertEqual(json.loads(self.settings.read_text())['hooks']['PermissionRequest'], [entries[0]])

    def test_hook_round_trip_and_fail_open_to_native_prompt(self):
        self.install()
        hook = self.home / '.token-cafe/claude_hook.py'
        payload = {'hook_event_name': 'PermissionRequest', 'session_id': 'synthetic-only',
                   'tool_name': 'Bash', 'tool_input': {'command': 'echo synthetic'}, 'cwd': '/tmp'}
        path = self.home / '.token-cafe/approval.sock'
        for behavior in ['allow', 'deny', None]:
            with socket.socket(socket.AF_UNIX) as server:
                server.bind(str(path))
                server.listen(1)
                received = []
                reply = {} if behavior is None else {'hookSpecificOutput': {
                    'hookEventName': 'PermissionRequest', 'decision': {'behavior': behavior}}}
                def serve():
                    client, _ = server.accept()
                    with client:
                        received.append(json.loads(client.makefile('rb').readline()))
                        client.sendall(json.dumps(reply).encode() + b'\n')
                thread = threading.Thread(target=serve)
                thread.start()
                result = subprocess.run([sys.executable, str(hook)], input=json.dumps(payload),
                                        text=True, capture_output=True, env=self.env, timeout=5, check=True)
                thread.join(2)
                self.assertEqual(received, [payload])
                self.assertEqual(json.loads(result.stdout) if result.stdout else {}, reply)
            path.unlink()
        # Missing broker never returns an allow decision.
        result = subprocess.run([sys.executable, str(hook)], input=json.dumps(payload), text=True,
                                capture_output=True, env=self.env, timeout=5, check=True)
        self.assertEqual(result.stdout, '')
        status = json.loads((self.home / '.token-cafe/approval-hook-status.json').read_text())
        self.assertEqual(status['event'], 'bridge_unavailable')

    def test_desktop_progress_retires_only_observed_permission_wait(self):
        root = self.home / 'custom-claude/sessions'
        root.mkdir()
        status = root / '42.json'
        request = {'session_id':'synthetic', 'tool_name':'Bash', 'tool_input':{'command':'true'}}
        with patch.dict(os.environ, self.env):
            status.write_text(json.dumps({'sessionId':'synthetic','status':'busy'}))
            watch = RequestWatch(request)
            self.assertIsNone(watch.poll(), 'initial busy state is not a decision')
            status.write_text(json.dumps({'sessionId':'other','status':'waiting'}))
            self.assertIsNone(watch.poll())
            status.write_text(json.dumps({'sessionId':'synthetic','status':'waiting','waitingFor':'permission prompt'}))
            self.assertIsNone(watch.poll())
            status.write_text(json.dumps({'sessionId':'synthetic','status':'busy'}))
            self.assertEqual(watch.poll(), {'resolved':True})

    def test_transcript_resolution_uses_exact_tool_id_and_never_guesses_denial(self):
        path = self.home / 'transcript.jsonl'
        request = {'session_id':'synthetic','transcript_path':str(path),
                   'tool_name':'Bash','tool_input':{'command':'true'}}
        call = {'type':'tool_use','id':'current','name':'Bash','input':{'command':'true'}}
        def write(block, mode='a'):
            with path.open(mode) as out:
                out.write(json.dumps({'message':{'content':[block]}}) + '\n')
        for is_error, expected in [(False, 'allow'), (True, None)]:
            write(call, 'w')
            with patch.dict(os.environ, self.env):
                watch = RequestWatch(request)
                write({'type':'tool_result','tool_use_id':'other','is_error':False})
                self.assertIsNone(watch.poll())
                write({'type':'tool_result','tool_use_id':'current','is_error':is_error})
                self.assertEqual(watch.poll(), {'resolved':True,'decision':expected})
        # A completed earlier invocation of the same command is not this request.
        with patch.dict(os.environ, self.env):
            watch = RequestWatch(request)
            self.assertIsNone(watch.tool_id)
            self.assertIsNone(watch.poll())

    def test_disabled_hooks_report_unhealthy_without_overriding_setting(self):
        self.settings.write_text(json.dumps({'disableAllHooks': True}))
        self.install()
        result = subprocess.run([sys.executable, str(ROOT / 'install.py'), '--check'], env=self.env,
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertTrue(json.loads(self.settings.read_text())['disableAllHooks'])


if __name__ == '__main__':
    unittest.main()
