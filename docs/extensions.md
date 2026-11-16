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

The table's columns are measured with the values at the end of the table, usually the widest totals. Keep carry-over rows to one line: their height is that of the row as measured.

[`tests/corpus/quittio/many-lines.html`](../tests/corpus/quittio/many-lines.html) shows Quittio's invoice with its carry-over rows.
