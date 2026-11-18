#!/usr/bin/env python3
"""Renders the benchmark invoice of tools/gen.py at 0, 3, 150 and 1,700 lines (1, 1, 10 and about
100 pages) with `fagus render --repeat` and fails if a median exceeds its budget times FACTOR.

Usage: tools/bench-smoke.py [OUT_DIR]

The budgets are those Fagus is built to (bench/README.md measures it): 2.5 ms for the invoice
without lines, 5 / 35 / 300 ms for 1 / 10 / 100 pages, and 10 ms for one page with the start of a
new process. FACTOR (default 3) leaves room for slow and noisy machines such as CI runners, so the
check only catches gross regressions; a median over its limit is measured again before it fails.
FAGUS names the binary (default target/release/fagus). The results are printed and, on GitHub
Actions, added to the job summary.
"""

import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FAGUS = os.environ.get("FAGUS", str(ROOT / "target" / "release" / "fagus"))
FACTOR = float(os.environ.get("FACTOR", "3"))

# Name, invoice lines, renders in one engine, budget in milliseconds.
CASES = [
    ("Invoice without lines (fixed cost)", 0, 100, 2.5),
    ("Invoice, 1 page", 3, 100, 5),
    ("Invoice, 10 pages", 150, 20, 35),
    ("Invoice, about 100 pages", 1700, 3, 300),
]
PROCESS_RUNS = 30
PROCESS_BUDGET = 10
ATTEMPTS = 3


def render(document: Path, repeat: int) -> tuple[int, float]:
    """Renders a document `repeat` times in one engine; returns the pages and the median."""
    result = subprocess.run(
        [FAGUS, "render", str(document), "-o", str(document.with_suffix(".pdf")), "--timings", "--repeat", str(repeat)],
        capture_output=True,
        text=True,
        check=True,
    )
    match = re.search(r"^(\d+) pages?, median ([\d.]+) ms", result.stderr, re.MULTILINE)
    if not match:
        sys.exit(f"no timings in the output of fagus:\n{result.stderr}")
    return int(match.group(1)), float(match.group(2))


def main() -> int:
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(tempfile.mkdtemp(prefix="fagus-bench-"))
    (out / "fonts").mkdir(parents=True, exist_ok=True)
    for font in ("Inter-Regular.ttf", "Inter-Bold.ttf"):
        shutil.copy(ROOT / "tests" / "fonts" / font, out / "fonts" / font)
    documents = {}
    for _, lines, _, _ in CASES:
        document = out / f"invoice-{lines}.html"
        html = subprocess.run(
            [sys.executable, str(ROOT / "tools" / "gen.py"), str(lines), "fonts"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
        document.write_text(html, encoding="utf-8")
        documents[lines] = document

    rows = []
    for name, lines, repeat, budget in CASES:
        # One render first, so that the file system cache holds the binary and the fonts.
        render(documents[lines], 1)
        pages, median = render(documents[lines], repeat)
        # A median over the limit is measured again, up to twice, so that a moment of load on a
        # shared machine does not fail the check; the best median counts.
        for _ in range(ATTEMPTS - 1):
            if median <= budget * FACTOR:
                break
            median = min(median, render(documents[lines], repeat)[1])
        rows.append((name, pages, median, budget))

    # A new process per PDF, as PHP-FPM starts one: the time from the start of the process to its
    # end, with the 1-page invoice.
    command = [FAGUS, "render", str(documents[3]), "-o", str(out / "process.pdf")]
    subprocess.run(command, check=True)
    median = float("inf")
    for _ in range(ATTEMPTS):
        durations = []
        for _ in range(PROCESS_RUNS):
            start = time.perf_counter()
            subprocess.run(command, check=True)
            durations.append((time.perf_counter() - start) * 1000)
        median = min(median, statistics.median(durations))
        if median <= PROCESS_BUDGET * FACTOR:
            break
    rows.append(("Invoice, 1 page, new process", 1, median, PROCESS_BUDGET))

    failed = False
    table = [
        f"| Document | Pages | Median (ms) | Budget (ms) | Limit ×{FACTOR:g} (ms) | |",
        "|---|--:|--:|--:|--:|---|",
    ]
    for name, pages, median, budget in rows:
        limit = budget * FACTOR
        ok = median <= limit
        failed |= not ok
        table.append(f"| {name} | {pages} | {median:.2f} | {budget:g} | {limit:g} | {'ok' if ok else 'too slow'} |")
    print("\n".join(table), flush=True)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as file:
            file.write("### Benchmark smoke test\n\n" + "\n".join(table) + "\n")
    if failed:
        print(f"A median exceeds {FACTOR:g} times its budget.", file=sys.stderr)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
