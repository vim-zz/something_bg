import copy
from datetime import datetime, timezone
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location(
    "download_stats", Path(__file__).with_name("capture-download-stats.py")
)
stats = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stats)


def asset(asset_id=1, count=10, os_name="macos", version="v1.0.0", created="2026-09-01T00:00:00Z"):
    return {
        "id": asset_id, "download_count": count, "os": os_name,
        "version": version, "created_at": created,
        "name": f"something_bg-{os_name}-arm64.zip",
    }


def snapshot(day, assets):
    return {
        "schema_version": 1, "repository": "owner/repo",
        "captured_at": f"2026-09-{day:02d}T00:17:00Z", "assets": assets,
    }


class DownloadStatsTests(unittest.TestCase):
    def test_first_observation_is_baseline_not_daily_downloads(self):
        row = stats.interval_rows(None, snapshot(17, [asset(count=100)]))[0]
        self.assertEqual(row["downloads"], "")
        self.assertEqual(row["total"], 100)
        self.assertEqual(row["status"], "baseline")

    def test_deltas_combine_architectures_but_separate_versions_and_os(self):
        before = [asset(1, 10), asset(2, 5), asset(3, 3, "linux"), asset(4, 1, version="v2.0.0")]
        after = copy.deepcopy(before)
        for item, increment in zip(after, [3, 2, 0, 4]):
            item["download_count"] += increment
        rows = stats.interval_rows(snapshot(17, before), snapshot(18, after))
        self.assertEqual(
            {(r["version"], r["os"]): r["downloads"] for r in rows},
            {("v1.0.0", "macos"): 5, ("v1.0.0", "linux"): 0, ("v2.0.0", "macos"): 4},
        )
        self.assertTrue(all(row["status"] == "ok" for row in rows))

    def test_new_asset_counts_only_if_created_during_interval(self):
        rows = stats.interval_rows(snapshot(17, []), snapshot(18, [
            asset(1, 5, created="2026-09-17T12:00:00Z"),
            asset(2, 40, "linux"),
        ]))
        by_os = {row["os"]: row for row in rows}
        self.assertEqual(by_os["macos"]["downloads"], 5)
        self.assertEqual(by_os["linux"]["downloads"], "")
        self.assertEqual(by_os["linux"]["status"], "asset_baseline")

    def test_replaced_deleted_reset_and_renamed_assets_are_unknown(self):
        before = snapshot(17, [asset()])
        cases = [
            ([asset(2, 3, created="2026-09-17T12:00:00Z")], "asset_missing"),
            ([], "asset_missing"),
            ([asset(count=2)], "counter_reset"),
            ([asset(version="v2.0.0")], "asset_changed"),
        ]
        for assets, status in cases:
            with self.subTest(status=status):
                rows = stats.interval_rows(before, snapshot(18, assets))
                self.assertTrue(all(row["downloads"] == "" for row in rows))
                self.assertTrue(all(row["status"] == status for row in rows))

    def test_gap_is_one_interval_not_fabricated_daily_data(self):
        row = stats.interval_rows(snapshot(17, [asset()]), snapshot(20, [asset(count=16)]))[0]
        self.assertEqual(row["downloads"], 6)
        self.assertEqual(row["status"], "multi_day")
        self.assertEqual(row["interval_start"], "2026-09-17T00:17:00Z")
        self.assertEqual(row["interval_end"], "2026-09-20T00:17:00Z")

    def test_pagination_and_api_failure_propagation(self):
        with patch.object(stats.subprocess, "run") as run:
            run.return_value.stdout = '[[{"id": 1}], [{"id": 2}]]'
            self.assertEqual(stats.api_list("endpoint"), [{"id": 1}, {"id": 2}])
            self.assertIn("--paginate", run.call_args.args[0])
            self.assertIn("--slurp", run.call_args.args[0])
            run.side_effect = subprocess.CalledProcessError(1, "gh")
            with self.assertRaises(subprocess.CalledProcessError):
                stats.api_list("endpoint")

    def test_collect_filters_auxiliary_assets_and_drafts_keeps_prereleases(self):
        releases = [
            {"id": 1, "draft": True, "tag_name": "v3.0.0"},
            {"id": 2, "draft": False, "prerelease": True, "tag_name": "v2.0.0-beta"},
        ]
        names = ["appcast.xml", "something_bg-macos-arm64.zip.sha256",
                 "something_bg-macos-unsigned.zip", "something_bg-macos-universal.zip",
                 "something_bg-linux-x86_64-unknown-linux-gnu.tar.gz",
                 "something_bg-windows-x86_64-pc-windows-msvc.zip"]
        assets = [{**asset(i), "name": name} for i, name in enumerate(names)]
        with patch.object(stats, "api_list", side_effect=[releases, assets]) as api:
            result = stats.collect("owner/repo", "2026-09-17T00:17:00Z")
        self.assertEqual(api.call_count, 2)
        self.assertIn("/releases/2/assets", api.call_args.args[0])
        self.assertEqual([a["os"] for a in result["assets"]], ["macos", "linux", "windows"])
        self.assertTrue(all(a["version"] == "v2.0.0-beta" for a in result["assets"]))

    def test_capture_is_idempotent_and_reports_survive_reruns(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            with patch.object(stats, "collect", return_value=snapshot(17, [asset()])) as collect:
                stats.capture("owner/repo", output, datetime(2026, 9, 17, tzinfo=timezone.utc))
                first = (output / "snapshots/2026-09-17.json").read_bytes()
                stats.capture("owner/repo", output, datetime(2026, 9, 17, 12, tzinfo=timezone.utc))
                self.assertEqual(collect.call_count, 1)
                self.assertEqual(first, (output / "snapshots/2026-09-17.json").read_bytes())
            with patch.object(stats, "collect", return_value=snapshot(18, [asset(count=13)])):
                stats.capture("owner/repo", output, datetime(2026, 9, 18, tzinfo=timezone.utc))
            with (output / "daily.csv").open(newline="") as stream:
                rows = list(stats.csv.DictReader(stream))
            self.assertEqual([row["downloads"] for row in rows], ["", "3"])
            report = (output / "REPORT.md").read_text()
            self.assertIn("| v1.0.0 | macos | 3 | 13 | ok |", report)
            self.assertIn("baseline", report)

    def test_failed_collection_does_not_write_partial_snapshot(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            with patch.object(stats, "collect", side_effect=subprocess.CalledProcessError(1, "gh")):
                with self.assertRaises(subprocess.CalledProcessError):
                    stats.capture("owner/repo", output)
            self.assertEqual(list((output / "snapshots").iterdir()), [])


if __name__ == "__main__":
    unittest.main()
