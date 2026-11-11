"""Inspect or refresh only the pinned rusqlite lock on a GitHub-hosted Windows runner."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tomllib
from datetime import datetime, timezone
from pathlib import Path


TOOLCHAIN = "1.99.1-x86_64-pc-windows-msvc"
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
MINIMUM_SQLITE_SYS = (0, 39, 0)
PREPARED_NOTICE_SQLITE_SYS = "0.39.1"


def require_runner(update: bool) -> None:
    if (os.name != "nt" or os.environ.get("GITHUB_ACTIONS") != "true"
            or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted"
            or os.environ.get("RUNNER_OS") != "Windows"):
        raise ValueError("Lock preparation is restricted to GitHub-hosted Windows; do not run it on the development PC.")
    if update and (os.environ.get("GITHUB_REPOSITORY") != "iiinno/UltraTasks"
                   or os.environ.get("GITHUB_EVENT_NAME") not in {"push", "workflow_dispatch"}
                   or os.environ.get("GITHUB_REF") not in {"refs/heads/main", "refs/heads/prototype/pc-first"}):
        raise ValueError("Only trusted branch push/dispatch runs may generate a replacement lock.")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def sqlite_sys_identity(packages: list[dict]) -> dict:
    entries = [package for package in packages if package["name"] == "libsqlite3-sys"]
    if len(entries) != 1 or entries[0].get("source") != REGISTRY:
        raise ValueError("The lock must contain exactly one crates.io libsqlite3-sys package.")
    entry = entries[0]
    version = entry.get("version", "")
    checksum = entry.get("checksum", "")
    if not re.fullmatch(r"\d+\.\d+\.\d+", version) or not re.fullmatch(r"[0-9a-f]{64}", checksum):
        raise ValueError("libsqlite3-sys must have a stable version and a valid registry checksum in Cargo.lock.")
    return {
        "version": version, "checksum": checksum, "source": entry["source"],
        "minimum_version_met": tuple(map(int, version.split("."))) >= MINIMUM_SQLITE_SYS,
        "prepared_notice_version": PREPARED_NOTICE_SQLITE_SYS,
        "notice_review_required": version != PREPARED_NOTICE_SQLITE_SYS,
    }


def inspect_inputs(root: Path) -> tuple[dict, set[tuple[str, str, str]]]:
    manifest_path = root / "Cargo.toml"
    manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
    dependency = manifest["workspace"]["dependencies"]["rusqlite"]
    if not isinstance(dependency, dict) or any(key in dependency for key in ("path", "git", "registry")):
        raise ValueError("rusqlite must be an exactly pinned crates.io dependency table.")
    version = re.fullmatch(r"=(\d+\.\d+\.\d+)", dependency.get("version", ""))
    if version is None:
        raise ValueError("rusqlite must use an exact stable =MAJOR.MINOR.PATCH version; no floating or guessed update is allowed.")
    lock_path = root / "Cargo.lock"
    lock = tomllib.loads(lock_path.read_text(encoding="utf-8"))
    entries = [package for package in lock["package"] if package["name"] == "rusqlite"]
    if len(entries) != 1 or entries[0].get("source") != REGISTRY:
        raise ValueError("The existing lock must contain exactly one crates.io rusqlite package; do not regenerate the whole lock.")
    manifests = {"Cargo.toml": digest(manifest_path)}
    for member in manifest["workspace"]["members"]:
        path = (root / member / "Cargo.toml").resolve()
        if not path.is_relative_to(root):
            raise ValueError("Workspace manifests must remain inside the checkout.")
        manifests[path.relative_to(root).as_posix()] = digest(path)
    packages = {(package["name"], package["version"], package.get("source", "workspace")) for package in lock["package"]}
    return {
        "manifest_version": version[1], "manifest_features": dependency.get("features", []),
        "manifest_default_features": dependency.get("default-features", True),
        "locked_version": entries[0]["version"],
        "needs_update": entries[0]["version"] != version[1],
        "manifest_sha256": manifests["Cargo.toml"], "manifest_files": manifests,
        "lock_sha256": digest(lock_path), "libsqlite3_sys": sqlite_sys_identity(lock["package"]),
    }, packages


def provenance(root: Path) -> dict:
    source_sha = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
    if not re.fullmatch(r"[0-9a-f]{40}", source_sha) or source_sha != os.environ.get("GITHUB_SHA"):
        raise ValueError("Checkout SHA must match GITHUB_SHA.")
    if subprocess.run(["git", "-C", str(root), "diff", "--quiet", "HEAD", "--"], check=False).returncode != 0:
        raise ValueError("Lock preparation must start from a clean tracked checkout.")
    return {
        "source_sha": source_sha, "repository": os.environ["GITHUB_REPOSITORY"],
        "event": os.environ["GITHUB_EVENT_NAME"], "ref": os.environ["GITHUB_REF"],
        "run_id": os.environ["GITHUB_RUN_ID"], "run_attempt": os.environ["GITHUB_RUN_ATTEMPT"],
        "toolchain": TOOLCHAIN, "execution_host": "github-hosted",
    }


def save_json(path: Path, value: dict) -> None:
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def output(name: str, value: str) -> None:
    with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as stream:
        stream.write(f"{name}={value}\n")


def refresh(root: Path, evidence: Path, inputs: dict, packages: set, origin: dict) -> int:
    plan = json.loads((evidence / "lock-plan.json").read_text(encoding="utf-8"))
    if any(plan.get(key) != value for key, value in {**inputs, **origin}.items()) or not inputs["needs_update"]:
        raise ValueError("Source, manifests or original lock changed since inspection; refusing the update.")
    cargo = shutil.which("cargo.exe")
    if cargo is None:
        raise ValueError("The pinned Rust toolchain must be installed on the runner first.")
    command = [cargo, f"+{TOOLCHAIN}", "update", "--manifest-path", str(root / "Cargo.toml"),
               "--package", "rusqlite", "--precise", inputs["manifest_version"]]
    record = {
        "schema_version": 1, "purpose": "rusqlite-lock-update-only", **origin,
        "manifest_sha256": inputs["manifest_sha256"], "manifest_files": inputs["manifest_files"],
        "original_lock_sha256": inputs["lock_sha256"], "generated_lock_sha256": None,
        "previous_version": inputs["locked_version"], "requested_version": inputs["manifest_version"],
        "requested_features": inputs["manifest_features"], "default_features": inputs["manifest_default_features"],
        "previous_libsqlite3_sys": inputs["libsqlite3_sys"], "resolved_libsqlite3_sys": None,
        "minimum_libsqlite3_sys_version": ".".join(map(str, MINIMUM_SQLITE_SYS)), "lock_candidate_policy_passed": False,
        "command": command, "working_directory": str(root), "timeout_seconds": 600, "timed_out": False,
        "started_utc": datetime.now(timezone.utc).isoformat(), "status": "RUNNING", "exit_code": None,
        "full_verification": "NOT_RUN", "benchmark": "NOT_RUN", "release_eligible": False,
        "next_step": "Review and commit this lock, then run full verification with --locked on the new SHA.",
    }
    result_path = evidence / "lock-update.json"
    save_json(result_path, record)
    try:
        with (evidence / "cargo-update.stdout.log").open("wb") as stdout, (evidence / "cargo-update.stderr.log").open("wb") as stderr:
            process = subprocess.Popen(command, cwd=root, stdout=stdout, stderr=stderr)
            try:
                record["exit_code"] = process.wait(timeout=600)
            except subprocess.TimeoutExpired:
                record["exit_code"] = 124
                record["timed_out"] = True
                subprocess.run(["taskkill.exe", "/PID", str(process.pid), "/T", "/F"], stdout=stderr, stderr=stderr, check=False, timeout=30)
                process.wait(timeout=30)
        if record["exit_code"] != 0:
            raise ValueError(f"Targeted cargo update failed (exit {record['exit_code']}); no replacement lock is published.")
        updated, new_packages = inspect_inputs(root)
        record["generated_lock_sha256"] = updated["lock_sha256"]
        record["resolved_libsqlite3_sys"] = updated["libsqlite3_sys"]
        if not updated["libsqlite3_sys"]["minimum_version_met"]:
            raise ValueError(f"Rejected lock candidate: libsqlite3-sys {updated['libsqlite3_sys']['version']} is below 0.39.0.")
        if updated["needs_update"] or updated["manifest_files"] != inputs["manifest_files"]:
            raise ValueError("Update did not resolve the exact adopted rusqlite version, or changed a manifest.")
        changed = subprocess.check_output(["git", "-C", str(root), "diff", "--name-only", "HEAD", "--"], text=True).splitlines()
        if changed != ["Cargo.lock"]:
            raise ValueError("Targeted lock update must change Cargo.lock only.")
        record["lock_candidate_policy_passed"] = True
        if updated["libsqlite3_sys"]["notice_review_required"]:
            record["next_step"] = "Recheck notices/licenses against the resolved libsqlite3-sys version and checksum before accepting this lock; then commit and run --locked verification."
        record["removed_packages"] = sorted(packages - new_packages)
        record["added_packages"] = sorted(new_packages - packages)
        (evidence / "Cargo.lock").write_bytes((root / "Cargo.lock").read_bytes())
        record["status"] = "GENERATED_UNVERIFIED"
    except Exception as error:
        record["status"] = "FAILED"
        record["error"] = str(error)
    finally:
        record["finished_utc"] = datetime.now(timezone.utc).isoformat()
        save_json(result_path, record)
    if record["status"] != "GENERATED_UNVERIFIED":
        print(record.get("error", "Lock update failed."), file=sys.stderr)
        return 1
    output("generated", "true")
    print("Replacement Cargo.lock generated; full verification, benchmark and release are NOT_RUN.")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("inspect", "update"))
    parser.add_argument("--workspace", required=True, type=Path)
    parser.add_argument("--evidence", required=True, type=Path)
    args = parser.parse_args()
    require_runner(args.mode == "update")
    root = args.workspace.resolve()
    evidence = args.evidence.resolve()
    if evidence.is_relative_to(root):
        raise ValueError("Keep lock evidence outside the source checkout.")
    origin = provenance(root)
    inputs, packages = inspect_inputs(root)
    evidence.mkdir(parents=True, exist_ok=True)
    if args.mode == "inspect":
        save_json(evidence / "lock-plan.json", {"schema_version": 1, **origin, **inputs})
        if not inputs["needs_update"] and not inputs["libsqlite3_sys"]["minimum_version_met"]:
            raise ValueError("The matching lock still resolves libsqlite3-sys below 0.39.0; full verification and distribution are blocked.")
        output("needs_update", str(inputs["needs_update"]).lower())
        print(f"rusqlite manifest={inputs['manifest_version']}; lock={inputs['locked_version']}; needs_update={inputs['needs_update']}")
        print(f"libsqlite3-sys version={inputs['libsqlite3_sys']['version']}; checksum={inputs['libsqlite3_sys']['checksum']}; notice_review_required={inputs['libsqlite3_sys']['notice_review_required']}")
        return 0
    return refresh(root, evidence, inputs, packages, origin)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
