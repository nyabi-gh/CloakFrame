"""Signs the full packages a Velopack feed lists with the update key (RELEASING, signing keys).

Usage: python -I tools/package/sign_updates.py <folder> <channel>

<folder> holds releases.<channel>.json and the packages `vpk pack` wrote beside it. Each full
package gets <package>.sig, the base64 Ed25519 signature over the text the app checks
(crates/cloakframe-engine/src/update.rs): the channel, the version and file name the feed gives,
and the package's SHA-256, which has to match the feed's. The app installs no delta package.

Reads CLOAKFRAME2_UPDATE_PRIVATE_KEY (PEM) and CLOAKFRAME2_UPDATE_PUBLIC_KEY (base64), and needs
OpenSSL 3 on PATH.
"""

import base64
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path


def payload(channel, version, file_name, sha256):
    for field in (channel, version, file_name):
        if not field or "\n" in field or "\r" in field:
            raise SystemExit(f"the feed declares {field!r}, which cannot be signed")
    return (
        f"CloakFrame update v2\nchannel: {channel}\nversion: {version}\n"
        f"file: {file_name}\nsha256: {sha256}\n"
    ).encode()


def sha256_of(path):
    digest = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def openssl(*args, **kwargs):
    return subprocess.run(["openssl", *args], check=True, capture_output=True, **kwargs).stdout


def main():
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    folder, channel = Path(sys.argv[1]), sys.argv[2]
    private = os.environ.get("CLOAKFRAME2_UPDATE_PRIVATE_KEY")
    public = os.environ.get("CLOAKFRAME2_UPDATE_PUBLIC_KEY")
    if not private or not public:
        raise SystemExit("CLOAKFRAME2_UPDATE_PRIVATE_KEY and CLOAKFRAME2_UPDATE_PUBLIC_KEY are needed")
    feed = json.loads((folder / f"releases.{channel}.json").read_text(encoding="utf-8"))
    assets = [a for a in feed["Assets"] if a["Type"].lower() == "full"]
    if not assets:
        raise SystemExit(f"releases.{channel}.json lists no full package")

    with tempfile.TemporaryDirectory() as work:
        work = Path(work)
        key = work / "update-key.pem"
        key.write_text(private.strip() + "\n", encoding="ascii")
        # A private key that is not the pinned public key's other half would be caught by every
        # client and by nothing before them.
        derived = base64.b64encode(openssl("pkey", "-in", key, "-pubout", "-outform", "DER")[-32:])
        if derived.decode() != public.strip():
            raise SystemExit(
                f"the private key's public half is {derived.decode()}, not "
                "CLOAKFRAME2_UPDATE_PUBLIC_KEY; every client would refuse these updates"
            )
        public_pem = work / "update-key.pub"
        openssl("pkey", "-in", key, "-pubout", "-out", public_pem)

        for asset in assets:
            package = folder / asset["FileName"]
            digest = sha256_of(package)
            if digest != asset["SHA256"].lower():
                raise SystemExit(f"{package} has SHA-256 {digest}, not the feed's {asset['SHA256']}")
            signed = work / "payload"
            signed.write_bytes(payload(channel, asset["Version"], asset["FileName"], digest))
            signature = work / "signature"
            openssl("pkeyutl", "-sign", "-inkey", key, "-rawin", "-in", signed, "-out", signature)
            # A signature that does not verify would only show on the clients.
            openssl("pkeyutl", "-verify", "-pubin", "-inkey", public_pem, "-rawin",
                    "-in", signed, "-sigfile", signature)
            sig = folder / f"{asset['FileName']}.sig"
            sig.write_bytes(base64.b64encode(signature.read_bytes()))
            print(f"signed {asset['FileName']} {channel} {asset['Version']} {digest}")


if __name__ == "__main__":
    main()
