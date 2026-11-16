# Changelog

All notable changes to this project are documented in this file. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


## [Unreleased]

## [0.1.0] - 2026-11-16

The first release: a native engine that turns HTML and CSS into PDF the way Chrome prints them, with a PHP package and a Laravel integration on top. Releases before 1.0 may still change the API.

### Added

#### PHP and Laravel

- One Composer package, `nforst/fagus`, for PHP 8.3 and newer. It is a Composer plugin that installs the `fagus` binary of its release for the platform after `composer install` and `composer update`, checked against the SHA-256 the package lists. Binaries exist for Linux (x64 and arm64, glibc 2.17 or newer and musl), macOS (x64 and arm64) and Windows x64.
- An immutable document builder (`Fagus::html()`, `Fagus::xhtml()`) with a worker process that stays warm between documents, resources handed over as bytes or found through a resolver, PDF/A, PDF/UA, Factur-X, attachments, metadata and diagnostics with the position of what was ignored or missing.
- Laravel 13: `Fagus::view()` for Blade views, PDFs as inline or download responses and on filesystem disks, configuration through `config/fagus.php` and environment variables, registered by package discovery.
- Documentation in `docs/`: getting started, the PHP API, Laravel, resources, deployment (Docker, Alpine, servers without access to GitHub, binaries of your own), troubleshooting, a CSS reference, paged media, fonts, PDF output, the command line, the Rust API and the worker protocol.

#### Command line and Rust

- The `fagus` command line tool: `fagus render input.html -o output.pdf` with font directories, a root directory for files, PDF/A and PDF/UA, attachments, Factur-X, `--created` for byte-identical output, `--system-fonts`, `--locale` and `--timings`.
- `fagus worker`, which renders documents for another program over standard input and output and asks it for resources (the protocol is in `docs/protocol.md`).
- The Rust crates of the engine (`fagus` with `Engine`, and the crates below it), used as a git dependency; they are not published on crates.io.

#### Rendering as Chrome prints

- The cascade with origins, layers, `@scope`, nesting, `:has()` and the other selectors of Chrome, custom properties, `@property`, `@container`, `@supports`, `@import`, `revert` and `revert-layer`, presentational attributes of HTML and quirks mode for documents without a doctype.
- Block, inline, float, flex, grid (with subgrid, named areas and lines), table and multi-column layout, positioning with stacking and anchor positioning, intrinsic sizing keywords, `aspect-ratio`, `zoom` and container queries, with many of Blink's details ported, so that documents written with Tailwind, Bootstrap, Bulma and other frameworks print as in Chrome.
- Text: shaping in every script with HarfRust, line breaking and `text-wrap: pretty` and `balance` as in Chrome, hyphenation with the dictionaries of Chrome on Linux (the same on every system), bidirectional text, vertical writing modes, ruby, emphasis marks, `initial-letter`, `text-fit`, OpenType features and variable fonts.
- Fonts: `@font-face` with TTF, OTF, WOFF and WOFF2, `unicode-range`, synthetic bold and italic, and Inter built in, so that Fagus renders without fonts of its own. Installed fonts only when asked for, so that every platform prints the same PDF.
- Painting: colors of CSS Color 4 and 5, gradients in any color space, borders as Blink draws them (including `corner-shape` and `border-image`), shadows, 2D and 3D transforms, filters, masks, `clip-path`, blend modes, and backgrounds sliced across pages.
- Images and graphics: PNG, JPEG, GIF, WebP and SVG (drawn as vectors, inline or as images, with CSS styling, filters and `<foreignObject>`), responsive images, EXIF orientation, MathML Core, form controls, and `<iframe>`, `<object>` and `<embed>` documents.
- Paged media: `@page` sizes, margins and named pages, margin boxes, running elements, named strings, page counters, `target-counter()` and `target-text()`, page breaks inside paragraphs, tables, flex, grid, floats and columns, repeated table headers and footers, and carry-over rows (`-fagus-repeat: at-break`, `docs/extensions.md`).

#### PDF

- Fonts embedded as subsets, vector gradients, links, the document outline from the headings, and compact files with compressed object streams.
- Tagged PDF by default, with the structure of the document in reading order; PDF/UA-1, PDF/A-2b, 2u and 2a, PDF/A-3b, 3u and 3a, attached files, and ZUGFeRD 2 and Factur-X invoices with the XMP metadata they need. `tools/validate.sh` checks the output of the corpus with veraPDF and Mustang.

#### Safety

- No network access and no scripts: resources come only from `data:` URLs, the caller and files inside the document's root directory.
- Limits on nesting, grids, frames, images and the size of substituted values, so that hostile documents render in bounded time and memory, and a panic becomes an error instead of ending the caller. The engine has no `unsafe` code of its own.

[Unreleased]: https://github.com/nforst/fagus/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/nforst/fagus/releases/tag/v0.1.0
