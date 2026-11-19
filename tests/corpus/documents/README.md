# Business documents

The stylesheets, icons and template output of other projects in these documents, and their
licences, are listed in `NOTICE.md`.

Documents as applications print them, each written the way such documents usually are:

- `quote-tailwind.html`: a quote styled with Tailwind CSS v4 utilities (the compiled CSS of the
  utilities it uses, with Tailwind's preflight, `@layer`s and `@property` registrations): flex
  rows with auto margins, a grid of addresses, a table whose header cells take the table's
  `text-left`, and page counters in the page margins;
- `invoice-bootstrap.html`: an invoice styled with Bootstrap 5.3 (a subset of its reboot, grid,
  tables, cards and utilities): `ms-auto`, a bordered table whose header cells have widths with
  `box-sizing: border-box`, and totals in cells that span columns;
- `contract.html`: a contract with clauses and paragraphs numbered by CSS counters, justified
  and hyphenated text, a running header that the first page leaves out, initials and "Seite x
  von y" in the page margins, and an annex that starts on a new page;
- `report.html`: an annual report with a cover page, headings numbered by counters that keep
  with their text, KPI tiles in a grid, bar, line and donut charts in inline SVG, a table of
  monthly figures and nested lists with custom markers;
- `newsletter.html`: a club newsletter in three balanced columns with rules, a lead story and
  an advertisement that span the columns, a drop cap, boxes that do not break, justified and
  hyphenated text, and articles whose bottom margins end the columns;
- `roster.html`: a duty roster on a landscape page, a table with a fixed layout whose header
  cells span columns and rows, with vertical header cells whose padding along their lines
  widens their columns (as in Chrome), and coloured cells for the shifts;
- `catalog.html`: a product catalogue with photos floated left and right in turn beside the
  text, prices and tags, and a pull quote floated beside the closing text whose bottom margin
  does not fit on the first page, so that it moves to the next one;
- `invoice-tailwind4.html`: an invoice in compiled Tailwind CSS v4 (`@layer`, `@property`,
  nesting, `oklch()` colours mixed with `color-mix()`, logical properties, `:has()`, `:is()`),
  whose introduction is as wide as `max-w-prose` (`65ch`, the advance of Inter's "0") and whose
  footer breaks inside its top padding before its grid row, as Chrome slices it;
- `programme-cards.html`: a workshop programme of cards in query containers (`@container`,
  `cqi` units) with images that fill their grid areas (`height: 100%`, `object-fit`),
  `aspect-ratio`, `-webkit-line-clamp`, `text-wrap: balance` under a heading of `30ch`, and a
  schedule of subgrid rows that breaks across pages;
- `pricelist-greek-cyrillic.html`: a Greek and Russian wholesale price list whose Greek
  capitals (`text-transform: uppercase` in `lang="el"`) drop their accents as in Chrome, with
  two-column text, tables and lists numbered in Greek letters;
- `invoice-nested-tables.html`: an invoice in the style of dompdf templates (XHTML 1.0
  Transitional, so limited-quirks mode): nested tables with `width`, `align`, `valign`,
  `bgcolor`, `cellpadding`, `border`, `bordercolor` and `nowrap`, cells of 50 % beside a fixed
  one, `<font>`, `<center>`, `<hr size noshade>`, a floated image with `hspace` and `vspace`, and
  a floated totals table cleared by `<br clear="all">`;
- `quote-quirks.html`: a quote in the style of mPDF templates without a doctype (quirks mode):
  tables that do not inherit the font size, paragraphs in cells without their default margins,
  boxes of `height: 100%` in cells and a `<form>` with its quirky bottom margin;
- `datasheet-legacy-grids.html`: a data sheet laid out with the techniques of before flexbox:
  `display: table` cells, float columns with a clearfix in a row with negative margins that
  reaches past the page (which Chrome then shrinks to fit), `inline-block` tiles without
  whitespace gaps (`font-size: 0`), a media object and a `.pull-right`;
- `pricelist-groups.html`: a three-page price list whose table repeats its header and footer on
  every page, with product groups in cells that span their rows, where the rows of a group
  break between them, the footer following the last row on the page and the group's cell ending
  with it, and "Seite x von y" in the page margins;
- `handbook.html`: a warehouse handbook on A5 pages whose first page has a larger top margin,
  with a header and footer that are `position: fixed` and repeat on every page (the footer sits
  where the first page's area ends, as in Chrome), chapters that start on a new page with their
  top margin kept, paragraphs with large top margins, and a "DRAFT" stamp placed absolutely
  from the top of the first page although it comes last in the source;
- `sites-cards.html`: cards in a wrapping flex container, steps in a column flex container and
  a contact grid, all breaking across pages, where a grid item whose bottom padding and border
  do not fit below its last line moves two lines to the next page for `widows`, as in Chrome;
- `invoice-webfonts.html`: an invoice whose brand font comes as web fonts the way font services
  serve them (Inter as WOFF2 and WOFF subsets in `../../webfonts`): `unicode-range`, sources
  passed over for an unknown `local()` face, a `format()` and a `tech()` Chrome does not load,
  and a missing file; Polish letters the subset lacks from a fallback face with `size-adjust`
  and metric overrides, made bold and slanted where it has no such face; tabular figures with a
  slashed zero, stylistic sets and `font-variant-position`;
- `report-variable.html`: a report on A5 pages set in one variable font (Inter's `wght` and
  `opsz` axes, `../../webfonts`): weights from 100 to 900 and in between, optical sizes by font
  size, turned off and set with `font-variation-settings`, a face of `font-weight: auto` that
  takes heavy weights from its axis without synthetic bold, made-up small capitals, tabular and
  diagonal-fraction figures, and `text-rendering: optimizeSpeed`;
- `receipt-thermal.html`: a bakery receipt on an 80mm thermal roll (`size: 80mm 200mm`, margins
  in millimetres), with a dashed rule, a table of items and a Code 128 barcode twice: as inline
  SVG and as a flex row of bars sized in modules with `calc()`; Chrome prints the page as
  227.04pt by 567.12pt (whole points, then 1/300 inch);
- `label-shipping.html`: a 4×6 inch shipping label in a bordered grid, with a Code 128 barcode in
  SVG and as divs and a QR code as a PNG (`image-rendering: pixelated`) and in SVG; its barcode
  of divs is wider than its cell, so Chrome shrinks the content to fit the page;
- `tickets-event.html`: two event tickets on pages of 210mm by 74mm, with gradient backgrounds, a
  dashed perforation line and a stub whose text is turned (`rotate(-90deg)`, and
  `writing-mode: vertical-rl` turned upside down);
- `invoice-dompdf-floats.html`: an invoice as dompdf templates lay it out (HTML 4.01
  Transitional): a header and blocks of floats in percentages cleared by a clearfix, a stamp
  floated beside the introduction, totals floated beside the payment terms, a footer that is
  `position: fixed` at the bottom of every page, and terms after `page-break-before: always`
  with a heading of `page-break-after: avoid`;
- `cv-two-column.html`: a curriculum vitae on a page without margins, a grid of a dark sidebar
  as tall as the page and the main column, a round photo (`object-fit: cover`), skill bars whose
  fill takes the track's corners with `border-radius: inherit`, rating dots, headings ruled by a
  flex `::after`, a timeline of entries with circles of `::before` and `::marker` colours;
- `timetable-vertical.html`: a school timetable on a landscape page, a table of a fixed layout
  with collapsed borders and header cells spanning rows whose text runs upwards
  (`writing-mode: vertical-rl` turned by `rotate(180deg)`): their backgrounds, which paint after
  the borders, reach halfway under them, as in Chrome; break rows with striped
  `repeating-linear-gradient` backgrounds and a legend of swatches from custom properties;
- `certificate.html`: a certificate on a landscape page without margins, with a background of
  radial gradients over a repeated pattern image, a double border with an outline drawn inside
  it, rounded corner ornaments, a title in heavy weights of Inter's variable font whose gradient
  shows through its letters (`background-clip: text`, `color: transparent`), signatures turned
  by a few degrees and a seal of a `repeating-conic-gradient` of many rays;
- `delivery-note-barcodes.html`: a delivery note with EAN-13 barcodes of its items and a
  Code 128 barcode of its number as inline SVG (`shape-rendering: crispEdges`,
  `preserveAspectRatio="none"`), a QR code in SVG, checkboxes, tabular figures, address boxes in
  a flex row and "Seite x / y" in the page margin;
- `contract-markers.html`: a licence agreement whose clauses are numbered by `counter()` in
  `::before` ("§ 1", "2.3" in a hanging column), with lists of `::marker` styles of their own
  (`content: "(" counter(list-item, lower-alpha) ") "`, a check mark, a dash given by
  `list-style-type` and coloured, bold upper-roman markers at 85 %), a definition grid,
  justified and hyphenated text, and an annex after a page break;
- `menu-columns.html`: a restaurant menu on a tinted page (`@page { background }`) in two columns
  with a dotted rule (`column-count`, `column-rule`), sections that do not break, dotted leaders
  in flex rows, a special offer spanning the columns (`column-span: all`) and a title in small
  capitals of Inter's variable font;
- `kontoauszug.html`: a German bank statement over two pages with a table header repeated on
  the second, amounts in tabular figures (`font-variant-numeric: tabular-nums`, with
  `lining-nums slashed-zero` in the IBAN), long references that break anywhere, balance rows and
  "Blatt x von y" in the page margins;
- `dashboard-export.html`: a sales dashboard on a landscape page: KPI tiles with sparklines of
  flex items, a stacked bar chart of flex columns as tall as their row whose bars are percentages
  of the column (as in Chrome, a percentage height of a flex item refers to a row of a definite
  height) with labels placed by `calc()` from the bottom, a pie of `conic-gradient`, a donut cut
  by a radial `mask`, a heat map of `color-mix(in oklab, …)` cells and horizontal bars;
- `letter-din5008.html`: a business letter after DIN 5008 Form B on a page without margins:
  fold and punch marks, the letterhead, the address field of a window envelope with its return
  address and note zone, the information block and the text placed absolutely in millimetres, a
  footer of four columns, and a second page of a named page (`page: follow`) with its own margins
  and margin boxes;
- `report-lab.html`: a laboratory report with a letterhead grid, a block of patient data, tables
  of results with units, reference ranges and flags ("H", "L" with a superscript mark from
  `::before` and `::after`), bars of `linear-gradient` with markers placed by percentages, a
  comment with a coloured edge, "Seite x / y" in the page margins and a rotated stamp;
- `manifest-shipping.html`: a shipping manifest of four landscape pages, a table with a fixed
  column group, a body (`<tbody>`) per tour with a group row that keeps with the next one, rows
  that do not break, subtotal rows and a total in the footer, a header row repeated on every page,
  labels in flags and signature lines;
- `mandate-sepa.html`: a SEPA direct-debit mandate with form controls: text fields with an
  underline, a box of one character each for the IBAN in a flex row, a `<select>`, radio buttons
  and checkboxes with `accent-color`, a `<textarea>` and fieldsets with legends;
- `xrechnung-view.html`: an XRechnung shown in the way the visualisation's XSLT writes it: tabs
  for the screen that print hides, boxes of `display: table` rows and cells with business term
  numbers (BT-, BG-), all tabs shown in print one after another on new pages, and a script that
  only runs on clicks;
- `book-paged.html`: a handbook on A5 pages in the style of WeasyPrint and Paged.js books: a cover
  of a named page without margins, a contents page of its own with Roman page numbers, mirrored
  margins on left and right pages, chapters that start on a right page, running heads from the
  chapter titles (`string-set`, `string()`) and page numbers in the contents and in references
  (`target-counter()`). Chrome knows neither `string()` nor `target-counter()` and leaves the
  running heads, the contents' page numbers and the references' "(S. x)" out, which Fagus prints.
- `receipt-payment.html`: a card payment receipt in the style of payment providers' hosted receipts
  on a Letter page: a banner of a gradient that the card overlaps with a negative margin, a round
  icon, three facts in a flex row, a card brand drawn by `::after`, a summary table with rules of
  table rows of one pixel and a muted footer;
- `timesheet-landscape.html`: a monthly timesheet whose cover and confirmation are portrait pages
  and whose daily records go on a named landscape page (`@page wide { size: A4 landscape }`,
  `page: wide`) with margin boxes of their own, a fixed table layout with a `colgroup`, a header
  repeated on the second landscape page, weekends, holidays and leave as coloured rows and cells
  that end in an ellipsis;
- `labels-avery.html`: an address label sheet laid out in exact millimetres for Avery Zweckform
  L7163 (two columns of seven labels of 99.1 by 38.1 mm on a page without margins), a grid of a
  page per sheet with dashed outlines inside the labels, sender lines that end in an ellipsis and
  addresses centred in flex columns;
- `diploma-fullbleed.html`: a diploma on a landscape page without margins whose photo fills the
  page (`background-size: cover` under a gradient), a round badge, signature lines in a flex row
  at the bottom, and a second page of a named page with margins on the root's background pattern,
  with two justified and hyphenated columns and a table of grades;
- `calendar-month.html`: a month calendar on a landscape page, a grid of week numbers and seven
  columns of `1fr` rows in a height in millimetres, days of the months before and after striped
  by `repeating-linear-gradient`, weekends tinted, today circled, events as chips with coloured
  edges that end in an ellipsis, and two mini calendars with captions in a header aligned by
  baselines (a table with a caption stands on its first row, as in Chrome);
- `terms-numbered-clauses.html`: a services agreement whose clauses are numbered by
  `counter(list-item)` and `counters(list-item, ".")` in `::marker` with colours and tabular
  figures, sub-clauses lettered and numbered in Roman by a counter of their own, definitions
  quoted by `::before` and `::after`, footnote references with their notes at the end (and a
  span of `float: footnote`, a footnote at the bottom of its page, which Chrome leaves in the
  text), justified and hyphenated text,
  and "Page x / y" in the page margins but on the first page;
- `ledger-long-table.html`: an invoice ledger of 180 rows over six pages whose header of two rows
  and footer repeat on every page, zebra rows (`nth-child(even)`), status badges, a caption below
  the table, and a column without a width beside columns of fixed `<col>` widths that give way to
  a long word in it (as in Chrome, a `<col>` width is a column's largest width, not its least).
- `report-flowbite.html`: a project status report in the utilities of Tailwind CSS 3.4 as its CLI
  compiles them (Flowbite's components): `divide-y` between list items, `ring` and `ring-inset`
  as box shadows of custom properties, `-space-x-2` avatars that overlap, `truncate`,
  `line-clamp-2` and `line-clamp-3`, `aspect-video` screens and gradients from
  `--tw-gradient-stops`;
- `report-svg-charts.html`: a sales report whose charts are SVG as the renderers of ECharts 5 and
  ApexCharts 3 export them: labels with `dominant-baseline` (`central`, `middle`, `hanging`,
  `text-before-edge`), `text-anchor` and a `<tspan dx>` in an anchored chunk, a rotated axis title,
  a dashed target line under a clip path, an area under a gradient with stop opacities, a donut
  of arcs, and two charts of `width=330` under `max-width: 100%` in flex items narrower than
  that, which they shrink to as in Chrome;
- `packing-list-code128.html`: a packing list with Code 128 barcodes drawn as SVG rectangles, a
  caption under a barcode whose CSS (`font`, `letter-spacing`) wins over the SVG's own font, items
  numbered by `decimal-leading-zero` counters, and carton labels with a stub of
  `writing-mode: vertical-rl` turned upside down and the barcode of an SSCC stretched to its width;
- `pricelist-leaders.html`: a wine merchant's price list in two ruled columns with
  `font-variant-numeric: tabular-nums`: leaders of a dotted border between name and price in flex
  rows, prices aligned at the decimal comma by an integer part and a decimal part of their own
  (as `text-align: "."` would), leaders of a radial-gradient pattern behind a table, and a grid of
  figures in `ch` columns;
- `cv-timeline.html`: a two-page curriculum vitae: a header grid with a round photo ringed by
  two box shadows, a grid of the main column and a sidebar that breaks across the pages, a
  timeline whose entries keep together (`break-inside: avoid`), and page numbers in the margin of
  all pages but the first;
- `tickets-notched.html`: three concert tickets on an A4 page whose notches at the perforation are
  cut by a `mask` of two radial gradients, with a dashed stub, made-up QR codes in SVG and a
  label with `width: max-content` in a column flex container;
- `letter-din5008-form-a.html`: a two-page letter after DIN 5008 Form A: on the first page, whose
  top margin `@page :first` removes, the letterhead, fold and punch marks, the address field of
  a window envelope and the information block are placed absolutely in millimetres; the following
  page has a running header with "Seite x von y";
- `contract-facing-pages.html`: a five-page licence agreement on facing pages (`@page :left` and
  `:right` with the wider margin at the binding edge and their own margin boxes), a cover page
  of `@page :first`, clauses numbered by counters in a hanging indent, and an annex on a named
  landscape page;
- `plans-nested-flex.html`: hosting plans as cards stretched to the tallest in a flex row, each a
  column flex container whose footer `margin-top: auto` pins to its bottom; tiles stretched to a
  row's minimum height whose bars and columns take percentages of that height (definite for
  them, as in Chrome); and an aside stretched beside a table that breaks across the pages.

Documents as office programs, generators and frameworks write them; Inter stands in for the fonts
they name, through `@font-face` rules ahead of their own styles. The stylesheets of Bootstrap 4,
Foundation for Emails and Font Awesome Free lie unchanged in `vendor/` (MIT, each with its
licence beside it; Font Awesome's fonts are under the SIL Open Font License 1.1, its
`LICENSE.txt` beside them):

- `offer-word-export.html`: an offer saved by Microsoft Word as a web page (`Save as`, filtered
  less): `mso-*` properties, `@font-face` rules without sources, `<o:p>`, conditional comments,
  list paragraphs whose bullets are spans in Symbol between `<![if !supportLists]>`, a
  `MsoTableGrid` table with `width` and cell borders in points, and the section's page from
  `@page WordSection1` (`page: WordSection1`, a name with capitals);
- `minutes-libreoffice.html`: minutes exported by LibreOffice Writer (HTML 4.0 Transitional):
  `<col width="64*">`, cells with borders and padding in centimetres, `<font face size color>`,
  `<div title="header">`, lists of paragraphs in a smaller font than the list items, so that
  Chrome moves each paragraph down below its marker (Blink's `UnpositionedListMarker::AddToBox`),
  and justified text whose no-break spaces widen like the other spaces;
- `proposal-google-docs.html`: a proposal downloaded from Google Docs as HTML (no doctype, so in
  quirks mode): the body as a page of `max-width` and padding, classes `c0` to `c24`, lists numbered
  by `counter-reset` and `counter-increment` across separate `<ol>` elements with
  `::before` markers of `min-width`, empty paragraphs of `height: 11pt`, an image in a clipping
  span, a table and a footnote;
- `paper-pandoc.html`: a paper written by pandoc with its HTML template and citeproc: a title
  block, an abstract, a table of contents, numbered sections, mathematics as plain HTML (pandoc's
  default without MathML), a figure, a table, a block quote, a bibliography with hanging indents
  and footnotes at the end, whose references after the last word of a paragraph keep that word
  whole as Chrome does;
- `paper-pandoc-mathml.html`: the same template with mathematics as pandoc writes it with
  `--mathml`: inline and display formulas in `<math>` with TeX annotations, set in STIX Two Math
  from `../../webfonts` (SIL OFL): fractions, roots, scripts, a sum with limits beside it in a
  line, an integral with limits, stretched parentheses around a matrix, a brace around cases and
  a binomial coefficient, aligned equations in an `<mtable>` whose cells pandoc aligns with
  `text-align` (which leaves them at the start, as in Chrome), accents and `lim` with a limit
  below;
- `paper-katex-mathml.html`: a four-page text whose formulas KaTeX 0.16 (MIT) wrote with
  `output: 'mathml'`, in STIX Two Math from `../../webfonts`: inline formulas in `.katex` spans
  and display ones in `.katex-display`, with TeX annotations, `cases`, matrices with stretched
  fences, aligned equations, `\left`/`\right`, roots with an index, braces over and under,
  accents, an `array` with a rule, and chemical equations of `mhchem` (prescripts, charges,
  arrows with text over them);
- `paper-katex-html.html`: the same text and formulas in KaTeX's default output, its HTML with
  the MathML hidden beside it, styled by the unchanged `katex.min.css` of KaTeX 0.16.47
  (`vendor/katex-0.16.47`, MIT, its `KaTeX_*` WOFF2 fonts under the SIL Open Font License 1.1,
  their `LICENSE.txt` beside them; the WOFF and TrueType files it names after them are left out):
  scripts and fractions stacked in `inline-table` spans
  (`vlist`) of struts and relative offsets, radicals, braces and arrows as inline SVGs of a
  `viewBox` in boxes of other proportions (`preserveAspectRatio="xMinYMin slice"`) or of
  percentages (`\cancel`), positioned absolutely in centred formulas, delimiters of the
  `KaTeX_Size` fonts in inline boxes that Chrome gives fragments of their own, `\boxed`,
  colours, and more notation;
- `paper-mathjax-mathml.html`: the same formulas as MathJax 3 (Apache 2.0) converts TeX to
  MathML, in `<mjx-container jax="MathML">` elements, with its `data-mjx-texclass` rows,
  `mpadded` and `mstyle` wrappers and operators with explicit `stretchy` and spacing;
- `article-wikipedia-mathml.html`: formulas as Wikipedia writes them: `<math alttext>` with the
  TeX in an `<annotation>`, everything in an `<mstyle displaystyle="true" scriptlevel="0">`, display
  formulas indented in `<dl><dd>`;
- `mail-mjml.html`: an order notification compiled by MJML: nested presentation tables with
  `align`, `bgcolor`, `cellpadding` and `role`, columns as inline blocks whose widths come from
  a `screen` media query that print does not take, conditional comments for Outlook, a hidden
  preheader, a bulletproof button, a divider and a table of items;
- `receipt-foundation-emails.html`: a payment receipt in Foundation for Emails 2.4.0 (as Inky
  writes it, `vendor/foundation-emails-2.4.0.css`): `table.body`, a centred container, rows of
  `th.columns` with expanders, spacers, an expanded button, a callout and collapsed columns;
- `quote-daisyui.html`: a quote in Tailwind CSS v4 with daisyUI 5 (the compiled CSS of the
  classes it uses): theme variables in `oklch()`, `color-mix()` for soft badges and alerts, a
  card grid, `stats` in a grid that flows by columns (`grid-auto-flow: column`), a zebra table,
  a divider with a label, `steps` of counters in a grid and a `kbd`;
- `admin-bootstrap4.html`: a support report exported from an admin panel in the complete Bootstrap
  4.6.2 (`vendor/bootstrap-4.6.2.min.css`, whose print styles hide the navbar) with icons of Font
  Awesome Free 6.7.2 (`vendor/fontawesome-free-6.7.2`, its CSS and its solid and regular WOFF2
  fonts): cards with coloured left borders in `no-gutters` rows, progress bars, a flush list group
  with badges, a small striped table with a dark head, and an alert.
- `sheet-excel-export.html`: a budget sheet published by Microsoft Excel as a web page (no
  doctype, so in quirks mode): `xl` classes with `mso-number-format`, `mso-ignore: padding` and
  `text-align: general`, borders in `windowtext`, a table of a fixed layout with `<col>` widths in
  points, merged cells (`colspan`, `rowspan`), rows whose `height` in points includes their cells'
  padding and borders (the table cell height quirk), `x:num` and `x:fmla` attributes and the row
  of column widths behind `<![if supportMisalignedColumns]>`;
- `invoice-laravel-invoices.html`: an invoice in the style of the default template of
  laraveldaily/laravel-invoices, with a stylesheet of its own as such a template hands it to
  dompdf: a small reboot and table and utility classes inline, `* { font-family: "DejaVu Sans" }`
  (here Inter) over a system font stack, `font-weight: bolder` in headings of weight 500, the logo
  of `height=100`, tables of `width` attributes in percent, summary rows beside cells that span
  the item columns and a `<script type="text/php">` that browsers ignore;
- `page-confluence-export.html`: an incident review exported from Confluence as HTML: breadcrumbs,
  a page title and metadata, a two-column section layout with a properties table and a table of
  contents, status lozenges, information, warning and tip panels with icons of `::before`, a
  table with highlighted cells, a code panel, an ordered list, a task list with checkboxes drawn
  by `::before` and `::after`, and user mentions; its code is in Menlo, which macOS has, and
  prints as in Chrome where `--font-dir` adds it.

Six documents are styled with the unchanged stylesheets of CSS frameworks, Markdown renderers
and Laravel's mail theme, which lie in `vendor/` with their licence headers and their licences
beside them (all MIT). The code of the first
three is in Menlo, which macOS has, and prints as in Chrome where `--font-dir` adds it:

- `readme-github.html`: a README as GitHub renders Markdown, in github-markdown-css 5.8.1: a
  note alert, nested lists, highlighted code blocks (`pl-*` spans) and a code block with tabs,
  inline code with `white-space: break-spaces` whose spaces do not hang at the end of a line, a
  table that is a block of `max-content` width with aligned columns, `<details>` open and
  closed, `<kbd>`, a task list with checkboxes and footnotes;
- `membership-pico.html`: a membership application in Pico CSS 2.0.6: a card, form controls
  without their native look (selects keeping their line height, a switch whose thumb is the
  checkbox's `::before`), fieldsets that keep the margins of their labels inside, grids whose
  gaps come from `grid-row-gap` and `grid-column-gap`, a striped table and buttons;
- `article-tailwind-prose.html`: an article in Tailwind CSS 3.4.17 with the typography plugin
  (`prose prose-lg`, compiled for the classes it uses): `:where()` and `:is()` selectors,
  `--tw-*` variables composing rings, `divide-y` and `space-x-*`, `text-wrap: balance`, headings
  whose `line-height: 1.3333333` is a little under 40 pixels as in Chrome, and hyphenated text
  in which capitalized English words stay whole;
- `order-bootstrap.html`: an order confirmation in the complete Bootstrap 5.3.3 (`bootstrap.min.css`,
  its fonts set through `--bs-*` variables): badges, a definition list in grid columns, an alert,
  a striped table with a dark head, `table-group-divider` and warning rows, a card with a list
  group, a progress bar and an offset column;
- `report-bulma.html`: a quarterly service report in the complete Bulma 1.0.2 (`bulma.min.css`,
  made of `--bulma-*` variables and `hsl()` calculations): a level, KPI boxes in columns,
  `content` typography, a notification, a striped and bordered table with a selected row and
  `<progress>` bars, which without their native look are built from their track and value
  (`::-webkit-progress-bar`, `::-webkit-progress-value`) as in Chrome, a message and tags;
- `mail-laravel-markdown.html`: a shipping notification as Laravel renders a Markdown mail
  (its HTML layout of nested presentation tables, `vendor/laravel-mail-11.x-default.css`), printed:
  the header logo, an inner body of 570 pixels, a table component, a button, a panel and a
  subcopy with a long link that breaks anywhere.

Three documents are written right to left in the system fonts of macOS whose Arabic and Hebrew
glyphs Chrome takes (Arial, Tahoma), and print as in Chrome only with `--system-fonts`; without
those fonts their Arabic, Hebrew and Persian letters have no glyphs:

- `invoice-arabic-english.html`: a bilingual Arabic and English invoice (`dir="rtl"`) in Arial:
  a three-column header grid with an English column set left to right, a definition list in a
  grid, a table whose columns run from the right with English, Arabic and numbers in left-to-right
  cells, totals in flex boxes with `dir="ltr"` amounts, and a letter-spaced title, whose Arabic
  letters take no spacing (as in Chrome, which spaces no cursive script);
- `statement-hebrew.html`: a two-page Hebrew bank statement whose table header repeats, with
  amounts isolated left to right (`unicode-bidi: isolate`), and margin boxes on both sides; the
  content and the margin boxes of a right-to-left document are placed from the right margin, a
  fraction of a pixel right of where a left-to-right one goes (as in Chrome);
- `quote-persian.html`: a Persian quote with Persian digits and separators, a logo floated right
  beside the header, a table with logical borders (`border-inline-start`) and a total row that
  spans columns, conditions in an ordered list with markers on the right, and signature lines in
  a flex row.

Three more take fonts from macOS and print as in Chrome only with `--system-fonts`:

- `invoice-japanese.html`: a Japanese invoice (請求書) in Hiragino Sans: a letter-spaced title over a
  double rule, the addressee with 御中, the issuer's details with a round seal of two vertical lines
  (`writing-mode: vertical-rl` in a centred flex container, in Hiragino Mincho), the amount, a table
  of items, totals, and justified notes with `line-break: strict` whose gaps between ideographs
  widen as in Chrome (`text-justify: auto`);
- `receipt-thai.html`: a Thai receipt and tax invoice on an A5 page in Thonburi: a header grid, the
  parties in tinted boxes, a table of items with totals in its footer, the amount in words and
  justified terms that break between Thai words as in Chrome;
- `form-application.html`: a room booking request printed from a web form with controls in their
  native look: text, e-mail, telephone, date and number fields, a `<select>`, radio buttons and
  checkboxes, a `<textarea>`, buttons and fieldsets with legends. Its controls are in the system
  font; Chrome writes the date in the format of its user interface's language (`--locale`).

Four more print as in Chrome only with `--system-fonts`: three have their code in Menlo, the
fourth names in Arabic, Persian and Hebrew in Arial:

- `docs-mkdocs-material.html`: a page of documentation as Material for MkDocs prints it:
  admonitions whose icons are masks of SVG data URLs, a collapsible block open and one closed,
  task lists, code tables with line numbers (pymdownx) and highlighted lines, content tabs that
  `display: contents` lays out one after the other in print, and lengths in `rem` of a root at
  125 %, which Chrome multiplies from the numbers as written;
- `docs-vitepress.html`: a page of documentation as VitePress prints it: custom blocks, badges,
  a code block with line numbers in an absolutely positioned column, highlighted and diff lines,
  a code group whose inactive tabs are hidden, a table and the pager;
- `transfer-girocode.html`: a payment request with a SEPA credit transfer slip of comb fields (a
  grid of 4.6mm cells, every fourth of the IBAN's ruled darker), a GiroCode as an SVG path of
  made-up modules, the IBAN in groups of four in Menlo and the code's content in a `<pre>`;
- `letter-bidi-names.html`: a German letter with Arabic, Persian and Hebrew names in `<bdi>`, a
  table with cells of `dir="rtl"`, Arabic-Indic and Persian digits, right-to-left quotations with
  `border-inline-start`, and a box in Arabic beside its German twin.

Documents whose text comes from the rich-text editors and Markdown renderers that applications
use, in CSS frameworks not covered above, and templates written for older PDF engines (the
frameworks' builds in `vendor/`, each with its license):

- `reference-tiptap.html`: an Arbeitszeugnis whose text a TipTap (ProseMirror) editor stores:
  `* + *` spacing, justified and hyphenated paragraphs, lists of paragraphs, a fixed-layout
  table with a `colgroup` of pixel widths, highlights with `box-decoration-break: clone`, a
  quotation and a trailing break of ProseMirror's;
- `quote-bootstrap3.html`: a garage's estimate in Bootstrap 3.4.1 (`vendor/bootstrap-3.4.1.min.css`)
  and its print styles: links and abbreviations followed by `(attr(href))`, backgrounds and
  colours removed, a navbar hidden, floated `col-xs-*` columns in rows cleared by `.row:after`,
  panels, a horizontal definition list, a striped condensed table, a well and the columns of
  its last row breaking across the page, after which the clearfix starts below them;
- `invoice-partial-foundation.html`: a third Abschlagsrechnung (partial invoice for building
  work) in Foundation 6.8.1 (`vendor/foundation-6.8.1.min.css`): the XY grid, callouts, tables
  with `width` attributes, the table of payments so far and the calculation with the retention,
  and its print styles (`print-color-adjust: economy`, `.show-for-print`, `.hide-for-print`);
- `expenses-pure.html`: a travel expense report in Pure 3.0.0 (`vendor/pure-3.0.0.min.css`): the
  flex grid of `pure-u-*` units, a stacked form of read-only fields and checkboxes, horizontal,
  bordered and striped tables, and buttons;
- `order-uikit.html`: a purchase order in UIkit 3.21.16 (`vendor/uikit-3.21.16.min.css`): grids of
  cards with `uk-child-width-*`, a divided striped table with shrinking and expanding columns,
  bullet lists, a divided description list, an alert, a comment and the print styles;
- `invoice-materialize.html`: a caretaker's invoice in Materialize 2.1.1
  (`vendor/materialize-2.1.1.min.css`): its 12-column CSS grid, chips, cards, a striped table and
  a collection;
- `dunning-wkhtmltopdf.html`: a second Mahnung with a fee and interest table, written for
  wkhtmltopdf: `display: -webkit-box` with `-webkit-box-flex` (the flexible box of 2009, laid out
  as Chrome does: no automatic minimum, `fit-content` bases, the alignment of the box), `zoom`
  against its DPI, `page-break-*`, a `tfoot` turned into a row group and a forced break;
- `reminder-dompdf-sepa.html`: a Zahlungserinnerung with a SEPA mandate reference and a new
  mandate on a second page, written for dompdf: margins in centimetres, a header and footer fixed
  at negative offsets in the page margins (which Chrome places on its pages where it puts fixed
  boxes, not in the margins), a meta table positioned absolutely and `.page_break`;
- `statement-mpdf.html`: a commission statement written for mPDF: `<htmlpageheader>`,
  `<sethtmlpagefooter>`, `<pagebreak />`, `<columns>` and `<barcode>`, which browsers keep as
  unknown inline elements with all that follows them inside (so the page does not break), and
  `{PAGENO}` printed as written.

Five more take Menlo (code), Symbol or the symbols for checkboxes and arrows from macOS and print
as in Chrome only with `--system-fonts`:

- `letter-ckeditor5.html`: an offer letter whose body is HTML as CKEditor 5 stores it, styled by
  the template's own CSS for the classes in that markup: a captioned image floated beside the text
  (`figure.image`, a table with `max-width: 50%` around an image of `max-width` and
  `min-width: 100%`), a `figure.table` with a caption on top and a `colgroup`, markers and a red
  pen, nested ordered lists, a to-do list whose checkboxes are drawn by `::before` and `::after`
  of `appearance: none` inputs, a quotation that breaks across pages, and the page-break widget;
- `offer-quill.html`: an offer whose text Quill 2.0.3 stores, shown with its snow stylesheet
  (`vendor/quill-2.0.3.snow.css`): lists that are all `<ol>`, with `data-list` items whose bullets,
  numbers and checkboxes are counters in `::before` of `.ql-ui` spans, `ql-indent-*` levels,
  `ql-align-*`, `ql-size-*`, empty paragraphs of a `<br>` and a code block;
- `credit-note-tinymce.html`: a Gutschrift with text blocks from TinyMCE: inline styles, a
  `border=1` table with a `colgroup` of percentages and rows of fractional pixel heights,
  paragraphs pasted from Word with Symbol bullets in `mso-list: Ignore` spans, a list with
  `list-style-type` styles and `<o:p>`;
- `delivery-note-trix.html`: a Lieferschein with notes from Trix / Action Text
  (`vendor/trix-2.1.8.css`): `<div>` paragraphs with `<br>`, a heading, a quotation, an
  inline-block `<pre>`, a gallery of two image attachments with captions and a file attachment;
- `terms-commonmark.html`: terms rendered from Markdown by league/commonmark: heading permalinks,
  a table of contents in two columns, task lists, GFM tables with `align` attributes, strikethrough,
  a definition list and footnotes with back references.

Documents whose fonts and scripts meet font fallback, with Inter served as Google Fonts serves it
(WOFF2 subsets per script in `../../webfonts`, `Inter-400-latin.woff2` and the like, each with the
`unicode-range` of its subset). Three print as in Chrome as they are:

- `letter-vietnamese.html`: a Vietnamese order confirmation with Russian and Greek summaries, the
  brand font in Google Fonts' latin, latin-ext, Vietnamese, Cyrillic and Greek subsets, an italic
  sentence made up from the regular face, a table with `₫` amounts and justified text in which the
  numero sign `№` takes room on both sides, as Chrome spaces out CJK symbols;
- `statement-print-media.html`: a bank statement on an A5 landscape page whose screen styles
  (navigation, a banner, a dark colour scheme) give way to its print styles, `print-color-adjust:
  exact` header and cards, link targets after links, and words that say which media features
  print answers (`print`, a light colour scheme, colour, no hover, no forced colours);
- `pack-page-sizes.html`: a tender pack of named pages in A4, A3 landscape, Letter, Legal, A6
  landscape (a keyword Chrome does not know, so that page is A4 as the document's), A5, B5 and
  JIS B5, and a floor plan on a dotted grid.

Nine take fonts from macOS and print as in Chrome only with `--system-fonts`:

- `invoice-hindi-english.html`: a GST invoice in English and Hindi, Devanagari from Kohinoor
  Devanagari beside the brand font's Latin, the rupee sign from its latin-ext subset, and
  tabular figures;
- `quote-korean.html`: a Korean quote (견적서) in Apple SD Gothic Neo after families that are not
  installed, `word-break: keep-all`, a letter-spaced title, a stamp and justified notes that
  Chrome widens at spaces only (Hangul is not spaced out like ideographs);
- `delivery-note-chinese.html`: a Chinese delivery note on an A4 landscape page in PingFang SC,
  with a box in traditional Chinese, `text-overflow: ellipsis` and the multiplication sign, which
  PingFang lacks and Chrome takes from Helvetica;
- `product-sheet-emoji.html`: a German product data sheet with emoji in Apple Color Emoji (ZWJ
  sequences, skin tones, keycaps, flags), star ratings, check marks from `::before` and Font Awesome
  icons (`vendor/fontawesome-free-6.7.2`), reviews in French, Portuguese and Japanese, and the
  names of languages in Greek and Ukrainian from the system font;
- `letter-japanese-vertical.html`: a Japanese letter set top to bottom (`writing-mode: vertical-rl`)
  in Hiragino Mincho, with ruby, digits combined upright (`text-combine-upright: all`) in the
  half-, third- and quarter-width forms of the face and without the letter spacing of the text,
  and an address block between rules;
- `certificate-caucasus-ethiopic.html`: a certificate on a landscape page without margins in
  Georgia, with Armenian, Georgian and Amharic lines that fall back to the system's fonts for
  those scripts, a gradient seal and `print-color-adjust: exact`;
- `newsletter-nextjs-fonts.html`: release notes styled as Next.js loads fonts (`next/font`): a
  variable face of weights 100 to 900, the metric-adjusted fallback face it generates from
  `local("Arial")` (`size-adjust`, `ascent-override` …) that draws the letters the subset lacks,
  a mono face from `local()` names, a face of one style made bold and slanted or not by
  `font-synthesis`, optical sizes, small capitals, ligatures and kerning turned off;
- `receipt-bengali-tamil.html`: a money transfer receipt on an A5 page with Bengali and Tamil in
  named fonts and by fallback, and the taka and rupee signs;
- `pricelist-fallback-nfd.html`: an export price list in a Latin-only web font whose Cyrillic,
  Greek, Armenian and Georgian come from the next family character by character, and names written
  with combining accents (NFD), whose base letter and accents stay in one face as in Chrome: the
  web font where it has the composed letter, else the next family.

Invoices, packing slips and reports in the style of the templates that shop, ERP, billing and
content systems print (written for this corpus after them, none copied), with Inter in place of
the fonts they name; the first two use the unchanged Bootstrap builds in `vendor/`:

- `invoice-odoo-qweb.html`: an invoice in the markup of the HTML view of an Odoo 17 QWeb report,
  in Bootstrap 5.3.3 with a few rules of its own (not Odoo's) for Odoo's classes: the standard
  layout's header with a logo and a rule, the `.article` with the addresses in an offset column,
  `#informations` in `col-auto col-3 mw-100` columns, a borderless `o_main_table` with section,
  note and subtotal rows of `colspan="99"`, the totals in `ms-auto` and a footer of inline list
  items separated by `::after`;
- `invoice-erpnext.html`: a sales invoice in ERPNext's "Standard" print format, in Bootstrap 3.4.1
  (whose print styles force black text and white table cells): `section-break` rows of `col-xs-6`
  columns of label and value rows, a bordered item table with an image floated in a cell, totals
  aligned right and a fixed, rotated and translucent "BEZAHLT" watermark that the table's white
  cells cover;
- `invoice-ninja-bold.html`: an invoice in the style of Invoice Ninja's "Bold" template, styled
  mostly through attribute selectors (`=`, `~=`, `^=` and `[data-sum]:empty`): the page in
  one cell of a table whose header and footer rows hold spacers, a dark header grid, a line
  table of a fixed layout with percentage columns, `overflow-wrap: break-word`, an item name
  broken at a soft hyphen and descriptions in `white-space: pre-line`, a totals grid, a stamp
  placed absolutely and turned by `rotate()`, and the whole page at `zoom: 80%`;
- `invoice-stripe-hosted.html`: an invoice as a payment provider's hosted invoice e-mail prints on
  a Letter page, in the system font stack (`-apple-system`): presentation tables, spacer rows of a
  pixel's font, a card with a badge raised by `vertical-align: 2px`, a brand row of an empty
  round icon beside the name in an inline flex container (whose baseline is the icon's bottom, as
  in Chrome), and line items in a fixed table of `width: 100%` inside the auto tables whose long
  item ends in an ellipsis without widening them (a fixed table's least width is that of its given
  columns);
- `invoice-woocommerce.html`: an invoice laid out like the "Simple" template of the PDF invoice
  plugins for WooCommerce, with a stylesheet of its own written the way dompdf templates are:
  tables for the layout, `line-height: 100%` on the body, item meta of a `dl` with inline
  `dt`/`dd` and a line break from `dd::after { content: "\A" }`, options of a list with inline
  paragraphs, a bundle with indented rows and a hidden one, a totals table of
  `table-layout: fixed` with cells of 50 % and padding in a cell spanning two columns (Chrome
  scales its percentage columns down to the room there is), and a footer placed absolutely below
  the page area; its `counter(page)` in the footer, which Chrome leaves at 0, Fagus counts;
- `packing-slip-shopify.html`: a packing slip laid out like Shopify's default packing-slip
  template, with a stylesheet of its own, on a Letter page: a header of `display: -webkit-box`
  before `display: flex`, the addresses in a flex row, line items as flex rows of a thumbnail with
  `object-fit: cover`, a description with `overflow-wrap: anywhere` and a quantity, a badge, notes
  of `white-space: pre-wrap`, a dashed gift note with a label of `::before` on its border and a QR
  code;
- `invoice-saas-qr.html`: a German invoice of a software company in the style of Lexoffice and
  sevDesk: a DIN-like address block in a grid, a definition grid of the invoice data, tabular
  figures, totals with a double rule, a payment box with a GiroCode, and the company's details in
  three margin boxes of several lines (`content: "…\A…"` with `white-space: pre`);
- `invoice-swiss-qr-bill.html`: a Swiss invoice with a QR-bill on a page without margins: the body
  a column flex container of `min-height: 100vh`, the receipt and payment part of 62 and 148 mm
  side by side at the bottom, a dashed perforation with scissors from an SVG data URL in
  `::before`, the payment part a grid of `auto 1fr auto` rows and the Swiss cross over the QR code;
- `article-cms-print.html`: a news article as a Craft CMS or Statamic theme prints it with the
  print styles of the HTML5 Boilerplate: `a[href]::after` with `attr(href)` (but not for `#`
  links), `abbr[title]::after`, navigation, share buttons and a fixed cookie banner hidden, a
  figure with `aspect-ratio`, `quotes` for German, a fact box grid, a table with its caption below,
  an open correction in `<details>` and footnotes;
- `mail-sylius-order.html`: an order confirmation e-mail in the style of Sylius, printed: XHTML 1.0
  Transitional, a hidden preheader, a 600-pixel content table, a coloured header with rounded top
  corners, item rows with thumbnails, a summary, address boxes in cells of 50 %, `<center>` and a
  button;
- `export-filament-table.html`: a customer table exported from a Laravel Filament 3 admin panel on
  landscape pages, in the compiled CSS of its Tailwind theme: colours as RGB triplets in custom
  properties (`rgba(var(--c-600), .1)`), badges with inset rings and SVG icons, avatars of
  `hsl(var(--hue) …)`, `divide-y`, `truncate` in `max-w-48`, `:has()` marking overdue rows with an
  inset shadow, and a header and summary row repeated on each of five pages;
- `report-ledger-sticky.html`: a trial balance of 40 accounts on two landscape pages whose table
  header, account numbers and group rows are `position: sticky` on the screen (with `top`,
  `left` and `bottom` offsets, in a scrolling `.table-responsive`), which print where they stand
  in the flow, with the header and footer repeated, negative amounts with `::before` and a legend
  in two columns;
- `report-modern-cascade.html`: a release report written with the newest cascade features: cascade
  layers (`@layer`, where an `!important` in the first layer wins), nesting with `&` (also
  `h1 &`), `:has()` (including `tr:has(+ tr .status[data-state="blocked"])`, a row before a
  blocked one), `nth-child(odd of :not(.group))`, container queries by name and by range, `@scope`
  with a lower limit, `light-dark()`, `color-mix()` and `oklch()`, and stripes of a repeating
  gradient from `transparent`, which fade from the stripe's colour as in Chrome;
- `statement-container-queries.html`: a bank statement in registered custom properties
  (`@property` of a percentage, a number and a length), sub-layers (`components.cards`),
  `all: revert-layer`, container size queries with `cqi` units and a style query
  (`@container style(--theme: warning)`), relative colours (`oklch(from var(--accent) …)`),
  `round()`, `repeat(auto-fit, minmax())`, bars of gradients sized by a custom property and
  `text-wrap: balance`;
- `annual-report-long.html`: a 36-page annual report with a full-bleed cover on a named page
  without margins, a table of contents with dotted leaders and page numbers from
  `target-counter()` (which only Fagus prints; Chrome drops the whole `content`), a running
  header that is `position: fixed` above the page area, chapters that start on a new page,
  sections in two columns with a rule, bar and line charts in inline SVG, tables with captions
  below, pull quotes and footnotes numbered across chapters, justified and hyphenated;
- `manual-technical.html`: a 33-page installation manual with headings numbered by counters on
  three levels, chapters on new pages, steps in ordered lists nested with letters and coloured
  markers, code blocks whose lines are numbered by a counter in absolutely positioned
  `::before`, admonitions, figures with captions and register tables that break between rows
  with their header repeated, and the page number "of" the page count in the margins;
- `catalog-price-grid.html`: a price catalogue of 100 products in a grid of three columns, each
  product a grid of a photo (`object-fit: cover`) and its text that avoids breaks inside it,
  under category headings that span the columns and avoid a break after them: where the row
  below a heading does not fit, Chrome breaks inside its items rather than before the heading
  (Blink's grid takes no better break between its rows), a break so poor that the whole grid
  moves to the next page if it starts below other content;
- `contract-signatures.html`: a supply agreement with clauses and paragraphs numbered by
  counters (`1.1`, `1.2` in absolutely positioned `::before`), lists of letters, justified and
  hyphenated text, schedules that start on new pages and signature blocks that do not break;
- `statement-running-balance.html`: an eight-page bank statement of 159 bookings with a running
  balance, striped rows that do not break, a repeated header and a footer row with the new
  balance;
- `report-cards.html`: twelve school report cards, one per page (`break-after: page`, except
  for the last), each with a header of the school's logo, a grid of facts, a striped table of
  grades and a grid of signature lines;
- `programme-timeslots.html`: a conference programme of three days, each a grid of time slots
  (`minmax(11mm, auto)` rows) and rooms whose sessions span rooms and slots: without a definite
  height the rows grow to their items, and an item spanning two rows leaves what it needs more
  to the row without an item of its own, as in Chrome;
- `policy-landscape-schedules.html`: an insurance policy whose schedule of 70 insured locations
  is on landscape pages of a named page (`page: schedule`) with margin boxes of their own,
  between portrait pages;
- `datasheet-spec-tables.html`: eight product data sheets, one per page, with a photo beside
  bullet points and specification tables with captions in two columns that do not break;
- `newsletter-email-long.html`: a long HTML e-mail newsletter printed (XHTML 1.0 Transitional,
  nested presentation tables of 600px with `cellpadding`, `bgcolor`, `valign`, two-column
  stories with photos, buttons in table cells and quotes with a coloured left border);
- `thesis-footnotes.html`: a thesis chapter whose `counter-reset: page 47` on the body leaves
  the page numbers in the margin alone, as in Chrome, with a running head left out on the first
  page, indented justified paragraphs, block quotes, a figure, and its 45 footnotes as Chrome
  prints them: endnotes after the text, with hanging numbers in raised inline blocks;
- `manifest-page-subtotals.html`: a 30-page shipping manifest whose sheets each hold a table of
  22 packages with a subtotal per page and the total carried forward to the next one, a header
  in the top margin and `break-after: page` on all sheets but the last.

Printed from web applications, with their form controls, dialogs and app grids:

- `form-registration-styled.html`: an event registration form whose controls have no native
  look (`appearance: none`): fields with italic `::placeholder` text, read-only and disabled
  states, a `<select>` with an arrow as background image, custom checkboxes whose check mark
  is a `::before` centered by `display: grid`, `accent-color` on native ones, and a slider whose
  margins make the page shrink a little, as Chrome measures the end margin of a line's last box;
- `help-details-dialog.html`: a help centre article with `<details>` open and closed (markers
  from `list-style-type` strings and `::after`), open `<dialog>`s, one of them centred in a
  positioned box as Chrome's user agent style places it;
- `dashboard-revenue-grid.html`: a revenue dashboard on a landscape page, a grid of KPI cards
  with inline SVG sparklines, a bar chart of flex items, a donut chart, a native progress bar
  and a table of invoices with status pills;
- `calendar-week.html`: a calendar's week view, a grid of 24 fixed rows in which events span
  their hours and clip their text (stretched items shrink to rows of a fixed size), above the
  hour lines with `z-index` on grid items that are not positioned;
- `order-admin-filament.html`: an order's edit page of a Filament-like admin panel whose
  sidebar `@media print` hides: input wrappers with affixes, selects, a toggle, a textarea with
  a placeholder, a table of items, a summary list and a timeline;
- `settings-toggles.html`: a settings page with toggle switches built of hidden checkboxes and
  of `role=switch` checkboxes without their look, segmented radio buttons, time fields and a
  `<select>`;
- `invoice-app-shell.html`: an invoice previewed in an app shell (a grid of a sidebar, a sticky
  top bar and a toast, sized in `vh`) that `@media print` takes apart to print the paper alone;
- `checkout-modern-selectors.html`: a checkout form on an A5 page styled by `dvh`, `svh`,
  `lvh` and `dvb` units, `:valid`, `:invalid`, `:in-range` and `:out-of-range` (from the values,
  types, patterns and ranges in the attributes), `:has(input:invalid)`, `:placeholder-shown`,
  `:read-only`, `:indeterminate`, `:user-invalid` and `:focus-visible` (which match nothing in
  print), a popover that stays hidden and one shown by its style, an `inert` section and a
  dialog shown with `display: block`;
- `form-bootstrap-validation.html`: an account application in Bootstrap 5 form styles after
  validation (`.was-validated`): floating labels, input groups, `:valid` and `:invalid` fields
  with their icons and feedback, required selects with a placeholder option, switches and a
  range whose track and thumb are styled (`::-webkit-slider-runnable-track`,
  `::-webkit-slider-thumb`).

`form-registration-styled.html` and `settings-toggles.html` show dates and times, and
`order-admin-filament.html` a number field, in the formats of Chrome's user interface
(American English, as Fagus without `--locale`).

Four more print as in Chrome only with `--system-fonts`:

- `form-contact-controls.html`: a German contact preferences form with every control in its
  native look and the system font (text, e-mail, telephone, password, URL, search, number, date,
  time, `datetime-local`, month, week, color, range and file inputs, checkboxes and radio buttons
  checked and disabled, a `<select>`, a `<select multiple>`, a field with a `<datalist>`, a
  `<textarea>`, progress bars and meters of every state, `<output>`, buttons and fieldsets);
  Chrome writes its dates in German with its user interface in German (`--locale de-DE`);
- `table-customers-sticky.html`: a four-page table of customers exported from an app: a sticky
  header that repeats, zebra rows with selected rows highlighted by `tr:has(input:checked)`
  (which counts as specific as its argument), sort icons, status badges, icon buttons with
  `✎` and `⋯` from the system's fonts and pagination controls;
- `board-kanban.html`: a sprint board of four columns of cards with labels, progress bars,
  priority triangles and avatars, its symbols from the system's fonts;
- `survey-likert.html`: a German patient survey in Arial over three pages, with a scale of
  radio buttons in tables whose columns of 13 % share the width left as Chrome shares it, where
  each group of radio buttons has the last one marked `checked` checked (`--locale de-DE`).

`invoice-stripe-hosted.html` prints as in Chrome only with `--system-fonts` (San Francisco).

Eleven everyday documents more, in Inter only:

- `itinerary-boarding-pass.html`: a travel itinerary with a timeline of dashed lines and dots, a
  boarding pass with a PDF417-like barcode and a QR code in SVG, text turned along its edge
  (`rotate(90deg)`), a stub behind a dashed perforation with notches of radial gradients, and a
  footer that is `position: fixed`;
- `expose-real-estate.html`: a real estate exposé on pages without margins for its cover and
  named pages with margins for the rest: a photo grid of `object-fit` `cover`, `contain` and
  `none` at `object-position`s and `aspect-ratio`s, an energy scale with a marker, a floor plan in
  SVG (hatch pattern, dashed door swings, dimension lines) and a map drawn with gradients;
- `menu-allergens.html`: a restaurant menu with dot leaders made the W3C way (a floated row of
  dots cut off by the price), allergen icons as circles and in SVG, ornaments of repeated radial
  gradients, a chef's quote with a round photo and a wine list in tables;
- `recipe-cards.html`: recipe cards two per page (`break-inside: avoid`, 135mm tall), each a grid
  of `auto 1fr auto` rows whose side column spans the last two rows: where its content is taller
  than they are, it overflows them instead of growing the flexible row, as in Chrome;
- `form-tax-overlay.html`: a German tax form filled in over a background image (an SVG data URL
  of 210 by 297mm) with fields placed absolutely in millimetres, digit combs, checkboxes drawn
  with gradients and native ones, and a turned stamp;
- `lab-report-ranges.html`: a laboratory report whose table groups do not break, with values
  flagged high or low and their place in the reference range on inline bars, and margin boxes
  that the first page leaves out;
- `labels-product-100x150.html`: product labels on pages of 100 by 150mm with a side strip of
  `writing-mode: vertical-rl` turned upside down, a nutrition table, icons and an EAN-13 barcode
  in SVG;
- `certificate-conformity-seals.html`: an EU declaration of conformity with a striped border of a
  `repeating-linear-gradient` in its border box, a turned watermark, a transparent PNG stamp
  (`../images/stamp.png`) and a seal of rays masked to a ring blending with `mix-blend-mode:
  multiply` (as one group, its shadows over each other first), and a table with rounded corners;
- `timesheet-stacked-bars.html`: a time report on a landscape page with KPI tiles, a stacked bar
  chart in SVG, legend bars and a table of `tabular-nums` that breaks across pages;
- `voucher-clip-shapes.html`: gift vouchers on pages of 210 by 99mm cut with `clip-path`:
  `path()` for a wave, `polygon()` for hexagons, a star and an arrow tag, `circle()` for a photo
  and `inset(round)`, over linear, radial and conic gradients;
- `newsletter-pull-quotes.html`: a newsletter in two columns with headings and a box of dates that
  span them (`column-span: all`), pull quotes floated right and left inside the columns, a
  floated `::first-letter` and a figure that does not break;
- `press-kit-srcset.html`: a press kit of responsive images: `srcset` with density and width
  descriptors, `sizes` with media conditions, `calc()` and viewport units (of the 756 × 469
  window Chrome loads a page in), `<picture>` sources skipped by `media` and by an image `type`
  Fagus does not draw, `image-set()` and a background of a `min-resolution: 2dppx` query, which
  holds in print (300 dpi);
- `letterhead-svg-sprites.html`: a letterhead with SVG icons sized by CSS alone (`<svg
  class=icon><use href=#…>` without a `viewBox`), a `<use>` of a symbol in a sprite file,
  images of a sprite's `<view>`, `svgView()` and `:target` by the fragment of their URL, and
  SVG images, backgrounds and data URLs with gradients, patterns, masks and filters;
- `proof-image-formats.html`: a proof sheet of PNGs with and without `gAMA` chunks, a CMYK
  JPEG without a profile (Adobe's inverted CMYK, which Chrome draws as RGB), WebP, an animated
  GIF and JPEGs turned by their EXIF orientation, with `image-orientation: none` beside them;
- `catalog-thumbnails.html`: a catalogue of 200 product cards in a grid of five `1fr` columns
  (whose widths Chrome cuts to 1/64 pixel, carrying the rest to the next column) with square
  thumbnails (`aspect-ratio`, `object-fit: cover`) under a heading with a fixed background
  (`background-attachment: fixed`, positioned in the window from the corner of the page).

Books, reports and other long documents as authors write them for Chrome's print:

- `book-chapter-facing.html`: three chapters of a book on facing A5 pages, each starting on a
  right page (`break-before: right`, with a blank page of `@page :blank` before it), page
  numbers and running titles in the outer corners of `:left` and `:right` pages, an initial
  letter of three lines (`initial-letter: 3`) whose first line is in small capitals made up of
  Inter's capitals (`::first-line`, as Chrome splits the words into runs of their own), balanced
  headings and indented justified paragraphs;
- `annual-report-landscape-segments.html`: an annual report with a cover of a named page
  without margins, "Page x of y" in the margins, sections in two ruled columns and a table of
  48 operating units on a named landscape page (`size: A4 landscape`) with a caption above it
  and its header and footer repeated;
- `contents-dotted-leaders.html`: a table of contents on a named page with Roman page numbers,
  its leaders made by hand as Chrome has no `leader()`: dotted borders of flex items between
  title and number and a line of dots cut off behind a floated number; the entries link to the
  chapters, which make the document outline;
- `test-report-long-tables.html`: a German test report whose three long tables repeat their
  header and footer on every page, with captions above and below, striped rows and figures with
  captions that do not break;
- `index-three-columns.html`: a book index in three balanced columns with a heading that spans
  them, letter groups that keep with their entries and hanging sub-entries;
- `essay-endnotes-sidenotes.html`: an essay with a wide outer margin holding sidenotes placed
  absolutely beside their paragraphs and notes floated into it by a negative margin, and
  endnotes linked both ways (references and `↩` back links);
- `contract-paragraph-styles.html`: a German framework agreement whose clauses are numbered by a
  `@counter-style` of `§`, its paragraphs by counters in absolutely positioned `::before`, its
  lists by alphabetic and additive (`(iv)`) counter styles of their own, an annex numbered by
  `counters()` on three levels, a cyclic check mark and signature lines that do not break;
- `chapter-drop-caps.html`: drop caps in their variations: `initial-letter` of two, three and
  four lines, raised ones, one with a background and one in italics, a floated `::first-letter`, a
  larger one in the line and a `::first-line` in bold capitals with letter spacing, the
  initial letters sized from the cap height of the text at 10pt (set at 13.33 pixels, so the
  letter comes out a pixel smaller, as in Chrome) and the spaces after letters taken out of the
  line removed;
- `atlas-mixed-orientation.html`: portrait, landscape, rotated (`page-orientation:
  rotate-left`), Letter and A5 pages in one document by named pages, each with margin boxes of
  its own;
- `guide-blank-pages.html`: a field guide on A5 pages with sections that start on right, left,
  recto and verso pages and after `break-after: right`, the blank pages that they need, and a
  running title in an italic that the content does not use: Chrome has not loaded that face of
  the `@font-face` rules when it sets the page margins, and sets the title in the next family
  (with `--system-fonts` in Helvetica Oblique, as Chrome; without them, where no other family
  has a face, in Inter's italic after all);
- `rapport-footnotes-positioned.html`: a French report whose footnotes stand at the bottom of
  page-sized sheets, placed absolutely, beside floated boxes, with quotation marks from
  `quotes` whose spaces are removed where they end a line, so that the page is not shrunk;
- `novel-german-pretty.html`: a German novel on pages of 125 by 200mm whose chapters start on
  recto pages of a named page without a page number on their first one, with floated drop caps,
  `text-wrap: pretty` paragraphs (whose last lines Chrome breaks anew beside the drop cap and on
  the page a paragraph goes on to) and quotations in French and English;
- `notebook-jupyterlab.html`: a notebook exported by nbconvert's lab template: cells of
  `display: table` prompts beside Pygments listings, DataFrames with `border="1"` and
  `<style scoped>`, a header of two levels, a Matplotlib PNG as a data URL, an SVG figure with
  its XML declaration and doctype inside the body, stderr with ANSI colours and a traceback;
- `notebook-jupyter-classic.html`: a notebook exported by nbconvert's classic template, with the
  old flexbox helpers (`display: -webkit-box`, `box-orient`), a pandas Styler table of
  background gradients and bars of `linear-gradient` and a caption on top;
- `docs-sphinx-alabaster.html`: an API page as Sphinx builds it with Alabaster: a floated
  sidebar, a table of contents, signatures of `span.pre`, field lists in a grid of
  `fit-content(30%)`, a listing with line numbers in a flex row, admonitions whose titles get
  their colons from `::after`, a table of `<colgroup>` widths and footnotes of floated labels;
  the `¶` links are hidden, and bookmarks leave them out, as in Chrome;
- `docs-docusaurus.html`: a page of Docusaurus (Infima): breadcrumbs with an SVG data URL as
  separator, admonitions with icons, tabs, a code block with a title, line numbers from counters
  and highlighted lines, `<details>` open and closed, and heading anchors of `opacity: 0` whose
  `#` and links Chrome leaves out of the PDF;
- `page-notion-export.html`: a page in the style of a Notion HTML export, with the export's
  markup and class names and a stylesheet of its own (no doctype, `white-space: pre-wrap` on the
  body): a property table with inline SVG icons, callouts with emoji, to-do lists with
  checkboxes of SVG data URLs whose sizes have no unit, a column list, a simple table, a toggle,
  a bookmark card and colours of selects;
- `sheet-libreoffice-calc.html`: two sheets saved as HTML by LibreOffice Calc (HTML 4.0
  Transitional): `<colgroup width>`, `sdval` and `sdnum` attributes, cells that span rows and
  columns, `bgcolor`, `<font>`, a double bottom border, a comment and an overview whose links lead
  to `<a name>` around the sheets' headings, which have no box of their own;
- `mail-outlook-thread.html`: an e-mail thread printed from Outlook: `MsoNormal` paragraphs,
  `mso-` properties, `<![if !supportLists]>` bullets, a VML logo with its image fallback, a
  signature table with a coloured rule and the replies under rules of `#E1E1E1`, on a named page
  of US Letter (`WordSection1`);
- `mail-thunderbird-quotes.html`: a message as Thunderbird prints it (its markup and `moz-`
  classes, with a stylesheet of our own): header tables, flowed text in `pre-wrap`, a signature
  and four levels of quotes in `blockquote type=cite`, each level with its own colour for the
  rule (`border-inline-start`) and the text, a plain-text table in `pre`, an attachment list and
  page margin boxes;
- `issue-github-timeline.html`: a GitHub issue: a grid of the conversation and a sidebar, a
  timeline whose line is a `::before`, comments with carets cut by `clip-path`, labels whose
  colours GitHub computes from `--label-r`/`-g`/`-b` with `calc()` in `hsl()`, a suggested change
  as a diff table with `attr()` line numbers, a task list and reactions;
- `notes-obsidian-mermaid.html`: a note as Obsidian's "Export to PDF" writes it (its markup and
  Lucide icons, with a stylesheet of our own): callouts tinted with `rgba(var(--tone), .1)`
  (the colour kept as bare r, g, b in a custom property) and `mix-blend-mode: multiply`, nested
  ones, a Mermaid flowchart whose labels are HTML in `<foreignObject>` (moved to whole pixels
  with the SVG, as Chrome does), checkboxes of `appearance: none` whose colour ends halfway into
  their rounded border, as in Chrome, tags and footnotes;
- `notes-typora-mermaid.html`: a document as Typora exports it with its GitHub theme (its
  markup, with a stylesheet of our own): a table of contents, a Mermaid diagram with SVG text
  labels inside a `<pre>`, a CodeMirror listing whose measuring elements are hidden, a table in
  a `<figure>`, `<kbd>`, a task list and footnotes;
- `reference-redoc.html`: an OpenAPI reference as Redoc renders it, in print: a property tree
  drawn with gradients, `::before` and `::after`, nested schemas, coloured response rows, an
  endpoint bar and JSON samples on a dark panel, and sections of `min-height: calc(100vh + 1px)`
  whose rule at the end is positioned absolutely.
- `invoice-zugferd-long-positions.html`: a ZUGFeRD-style invoice of 85 positions over five
  pages, with a repeated header row, a footer row saying the list continues, zebra rows, rows
  that do not break, and "Seite x von y" and the invoice number in the page margins;
- `invoice-din5008-fold-marks.html`: a German invoice with a DIN 5008 address window and
  information block placed absolutely on a first page without a top margin, fixed fold marks
  and footer, a GiroCode in SVG floated beside the text, and a letterhead whose letter-spaced
  name kerns across its coloured middle dot;
- `credit-note-tailwind4-oklch.html`: a credit note in Tailwind CSS v4 utilities: `oklch()`
  colours, `color-mix()` for translucent borders and backgrounds, a gradient `in oklab`, ring
  and shadow stacks of registered custom properties, `divide-y`, `odd:` rows, a `print:`
  variant nested in its rule, and tabular figures;
- `quote-bootstrap5-terms-columns.html`: a quote in Bootstrap 5.3 (rows, cards, striped tables,
  badges) with terms in two justified columns with a rule, headings kept with their text, and a
  signature block that does not break;
- `statement-apexcharts-svg.html`: a monthly account statement with KPI cards in an
  `auto-fit` grid, a smoothed area chart, a donut of dashed circles and grouped bars in inline
  SVG as chart libraries draw them, a `@media print` override, and a long table with sticky
  header cells and rounded corners;
- `packing-slip-svg-barcodes.html`: an A5 packing slip with Code 128 barcodes as inline SVG, as
  SVG data URIs and as a one-pixel-high PNG scaled with `image-rendering: pixelated`, checked
  boxes, quantities framed by outlines inside their cells (`outline-offset` below zero, with
  collapsed borders), and a rotated stamp;
- `labels-shipping-vertical-notes.html`: shipping labels of 105 × 148 mm on pages without
  margins, with routing codes, a QR code in a padded SVG, a hazard sign of repeating gradients,
  vertical notes placed absolutely at `top: 50%` (whose lines are as long as the room below),
  and a customs declaration on a landscape named page;
- `certificate-fullbleed-seal.html`: a certificate on a landscape page without margins: layered
  radial gradients, a `border-image` of a repeating gradient, mirrored SVG corners, a ribbon, a
  seal of a conic gradient with box shadows as rings, and a watermark letter;
- `tickets-masked-notches.html`: event tickets three to a page, notched by masks of radial
  gradients, with headings clipped to a gradient (`background-clip: text`), a perforation, grid
  areas, QR codes and a vertical stub text;
- `payslip-dense-rowspans.html`: a German payslip of dense tables: captions, cells that span
  rows and columns, tabular figures, a hatched empty cell, a total in a double border and
  units from `attr()`;
- `timesheet-sticky-weeks.html`: a monthly timesheet on landscape pages with a fixed table
  layout, day cells spanning their entries, weekly subtotals, sticky header cells, slashed
  zeros and a screen-only scroll box that print undoes;
- `letter-margin-boxes-running-footer.html`: an offer letter with margin boxes for the header,
  the date and "Seite x von y", left pages that move the page number, a first page without the
  header, and a running footer (`position: running()` and `element()`, which Chrome does not
  support and prints in the flow);
- `invoice-ninja-clean.html`: an invoice in the style of Invoice Ninja's "Clean" template:
  `zoom: 80%`, a gradient bar fixed to the top of the page, a grid header with an inline SVG
  logo, a table of the invoice's number, dates and balance, a product table with a fixed
  layout and a long URL, notes with `white-space: pre-line` and a totals grid of a `dl`;
- `report-print-media-meters.html`: a dark dashboard that `@media print` turns light: KPI
  cards with `<meter>` and `<progress>`, a bar chart of flex items with percentage heights, a
  donut of a conic gradient, and a table whose account names give up their width
  (`max-width: 0` with `text-overflow: ellipsis`) to a percentage column;
- `delivery-note-returns-a5.html`: a delivery note with a repeated table header and footer,
  justified terms in two columns under a heading that spans them, and a returns slip on an A5
  landscape named page;
- `contract-nested-clauses-de.html`: a German service contract: clauses numbered `§ n` with
  `n.m` and `n.m.k` items from three counters, hanging indents with a negative `text-indent`,
  `::marker` content in lettered lists, justified text with soft hyphens, page numbers and
  initials in the page margins, a signature table and an annex on a page of its own;
- `terms-marker-counters.html`: terms of service on US Letter numbered by `counters(list-item,
  ".")` in `::marker`, upper-case disclaimers, a floated initials box, a callout and "Page x of
  y" in the margin boxes;
- `cv-grid-sidebar-skill-bars.html`: a full-bleed résumé: a grid with a tinted sidebar, a round
  photo (`object-fit: cover`), inline SVG icons, gradient skill bars, dots for languages and
  tags, letter-spaced upper-case headings;
- `cv-timeline-signature-de.html`: a German CV with a timeline drawn by a border and `::before`
  dots, dates positioned beside it, a photo with rounded corners, `h2::after` rules in flex and
  an SVG signature;
- `paper-two-columns-figures.html`: a paper in two columns with a title and a figure spanning
  them, numbered sections, figures and tables from counters, equations in inline SVG, numbered
  references in `::marker` and notes in two columns;
- `report-toc-target-counter-mathml.html`: a technical report with a cover, a table of contents
  whose page numbers come from `target-counter()` after dotted leaders drawn as borders,
  numbered display equations in MathML and a chart in SVG;
- `form-membership-fieldsets.html`: a German membership form: fieldsets with legends, a grid of
  labelled text, date, number and e-mail fields, selects, text areas, radio buttons and check
  boxes checked and not, an IBAN in boxes of single-letter fields and a progress bar;
- `form-survey-listboxes.html`: a survey: Likert scales, a matrix of radio buttons, custom check
  boxes and radio buttons of `appearance: none`, list boxes with selected options (`size`,
  `multiple`, a height), progress bars, meters, a range and buttons;
- `statutes-justified-de.html`: statutes of an association over twenty A5 pages: justified
  text with soft hyphens, „…“ quotes and no-break spaces, numbered paragraphs whose `(n)` is
  generated content, a table of contents and signatures;
- `manual-justified-en.html`: the English counterpart, a user manual of fifteen A5 pages in
  justified text;
- `agreement-margin-initials.html`: a services agreement with a full-bleed cover on a page of
  its own (named page without margin boxes), clauses with hanging numbers, definitions in a
  grid, a striped service level table and a signature table with seals;
- `form-tax-comb-fields.html`: a tax form: boxes for each letter, a fixed table of numbered
  lines with codes and amounts, check boxes, a note in two ruled columns and a barcode made of
  a repeating gradient;
- `cv-grid-areas-weights.html`: a résumé in grid template areas with four weights of one
  family, tracked upper-case labels, small caps, meters and chips;
- `article-floats-endnotes-de.html`: a German article with figures floated left and right
  beside justified text, footnote numbers from counters in `::after` of links, a table with its
  caption below, MathML formulas inline and on their own, and endnotes.

The texts, names and figures are made up. Otherwise the only font is Inter (`../../fonts`, and
`../../webfonts` for the web fonts); weights without a face of their own fall back to the
nearest one, as in browsers. The catalogue's, the programme's, the nested-table invoice's, the
status report's, the timeline CVs' and the sidebar résumé's photo is `../images/photo.jpg`; the
QR codes are made-up patterns. The images in `../images/media` are generated (gradients and shapes drawn for these
documents).
