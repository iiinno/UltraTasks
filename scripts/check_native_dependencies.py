#!/usr/bin/env python3
"""Check native client dependency graphs for forbidden WebView packages."""
import subprocess

for client in ("ultratasks-desktop", "ultratasks-android"):
    tree = subprocess.check_output(
        ["cargo", "tree", "--locked", "-p", client, "--target", "all",
         "--prefix", "none", "--format", "{p}"], text=True
    )
    packages = {line.split()[0] for line in tree.splitlines() if line.strip()}
    forbidden = sorted(name for name in packages if name in {"wry", "tauri", "cef"} or any(
        fragment in name.lower() for fragment in ("webview", "webkit", "web-kit", "cef-")
    ))
    if forbidden:
        raise SystemExit(client + ": unexpected WebView packages: " + ", ".join(forbidden))
    print(f"{client}: checked {len(packages)} selected packages; no forbidden WebView package.")
