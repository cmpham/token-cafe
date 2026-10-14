#!/usr/bin/env python3
"""Merge only the Token Cafe hook into Claude user settings; keep a backup."""
import argparse
import json
from pathlib import Path
import shlex
import shutil
import sys
import time

parser=argparse.ArgumentParser()
parser.add_argument('--remove',action='store_true')
args=parser.parse_args()
settings=Path.home()/'.claude/settings.json'
data=json.loads(settings.read_text()) if settings.exists() else {}
command=shlex.join([sys.executable,str(Path(__file__).resolve().with_name('claude_hook.py'))])
hooks=data.setdefault('hooks',{})
entries=hooks.setdefault('PermissionRequest',[])
# Match our exact command; never remove other hook handlers.
for entry in entries:
    entry['hooks']=[h for h in entry.get('hooks',[]) if h.get('command')!=command]
entries[:]=[entry for entry in entries if entry.get('hooks')]
if not args.remove:
    entries.append({'matcher':'','hooks':[{'type':'command','command':command,'timeout':600}]})
settings.parent.mkdir(parents=True,exist_ok=True)
if settings.exists():
    backup=settings.with_name(f'settings.json.token-cafe-backup-{time.time_ns()}')
    shutil.copy2(settings,backup)
    print(f'Backup: {backup}')
tmp=settings.with_name('settings.json.token-cafe-tmp')
with tmp.open('x') as output:
    json.dump(data,output,indent=2);output.write('\n')
tmp.chmod(settings.stat().st_mode & 0o777 if settings.exists() else 0o600)
tmp.replace(settings)
print('Removed' if args.remove else 'Installed', 'Token Cafe PermissionRequest hook')
