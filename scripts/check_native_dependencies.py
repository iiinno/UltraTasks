#!/usr/bin/env python3
"""Check the selected desktop dependency graph for forbidden WebView packages."""
import subprocess

tree = subprocess.check_output(
    ["cargo", "tree", "--locked", "-p", "ultratasks-desktop", "--target", "all",
     "--prefix", "none", "--format", "{p}"], text=True
)
packages = {line.split()[0] for line in tree.splitlines() if line.strip()}
forbidden = sorted(name for name in packages if name in {"wry", "tauri", "cef"} or any(
    fragment in name.lower() for fragment in ("webview", "webkit", "web-kit", "cef-")
))
if forbidden:
    raise SystemExit("Unexpected WebView packages: " + ", ".join(forbidden))
print(f"Checked {len(packages)} selected desktop packages: no forbidden WebView package found.")
