#!/usr/bin/env python3
"""Fetch pinned JVM support for Android's OS TLS verifier, verifying SHA-256."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tomllib
import zipfile

source = Path(__file__).resolve().parent
target = Path(sys.argv[1])
target.mkdir(parents=True, exist_ok=True)
lock = json.loads((source / "dependencies.json").read_text())
cargo = tomllib.loads((source.parent.parent / "Cargo.lock").read_text())
versions = {p["version"] for p in cargo["package"] if p["name"] == "rustls-platform-verifier-android"}
if versions != {lock["verifier_cargo_version"]}:
    raise SystemExit("Verifier Cargo version changed; update and review Android dependency lock first")
for artifact in lock["artifacts"]:
    path = target / artifact["file"]
    if not path.exists() or hashlib.sha256(path.read_bytes()).hexdigest() != artifact["sha256"]:
        subprocess.run(["curl", "--fail", "--location", "--silent", "--show-error", "--max-time", "120", artifact["url"], "-o", str(path)], check=True)
    if hashlib.sha256(path.read_bytes()).hexdigest() != artifact["sha256"]:
        raise SystemExit(f"Checksum mismatch for {artifact['file']}")
with zipfile.ZipFile(target / "verifier.aar") as archive:
    # Extract only the expected public support files, never arbitrary paths.
    for name in ["classes.jar", "res/xml/network_security_config.xml"]:
        path = target / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(archive.read(name))
