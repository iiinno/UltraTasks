"""生sampleを保持し、試行別・同一条件の合算をJSON/CSVへ出す。標準ライブラリのみ。"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import os
import stat
import statistics
import sys
import uuid
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path


CORE_METRICS = {
    "processing_ms": ("processing_ns", 1_000_000),
    "verification_ms": ("verification_ns", 1_000_000),
    "queue_wait_ms": ("queue_wait_ns", 1_000_000),
    "presentation_ms": ("presentation_ns", 1_000_000),
}
PROCESS_METRICS = {
    "interval_ms": ("interval_ms", 1),
    "cpu_delta_ms": ("cpu_delta_ms", 1),
    "cpu_one_core_pct": ("cpu_one_core_pct", 1),
    "cpu_all_cores_pct": ("cpu_all_cores_pct", 1),
    "private_bytes": ("private_bytes", 1),
    "working_set_bytes": ("working_set_bytes", 1),
    "observer_read_ms": ("observer_read_ms", 1),
}
STATUSES = {"success", "failure", "timeout"}


def distribution(values: list[float]) -> dict:
    ordered = sorted(values)
    if not ordered:
        return {"n": 0, "median": None, "p95": None, "p99": None, "min": None, "max": None}
    return {
        "n": len(ordered),
        "median": statistics.median(ordered),
        "p95": ordered[math.ceil(len(ordered) * 95 / 100) - 1],
        "p99": ordered[math.ceil(len(ordered) * 99 / 100) - 1],
        "min": ordered[0],
        "max": ordered[-1],
    }


def input_status(path: Path, root: Path):
    path.relative_to(root)
    if not path.is_absolute() or ".." in path.parts:
        raise ValueError(f"入力root外または不正なパス: {path}")
    for component in (*reversed(path.parents), path):
        info = component.lstat()
        if (stat.S_ISLNK(info.st_mode)
                or getattr(info, "st_file_attributes", 0) & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400)):
            raise ValueError(f"symlink/reparse pointは読み取らない: {component}")
        if component != path and not stat.S_ISDIR(info.st_mode):
            raise ValueError(f"入力の親がディレクトリではない: {component}")
    return info


def open_input(path: Path, root: Path, mode="rb", **kwargs):
    if not stat.S_ISREG(input_status(path, root).st_mode):
        raise ValueError(f"通常ファイルではない: {path}")
    return path.open(mode, **kwargs)


def find_manifests(root: Path, issues: list[str]) -> list[Path]:
    pending, manifests = [root], []
    while pending:
        directory = pending.pop()
        try:
            if not stat.S_ISDIR(input_status(directory, root).st_mode):
                raise ValueError(f"ディレクトリではない: {directory}")
            for child in sorted(directory.iterdir()):
                try:
                    info = input_status(child, root)
                    if stat.S_ISDIR(info.st_mode):
                        pending.append(child)
                    elif child.name == "run.json" and stat.S_ISREG(info.st_mode):
                        manifests.append(child)
                except (OSError, ValueError) as error:
                    issues.append(f"{child}: {error}")
        except (OSError, ValueError) as error:
            issues.append(f"{directory}: {error}")
    return sorted(manifests)


def digest_input(path: Path, root: Path) -> dict:
    identity = {"path": str(path), "provenance": "local_input_artifact", "captured_at_summary": True}
    try:
        with open_input(path, root) as stream:
            value = hashlib.file_digest(stream, "sha256").hexdigest()
        return {**identity, "sha256": value}
    except (OSError, ValueError) as error:
        return {**identity, "sha256": None, "reason": str(error)}


def runtime_hash_claims(metadata: dict) -> dict:
    environment = metadata.get("environment")
    environment = environment if isinstance(environment, dict) else {}
    identity = metadata.get("identity")
    identity = identity if isinstance(identity, dict) else {}
    entries = [(f"/environment/{key}", environment.get(key))
               for key in ("product_binary", "measure_binary", "lock")]
    entries.append(("/identity/binary", identity.get("binary")))
    sources = environment.get("sources")
    if isinstance(sources, list):
        entries.extend((f"/environment/sources/{index}", entry) for index, entry in enumerate(sources))
    return {
        "provenance": "unverified_runtime_claims_from_run.json",
        "runner_paths_opened": False,
        "records": [{"source": "run.json#" + pointer, "reported_path": entry.get("path"),
                     "sha256": entry.get("sha256"), "bytes": entry.get("bytes")}
                    for pointer, entry in entries if isinstance(entry, dict) and "sha256" in entry],
    }


def load_samples(path: Path, metadata: dict, issues: list[str], root: Path) -> list[dict]:
    samples = []
    seen = set()
    try:
        stream = open_input(path, root, "r", encoding="utf-8-sig")
    except (OSError, ValueError) as error:
        issues.append(f"{path}: 生sampleを読めない: {error}")
        return samples
    with stream:
        for line_number, line in enumerate(stream, 1):
            try:
                row = json.loads(line)
                key = (row["size"], row["trial"], row["phase"], row["operation"], row["sample"])
                limit = metadata["warmup"] if row["phase"] == "warmup" else metadata["samples"]
                if (row["schema_version"] != 1 or row["status"] not in STATUSES
                        or row["size"] not in metadata["sizes"]
                        or row["phase"] not in {"sample", "warmup"}
                        or row["operation"] not in metadata["operations"]
                        or not 1 <= row["trial"] <= metadata["trials"]
                        or not 0 <= row["sample"] < limit or key in seen):
                    raise ValueError("重複またはmanifestと一致しないsample")
                if row["status"] == "success" and not row["verified"]:
                    raise ValueError("未検査sampleがsuccessになっています")
                metrics = CORE_METRICS if metadata["kind"] == "core" else PROCESS_METRICS
                for field, _ in metrics.values():
                    value = row.get(field)
                    if value is not None and (isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value < 0):
                        raise ValueError(f"不正な数値: {field}")
                seen.add(key)
                samples.append(row)
            except (ValueError, TypeError, KeyError) as error:
                issues.append(f"{path}:{line_number}: {error}")
    return samples


def metric_values(rows: list[dict], field: str, divisor: float) -> list[float]:
    return [row[field] / divisor for row in rows if row.get(field) is not None]


def summarize_group(run: str, metadata: dict, size: int, phase: str, operation: str,
                    trial: int | None, rows: list[dict]) -> dict:
    trial_count = metadata["trials"] if trial is None else 1
    per_trial = metadata["warmup"] if phase == "warmup" else metadata["samples"]
    expected = per_trial * trial_count
    successes = [row for row in rows if row["status"] == "success"]
    result = {
        "run": run, "kind": metadata["kind"], "size": size,
        "distribution": metadata.get("distribution"),
        "phase": phase, "operation": operation,
        "scope": "pooled" if trial is None else "trial", "trial": trial,
        "trials_planned": trial_count, "trials_observed": len({row["trial"] for row in rows}),
        "expected_samples": expected, "sample_count": len(rows),
        "success_count": len(successes),
        "failure_count": sum(row["status"] == "failure" for row in rows),
        "timeout_count": sum(row["status"] == "timeout" for row in rows),
        "deadline_exceeded_count": sum(bool(row.get("deadline_exceeded")) for row in rows),
        "verified_count": sum(bool(row["verified"]) for row in rows),
        "not_run_count": max(0, expected - len(rows)),
        "metrics": {},
    }
    metrics = CORE_METRICS if metadata["kind"] == "core" else PROCESS_METRICS
    by_trial = defaultdict(list)
    for row in rows:
        by_trial[row["trial"]].append(row)
    for name, (field, divisor) in metrics.items():
        values = metric_values(rows, field, divisor)
        all_stats = distribution(values)
        trial_medians = [statistics.median(trial_values) for trial_rows in by_trial.values()
                         if (trial_values := metric_values(trial_rows, field, divisor))]
        result["metrics"][name] = {
            "all_attempts": all_stats,
            "success_only": distribution(metric_values(successes, field, divisor)),
            "unobserved_count": len(rows) - len(values),
            "trial_medians": distribution(trial_medians),
        }
    return result


def summarize_run(path: Path, root: Path) -> tuple[dict, list[dict], list[str]]:
    with open_input(path, root, "r", encoding="utf-8-sig") as stream:
        metadata = json.load(stream)
    if metadata["schema_version"] != 1 or metadata["kind"] not in {"core", "process"}:
        raise ValueError(f"未対応manifest: {path}")
    issues: list[str] = []
    source = path.parent / ("core-samples.jsonl" if metadata["kind"] == "core" else "process-samples.jsonl")
    samples = load_samples(source, metadata, issues, root)
    rows_by_group = defaultdict(list)
    for row in samples:
        rows_by_group[(row["size"], row["phase"], row["operation"], row["trial"])].append(row)
    groups = []
    for size in metadata["sizes"]:
        for phase, count in (("warmup", metadata["warmup"]), ("sample", metadata["samples"])):
            if not count:
                continue
            for operation in metadata["operations"]:
                pooled = []
                for trial in range(1, metadata["trials"] + 1):
                    rows = rows_by_group[(size, phase, operation, trial)]
                    pooled.extend(rows)
                    groups.append(summarize_group(str(path.parent), metadata, size, phase, operation, trial, rows))
                groups.append(summarize_group(str(path.parent), metadata, size, phase, operation, None, pooled))
    completion_path = path.parent / "completion.json"
    completion = None
    try:
        with open_input(completion_path, root, "r", encoding="utf-8-sig") as stream:
            completion = json.load(stream)
    except (OSError, ValueError) as error:
        issues.append(f"{completion_path}: {error}")
    if completion is None or not completion.get("completed"):
        issues.append(f"{path.parent}: 試行未完了。completion.jsonとtrial JSONを確認")
    environment = {
        "run": str(path.parent), "metadata": metadata, "completion": completion,
        "manifest_sha256": digest_input(path, root),
        "sample_file": str(source), "sample_sha256": digest_input(source, root),
        "reported_runtime_hashes": runtime_hash_claims(metadata),
    }
    return environment, groups, issues


def csv_cell(value):
    if isinstance(value, str) and (value.startswith(("\t", "\r", "\n"))
            or value.lstrip(" \t\r\n\v\f\ufeff").startswith(("=", "+", "-", "@"))):
        return "'" + value
    return value


def write_csv(path: Path, groups: list[dict]) -> None:
    identity = ["run", "kind", "size", "distribution", "phase", "operation", "scope", "trial",
                "trials_planned", "trials_observed", "expected_samples", "sample_count",
                "success_count", "failure_count", "timeout_count", "deadline_exceeded_count",
                "verified_count", "not_run_count"]
    columns = identity + ["metric", "population", "n", "median", "p95", "p99", "min", "max", "unobserved_count"]
    with path.open("x", encoding="utf-8", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=columns)
        writer.writeheader()
        for group in groups:
            for metric, stats in group["metrics"].items():
                for population in ("all_attempts", "success_only", "trial_medians"):
                    row = {**{key: group[key] for key in identity},
                           "metric": metric, "population": population,
                           **stats[population], "unobserved_count": stats["unobserved_count"]}
                    writer.writerow({key: csv_cell(value) for key, value in row.items()})


def main() -> int:
    parser = argparse.ArgumentParser(description="性能生sampleの集計。既存出力は上書きしない。")
    parser.add_argument("--input", type=Path, required=True, help="run.jsonを含む専用の測定ディレクトリ")
    parser.add_argument("--out", type=Path, help="未作成の集計ディレクトリ。省略時はinput直下に新規作成")
    args = parser.parse_args()
    source = Path(os.path.abspath(args.input))
    if not stat.S_ISDIR(input_status(source, source).st_mode):
        parser.error("inputはディレクトリです")
    environments, groups, issues = [], [], []
    manifests = find_manifests(source, issues)
    if not manifests and not issues:
        parser.error("run.jsonがありません")
    target = args.out or source / ("summary-" + uuid.uuid4().hex)
    input_status(source, source)
    target.mkdir(parents=False, exist_ok=False)
    for path in manifests:
        try:
            environment, run_groups, run_issues = summarize_run(path, source)
            environments.append(environment)
            groups.extend(run_groups)
            issues.extend(run_issues)
        except (OSError, ValueError, KeyError, TypeError) as error:
            issues.append(f"{path}: {error}")
    valid = bool(groups) and not issues and all(
        group["failure_count"] == group["timeout_count"] == group["not_run_count"] == 0
        for group in groups)
    result = {
        "schema_version": 1, "created_utc": datetime.now(timezone.utc).isoformat(),
        "measurement_complete_without_errors": valid,
        "performance_verdict": "未判定。比較・律速解析・実GUIの証拠を別途確認する",
        "statistics": {
            "median": "中央二値の平均（偶数sample）", "percentiles": "nearest-rank: sorted[ceil(n*p)-1]",
            "populations": "all_attemptsは失敗/期限超過も含む。success_onlyを別記。外れ値除外なし",
            "trial_medians": "各試行のall_attempts中央値の分布。pooledと試行変動は別物",
            "small_sample_warning": "20sampleのp99は最大値。少数sampleの尾部推定は粗い",
            "null": "未観測/未実行。0msや成功とは解釈しない",
        },
        "runs": environments, "groups": groups, "issues": issues,
    }
    with (target / "summary.json").open("x", encoding="utf-8") as stream:
        json.dump(result, stream, ensure_ascii=False, indent=2, allow_nan=False)
        stream.write("\n")
    write_csv(target / "summary.csv", groups)
    print(f"SUMMARY: {target.resolve()}")
    print(f"MEASUREMENT_COMPLETE_WITHOUT_ERRORS={valid}; runs={len(environments)}; issues={len(issues)}")
    return 0 if valid else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError) as error:
        print(f"SUMMARY_FAILED: {error}", file=sys.stderr)
        raise SystemExit(1) from error
