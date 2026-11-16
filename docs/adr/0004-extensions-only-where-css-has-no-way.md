# 4. Extensions only where CSS has no way, with the `-fagus-` prefix

- Status: accepted
- Date: 2026-11-03

## Context

Fagus renders CSS as browsers and the print specifications define it. Some documents need things that no CSS specification offers. The first such need is the carry-over (*Übertrag*) of German invoices: where the table of positions breaks, a row at the bottom of the page shows the running total ("Continued on page 2 · Carried forward · 612.60 €"), and a row at the top of the next page brings it forward. Named strings (`string-set`, `string()`, CSS GCPM 3) carry such a value into the page margins, but not into a table row. XSL-FO has `retrieve-table-marker` for this; Quittio's mPDF renderer lays the table out twice to insert the rows itself.

## Decision

- Extensions exist only where CSS has no way to reach the result, and their names start with `-fagus-`, so that they can never clash with a future standard. Browsers ignore them.
- `@supports` knows the extensions, so that a stylesheet can hide what only Fagus can show: `@supports not (-fagus-repeat: at-break) { … }`.
- Where an extension needs standard syntax in a place the specifications leave open, Fagus gives it the meaning that follows from the specification: `string()` in the content of an element shows the value the named string has at that point of the document. In a row of a table's header or footer, that point is where the row is placed on each page.
- The first extensions serve the carry-over:
  - `-fagus-repeat: auto | at-break` on a row of the header or footer (or on the row group): with `at-break`, the row shows only where the table breaks, in the header after a break and in the footer before one.
  - `counter(-fagus-next-page)` and `counter(-fagus-previous-page)`: the page numbers beside the current one, for "continued on page 3" and "carried forward from page 2".
- The running totals come from the document, for example as `data-` attributes that the code producing the HTML computes. Fagus does not add up numbers: formatting amounts, currencies and rounding belongs to the application.

## Consequences

A template with carry-over rows is a standard HTML table plus a few declarations; it renders in browsers without the carry-over rows. Each new extension needs a record like this one, documentation in `docs/extensions.md` and an entry in `@supports`.
