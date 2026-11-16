# Quittio's default template in standard CSS

Quittio's default invoice template, written for mPDF, rebuilt with standard CSS, with the data and
design values of Quittio's fixtures `simple` and `many-lines`:

- `<htmlpageheader>` and `<htmlpagefooter>` become running elements in page margin boxes, and
  `{PAGENO}` / `{nbpg}` become `counter(page)` / `counter(pages)`;
- the first page has its own margins and no running header (`@page :first`);
- the address field and the fold marks are positioned relative to the page area (DIN 5008 form B:
  address field 45 mm from the top edge, fold marks at 105 and 210 mm, punch mark at 148.5 mm).

- the carry-over rows (*Übertrag*) at the page breaks of the line table, which Quittio's mPDF
  renderer inserts itself, are rows of the table's header and footer with `-fagus-repeat: at-break`
  (see [docs/extensions.md](../../../docs/extensions.md)); the rows carry their running totals in
  `data-carry-over`, as Quittio's template writes them. The pages break after the same positions
  as with mPDF.

The fonts are Geist (SIL Open Font License, see `fonts/LICENSE.txt`), as bundled with Quittio.

`qr-bill.html`, `long-texts.html` and `reminder.html` are the HTML that Quittio's Blade views
(release 1.0) write for mPDF, with Quittio's sample data and the font Inter (`../../fonts`),
converted mechanically instead of rebuilt by hand, so that they keep the template's tables and
class names:

- the mPDF constructs as above; `<pagebreak page-selector="slip">` becomes a block on the named
  page `slip`, whose page area spans the sheet below the header, and the boxes of the payment slip
  are positioned in it; `<watermarktext>` is left out;
- the carry-over rows that the template writes as `<!--quittio:carry-out …-->` and
  `<!--quittio:carry-in …-->` become rows of the line table's footer and header with
  `-fagus-repeat: at-break`, `{amount}` the named string `carry-over`, `{page}` the next and
  previous page numbers;
- the document gets a doctype, `<meta charset>` and `body { margin: 0 }` (mPDF has no body
  margin); the fold marks stay at the left edge of the sheet, outside the page area, where
  browsers and Fagus clip them.

`qr-bill.html` is a Swiss invoice of 18 lines over two pages with the QR-bill payment slip on a
third (its font, Liberation Sans in Quittio, replaced by Inter), `long-texts.html` an invoice
whose one line has a description of six paragraphs and breaks across the page, and
`reminder.html` a payment reminder with its table of open items and fees.
