#!/usr/bin/env python3
"""Install a durable Claude permission bridge, preserving unrelated settings."""
import argparse
import json
import os
from pathlib import Path
import shlex
import shutil
import sys
import tempfile
import time


def owned_hook(handler, destination):
    """Recognize our stable installation and the former checkout-based command."""
    if handler.get('type') != 'command':
        return False
    try:
        argv = shlex.split(handler.get('command', ''))
    except ValueError:
        return False
    # Only the exact two-argument Python invocation emitted by our installer.
    if len(argv) != 2 or not Path(argv[0]).name.startswith('python'):
        return False
    script = Path(argv[1])
    return script == destination or script.parts[-4:] == ('token-cafe', 'tools', 'approval', 'claude_hook.py')


def atomic_write(path, content, mode):
    fd, temporary = tempfile.mkstemp(prefix=path.name + '.', dir=path.parent)
    try:
        os.fchmod(fd, mode)
        with os.fdopen(fd, 'w') as output:
            output.write(content)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--remove', action='store_true')
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    settings = Path(os.environ.get('CLAUDE_CONFIG_DIR') or Path.home() / '.claude') / 'settings.json'
    destination = Path.home() / '.token-cafe' / 'claude_hook.py'
    data = json.loads(settings.read_text()) if settings.exists() else {}
    hooks = data.get('hooks', {})
    entries = hooks.get('PermissionRequest', [])
    source = Path(__file__).resolve().with_name('claude_hook.py')
    command = shlex.join([sys.executable, str(destination)])
    if args.check:
        installed = [h for entry in entries for h in entry.get('hooks', []) if owned_hook(h, destination)]
        healthy = (not data.get('disableAllHooks') and len(installed) == 1
                   and installed[0].get('command') == command
                   and installed[0].get('timeout', 0) >= 600
                   and destination.is_file() and destination.read_bytes() == source.read_bytes())
        print('Permission bridge installation: ' + ('OK' if healthy else 'repair needed'))
        return 0 if healthy else 1
    # Preserve matchers and other handlers, including mixed-handler entries.
    remaining = []
    for entry in entries:
        handlers = [h for h in entry.get('hooks', []) if not owned_hook(h, destination)]
        if handlers:
            remaining.append({**entry, 'hooks': handlers})
    if not args.remove:
        destination.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        if destination.parent.is_symlink() or destination.parent.stat().st_mode & 0o077:
            raise ValueError('Approval directory must be private (0700)')
        atomic_write(destination, source.read_text(), 0o600)
        remaining.append({'matcher': '', 'hooks': [{'type': 'command', 'command': command, 'timeout': 600}]})
    data.setdefault('hooks', {})['PermissionRequest'] = remaining
    settings.parent.mkdir(parents=True, exist_ok=True)
    if settings.exists():
        backup = settings.with_name(f'settings.json.token-cafe-backup-{time.time_ns()}')
        shutil.copy2(settings, backup)
        backup.chmod(0o600)
        print(f'Backup: {backup}')
    atomic_write(settings, json.dumps(data, indent=2) + '\n', settings.stat().st_mode & 0o777 if settings.exists() else 0o600)
    print('Removed' if args.remove else 'Installed', 'Token Cafe PermissionRequest hook')
    if data.get('disableAllHooks'):
        print('Warning: Claude settings disable all hooks; this setting was preserved.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
