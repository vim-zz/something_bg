#!/usr/bin/env python3
"""Save GitHub release counters and derive observation intervals by OS/version."""

import argparse
import csv
from datetime import datetime, timezone
import html
import json
from pathlib import Path
import re
import subprocess


PACKAGE = re.compile(r"^something_bg-(macos|linux|windows)-.+\.(zip|tar\.gz|dmg|exe)$")
FIELDS = ["date", "interval_start", "interval_end", "version", "os", "downloads", "total", "status"]


def timestamp(value):
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


def api_list(endpoint):
    result = subprocess.run(
        ["gh", "api", "--paginate", "--slurp", endpoint],
        check=True, capture_output=True, text=True,
    )
    return [item for page in json.loads(result.stdout) for item in page]


def collect(repository, captured_at):
    assets = []
    for release in api_list(f"repos/{repository}/releases?per_page=100"):
        if release["draft"]:
            continue
        # Fetch assets separately so large releases are also fully paginated.
        for asset in api_list(f"repos/{repository}/releases/{release['id']}/assets?per_page=100"):
            match = PACKAGE.fullmatch(asset["name"])
            if not match or asset["name"] == "something_bg-macos-unsigned.zip":
                continue
            assets.append({
                "id": asset["id"],
                "name": asset["name"],
                "version": release["tag_name"],
                "os": match[1],
                "created_at": asset["created_at"],
                "download_count": asset["download_count"],
            })
    return {
        "schema_version": 1,
        "repository": repository,
        "captured_at": captured_at,
        "assets": sorted(assets, key=lambda asset: asset["id"]),
    }


def interval_rows(previous, current):
    before = {asset["id"]: asset for asset in previous["assets"]} if previous else {}
    after = {asset["id"]: asset for asset in current["assets"]}
    groups = {}
    for asset in list(before.values()) + list(after.values()):
        groups.setdefault((asset["version"], asset["os"]), set()).add(asset["id"])
    end = current["captured_at"]
    start = previous["captured_at"] if previous else ""
    gap = previous and (timestamp(end).date() - timestamp(start).date()).days > 1
    rows = []
    for (version, os_name), ids in sorted(groups.items()):
        delta = 0
        problems = set()
        for asset_id in ids:
            old, new = before.get(asset_id), after.get(asset_id)
            if not previous:
                problems.add("baseline")
            elif new is None:
                problems.add("asset_missing")
            elif old is None:
                # Older assets discovered later have no known starting count.
                if timestamp(new["created_at"]) >= timestamp(start):
                    delta += new["download_count"]
                else:
                    problems.add("asset_baseline")
            elif (old["version"], old["os"]) != (new["version"], new["os"]):
                problems.add("asset_changed")
            elif new["download_count"] < old["download_count"]:
                problems.add("counter_reset")
            else:
                delta += new["download_count"] - old["download_count"]
        rows.append({
            "date": end[:10],
            "interval_start": start,
            "interval_end": end,
            "version": version,
            "os": os_name,
            "downloads": "" if problems else delta,
            "total": sum(after[asset_id]["download_count"] for asset_id in ids if asset_id in after),
            "status": ", ".join(sorted(problems)) if problems else ("multi_day" if gap else "ok"),
        })
    return rows


def render_reports(output):
    snapshots = [json.loads(path.read_text()) for path in sorted((output / "snapshots").glob("*.json"))]
    rows = []
    previous = None
    for snapshot in snapshots:
        if previous and snapshot["repository"] != previous["repository"]:
            raise ValueError("Cannot combine snapshots from different repositories")
        rows.extend(interval_rows(previous, snapshot))
        previous = snapshot
    with (output / "daily.csv").open("w", newline="", encoding="utf-8") as stream:
        writer = csv.DictWriter(stream, fieldnames=FIELDS)
        writer.writeheader()
        writer.writerows(rows)

    report = [
        "# Download statistics", "",
        f"Latest snapshot: {snapshots[-1]['captured_at']} (UTC).", "",
        "Counts cover downloads between snapshots, not unique users or exact calendar days. "
        "The first snapshot is a baseline; no earlier download history is available.", "",
        "Blank downloads mean an unknown interval (baseline, missing/replaced asset, or counter reset). "
        "A multi_day row covers a gap; its downloads cannot be assigned to individual days. "
        "Totals are current counters for assets still available, including downloads before tracking began.", "",
        "All observations and exact UTC interval boundaries are in [daily.csv](daily.csv); "
        "raw asset counters are in [snapshots/](snapshots/). "
        "This table shows the latest 60 snapshot dates, newest first.", "",
        "| Snapshot date (UTC) | Version | OS | Downloads since previous snapshot | Total | Status |",
        "| --- | --- | --- | ---: | ---: | --- |",
    ]
    dates = sorted({row["date"] for row in rows})[-60:]
    for row in sorted(rows, key=lambda row: row["date"], reverse=True):
        if row["date"] not in dates:
            continue
        values = [row[key] for key in ["date", "version", "os", "downloads", "total", "status"]]
        safe = [html.escape(str(value)).replace("|", "&#124;").replace("\n", " ").replace("\r", " ") for value in values]
        report.append("| " + " | ".join(safe) + " |")
    (output / "REPORT.md").write_text("\n".join(report) + "\n", encoding="utf-8")


def capture(repository, output, now=None):
    now = now or datetime.now(timezone.utc)
    captured_at = now.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    directory = output / "snapshots"
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / f"{captured_at[:10]}.json"
    # Keep the first successful observation each UTC day, including manual reruns.
    if not path.exists():
        snapshot = collect(repository, captured_at)
        path.write_text(json.dumps(snapshot, indent=2) + "\n", encoding="utf-8")
    elif json.loads(path.read_text())["repository"] != repository:
        raise ValueError("Snapshot belongs to another repository")
    render_reports(output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True, help="GitHub OWNER/REPO")
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repository):
        parser.error("repository must be OWNER/REPO")
    capture(args.repository, args.output)


if __name__ == "__main__":
    main()
