# Extensions

Fagus renders standard CSS. Where a document needs something that no CSS specification offers, Fagus has an extension. Its name starts with `-fagus-`, browsers ignore it, and `@supports` tells whether it is there (see [ADR 4](adr/0004-extensions-only-where-css-has-no-way.md)).

## Carry-over rows at page breaks

An invoice whose table of positions breaks across pages often shows the running total at the bottom of the page and brings it forward at the top of the next one (*Übertrag*). Fagus does this with three pieces:

- **`-fagus-repeat: at-break`** on a row of the table's header (`<thead>`) or footer (`<tfoot>`), or on the row group: the row shows only where the table breaks. In the header, it shows on the pages after the first page of the table; in the footer, on the pages before the last. The value `auto` (the initial value) repeats the row on every page, as the header and footer are.
- **`string()`** in the content of a cell of such a row shows the value the named string has where the row is placed: in the footer after the last row of the page, in the header after the last row of the page before. The rows of the body set the string with `string-set`.
- **`counter(-fagus-next-page)`** and **`counter(-fagus-previous-page)`** show the numbers of the next and of the previous page, as `counter(page)` shows the current one.

The running totals come from the code that writes the HTML, for example as an attribute of each row. Fagus does not add up numbers, so amounts keep the format, currency and rounding of the application.

```html
<style>
  .lines tbody tr { string-set: carried attr(data-carried); }
  .lines .carry { -fagus-repeat: at-break; }
  .lines .carry .amount::after { content: string(carried); }
  .lines .continued::after { content: "Continued on page " counter(-fagus-next-page); }
  .lines .forward::after { content: "Brought forward from page " counter(-fagus-previous-page); }
  /* Browsers would show the carry-over rows on every page. */
  @supports not (-fagus-repeat: at-break) { .lines .carry { display: none; } }
</style>
<table class="lines">
  <thead>
    <tr><th>Item</th><th>Amount</th></tr>
    <tr class="carry"><td class="forward"></td><td class="amount"></td></tr>
  </thead>
  <tfoot>
    <tr class="carry"><td class="continued"></td><td class="amount"></td></tr>
  </tfoot>
  <tbody>
    <tr data-carried="120.00 €"><td>Consulting</td><td>120.00 €</td></tr>
    <tr data-carried="215.50 €"><td>Travel</td><td>95.50 €</td></tr>
    …
  </tbody>
</table>
```

A footer row without `-fagus-repeat` shows `string()` on every page, with the final value on the last one: a subtotal per page and the total at the end.

A header or footer repeats only where it is no taller than a quarter of the page and avoids breaks inside (`break-inside: avoid`, the default for `<thead>` and `<tfoot>`), as in Chrome. One that does not repeat comes once, at the start or end of the table, without its carry-over rows; those repeat on their own where the table breaks, if they are short enough and their row group avoids breaks inside, and show nowhere otherwise. Where a table breaks right before its footer, which then goes on to the next page, the carry-over rows of the footer show below the last rows, as on any page before a break.

The table's columns are measured with the values at the end of the table, usually the widest totals. Keep carry-over rows to one line: their height is that of the row as measured.

With `border-collapse: collapse`, the borders on each page are resolved as if the carry-over rows that do not show there were not in the table: on the first page, the bottom border of the head row meets the first row of the body; on the last page, the last row meets the rows of the footer that remain.

[`tests/corpus/quittio/many-lines.html`](../tests/corpus/quittio/many-lines.html) shows Quittio's invoice with its carry-over rows.

## Pages that grow with their content

A receipt printer prints on a roll and cuts the paper where the document ends. **`-fagus-page-height: fit-content`** in `@page` makes the page as tall as its content and its margins, one page however long the receipt is ([ADR 6](adr/0006-pages-that-grow-with-their-content.md)). The value `auto` (the initial value) keeps the height that `size` gives.

```html
<style>
  @page {
    size: 80mm 40mm;
    margin: 4mm;
    -fagus-page-height: fit-content;
    @bottom-center { content: "Thank you!"; }
  }
  body { margin: 0; font: 9pt/1.3 sans-serif; }
</style>
<h1>Café Fagus</h1>
<table class="lines">…</table>
<p class="total">Total 18.40 €</p>
```

- The height of `size` is the least the page is tall: a short receipt is 40mm long here. It is also what viewport units (`vh`) and percentages of the page area refer to, as in the browser's print preview, which shows the page at that size.
- The page is at most 14,400pt (about 5 metres) tall, the largest page a PDF holds. Content past that goes on to a next page, which grows in turn. Forced breaks (`break-before: page`) still start a new page: each page grows with what it holds.
- The content's height is that of the root element with its margins (the `body`'s default margin of 8px counts), or of lines and images that reach further. Absolutely positioned boxes do not make the page taller. Fixed boxes are placed on the grown page: `position: fixed; bottom: 0` lands at the bottom of its page area.
- Margin boxes, page counters and the backgrounds of the page and of the root element follow the grown page: `@bottom-center` sits at its end, and `counter(pages)` is 1.
- The page is rounded up to whole points, as the page size of the PDF is.
- In vertical writing modes the descriptor is ignored.
- `size: 80mm auto` is invalid CSS, which Chrome ignores, and so does Fagus, with a `css.invalid-value` warning that points here. A value of `-fagus-page-height` other than `auto` and `fit-content` is ignored with the same warning.

Without Fagus, a browser prints the page at the size of `size`. `@supports (-fagus-page-height: fit-content)` tells the two apart. There is no need to render a document several times to find the height that fits it on one page: one render with `fit-content` gives that page, and the [layout](php.md#find-where-elements-landed) of the PDF tells where its content ended up.

