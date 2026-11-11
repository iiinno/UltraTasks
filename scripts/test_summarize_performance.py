"""Synthetic-only regression checks; run with python -I -B scripts/test_summarize_performance.py."""
import contextlib
import csv
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).with_name("summarize-performance.py")
SPEC = importlib.util.spec_from_file_location("performance_summary", SCRIPT)
SUMMARY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SUMMARY)


class SummaryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="ultratasks-summary-test-")
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).absolute()
        self.root = self.base / "artifact"
        self.root.mkdir()
        self.run = self.root / "run-1"
        self.run.mkdir()
        self.metadata = {
            "schema_version": 1, "kind": "core", "sizes": [1000], "warmup": 0,
            "samples": 2, "trials": 1, "operations": ["search_and_count"], "distribution": "spread",
        }

    def fixture(self, rows=None, completed=True):
        if rows is None:
            rows = [self.sample(index, (index + 1) * 1_000_000) for index in range(self.metadata["samples"])]
        (self.run / "run.json").write_text(json.dumps(self.metadata), encoding="utf-8")
        (self.run / "core-samples.jsonl").write_text("\n".join(json.dumps(row) for row in rows) + "\n", encoding="utf-8")
        (self.run / "completion.json").write_text(json.dumps({"completed": completed}), encoding="utf-8")

    def sample(self, index, duration, status="success"):
        return {
            "schema_version": 1, "size": 1000, "trial": 1, "phase": "sample",
            "operation": self.metadata["operations"][0], "sample": index, "status": status,
            "verified": status != "failure", "deadline_exceeded": status == "timeout",
            "processing_ns": duration, "verification_ns": 100, "queue_wait_ns": None, "presentation_ns": None,
        }

    def summarize(self, output="summary"):
        target = self.base / output
        with patch.object(sys, "argv", [str(SCRIPT), "--input", str(self.root), "--out", str(target)]), contextlib.redirect_stdout(io.StringIO()):
            code = SUMMARY.main()
        result = json.loads((target / "summary.json").read_text(encoding="utf-8"))
        return code, result, target

    @contextlib.contextmanager
    def forbid_metadata_io(self):
        with contextlib.ExitStack() as stack:
            for owner, names in ((Path, ("open", "stat", "lstat", "resolve")),
                                 (os, ("open", "stat", "lstat", "scandir"))):
                for name in names:
                    original = getattr(owner, name)
                    def guarded(path, *args, _original=original, **kwargs):
                        self.assertNotIn("ultratasks-no-io", str(path), "metadata path reached filesystem I/O")
                        return _original(path, *args, **kwargs)
                    stack.enter_context(patch.object(owner, name, guarded))
            yield

    def test_runner_paths_never_reach_io_and_claims_keep_provenance(self):
        paths = [r"\\ultratasks-no-io.invalid\share\secret", "C:/ultratasks-no-io/secret",
                 "/ultratasks-no-io/secret", "../../ultratasks-no-io/secret"]
        for index, path in enumerate(paths):
            with self.subTest(path=path):
                self.metadata.update(executable=path, lock_file=path, harness_source=path)
                identity = {"path": path, "bytes": 123, "sha256": "a" * 64}
                self.metadata["environment"] = {"product_binary": identity, "measure_binary": identity,
                                                "lock": identity, "sources": [identity]}
                self.metadata["identity"] = {"binary": identity}
                self.fixture()
                original = (self.run / "core-samples.jsonl").read_bytes()
                with self.forbid_metadata_io():
                    code, result, _ = self.summarize(f"summary-{index}")
                self.assertEqual(code, 0)
                run = result["runs"][0]
                self.assertNotIn("hashes_at_summary", run)
                self.assertEqual(run["metadata"]["executable"], path)
                claims = run["reported_runtime_hashes"]
                self.assertEqual(claims["provenance"], "unverified_runtime_claims_from_run.json")
                self.assertFalse(claims["runner_paths_opened"])
                self.assertEqual(len(claims["records"]), 5)
                for claim in claims["records"]:
                    self.assertEqual(claim["reported_path"], path)
                    self.assertEqual(claim["sha256"], "a" * 64)
                    self.assertTrue(claim["source"].startswith("run.json#/"))
                self.assertEqual(run["sample_sha256"]["sha256"], hashlib.sha256(original).hexdigest())
                self.assertEqual(run["sample_sha256"]["provenance"], "local_input_artifact")
                self.assertTrue(run["manifest_sha256"]["captured_at_summary"])
                self.assertEqual((self.run / "core-samples.jsonl").read_bytes(), original)

    def test_missing_runtime_hash_is_not_filled_from_runner_path(self):
        self.metadata.update(executable=r"\\ultratasks-no-io.invalid\share\secret",
                             lock_file="../ultratasks-no-io", harness_source="C:/ultratasks-no-io")
        self.fixture()
        with self.forbid_metadata_io():
            code, result, _ = self.summarize()
        self.assertEqual(code, 0)
        self.assertEqual(result["runs"][0]["reported_runtime_hashes"]["records"], [])

    def test_percentiles_and_success_population_unchanged(self):
        self.metadata["samples"] = 100
        self.fixture()
        code, result, _ = self.summarize()
        self.assertEqual(code, 0)
        group = next(group for group in result["groups"] if group["scope"] == "pooled")
        stats = group["metrics"]["processing_ms"]
        self.assertEqual(stats["all_attempts"], {"n": 100, "median": 50.5, "p95": 95, "p99": 99, "min": 1, "max": 100})
        self.assertEqual(stats["success_only"], stats["all_attempts"])

    def test_failure_timeout_missing_and_null_are_retained(self):
        self.metadata["samples"] = 5
        self.fixture([self.sample(0, 1_000_000), self.sample(1, 2_000_000, "failure"),
                      self.sample(2, 10_000_000, "timeout"), self.sample(3, None)], completed=False)
        code, result, _ = self.summarize()
        self.assertEqual(code, 1)
        group = next(group for group in result["groups"] if group["scope"] == "pooled")
        self.assertEqual((group["sample_count"], group["failure_count"], group["timeout_count"], group["not_run_count"]), (4, 1, 1, 1))
        processing = group["metrics"]["processing_ms"]
        self.assertEqual(processing["all_attempts"]["median"], 2)
        self.assertEqual(processing["unobserved_count"], 1)
        self.assertEqual(processing["success_only"]["n"], 1)
        self.assertIsNone(group["metrics"]["queue_wait_ms"]["all_attempts"]["median"])
        self.assertEqual(group["metrics"]["presentation_ms"]["unobserved_count"], 4)

    @contextlib.contextmanager
    def fake_link(self, blocked, directory=False, reparse=True):
        original_lstat, original_open, original_iterdir = Path.lstat, Path.open, Path.iterdir
        def lstat(path, *args, **kwargs):
            if path == blocked:
                return SimpleNamespace(st_mode=(stat.S_IFDIR if directory else stat.S_IFREG) if reparse else stat.S_IFLNK,
                                       st_file_attributes=0x400 if reparse else 0)
            self.assertFalse(path != blocked and path.is_relative_to(blocked), "traversed linked parent")
            return original_lstat(path, *args, **kwargs)
        def checked(original):
            def call(path, *args, **kwargs):
                self.assertFalse(path == blocked or path.is_relative_to(blocked), "opened/scanned a link")
                return original(path, *args, **kwargs)
            return call
        with patch.object(Path, "lstat", lstat), patch.object(Path, "open", checked(original_open)), patch.object(Path, "iterdir", checked(original_iterdir)):
            yield

    def test_file_links_and_windows_reparse_points_are_not_opened(self):
        self.fixture()
        for index, name in enumerate(("run.json", "core-samples.jsonl", "completion.json")):
            for reparse in (True, False):
                with self.subTest(name=name, reparse=reparse), self.fake_link(self.run / name, reparse=reparse):
                    code, result, _ = self.summarize(f"blocked-{index}-{reparse}")
                    self.assertEqual(code, 1)
                    self.assertTrue(result["issues"])

    def test_directory_reparse_is_not_traversed(self):
        self.fixture()
        linked = self.root / "linked"
        linked.mkdir()
        with self.fake_link(linked, directory=True):
            code, result, _ = self.summarize()
        self.assertEqual(code, 1)
        self.assertTrue(result["issues"])
        self.assertEqual(len(result["runs"]), 1)

    def test_root_or_parent_link_is_rejected_before_read(self):
        self.fixture()
        for blocked in (self.root, self.run):
            with self.subTest(blocked=blocked), self.fake_link(blocked, directory=True):
                with self.assertRaises(ValueError):
                    SUMMARY.summarize_run(self.run / "run.json", self.root)

    def test_csv_formula_text_is_escaped_without_changing_json_or_numbers(self):
        values = ["=SUM(1,2)", "+1", "-1", "@SUM(1)", " \t=1", "\n=1", "\r+1", "\tlabel", "\ufeff=1"]
        for index, value in enumerate(values):
            with self.subTest(value=value):
                self.metadata["operations"] = [value]
                self.metadata["distribution"] = value
                self.fixture()
                code, result, target = self.summarize(f"csv-{index}")
                self.assertEqual(code, 0)
                self.assertEqual(result["groups"][0]["operation"], value)
                with (target / "summary.csv").open(encoding="utf-8", newline="") as stream:
                    rows = list(csv.DictReader(stream))
                self.assertTrue(all(row["operation"] == "'" + value and row["distribution"] == "'" + value for row in rows))
                self.assertEqual(float(rows[0]["median"]), 1.5)
                self.assertEqual(rows[0]["sample_count"], "2")
        self.assertEqual(SUMMARY.csv_cell("search_and_count"), "search_and_count")
        self.assertEqual(SUMMARY.csv_cell(-1), -1)


if __name__ == "__main__":
    unittest.main(verbosity=2)
