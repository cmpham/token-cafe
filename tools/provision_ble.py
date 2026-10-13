#!/usr/bin/env python3
"""Create/reuse a local board key and generate its ignored firmware header."""
from pathlib import Path
import os
import secrets

root = Path(__file__).resolve().parents[1]
path = root / 'target/esp/ble-pairing-key'
path.parent.mkdir(parents=True, exist_ok=True)
if not path.exists():
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'w') as output:
        output.write(secrets.token_hex(32))
key = bytes.fromhex(path.read_text().strip())
if len(key) != 32:
    raise ValueError('Pairing key must contain 32 bytes')
header = root / 'firmware/ble/main/pairing_key.h'
text = 'static const unsigned char pairing_key[32] = {' + ','.join(f'0x{b:02x}' for b in key) + '};\n'
if not header.exists() or header.read_text() != text:
    header.write_text(text)
    header.chmod(0o600)
print(f'BLE pairing key ready: {path} (keep private)')
