#!/usr/bin/env python3
"""The memory a `fagus worker` keeps after large documents, as the worker reports it.

Usage: tools/worker-memory.py [OUT_DIR]

Renders the benchmark invoice of tools/gen.py with about 100 pages (1,700 lines) through the worker
protocol (docs/protocol.md), then the 1-page invoice, then 20 more of the 100-page one and the
1-page one again, and prints the resident size the worker reports with each answer: `rss` after
the document (Linux) and `maxRss`, the peak (Linux and macOS). The size after the 1-page invoice
that follows the large ones is what a worker keeps; a new worker after the 1-page invoice is the
baseline. It measures and does not fail. FAGUS names the binary (default target/release/fagus);
on GitHub Actions the table goes into the job summary.
"""

import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FAGUS = os.environ.get("FAGUS", str(ROOT / "target" / "release" / "fagus"))
MB = 1 << 20


def frame(kind: str, header: dict, blobs: list[bytes]) -> bytes:
    if blobs:
        header = dict(header, blobs=[len(blob) for blob in blobs])
    head = json.dumps(header).encode()
    body = kind.encode() + struct.pack("<I", len(head)) + head + b"".join(blobs)
    return struct.pack("<I", len(body)) + body


def read_frame(stream) -> tuple[str, dict]:
    body = stream.read(struct.unpack("<I", stream.read(4))[0])
    head_length = struct.unpack("<I", body[1:5])[0]
    return chr(body[0]), json.loads(body[5 : 5 + head_length])


class Worker:
    def __init__(self) -> None:
        self.process = subprocess.Popen([FAGUS, "worker"], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        kind, _ = read_frame(self.process.stdout)
        assert kind == "H", kind
        self.id = 0

    def render(self, document: Path) -> dict:
        self.id += 1
        request = {"id": self.id, "html": 0, "base": str(document.parent)}
        self.process.stdin.write(frame("R", request, [document.read_bytes()]))
        self.process.stdin.flush()
        kind, header = read_frame(self.process.stdout)
        if kind != "P":
            sys.exit(f"the worker answered {kind}: {header}")
        return header

    def close(self) -> None:
        self.process.stdin.close()
        self.process.wait()


def size(header: dict, field: str) -> str:
    return f"{header[field] / MB:.0f}" if field in header else "–"


def main() -> int:
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(tempfile.mkdtemp(prefix="fagus-memory-"))
    (out / "fonts").mkdir(parents=True, exist_ok=True)
    for font in ("Inter-Regular.ttf", "Inter-Bold.ttf"):
        shutil.copy(ROOT / "tests" / "fonts" / font, out / "fonts" / font)
    documents = {}
    for lines in (3, 1700):
        html = subprocess.run([sys.executable, str(ROOT / "tools" / "gen.py"), str(lines), "fonts"],
                              capture_output=True, text=True, check=True).stdout
        documents[lines] = out / f"invoice-{lines}.html"
        documents[lines].write_text(html, encoding="utf-8")

    rows = []
    fresh = Worker()
    rows.append(("New worker, 1-page invoice", fresh.render(documents[3])))
    fresh.close()
    worker = Worker()
    rows.append(("100-page invoice", worker.render(documents[1700])))
    rows.append(("then the 1-page invoice", worker.render(documents[3])))
    for _ in range(20):
        last = worker.render(documents[1700])
    rows.append(("then 20 × the 100-page invoice", last))
    rows.append(("then the 1-page invoice", worker.render(documents[3])))
    worker.close()

    table = ["| Worker | `rss` (MB) | `maxRss` (MB) |", "|---|--:|--:|"]
    table += [f"| {name} | {size(header, 'rss')} | {size(header, 'maxRss')} |" for name, header in rows]
    print("\n".join(table), flush=True)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as file:
            file.write("### Memory a worker keeps\n\n" + "\n".join(table) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
