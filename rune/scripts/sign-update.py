#!/usr/bin/env python3
"""Create a signed Pickle update manifest using an existing Ed25519 PEM key.

Requires Python 3 and OpenSSL 3. This tool never generates or prints a private key.
The output is canonical JSON payload bytes wrapped with their Ed25519 signature.
"""
import argparse
import datetime as dt
import hashlib
import json
import pathlib
import re
import subprocess
import tempfile
import urllib.parse

DOMAIN = b"Pickle update manifest v1\0"
TARGETS = {
    "aarch64-apple-darwin": {".dmg", ".zip"},
    "x86_64-apple-darwin": {".dmg", ".zip"},
    "x86_64-pc-windows-msvc": {".exe", ".msi", ".msix", ".msixbundle"},
    "aarch64-pc-windows-msvc": {".exe", ".msi", ".msix", ".msixbundle"},
}


def canonical(value):
    # Object insertion order is the protocol's Rust struct field order.
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode("utf-8")


def https_url(value):
    parsed = urllib.parse.urlsplit(value)
    if (len(value) > 4096 or parsed.scheme != "https" or not parsed.hostname
            or parsed.username is not None or parsed.password is not None or parsed.fragment
            or (parsed.port is not None and not 1 <= parsed.port <= 65535)):
        raise ValueError("Every published update URL must use HTTPS without embedded credentials or fragments")
    return value


def artifact_record(target, filename, url):
    source = pathlib.Path(filename)
    name = source.name
    reserved = {"CON", "PRN", "AUX", "NUL", *("COM" + str(i) for i in range(1, 10)), *("LPT" + str(i) for i in range(1, 10))}
    if (target not in TARGETS or source.suffix not in TARGETS[target] or not source.is_file()
            or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,119}", name)
            or ".." in name or name.split(".")[0].upper() in reserved):
        raise ValueError("An artifact has an unsupported target, unsafe filename, or missing file")
    digest = hashlib.sha256()
    count = 0
    if not 0 < source.stat().st_size <= 512 * 1024 * 1024:
        raise ValueError("Installer size must be between 1 byte and 512 MiB")
    with source.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            count += len(chunk)
            if count > 512 * 1024 * 1024:
                raise ValueError("Installer size exceeds 512 MiB")
            digest.update(chunk)
    if not 0 < count <= 512 * 1024 * 1024:
        raise ValueError("Installer size must be between 1 byte and 512 MiB")
    return {"target": target, "url": https_url(url), "sha256": digest.hexdigest(), "size": count, "filename": name}


def openssl(command, *arguments):
    result = subprocess.run([command, *arguments], stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
    if result.returncode:
        raise ValueError("OpenSSL signing failed; verify OpenSSL 3 and the Ed25519 PEM key")
    return result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--key", required=True, type=pathlib.Path, help="Existing private Ed25519 PEM key outside the repository")
    parser.add_argument("--version", required=True)
    parser.add_argument("--channel", choices=("stable", "beta"), default="stable")
    parser.add_argument("--notes", default="", help="Release notes text, at most 4,000 UTF-8 bytes")
    parser.add_argument("--expires-days", type=int, default=14)
    parser.add_argument("--artifact", action="append", nargs=3, metavar=("TARGET", "FILE", "HTTPS_URL"), required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--openssl", default="openssl", help="Path to OpenSSL 3")
    args = parser.parse_args()
    version = re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?", args.version)
    if (version is None or any(int(part) > 2**64 - 1 for part in version.groups()[:3])
            or (version.group(4) is not None and any(part.isdigit() and len(part) > 1 and part.startswith("0") for part in version.group(4).split(".")))):
        raise ValueError("Version must be a semantic version")
    if args.channel == "stable" and "-" in args.version.split("+")[0]:
        raise ValueError("Stable manifests cannot publish prerelease versions")
    if len(args.notes.encode("utf-8")) > 4000 or not 1 <= args.expires_days <= 30:
        raise ValueError("Notes exceed 4,000 bytes or expiry is outside 1–30 days")
    records = [artifact_record(*record) for record in args.artifact]
    if len(records) > 16 or len({record["target"] for record in records}) != len(records):
        raise ValueError("Supply at most one artifact per target, and at most 16 artifacts")
    now = dt.datetime.now(dt.timezone.utc).replace(microsecond=0)
    stamp = lambda value: value.isoformat().replace("+00:00", "Z")
    payload = canonical({"version": args.version, "channel": args.channel, "published_at": stamp(now),
                         "expires_at": stamp(now + dt.timedelta(days=args.expires_days)), "notes": args.notes,
                         "artifacts": records})
    if len(payload) > 24000:
        raise ValueError("The signed payload exceeds its size budget")
    with tempfile.TemporaryDirectory(prefix="pickle-update-sign-") as temporary:
        root = pathlib.Path(temporary)
        message = root / "message.bin"
        signature_file = root / "signature.bin"
        message.write_bytes(DOMAIN + payload)
        openssl(args.openssl, "pkeyutl", "-sign", "-rawin", "-inkey", str(args.key), "-in", str(message), "-out", str(signature_file))
        signature = signature_file.read_bytes()
    if len(signature) != 64:
        raise ValueError("OpenSSL did not produce an Ed25519 signature")
    public_der = openssl(args.openssl, "pkey", "-in", str(args.key), "-pubout", "-outform", "DER")
    prefix = bytes.fromhex("302a300506032b6570032100")
    if len(public_der) != len(prefix) + 32 or not public_der.startswith(prefix):
        raise ValueError("The configured signing key is not Ed25519")
    envelope = {"schema": 1, "payload_hex": payload.hex(), "signature_hex": signature.hex()}
    encoded = json.dumps(envelope, indent=2).encode("utf-8") + b"\n"
    if len(encoded) > 65536:
        raise ValueError("The signed envelope exceeds its size budget")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=args.output.parent, prefix=".pickle-manifest-", delete=False) as temporary:
        staged = pathlib.Path(temporary.name)
        temporary.write(encoded)
    try:
        staged.replace(args.output)
    finally:
        staged.unlink(missing_ok=True)
    print("Signed update manifest written.")
    print("PICKLE_UPDATE_PUBLIC_KEY_HEX=" + public_der[len(prefix):].hex())


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError) as error:
        raise SystemExit(str(error))
