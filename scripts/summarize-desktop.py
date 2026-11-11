"""Desktop診断JSONLをファイル別に集計する。標準ライブラリのみ。"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import statistics
import sys
import uuid
from collections import Counter, defaultdict
from datetime import datetime, timezone
from pathlib import Path


DURATION = {"duration_ms": ("duration_us", 1000)}
METRICS = {
    "first_frame_callback": {"since_diagnostics_start_ms": ("elapsed_us", 1000)},
    "first_data_applied": {"since_diagnostics_start_ms": ("elapsed_us", 1000)},
    "queue_wait": DURATION,
    "core_response": DURATION,
    "response_receive": DURATION,
    "display_apply": DURATION,
    "input_handler_cpu": DURATION,
    "render_callback_cpu": {**DURATION, "cached_task_rows": ("count", 1)},
    "task_cache": {"cached_task_rows": ("count", 1)},
    "task_row_built": {"rows_built_per_event": ("count", 1)},
    "commit_applied": {"changed_entities_per_event": ("count", 1)},
    "minute_notify": {"notifications_per_event": ("count", 1)},
}
MEANINGS = {
    "first_frame_callback": "診断開始から初回on_next_frame callbackまで。プロセス起動時刻・画面提示時刻ではない",
    "first_data_applied": "診断開始からTask/agenda/all-day初回読込の状態反映まで。画面提示は未観測",
    "queue_wait": "request投入からDBワーカー処理開始まで",
    "core_response": "DBワーカーの処理区間。Open/fixture/設定保存も含む。操作別内訳はログにない。count=1はOk、0はErr",
    "response_receive": "ワーカー完了からUI応答受信まで",
    "display_apply": "read応答の受信・状態反映区間。layout/paint/画面提示を含まない",
    "input_handler_cpu": "計装された入力handler区間のみ。全入力やinput-to-photonではない",
    "render_callback_cpu": "GPUI Renderで要素を構築する同期区間。layout/paint/GPU/提示を含まない。countはTaskキャッシュ行数",
    "task_cache": "Taskキャッシュ行数。viewport内の表示行数ではない",
    "task_row_built": "Task行の構築イベント。再構築を含む延べ回数。実表示行数・フレーム別件数ではない",
    "commit_applied": "反映したcommitのchanged件数",
    "minute_notify": "分更新で記録されたnotify件数。全notify数ではない",
}
UNOBSERVED = {
    "presentation_time": None,
    "end_to_end_input_latency": None,
    "process_start_to_first_present": None,
    "visible_rows_per_frame": None,
    "layout_paint_gpu_time": None,
    "total_frame_requests": None,
    "all_notify_count": None,
}


def distribution(values: list[float], total: int) -> dict:
    ordered = sorted(values)
    return {
        "n": len(ordered), "unobserved_count": total - len(ordered),
        "median": statistics.median(ordered) if ordered else None,
        "p95": ordered[math.ceil(len(ordered) * 0.95) - 1] if ordered else None,
        "p99": ordered[math.ceil(len(ordered) * 0.99) - 1] if ordered else None,
        "min": ordered[0] if ordered else None,
        "max": ordered[-1] if ordered else None,
    }


def uint(value: object) -> bool:
    return isinstance(value, int) and not isinstance(value, bool) and 0 <= value <= 2**64 - 1


def summarize(source: Path, target: Path, index: int) -> dict:
    raw = source.read_bytes()
    snapshot = target / f"source-{index:04d}.jsonl"
    with snapshot.open("xb") as stream:
        stream.write(raw)
    issues = []
    try:
        text = raw.decode("utf-8-sig")
    except UnicodeError as error:
        issues.append(f"invalid UTF-8: {error}")
        text = raw.decode("utf-8-sig", errors="replace")
    by_kind = defaultdict(list)
    counts = Counter()
    metadata, footers = [], []
    last_line = 0
    for line_number, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        last_line = line_number
        try:
            row = json.loads(line)
            if not isinstance(row, dict) or not isinstance(row.get("kind"), str) or not row["kind"]:
                raise ValueError("expected an object with a kind")
        except (ValueError, TypeError) as error:
            issues.append(f"line {line_number}: {error}")
            continue
        kind = row["kind"]
        counts[kind] += 1
        if kind == "metadata":
            metadata.append(row)
        elif kind == "dropped_samples":
            footers.append((line_number, row.get("count")))
        elif kind in METRICS:
            for field in ("request_id", "elapsed_us", "duration_us", "count"):
                if not uint(row.get(field)):
                    issues.append(f"line {line_number}: {kind}.{field} not observed or invalid")
                    row[field] = None
            if kind == "core_response" and row["count"] not in (None, 0, 1):
                issues.append(f"line {line_number}: core_response.count must be 0 or 1")
                row["count"] = None
            by_kind[kind].append(row)
    if len(metadata) != 1:
        issues.append("expected exactly one metadata record")
    dropped = None
    if len(footers) == 1 and footers[0][0] == last_line and uint(footers[0][1]):
        dropped = footers[0][1]
    else:
        issues.append("final dropped_samples record missing, invalid or duplicated; loss count is unknown")
    groups = {}
    for kind, rows in sorted(by_kind.items()):
        metrics = {}
        for name, (field, divisor) in METRICS[kind].items():
            values = [row[field] / divisor for row in rows if row[field] is not None]
            metrics[name] = distribution(values, len(rows))
        group = {"sample_count": len(rows), "metrics": metrics}
        if kind == "core_response":
            group["result_counts"] = {
                "ok": sum(row["count"] == 1 for row in rows),
                "error": sum(row["count"] == 0 for row in rows),
                "unobserved": sum(row["count"] is None for row in rows),
            }
        if kind in {"task_row_built", "commit_applied", "minute_notify"}:
            values = [row["count"] for row in rows if row["count"] is not None]
            group["count_sum_observed"] = sum(values) if values else None
        groups[kind] = group
    return {
        "source": str(source), "raw_snapshot": snapshot.name,
        "sha256": hashlib.sha256(raw).hexdigest(), "bytes": len(raw),
        "line_count": len(text.splitlines()), "kind_counts": dict(sorted(counts.items())),
        "metadata": metadata, "groups": groups,
        "not_observed_kinds": sorted(METRICS.keys() - by_kind.keys()),
        "unrecognized_kinds": sorted(counts.keys() - METRICS.keys() - {"metadata", "dropped_samples"}),
        "dropped_samples": {"status": "OBSERVED" if dropped is not None else "NOT_OBSERVED", "count": dropped},
        "log_finalized_without_loss_or_issues": dropped == 0 and not issues,
        "issues": issues,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Desktop診断をファイル別に集計。元sampleを複製し、既存出力は上書きしない。")
    parser.add_argument("--input", type=Path, required=True, help="JSONLファイル、またはdesktop-*.jsonlを含む専用runディレクトリ")
    parser.add_argument("--out", type=Path, help="未作成の出力ディレクトリ")
    args = parser.parse_args()
    source = args.input.resolve(strict=True)
    files = sorted(source.rglob("desktop-*.jsonl")) if source.is_dir() else [source]
    if not files:
        parser.error("desktop-*.jsonlがありません")
    base = source if source.is_dir() else source.parent
    target = (args.out or base / ("desktop-summary-" + uuid.uuid4().hex)).resolve()
    target.mkdir(parents=False, exist_ok=False)
    runs = [summarize(path, target, index) for index, path in enumerate(files, 1)]
    complete = all(run["log_finalized_without_loss_or_issues"] for run in runs)
    result = {
        "schema_version": 1, "created_utc": datetime.now(timezone.utc).isoformat(),
        "log_finalized_without_loss_or_issues": complete,
        "statistics": {
            "median": "偶数件は中央2値の平均", "percentiles": "nearest-rank: sorted[ceil(n*p)-1]",
            "population": "保存された全sample。エラー・外れ値を除外せず、ファイル間は混ぜない",
            "duration": "Instantによる区間の経過時間。名前のcpuは区間名でありOSのCPU時間ではない",
            "missing": "nullと未観測件数を保持。missing kindやdrop欠落を0としない",
            "small_sample": "少数sampleのp99は最大値に近く、尾部の推定精度が低い",
            "startup": "起動イベントはelapsed_usを使用。duration_us=0を起動時間に流用しない",
            "exit": "0はログ終端・drop=0・構文/数値の整合のみ。性能目標や操作成功の判定ではない",
        },
        "meanings": MEANINGS, "not_observed_metrics": UNOBSERVED,
        "runs": runs,
    }
    with (target / "summary.json").open("x", encoding="utf-8") as stream:
        json.dump(result, stream, ensure_ascii=False, indent=2, allow_nan=False)
        stream.write("\n")
    print(f"DESKTOP_SUMMARY: {target}")
    print(f"LOG_FINALIZED_WITHOUT_LOSS_OR_ISSUES={complete}; files={len(runs)}")
    return 0 if complete else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError) as error:
        print(f"DESKTOP_SUMMARY_FAILED: {error}", file=sys.stderr)
        raise SystemExit(1)
