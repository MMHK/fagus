# Changelog

All notable changes to this project are documented in this file. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


## [Unreleased]

### Added

- Footnotes (CSS Generated Content for Paged Media 3), as WeasyPrint and Prince print them; Chrome has none. `float: footnote` moves an element into the footnote area at the bottom of the page its call is on, which `@page { @footnote { … } }` styles (margins, borders, padding, background, `max-height`; a top margin of `1em` by default) and which takes its room from the content of the page. `::footnote-call` (a superscript `counter(footnote)` by default) stays in the text, `::footnote-marker` (`counter(footnote) ". "`) starts the footnote. The `footnote` counter counts through the document, or from each page that resets it with `@page { counter-reset: footnote }`. `footnote-policy: auto` moves a footnote that does not fit to the next page, `line` and `block` take the line or the paragraph of its call with it; a footnote taller than a page breaks across pages. `footnote-display: inline` and `compact` set footnotes side by side. Tagged PDFs have each footnote as a `Note` with its marker as a label. Before, `float: footnote` was ignored with a `css.invalid-value` diagnostic, and `footnote-display` and `footnote-policy` were unknown properties; `float-reference` still is.
- `@page :nth(An+B)` and `:nth(An+B of name)` select pages by their number in the document or in each page group of a name, the pages of an element with a `page` name and a forced break before it (CSS Generated Content for Paged Media 3): `@page :nth(1 of chapter)` styles the first page of every chapter. `@page :first` stays the first page of the document.
- A `fagus` driver for spatie/laravel-pdf 2, registered when that package is installed: `Pdf::view(...)->driver('fagus')`, or `LARAVEL_PDF_DRIVER=fagus` for every PDF. The format, paper size, orientation and margins become an `@page` rule that wins over the document's, the header and footer views become running elements in the page margins with `@pageNumber` and `@totalPages`, and documents start with the base directory, root and tagging of `config/fagus.php`. `scale()` and `pageRanges()` throw a `CouldNotGeneratePdf`.
- `hanging-punctuation` (CSS Text 3), which Chrome does not support and Safari does: `first` and `last` let an opening quote or bracket at the start of a block's first line and a closing one at the end of its last line hang outside the text's edge, `force-end` lets a stop or comma at the end of any line hang, `allow-end` only where it does not fit otherwise. Hanging marks take no room for line breaking, alignment, justification and the widths of boxes that size to their content, and do not make the page shrink to fit. `@supports (hanging-punctuation: first)` is true now; an invalid value is ignored with a `css.invalid-value` diagnostic.
- PDF/A-1b and PDF/A-4, 4e and 4f: `Conformance::PdfA1b`, `PdfA4`, `PdfA4e` and `PdfA4f` in Rust and PHP, `'1b'`, `'4'`, `'4e'` and `'4f'` for `pdfA()`, `--pdfa` and the worker's `conformance`. PDF/A-1b is written as PDF 1.4 with a cross-reference table and without object streams; it forbids transparency, so content that does not show is clipped away, `isolation` is left out and images whose pixels are all opaque lose their alpha channel, and a document with other transparency (`opacity`, translucent colors, transparent images, blend modes, masks, shadows) is refused with a message that says what is transparent and on which page it first appears. PDF/A-4 is written as PDF 2.0, with its metadata in XMP alone; PDF/A-4f takes attachments. A Factur-X invoice raises PDF/A-1b to PDF/A-3b and refuses PDF/A-4, which Factur-X 1.07 does not allow; `accessible()` (PDF/UA-1, on PDF 1.7) goes with neither PDF/A-1b nor PDF/A-4. `tools/validate.sh` checks the new levels with veraPDF.

- `-fagus-page-height: fit-content` in `@page`, a Fagus extension ([ADR 6](docs/adr/0006-pages-that-grow-with-their-content.md)): the page grows with its content, one page as tall as the content and the margins, for receipt printers with 80mm rolls. The height of `size` is the least the page is tall and what viewport units refer to; the page is at most 14,400pt tall, the largest page a PDF holds, and rounded up to whole points. Margin boxes, page counters, backgrounds and fixed boxes follow the grown page. `@supports (-fagus-page-height: fit-content)` is true, other values are ignored with a `css.invalid-value` diagnostic, and the diagnostic for `size: 80mm auto` points to the extension. Rendering again and again to find the height at which `pages()` is 1 is no longer needed.

- Where elements ended up after rendering: with `reportLayout()` in PHP and Laravel, `$pdf->layout()` returns a `Fagus\Layout` with the pages and, for every element with an `id`, its boxes (page, position and size in PDF points from the top left of the page, one per page and per line) and, for tables, which rows went on which page (`Layout\Element`, `Layout\Box`, `Layout\Row`, `Layout\Section`, `Layout\Page`). It takes the place of the scripts people run after layout elsewhere: render, read where things landed, adjust, render again. Without `reportLayout()`, `layout()` throws a `FagusException` and rendering is as fast as before. In Rust, `Document::report_layout(true)` fills the new field `Output::layout` (`Layout`, `LayoutPage`, `LayoutElement`, `LayoutBox`, `LayoutRow`, `RowSection`); on the command line, `--layout-json <FILE>` writes it as JSON; in the worker protocol, `layout: true` in `R` adds a `layout` to the header of `P`.

- Password protection: PDFs encrypted with AES-256 as PDF 2.0 defines it (the standard security handler, revision 6), with a user password to open the file, an owner password that lifts restrictions, and permissions for printing, low-quality printing, changing, copying, annotating, filling in forms and assembling pages. `encrypt(userPassword: …, ownerPassword: …, permissions: [Permission::Print, …])` in PHP and Laravel with the new enum `Permission`, `Document::encrypt(Encryption::new()…)` in Rust, `encryption` in the worker protocol (with `encrypted` in its answer, so that the PHP package refuses a PDF from an older binary that left the passwords out), and `--user-password`, `--owner-password`, `--permissions` on the command line, the passwords also from files (`--user-password-file`, `--owner-password-file`) or the environment (`FAGUS_USER_PASSWORD`, `FAGUS_OWNER_PASSWORD`). Every string and stream is encrypted, including the metadata and attachments. The keys and salts are random, so an encrypted PDF is not byte-reproducible. PDF/A forbids encryption: a password with PDF/A, Factur-X or PDF/UA is refused.

### Changed

- A `content` value that combines `element()` with other values, such as `element(a) element(b)` or `element(title) " – " counter(page)`, is invalid, as CSS Generated Content for Paged Media 3 specifies: the declaration is ignored with a `css.invalid-value` diagnostic. Before, the margin box showed the first running element only.
- A `size` in `@page` that Chrome does not take, such as `80mm auto`, `A6` or three lengths, is still ignored, but now with a `css.invalid-value` diagnostic at its rule that says what is wrong: that a page's height cannot follow its content and both lengths are needed, or the size in millimetres of a paper size Chrome does not know (`105mm 148mm` for A6). Before, the page kept the size that applied before without a warning.

### Fixed

- In tables with `border-collapse: collapse`, rows with `-fagus-repeat: at-break` no longer take part in the borders of pages where they do not show: on the table's first page the head row's bottom border meets the first row (it was replaced by the border of the carry-over row below it), on its last page the last row meets the footer. This also holds for a header or footer with `-fagus-repeat: at-break` on the row group and for rows that break across pages.
- Where a table breaks right before its closing footer, which then goes on to the next page alone, the footer's rows with `-fagus-repeat: at-break` show below the rows on the page where it breaks. They were left out there.
- Rows with `-fagus-repeat: at-break` in a header or footer that does not repeat (taller than a quarter of the page, or with `break-inside: auto`) no longer show where the table does not break: such rows came with the other rows of the header at the table's start, or of the footer at its end. They now repeat on their own where the table breaks, where they are short enough to and their row group avoids breaks inside, and otherwise show nowhere.
- Tagged PDF keeps empty table cells as `TD` and `TH` elements, so that every row of a table has the same number of columns, as PDF/UA-1 requires. A table with an empty cell, such as an empty header in its corner, failed veraPDF's checks 7.2-42 and 7.2-43.
- Conic gradients drawn as images (in PDF/A, with more than 96 stops, and in boxes with `filter`) have the same pixels on macOS as on Linux. Their angles came from the system's `atan2f`, which on macOS differs in the last bit for about one pixel in six, so a few pixels came out one shade off and the files differed. The PDFs made on Linux stay the same.
- A font whose `cmap` maps characters to glyphs it does not have (past its number of glyphs, which format 12 allows) no longer aborts the process when text is written in it: those characters count as missing from the font and fall back to other fonts, and a glyph that shaping still yields past the end is drawn as `.notdef`.
- A font whose em square is not between 16 and 16,384 units, as OpenType requires, is rejected as Chrome rejects it: an `@font-face` rule falls back to the next source or font, and `add_font_data` (the worker's `F`) fails. One of 0 units made rendering stop with an internal error.
- Raster images in SVG images (`<image>` with a PNG, GIF or WebP) and the images that filters change are decoded only up to 200 megapixels, the limit of other images; larger ones are left out. A PNG of a few hundred bytes whose header claimed billions of pixels was decoded into a buffer of hundreds of gigabytes, which aborts the process where the system refuses that much memory.
- An SVG image with a no-break space (or other white space beyond XML's) between the attributes of a tag no longer makes rendering stop with an internal error.
- `fagus worker` reads a frame as its data arrives instead of reserving the length the frame claims first: a frame that claimed up to 1 GiB and ended early took that much memory.

## [0.2.0] - 2026-11-17

### Added

- The `Version` of the Factur-X XMP metadata and the description of the attached invoice XML can be set, for example `3.0` and "XRechnung Invoice" for an XRechnung: `facturX($xml, version: …, description: …)` in PHP and Laravel, `Document::factur_x_version()` and `factur_x_description()` in Rust, `version` and `description` of `facturX` in the worker protocol, and `--factur-x-version` and `--factur-x-description` on the command line. The defaults stay `1.0` and "Factur-X/ZUGFeRD invoice".

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

[Unreleased]: https://github.com/nforst/fagus/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/nforst/fagus/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/nforst/fagus/releases/tag/v0.1.0
