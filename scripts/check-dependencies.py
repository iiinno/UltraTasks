"""Windows向けcargo metadataの有効依存とfeatureを検査する。"""

import argparse
import json
from pathlib import Path
import re
import sys
import tomllib


KIT = {
    "gpui-kit", "gpui-base", "gpui-component", "gpui-component-macros",
    "gpui-kit-assets",
}
CORE_NAMES = {"gpui", "gpui-pre", "gpui-fast"}
WEBVIEW = re.compile(r"webview|webkit|(^|[-_/])wry($|[-_/])|tauri|electron|(^|[-_/])cef($|[-_/])", re.I)


def sqlite_versions():
    root = Path(__file__).resolve().parent.parent
    manifest = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    lock = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))
    versions = {}
    for name in ("rusqlite", "libsqlite3-sys"):
        found = [package["version"] for package in lock["package"] if package["name"] == name]
        if len(found) != 1:
            raise ValueError(f"Cargo.lock: {name}は単一版が必要: {found}")
        versions[name] = found[0]
    if manifest["workspace"]["dependencies"]["rusqlite"]["version"] != f"={versions['rusqlite']}":
        raise ValueError("Cargo.toml/Cargo.lockのrusqlite固定版が不一致")
    sqlite = versions["libsqlite3-sys"]
    if not re.fullmatch(r"\d+\.\d+\.\d+", sqlite) or tuple(map(int, sqlite.split("."))) < (0, 39, 0):
        raise ValueError("libsqlite3-sysは安定版0.39.0以上が必要")
    return versions


def check(metadata):
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    errors = []
    active_ids = set()
    pending = list(metadata["workspace_members"])
    while pending:
        package_id = pending.pop()
        if package_id in active_ids:
            continue
        active_ids.add(package_id)
        pending.extend(nodes[package_id]["dependencies"])
    active = [packages[package_id] for package_id in sorted(active_ids)]

    def require(condition, message):
        if not condition:
            errors.append(message)

    def version(name, expected):
        found = [package["version"] for package in active if package["name"] == name]
        require(found == [expected], f"{name}: expected [{expected}], got {found}")

    cores = [(package["name"], package["version"]) for package in active if package["name"] in CORE_NAMES]
    require(cores == [("gpui-pre", "0.3.11")], f"GPUI本体が単一のgpui-pre 0.3.11ではない: {cores}")
    for name in sorted(KIT):
        version(name, "0.9.0")
    for package in active:
        name = package["name"]
        if name.startswith("gpui-pre-") and name != "gpui-pre-reqwest":
            require(package["version"] == "0.3.11", f"GPUI系列の版違い: {name} {package['version']}")
        require(not name.startswith("gpui-fast"), f"別GPUI系列: {name}")
        require(not WEBVIEW.search(name), f"WebView関連package: {name}")
        for feature in nodes[package["id"]]["features"]:
            require(not WEBVIEW.search(feature), f"WebView関連feature: {name}/{feature}")
    for name, expected in {
        **sqlite_versions(), "jiff": "0.2.37",
        "jiff-tzdb": "0.1.8", "uuid": "1.29.2", "serde": "1.0.229",
        "serde_json": "1.0.154", "futures": "0.3.34", "chrono": "0.4.45",
        "tempfile": "3.27.0",
    }.items():
        version(name, expected)
    by_name = {package["name"]: package for package in active}
    for name, needed in {"rusqlite": {"bundled", "hooks"}, "jiff": {"std", "tzdb-bundle-always"}}.items():
        features = set(nodes[by_name[name]["id"]]["features"])
        require(needed <= features, f"{name}: 必須feature不足 {sorted(needed - features)}")
        if name == "jiff":
            forbidden = features & {"default", "tz-system", "tzdb-zoneinfo", "tzdb-concatenated", "tzdb-bundle-platform"}
            require(not forbidden, f"jiff: OSへの暗黙fallback feature {sorted(forbidden)}")
    core = by_name["ultratasks-core"]
    desktop = by_name["ultratasks-desktop"]
    require(core["features"].get("default") == [], "coreのdefault featureは空でなければならない")
    require("fixtures" in core["features"] and "test-support" in core["features"], "coreの明示featureが不足")
    for package in (core, desktop):
        for dependency in package["dependencies"]:
            if dependency["name"] == "tempfile":
                require(dependency["kind"] == "dev", "tempfileはdev-dependencyだけで使用する")
    core_dep = next(dep for dep in desktop["dependencies"] if dep["name"] == "ultratasks-core")
    require(not core_dep["uses_default_features"] and core_dep["features"] == ["fixtures"], "desktopはcoreのfixturesだけを有効にする")
    kit_dep = next(dep for dep in desktop["dependencies"] if dep["name"] == "gpui-kit")
    require(kit_dep["req"] == "=0.9.0" and not kit_dep["uses_default_features"] and set(kit_dep["features"]) == {"component", "assets"}, "Kitの固定依存・featureが契約と異なる")
    windows_dep = next(dep for dep in desktop["dependencies"] if dep["name"] == "windows")
    require(windows_dep["req"] == "=0.62.2", "Windows adapterの版は0.62.2に固定する")
    require("gpui-pre-windows" in by_name, "Windows向けmetadataではない")
    require(not ({"gpui-pre-web", "gpui-pre-linux", "gpui-pre-macos", "gpui-pre-ios"} & by_name.keys()), "対象外platformが有効。--filter-platform x86_64-pc-windows-msvcを使用する")
    return {"active_packages": len(active), "gpui_core": cores, "errors": errors}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("metadata", type=Path)
    args = parser.parse_args()
    try:
        result = check(json.loads(args.metadata.read_text(encoding="utf-8-sig")))
    except (OSError, ValueError, KeyError, TypeError, StopIteration) as error:
        print(f"CHECK_FAILED: metadataまたは依存定義が不正または不完全: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, ensure_ascii=True, indent=2))
    print("CHECK_FAILED" if result["errors"] else "CHECK_OK")
    return int(bool(result["errors"]))


if __name__ == "__main__":
    raise SystemExit(main())
