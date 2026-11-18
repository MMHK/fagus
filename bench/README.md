# Benchmarks

Measurements that Fagus is held against. Memory and processor time are in [Memory and CPU time](#memory-and-cpu-time-memory), the budgets of the plan in [Budgets](#budgets-memorybudgetspy); the [results of 2026-11-01](#results-of-2026-11-01) are at the end of this file; the comparison with the renderers PHP projects use today is in the [README](../README.md#compared-with-other-renderers).

## Against Dompdf, mPDF, Chrome and WeasyPrint (`compare/`)

The same invoices through Fagus (from PHP, through the worker), Dompdf, mPDF, Browsershot (a new Chrome per PDF), Chrome kept running (Puppeteer, one tab per PDF, as Gotenberg does) and WeasyPrint. `gen-classic.py` writes an invoice with CSS 2.1 only, so that every engine lays it out, with 3, 150 and 600 lines (1, 10 and 40 pages in Chrome); `tools/gen.py` writes the same invoice with flex, grid and margin boxes. Each engine renders each invoice in three new processes, once and then repeatedly; the medians are reported. The documents in `probes/` each test one feature (flex, grid, margin boxes, running elements, named strings, `target-counter()`, repeated table headers, footnotes, the outline, hyphenation), judged from the text of the PDF and its position.

```sh
cargo build --release -p fagus-cli && composer install   # in the repository
cd bench/compare
composer install
PUPPETEER_SKIP_DOWNLOAD=1 npm install                    # uses the Chrome that is installed (CHROME=… to choose)
python3 -m venv .venv && .venv/bin/pip install -r requirements.txt   # WeasyPrint needs Pango (brew install pango)
PYTHON=.venv/bin/python3 ./run.sh
```

The PDFs land in `out/`, for comparing them by eye. Results of 2026-11-14 on an Apple M2 Pro, macOS 27, PHP 8.5, Chrome 157 (milliseconds; *first* is the first PDF of a new process, the other columns further PDFs in the same process):

| Engine | 1 page, first | 1 page | 10 pages | 40 pages | modern CSS, 1 page | modern CSS, 10 pages |
|---|--:|--:|--:|--:|--:|--:|
| Fagus | 8.6 | 2.3 | 17.0 | 62.5 | 2.5 | 17.5 |
| Chrome, kept running | 103.4 | 74.6 | 121.5 | 217.3 | 81.4 | 122.7 |
| mPDF 8.3.1 | 63.1 | 24.9 | 232.8 (8 pages) | 897.8 (31 pages) | | |
| Dompdf 3.1.6 | 70.2 | 47.6 | 348.7 (12 pages) | 2,550.0 (44 pages) | | |
| WeasyPrint 66.0 | 158.2 | 105.0 | 667.5 | 2,596.4 | 134.1 | 714.7 |
| Browsershot 5.5.0 | 611.2 | 600.9 | 649.8 | 750.9 | | |

mPDF reads `size: A4` in `@page` wrongly (it then emits hundreds of empty pages), which is why the classic invoice gives the size in millimetres. mPDF does not load `@font-face` and sets the text in DejaVu Sans Condensed.

## Memory and CPU time (`memory/`)

The engines and invoices of `compare/`, with the memory and processor time of everything the renderer runs: PHP or Node and the processes they start (the Fagus worker, Chrome and its helpers). `driver.py` samples the process tree about every millisecond through `proc_pid_rusage` (macOS only) and sums the resident size (RSS) and the physical footprint of its processes. The footprint is what Activity Monitor shows as memory and what the system charges a process; RSS also counts the pages of the binary and of shared libraries and memory the allocator has handed back but the system has not reclaimed yet. Each engine renders each invoice in three new processes: one PDF, two seconds of rest, 20 more PDFs, three seconds of rest; the medians are reported. CPU time includes the time of helpers that have already ended.

```sh
cargo build --release -p fagus-cli && composer install   # in the repository
(cd bench/compare && composer install && PUPPETEER_SKIP_DOWNLOAD=1 npm install)
cd bench/memory
python3 -m venv .venv && .venv/bin/pip install -r requirements.txt
./run.sh                 # all engines, then four at once; the tables go to out/summary.md
.venv/bin/python3 budgets.py   # Fagus alone: the budgets below
```

Results of 2026-11-18 on an Apple M2 Pro (10 cores, 32 GB), macOS 27, PHP 8.5, Node 26, Chrome 157, Python 3.13 with WeasyPrint 66.0, mPDF 8.3.1, Dompdf 3.1.6, Browsershot 5.5.0. Peak footprint of the whole process tree over all 21 PDFs (peak RSS in parentheses):

| Engine | 1 page | 10 pages | 40 pages | modern CSS, 1 page | modern CSS, 10 pages |
|---|--:|--:|--:|--:|--:|
| Fagus (PHP and the worker) | 22 MB (48) | 33 MB (58) | 60 MB (90) | 22 MB (48) | 33 MB (58) |
| Chrome, kept running (Node and Chrome) | 253 MB (826) | 305 MB (861) | 402 MB (956) | 259 MB (813) | 313 MB (868) |
| Browsershot (PHP, Node and a Chrome per PDF) | 254 MB (807) | 286 MB (846) | 377 MB (936) | | |
| mPDF | 74 MB (90) | 88 MB (104) | 79 MB (95) | | |
| Dompdf | 36 MB (52) | 63 MB (79) | 301 MB (317) | | |
| WeasyPrint | 74 MB (106) | 127 MB (174) | 191 MB (228) | 68 MB (99) | 127 MB (174) |

Processor time per further PDF, all processes together, and of the first PDF of a new process including the start of PHP, Node or Python (milliseconds):

| Engine | 1 page, first | 1 page | 10 pages | 40 pages | modern CSS, 1 page | modern CSS, 10 pages |
|---|--:|--:|--:|--:|--:|--:|
| Fagus | 69 | 3.7 | 20.0 | 70.6 | 4.2 | 20.3 |
| Chrome, kept running | 1,196 | 163.4 | 210.9 | 332.3 | 162.9 | 214.1 |
| Browsershot | 1,336 | 1,263.1 | 1,309.7 | 1,452.8 | | |
| mPDF | 123 | 25.6 | 231.9 | 903.9 | | |
| Dompdf | 130 | 48.5 | 353.3 | 2,695.1 | | |
| WeasyPrint | 287 | 95.4 | 460.2 | 1,643.3 | 120.2 | 495.5 |

Chrome uses 1.5 to 2 times as much processor time as wall time, spread over its helper processes; mPDF and Dompdf set the 10 and 40 pages on other page counts, as above. At rest after the 21 PDFs, the Fagus worker alone holds 10 MB (20 MB RSS) after the 1-page invoice and 31 MB (61 MB RSS) after the 40-page one, and uses no processor time; Chrome holds 161 MB (about 570 MB RSS) whatever it rendered and uses 13 to 22 ms of processor time per second while idle.

Four at once, on the 10-page invoice (four PHP or Python processes; one Chrome with four tabs at a time):

| Setup | Peak footprint (RSS) | PDFs per second | Processor time per PDF |
|---|--:|--:|--:|
| Fagus, 4 processes | 147 MB (250) | 224.3 | 20.3 ms |
| Chrome, 1 browser, 4 tabs | 484 MB (1,400) | 24.2 | 190.0 ms |
| mPDF, 4 processes | 304 MB (368) | 16.3 | 241.0 ms |
| Dompdf, 4 processes | 255 MB (320) | 9.9 | 386.3 ms |
| WeasyPrint, 4 processes | 395 MB (586) | 7.4 | 537.0 ms |
| Browsershot, 4 processes | 898 MB (2,851) | 3.5 | 1,531.1 ms |

## Budgets (`memory/budgets.py`)

The budgets of the plan, measured on Fagus alone: the invoices of `compare/` with 3, 150 and 1,580 lines (classic) or 1,680 lines (modern CSS), which give 1, 10 and 100 pages. Render times come from `fagus render --repeat` (one warm engine, the median of 200, 50 or 10 renders; the median of three processes), peak memory from the kernel's peak footprint and `wait4`'s peak RSS of the fagus process, the start from Python starting `fagus worker` and speaking its protocol. The script waits for a load average below `MAX_LOAD` (4 by default) and records the load it measured under.

Results of 2026-11-19 on the same Apple M2 Pro, macOS 27, at commit 36c4647 (Fagus 0.2.0), with other jobs on the machine; the load average of the last minute is given:

| Budget | Target | Measured | Load |
|---|--:|--:|--:|
| Invoice with 1 page | ≤ 5 ms | 2.2 ms (modern CSS: 2.4 ms) | 1.1–1.4 |
| Invoice with 10 pages | ≤ 35 ms | 16.0 ms (16.4 ms) | 1.1–2.0 |
| Invoice with 100 pages | ≤ 300 ms | 172 ms (188 ms) | 1.1–2.0 |
| Each further page (from 10 to 100 pages) | ≤ 3 ms | 1.7 ms (1.9 ms) | 1.1–2.0 |
| Start of a new process until it is ready (`fagus worker` to its first message) | ≤ 3 ms | 3.3 ms, of which 1.8 ms start any process (`/usr/bin/true`) | 2.4 |
| The same, until the first byte of a trivial PDF, minus the render | | 3.5 ms | 2.4 |
| Invoice with 1 page, with the start of the process | ≤ 10 ms | `fagus render` 9.1 ms, the worker 8.5 ms, from PHP (new `Engine`) 8.4 ms | 2.3–2.4 |
| Peak memory at 100 pages, one render | ≤ 64 MB | **98 MB** footprint, 127 MB RSS (modern CSS: 108 MB, 135 MB) | 1.8–1.9 |
| Peak memory at 100 pages, 21 renders by one worker | ≤ 64 MB | **116 MB** footprint, 211 MB RSS (124 MB, 223 MB) | 1.9–10 |
| Binary size (macOS, arm64) | ≤ 30 MB | 25.3 MB | |

At 1 and 10 pages the peak is 9 and 20 MB footprint (19 and 30 MB RSS). The budget for memory at 100 pages is missed by half: the footprint grows steadily while the document is styled and laid out (about 0.7 MB per page of this invoice) and peaks while the PDF is written. With `MallocSpaceEfficient=1`, which hands freed memory back at once, the peak is still 85 MB, so most of it is memory in use, not memory the allocator keeps.

### Memory a worker keeps

After the 100-page invoice, a worker keeps a footprint of 52 MB (125 MB RSS) at rest, and it stays there when small documents follow; after 20 more 100-page invoices it keeps 107 MB (198 MB RSS). It is not the engine's caches: after one or after 60 renders of the 100-page invoice, `vmmap -summary` counts the same 3.4 MB in 19,400 allocations in use (fonts, stylesheets, shaped text, prepared images). The rest is free memory that macOS's allocator keeps for later: empty regions of its small allocations, and its cache of freed large allocations ("Malloc Large (empty)" in `vmmap`), which grows with every large document. In one worker that rendered the 100-page invoice again and again, the footprint at rest was 54 MB after 1 render, 119 MB after 20, 241 MB after 60 and 264 MB after 150; after 20 renders it fell from 108 to 84 MB within 30 seconds of rest and to 45 MB after four minutes. RSS is higher still, because it also counts freed pages the system has not taken back yet.

| Setting in the worker's environment | At rest after 1 × 100 pages | After 20 × 100 pages | Peak |
|---|--:|--:|--:|
| none | 52 MB (125 MB RSS) | 107 MB (198 MB RSS) | 107 MB |
| `MallocLargeCache=0` | 14 MB (120 MB RSS) | 12 MB (121 MB RSS) | 91 MB |
| `MallocSpaceEfficient=1` | 6 MB (119 MB RSS) | 6 MB (119 MB RSS) | 86 MB |

`MallocNanoZone=0`, `MallocMaxMagazines=1` and `RAYON_NUM_THREADS=1` change nothing. With either setting above, the render times differed by no more than the noise of the busy machine they were compared on (load 10 to 13). Linux, where Fagus runs in production, was not measured: glibc's allocator also keeps freed memory in its arenas (one per thread that allocates) and returns it only from the top of the heap or through `malloc_trim()`.

## Fagus

```sh
cargo build --release -p fagus-cli
./target/release/fagus render tests/corpus/invoice/invoice-3.html -o /tmp/invoice.pdf \
  --font-dir tests/fonts --timings --repeat 100
```

`--repeat` renders the document repeatedly with one warm engine and prints the median.

## Other engines (`competitors/`)

PlutoBook, fullbleed, fulgur and ironpress, through their Python packages in a Linux container. The image is x86-64 because PlutoBook only ships x86-64 wheels for Linux.

```sh
cd bench/competitors
mkdir -p fonts && cp ../../tests/fonts/Inter-Regular.ttf ../../tests/fonts/Inter-Bold.ttf fonts/
docker build --platform linux/amd64 -t fagus-competitors .
# Scaling (0 to 300 invoice lines): documents n0.html … n300.html, generated with tools/gen.py
docker run --rm --platform linux/amd64 -v "$PWD:/bench" fagus-competitors python /bench/scale.py pluto
```

| Script | Measures |
|---|---|
| `bench.py` | first render and warm median of one document |
| `scale.py` | render time over 0 to 300 invoice lines (fixed cost and cost per page) |
| `ablate.py`, `ablate2.py` | what the fixed cost per document consists of |
| `one.py` | renders a single document with one engine (for visual comparison) |
| `run-engines.sh` | all of the above inside a plain `python:3.13-slim` container, pinned to one CPU |

## Process overhead (`ipc/`)

What it costs PHP to call a native engine: a new process per call (`proc_open`), a persistent worker, or FFI. `engine.c` is a stand-in engine that only copies bytes.

```sh
cd bench/ipc
docker build -t fagus-ipc .
docker run --rm -v "$PWD:/bench" fagus-ipc sh -c \
  'gcc -O2 -static -o /tmp/engine engine.c && gcc -O2 -shared -fPIC -DLIBRARY -o /tmp/libengine.so engine.c && php bench.php /tmp/engine /tmp/libengine.so 20000 70000 300'
```

## Results of 2026-11-01

Taken before Fagus had any code, as the bar to clear. "Server" is an AMD EPYC-Rome virtual machine, pinned to one core; "Mac" is an Apple M3.

### Calling a native engine from PHP 8.4

The engine is the stand-in from `ipc/`, so only the cost of the call is measured.

| Way | Mac, Linux VM (20 KB / 1.5 MB input) | Server (20 KB / 1.5 MB input) |
|---|---|---|
| New process per call (`proc_open`) | 0.17 / 0.72 ms | 0.54 / 1.59 ms |
| Persistent worker | 0.06 / 0.21 ms | 0.20 / 0.66 ms |
| FFI | 0.04 / 0.06 ms | 0.16 / 0.23 ms |

### Engines on the server (test invoice with standard CSS)

| Engine | 1 page, warm | 1 page, new process | 150 lines (10 pages) |
|---|---|---|---|
| PlutoBook (C++) | 27 ms | 40–56 ms | 55 ms |
| fullbleed (Rust) | 21 ms | 27–29 ms | 694 ms |
| fulgur (Rust) | 46 ms | 45–65 ms | 242 ms |
| ironpress (Rust) | 45 ms | 170–230 ms | 731 ms |
| mPDF (PHP), production invoice template | 80 ms | ~80 ms | 1.8 s (120 lines) |
| Chrome 156, command line (Mac) | ~500 ms per document | | ~520 ms |

### Scaling (x86-64 under Rosetta on the Mac, about as fast as the server)

| Lines (pages) | PlutoBook | fullbleed | fulgur | ironpress |
|---|---|---|---|---|
| 0 (1) | 19.6 ms | 14.0 ms | 29.5 ms | 22.9 ms |
| 30 (3) | 31.3 ms | 54.2 ms | 62.5 ms | 113.7 ms |
| 150 (9–10) | 51.0 ms | 401.1 ms | 153.7 ms | 482.3 ms |
| 300 (18–19) | 73.1 ms | 1,447.9 ms | 291.3 ms | 992.3 ms |

### What the fixed cost per document consists of (invoice without lines)

| Engine | Total | Cause | Without the cause |
|---|---|---|---|
| PlutoBook | 19.8 ms | reloading the `@font-face` fonts for every document | 2.4 ms |
| fullbleed | 13.5 ms | embedding the font (~12 ms) | – |
| fulgur | 30.7 ms | second pass for margin boxes with page numbers | 7.4 ms |

### Modern CSS (24 features used by Tailwind v4, `tests/corpus/modern-css.html`)

| Engine | Supported | Notes |
|---|---|---|
| Chrome | 24 | reference |
| ironpress | ~21 | the table layout of the test page falls apart |
| fullbleed | 18 | no nesting, `inset`, `aspect-ratio`, transforms, shadows or `min()` with percentages |
| fulgur | 15 | wrong colors for `oklch()`, `oklab()` and `color-mix()`; no `:has()` or `@property` |
| PlutoBook | 7 | no CSS Color 4 colors, `@layer`, nesting, grid, `calc()` with percentages, `@property`, multiple backgrounds, `aspect-ratio`, individual transforms or shadows |
