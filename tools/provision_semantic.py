#!/usr/bin/env python3
"""Provision separate QA/production v2 secrets, never committed or printed."""
import argparse,json,os,pathlib,secrets
root=pathlib.Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument('--class',dest='trust',choices=['qa','production'],required=True);p.add_argument('--id',required=True);p.add_argument('--name',required=True);a=p.parse_args()
if not a.id.isascii() or not 1<=len(a.id)<=64 or any(c not in 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_' for c in a.id):raise ValueError('Invalid device ID')
if not a.name.isascii() or len(a.name)>24 or not a.name.replace('-','').isalnum():raise ValueError('Invalid name')
dir=root/'target/esp/v2-pairing';dir.mkdir(exist_ok=True,parents=True);dir.chmod(0o700)
keypath=dir/f'{a.id}-{a.trust}.key'
if not keypath.exists():
 fd=os.open(keypath,os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)
 with os.fdopen(fd,'w') as f:f.write(secrets.token_hex(32))
key=bytes.fromhex(keypath.read_text().strip());assert len(key)==32
header=root/'firmware/semantic/main/pairing.h'
header.write_text(f'#define TC_DEVICE_ID "{a.id}"\n#define TC_BOARD_NAME "{a.name}"\n#define TC_KEY_QA {1 if a.trust=="qa" else 0}\nstatic const uint8_t pairing_key[32]={{'+','.join(str(b) for b in key)+'};\n');header.chmod(0o600)
registry=dir/f'{a.trust}.json';registry.write_text(json.dumps({'boards':[{'device_id':a.id,'name':a.name,'trust':a.trust,'key_path':str(keypath)}]},indent=2));registry.chmod(0o600)
print(f'Prepared {a.trust} registry: {registry}')
