# Benchmarks

Measurements that Fagus is held against. The [results of 2026-11-01](#results-of-2026-11-01) are at the end of this file; the comparison with the renderers PHP projects use today is in the [README](../README.md#compared-with-other-renderers).

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
