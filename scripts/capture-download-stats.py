#!/usr/bin/env python3
"""Save GitHub release counters and derive observation intervals by OS/version."""

import argparse
import csv
from datetime import date, datetime, timedelta, timezone
import html
import json
from pathlib import Path
import re
import subprocess
import xml.etree.ElementTree as ET


PACKAGE = re.compile(r"^something_bg-(macos|linux|windows)-.+\.(zip|tar\.gz|dmg|exe)$")
FIELDS = ["date", "interval_start", "interval_end", "version", "os", "downloads", "total", "status"]
OS_STYLES = [("macos", "macOS", "#0969da", ""),
             ("linux", "Linux", "#9a6700", "8 4"),
             ("windows", "Windows", "#8250df", "2 4")]


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


def daily_series(rows, first_date, last_date):
    """Sum releases per OS, retaining unknown days rather than partial totals."""
    end = date.fromisoformat(last_date)
    start = max(date.fromisoformat(first_date), end - timedelta(days=59))
    dates = [(start + timedelta(days=i)).isoformat() for i in range((end - start).days + 1)]
    groups = {}
    for row in rows:
        groups.setdefault((row["date"], row["os"]), []).append(row)
    series = {}
    for os_name, _, _, _ in OS_STYLES:
        values = []
        for day in dates:
            observations = groups.get((day, os_name), [])
            known = observations and all(row["status"] == "ok" and row["downloads"] != ""
                                         for row in observations)
            values.append(sum(int(row["downloads"]) for row in observations) if known else None)
        series[os_name] = values
    return dates, series


def render_chart(output, rows, first_date, last_date):
    """Create a self-contained SVG for GitHub's native Markdown image support."""
    dates, series = daily_series(rows, first_date, last_date)
    svg = ET.Element("svg", {"xmlns": "http://www.w3.org/2000/svg", "viewBox": "0 0 960 440",
                             "role": "img", "aria-labelledby": "title description"})

    def element(tag, text=None, **attrs):
        node = ET.SubElement(svg, tag, {key.replace("_", "-"): str(value) for key, value in attrs.items()})
        node.text = text
        return node

    element("title", "Daily downloads by OS", id="title")
    element("desc", "Downloads across all releases by UTC snapshot date. Unknown counts and "
            "multi-day intervals are gaps, not zero downloads.", id="description")
    element("rect", width=960, height=440, rx=12, fill="#ffffff")

    def label(text, x, y, size=12, **attrs):
        return element("text", text, x=x, y=y, fill="#24292f", font_family="sans-serif",
                       font_size=size, **attrs)

    label("Daily downloads by OS", 28, 34, 22, font_weight=600)
    label(f"{dates[0]} to {dates[-1]} · All releases · UTC snapshot dates", 28, 58)
    for index, (os_name, name, color, dash) in enumerate(OS_STYLES):
        x = 600 + index * 114
        element("line", x1=x, y1=30, x2=x + 26, y2=30, stroke=color,
                stroke_width=3, stroke_dasharray=dash or "none")
        label(name, x + 32, 34)

    known_values = [value for values in series.values() for value in values if value is not None]
    maximum = max(known_values, default=0)
    step = max(1, (maximum + 3) // 4)
    ceiling = step * 4
    left, right, top, bottom = 72, 912, 104, 344

    def point(index, value):
        x = (left + right) / 2 if len(dates) == 1 else left + index * (right - left) / (len(dates) - 1)
        y = bottom - value * (bottom - top) / ceiling
        return x, y

    label("Downloads", left, top - 14)
    for value in range(0, ceiling + 1, step):
        _, y = point(0, value)
        element("line", x1=left, y1=y, x2=right, y2=y, stroke="#d0d7de")
        label(str(value), left - 12, y + 4, text_anchor="end")
    ticks = sorted({round(i * (len(dates) - 1) / 5) for i in range(6)})
    for index in ticks:
        x, _ = point(index, 0)
        label(dates[index], x, bottom + 24, text_anchor="middle")

    for os_name, name, color, dash in OS_STYLES:
        commands = []
        connected = False
        for index, value in enumerate(series[os_name]):
            if value is None:
                connected = False
                continue
            x, y = point(index, value)
            commands.append(f"{'L' if connected else 'M'} {x:.2f} {y:.2f}")
            connected = True
        element("path", d=" ".join(commands), fill="none", stroke=color,
                stroke_width=2.5, stroke_dasharray=dash or "none", data_os=os_name)
        for index, value in enumerate(series[os_name]):
            if value is None:
                continue
            x, y = point(index, value)
            marker = element("circle", cx=f"{x:.2f}", cy=f"{y:.2f}", r=3.5,
                             fill="#ffffff", stroke=color, stroke_width=2)
            ET.SubElement(marker, "title").text = f"{dates[index]} · {name}: {value} downloads"

    if not known_values:
        label("Waiting for consecutive daily snapshots with known counts", 492, 222, 16,
              text_anchor="middle")
    label("Gaps = unknown counts or missed days. Points show downloads since the previous daily snapshot.",
          28, 406)
    ET.ElementTree(svg).write(output / "daily-downloads.svg", encoding="utf-8", xml_declaration=True)


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

    render_chart(output, rows, snapshots[0]["captured_at"][:10], snapshots[-1]["captured_at"][:10])
    report = [
        "# Download statistics", "",
        "![Daily downloads by OS over the latest 60 days](daily-downloads.svg)", "",
        "The chart sums all releases per OS over the latest 60 calendar days. "
        "Unknown counts and multi-day intervals appear as gaps, not zeros.", "",
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
