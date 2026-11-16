# 2. Rust core with fragmentation inside layout

- Status: accepted
- Date: 2026-11-02

## Context

Measurements on 2026-11-01 (see [bench/README.md](../../bench/README.md#results-of-2026-11-01)) compared PlutoBook (C++), fulgur, fullbleed and ironpress (Rust), mPDF (PHP) and Chrome. PlutoBook was the fastest by far on multi-page documents (about 3 ms per page) because it breaks pages while laying out. Engines that split pages after layout or re-run layout per page were 5 to 50 times slower per page and got page breaks wrong (no repeated table headers).

## Decision

- The engine is written in Rust: memory safety for untrusted templates, C++-level speed, a single static binary.
- Every layout algorithm takes an optional break token and returns a fragment plus an optional break token for the next fragmentainer (page or column), following Chromium's LayoutNG.
- Lengths inside layout are integers in 1/64 px.

## Consequences

Flexbox, grid and tables break across pages with the same mechanism. Repeated table headers reuse fragments. Page counters ("page X of Y") are placed after pagination without a second layout pass.
