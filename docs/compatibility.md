# Compatibility

How much of CSS Fagus gets right is measured in two ways: with the reftests of
[web-platform-tests](https://web-platform-tests.org/) (WPT), the test suite browsers share, and
with a corpus of real documents compared with Chrome. Both run in CI, so a change that breaks a
test that passed before fails there.

## Web platform tests

A reftest is a test page and a reference page that must look the same (or, for a few, must not):
the test uses the feature under test, the reference draws the expected result in a simpler way.
Fagus prints both and compares the pages pixel by pixel at 96 dpi, with the tolerance the test
declares for antialiasing (`<meta name=fuzzy>`), as WPT compares browser screenshots. One
difference: browsers snap boxes and glyphs to whole pixels, while a PDF keeps their exact
positions, and a rasterizer draws a glyph's edge and a box's edge at the same fractional position
a little differently. So a pixel only counts as different when no pixel next to it in the other
page has its color; anything larger than an edge one pixel apart still counts. A test
prints on a page of 800 × 600 pixels without margins, the size of WPT's window, and its first page
is compared; print reftests (those with `-print` in their name) print on WPT's page of 5 × 3 inches
with half-inch margins and are compared page by page.

Tests that need JavaScript (a script in the test or its reference), WPT's own server or are SVG
documents are not run, since Fagus runs no scripts and renders HTML; the table lists them as not
run. A few tests are skipped on purpose, with the reason in
[`tools/wpt/expectations.txt`](../tools/wpt/expectations.txt). The pass rate is the share of the
tests run that pass.

Tests without a reference are manual tests: a person reads what the test says to expect and
judges the page. Where that expectation is exact, this repository has a reference of its own in
[`tools/wpt/references`](../tools/wpt/references), at the path of the test with `-ref` added, and
the test runs against it like a reftest. Those of `css/css-gcpm` (named strings in running heads)
are paged tests and print on a letter page with margins of half an inch, compared page by page;
their references put the expected text in the margin boxes directly. The tests of `leader()` in
`css/css-gcpm` have none, as the specification leaves it to the browser how the leader's pattern
lines up, and `leader-002.html` also needs `display: run-in`.

Fagus writes the same PDF on every platform, and the tolerance for edges covers what differs
between versions of Poppler: CI on Linux (Poppler 24) and macOS (Poppler 26) give every test the
same result. The numbers below are those of Linux.

<!-- The table up to the end marker is written by tools/wpt/run.sh --docs. -->

web-platform-tests at commit [`ffa9ce07f3`](https://github.com/web-platform-tests/wpt/tree/ffa9ce07f3641e22cc8156d8006e2584cf036cf5):

| Directory | Passed | Run | Pass rate | Not run |
|---|--:|--:|--:|---|
| `css/compositing` | 21 | 52 | 40.4 % | 8 needs JavaScript, 6 without a reference |
| `css/css-align` | 46 | 61 | 75.4 % | 3 needs JavaScript, 1 without a reference |
| `css/css-backgrounds` | 421 | 622 | 67.7 % | 98 needs JavaScript, 106 without a reference |
| `css/css-borders` | 66 | 113 | 58.4 % | 9 needs JavaScript |
| `css/css-box` | 39 | 39 | 100.0 % | 3 needs JavaScript |
| `css/css-break` | 873 | 1145 | 76.2 % | 33 needs JavaScript, 1 without a reference |
| `css/css-cascade` | 39 | 41 | 95.1 % | 10 needs JavaScript, 1 without a reference |
| `css/css-color` | 279 | 296 | 94.3 % | 12 needs JavaScript |
| `css/css-conditional` | 124 | 145 | 85.5 % | 28 needs JavaScript |
| `css/css-contain` | 252 | 300 | 84.0 % | 134 needs JavaScript, 2 needs the WPT server |
| `css/css-content` | 45 | 55 | 81.8 % | 11 needs JavaScript |
| `css/css-counter-styles` | 106 | 233 | 45.5 % | 26 needs JavaScript, 4 without a reference |
| `css/css-display` | 75 | 122 | 61.5 % | 100 needs JavaScript, 93 without a reference |
| `css/css-flexbox` | 866 | 955 | 90.7 % | 69 needs JavaScript, 36 without a reference |
| `css/css-fonts` | 248 | 346 | 71.7 % | 40 needs JavaScript, 3 without a reference |
| `css/css-gcpm` | 13 | 17 | 76.5 % | 3 without a reference |
| `css/css-grid` | 652 | 1614 | 40.4 % | 149 needs JavaScript, 2 without a reference |
| `css/css-images` | 251 | 418 | 60.0 % | 64 needs JavaScript, 5 needs the WPT server, 4 without a reference |
| `css/css-inline` | 80 | 174 | 46.0 % | 30 needs JavaScript |
| `css/css-lists` | 58 | 156 | 37.2 % | 42 needs JavaScript, 2 without a reference |
| `css/css-logical` | 5 | 5 | 100.0 % | 1 needs JavaScript |
| `css/css-masking` | 219 | 286 | 76.6 % | 98 needs JavaScript, 2 needs the WPT server, 114 SVG document, 4 without a reference |
| `css/css-multicol` | 278 | 404 | 68.8 % | 64 needs JavaScript, 1 without a reference |
| `css/css-nesting` | 8 | 12 | 66.7 % | 5 needs JavaScript |
| `css/css-overflow` | 211 | 546 | 38.6 % | 165 needs JavaScript, 2 without a reference |
| `css/css-page` | 174 | 222 | 78.4 % | 7 needs JavaScript, 20 without a reference |
| `css/css-position` | 79 | 123 | 64.2 % | 128 needs JavaScript, 2 needs the WPT server |
| `css/css-properties-values-api` | 5 | 6 | 83.3 % | 1 needs JavaScript |
| `css/css-pseudo` | 73 | 116 | 62.9 % | 163 needs JavaScript, 7 without a reference |
| `css/css-shapes` | 119 | 230 | 51.7 % | 8 needs JavaScript |
| `css/css-sizing` | 467 | 525 | 89.0 % | 41 needs JavaScript, 3 needs the WPT server |
| `css/css-tables` | 126 | 142 | 88.7 % | 31 needs JavaScript |
| `css/css-text` | 856 | 1447 | 59.2 % | 114 needs JavaScript, 28 without a reference |
| `css/css-text-decor` | 181 | 310 | 58.4 % | 29 needs JavaScript, 78 without a reference |
| `css/css-transforms` | 599 | 714 | 83.9 % | 83 needs JavaScript, 5 without a reference |
| `css/css-ui` | 97 | 117 | 82.9 % | 846 needs JavaScript, 209 without a reference |
| `css/css-values` | 125 | 160 | 78.1 % | 32 needs JavaScript, 22 needs the WPT server, 2 without a reference |
| `css/css-variables` | 117 | 181 | 64.6 % | 2 needs JavaScript |
| `css/css-writing-modes` | 628 | 1120 | 56.1 % | 44 needs JavaScript, 1 SVG document, 145 without a reference |
| `css/filter-effects` | 182 | 304 | 59.9 % | 67 needs JavaScript, 2 SVG document |
| `css/printing` | 29 | 32 | 90.6 % | 7 needs JavaScript |
| `css/selectors` | 163 | 234 | 69.7 % | 108 needs JavaScript, 6 without a reference |
| `css/CSS2/abspos` | 23 | 25 | 92.0 % | 4 needs JavaScript |
| `css/CSS2/box-display` | 82 | 86 | 95.3 % | 34 needs JavaScript, 17 without a reference |
| `css/CSS2/floats` | 101 | 116 | 87.1 % | 9 needs JavaScript |
| `css/CSS2/floats-clear` | 186 | 211 | 88.2 % | 3 needs JavaScript, 31 without a reference |
| `css/CSS2/generated-content` | 181 | 205 | 88.3 % | 21 needs JavaScript, 84 without a reference |
| `css/CSS2/linebox` | 169 | 191 | 88.5 % | 48 without a reference |
| `css/CSS2/lists` | 153 | 155 | 98.7 % | 1 needs JavaScript, 130 without a reference |
| `css/CSS2/margin-padding-clear` | 653 | 683 | 95.6 % | 57 without a reference |
| `css/CSS2/normal-flow` | 702 | 738 | 95.1 % | 53 needs JavaScript, 1 needs the WPT server, 83 without a reference |
| `css/CSS2/pagination` | 40 | 43 | 93.0 % | 60 without a reference |
| `css/CSS2/tables` | 223 | 250 | 89.2 % | 123 needs JavaScript, 713 without a reference |
| `css/CSS2/text` | 294 | 387 | 76.0 % | 23 needs JavaScript, 160 without a reference |
| `css/CSS2/visudet` | 24 | 37 | 64.9 % | 1 needs JavaScript, 1 without a reference |
| `css/CSS2/visuren` | 26 | 31 | 83.9 % | 8 needs JavaScript, 23 without a reference |
| **total** | 12152 | 17298 | 70.3 % | 3193 needs JavaScript, 37 needs the WPT server, 117 SVG document, 2176 without a reference |

<!-- End of the generated table. -->

Some directories measure features Fagus deliberately leaves out or that make little sense in a
PDF, such as vertical writing modes (`css-writing-modes`), scrolling (`css-overflow`) or
animations; see the [CSS reference](css.md) for what Fagus supports.

## Against Chrome

The documents of the corpus in `tests/corpus` (invoices, letters, reports, books, forms and
pages exported from common tools and frameworks, in many scripts and languages) are printed by
Chrome for Testing 157 on Linux and by Fagus, and both PDFs are drawn by the same rasterizer at
50 dpi. Both see the same fixed set of fonts: the test fonts, Liberation, DejaVu and one Noto
family for each other script of the corpus. A document matches when both have the same number of pages and on no page more than 0.5 %
of the pixels differ beyond antialiasing and glyphs a pixel apart. Documents that use print
features Chrome does not have (running elements, named strings, cross references, leaders,
footnotes, blank pages for left and right page breaks), Fagus' own layers, TIFF images or web
fonts with `font-display: optional` are left out, as Chrome cannot print them as intended or
reliably. The documents that are known to differ are listed
with the reason in [`tools/chrome-expectations.txt`](../tools/chrome-expectations.txt); the
release gates fail on every change that makes another document differ, and the share of matching
documents is in their summary.

## Run the tests yourself

Both need Poppler's `pdftoppm` (`poppler-utils`, or `brew install poppler`); the tests of WPT
are checked out with git into `target/wpt` at the commit `tools/wpt/fetch.sh` names.

```sh
tools/wpt/run.sh                            # all directories of tools/wpt/dirs.txt
tools/wpt/run.sh --filter css/css-cascade   # tests whose path contains this
tools/chrome-corpus.sh /tmp/chrome-corpus   # Linux x64; elsewhere set CHROME to a Chrome binary
```

`tools/wpt/run.sh` writes the result of every test, by directory, to
`target/wpt/results/results/`. It fails if a test fails that is not listed in
`tools/wpt/expectations.txt`; after fixing a bug, `tools/wpt/run.sh --update` removes the tests
that pass now, and `--docs docs/compatibility.md` updates the table above.
