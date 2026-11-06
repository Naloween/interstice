#!/usr/bin/env python3
"""Render Criterion's saved results for `interstice-bench --bench generic`
as a Markdown table: one row per benchmark, one column per target.

    python3 scripts/bench_table.py [target/criterion] [--baseline new]

Times are Criterion's median estimate.
"""
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 and not sys.argv[1].startswith("--") else "target/criterion")
baseline = sys.argv[sys.argv.index("--baseline") + 1] if "--baseline" in sys.argv else "new"

COLUMNS = ["raw/mem", "module/mem", "module/disk", "sqlite/mem", "sqlite/disk"]
results = {}
for bench in root.rglob(f"{baseline}/benchmark.json"):
    full_id = json.loads(bench.read_text())["full_id"]
    estimates = json.loads((bench.parent / "estimates.json").read_text())
    parts = full_id.split("/")
    if len(parts) < 3:
        continue
    column, row = "/".join(parts[:2]), "/".join(parts[2:])
    results.setdefault(row, {})[column] = estimates["median"]["point_estimate"]


def fmt(ns):
    if ns is None:
        return "–"
    if ns >= 1e6:
        return f"{ns / 1e6:.2f} ms"
    if ns >= 1e3:
        return f"{ns / 1e3:.1f} µs"
    return f"{ns:.0f} ns"


order = {"empty": 0, "insert_bulk": 1, "update_bulk": 2, "iterate": 3, "filter": 4}
print("| benchmark | " + " | ".join(COLUMNS) + " |")
print("|---|" + "---:|" * len(COLUMNS))
for row in sorted(results, key=lambda r: (order.get(r.split("/")[0], 9), r)):
    print(f"| `{row}` | " + " | ".join(fmt(results[row].get(c)) for c in COLUMNS) + " |")
