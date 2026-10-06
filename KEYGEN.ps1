$dir = "$env:LOCALAPPDATA\CatDesk-Offline-Signing"
New-Item -ItemType Directory -Force -Path $dir | Out-Null

py -m pip install cryptography

$env:CATDESK_KEYDIR = $dir

@'
import os
from pathlib import Path
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import (
    Encoding, PrivateFormat, PublicFormat, NoEncryption
)

root = Path(os.environ["CATDESK_KEYDIR"])

private_key = Ed25519PrivateKey.generate()

private_raw = private_key.private_bytes(
    Encoding.Raw,
    PrivateFormat.Raw,
    NoEncryption()
)

public_raw = private_key.public_key().public_bytes(
    Encoding.Raw,
    PublicFormat.Raw
)

(root / "catdesk-ed25519-private.key").write_bytes(private_raw)
(root / "catdesk-ed25519-public.key").write_bytes(public_raw)
(root / "catdesk-ed25519-public.hex").write_text(public_raw.hex() + "\n")

print()
print("KEYPAIR CREATED")
print("PRIVATE KEY:", root / "catdesk-ed25519-private.key")
print("PUBLIC KEY :", root / "catdesk-ed25519-public.key")
print()
print("PUBLIC_KEY_HEX=" + public_raw.hex())
'@ | py -