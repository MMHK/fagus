# The PDF

This page describes the PDF file Fagus writes and the options that change it: metadata, links and
bookmarks, tagged and accessible PDF, PDF/A, attachments, electronic invoices and passwords. How the pages
look is up to the HTML and CSS (see [css.md](css.md) and [paged-media.md](paged-media.md)); the
methods of the document builder are described in [php.md](php.md).

Every PDF Fagus writes is a PDF 1.7 file (PDF 1.4 for PDF/A-1b, PDF 2.0 for PDF/A-4) with:

- the fonts the text uses, embedded as subsets;
- the title, language, author, subject and keywords of the document;
- links and the document outline (bookmarks) from the headings;
- the logical structure for screen readers (tagged PDF), unless you turn it off;
- compressed streams and object streams (not in PDF/A-1b, which PDF 1.4 lacks them for).

```php
use Fagus\Conformance;
use Fagus\Fagus;

$pdf = Fagus::html($html)
    ->pdfA(Conformance::PdfA3b)                  // PDF/A
    ->accessible()                               // PDF/UA-1
    ->attach('timesheet.csv', $csv, 'text/csv', description: 'Timesheet')  // attachments
    ->facturX($invoiceXml)                       // ZUGFeRD / Factur-X
    ->created(new DateTimeImmutable('2026-11-01 09:00'))  // reproducible output
    ->render();
```

## Set the title, language and other metadata

Fagus takes the metadata from the document, as a browser does. There is no method to set it
separately: write it into the HTML.

| PDF field | Comes from |
|---|---|
| Title | `<title>`, its white space collapsed |
| Language (`Lang`) | `<html lang>`, else the engine's `locale` (see [php.md](php.md)) |
| Author | `<meta name="author" content="…">` |
| Subject | `<meta name="description" content="…">` |
| Keywords | `<meta name="keywords" content="…">`, split at commas |
| Producer | `Fagus` |
| Creation and modification date | `created()`, else the current time |

```html
<html lang="de">
<head>
  <title>Rechnung RE-2026-0042</title>
  <meta name="author" content="Muster GmbH">
  <meta name="description" content="Rechnung RE-2026-0042 an Kunde AG">
  <meta name="keywords" content="Rechnung, Webentwicklung">
</head>
```

The same values go into the document information dictionary and the XMP metadata. With a title,
the PDF asks viewers to show the title instead of the file name (`DisplayDocTitle`), as Chrome's
PDFs do.

## Links

An `<a href>` becomes a link in the PDF when it leads to:

- an element of the document: `href="#total"` leads to the element with `id="total"` or to an
  `<a name="total">`; `href="#"` and `href="#top"` lead to the top of the first page;
- a URL with the scheme `http:`, `https:`, `mailto:`, `tel:` or `ftp:`.

Other links are dropped: relative URLs (`page.html`), `javascript:`, `file:` and any other scheme.
A web address without a path gets one (`https://example.com` becomes `https://example.com/`), as
in the URLs Chrome writes.

Each inline box of a link gets its own link area, and so do images and inline blocks inside a
link. Links in margin boxes (headers and footers) work on every page. Links that are hidden or
fully transparent make no link area.

Page numbers of link targets, such as a table of contents with `target-counter()` and `leader()`,
are described in [paged-media.md](paged-media.md).

## Control the document outline

The document outline, the bookmarks a PDF viewer shows beside the pages, comes from the headings.
The default style sheet gives `<h1>` to `<h6>` the levels 1 to 6; an element with
`role="heading"` is an entry at its `aria-level` (2 without one), as in Chrome, unless its
`bookmark-level` is set: `bookmark-level: none` leaves it out too. An entry's title
is the heading's text as it is shown: hidden text is left out and `text-transform` applies.
Entries point at the top left of their element.

Three properties from CSS Generated Content for Paged Media change the outline:

| Property | Values | Effect |
|---|---|---|
| `bookmark-level` | `none` or an integer from 1 | Makes an element an entry at that level, or none |
| `bookmark-label` | `content(text)` (initial), strings, `attr()` | The title of the entry |
| `bookmark-state` | `open` (initial), `closed` | Whether the entry shows its children |

```css
/* Chapters are entries, their headings are not. */
section.chapter { bookmark-level: 1; bookmark-label: "Chapter " attr(data-number); }
section.chapter h1 { bookmark-level: none; }

/* Leave the invoice's headings out of the outline. */
.invoice h2, .invoice h3 { bookmark-level: none; }

/* Show appendices collapsed. */
.appendix h1 { bookmark-state: closed; }
```

Deeper levels nest under the entry before them. Elements in margin boxes make no entries, and an
entry with an empty title is left out. A document without entries has no outline, except in
PDF/UA, which asks for one.

## Tagged PDF

A tagged PDF contains the logical structure of the document: what is a heading, a paragraph, a
list or a table, in reading order. Screen readers read it, and PDF viewers use it to reflow text
and to copy tables. Fagus tags every PDF by default.

What Fagus tags:

- headings (`<h1>` to `<h6>`, `role="heading"`) and paragraphs;
- lists with their labels (the markers) and bodies;
- footnotes as notes with their markers as labels, after the text that calls them (see
  [Paged media](paged-media.md#footnotes));
- data tables, with header cells and their scope, row and column spans, captions, and header and
  footer groups; a header repeated on every page counts once;
- images (`<img>`, inline `<svg>`) as figures with their alternative text, MathML formulas with
  their `alttext` (or TeX annotation, or text);
- links, with their annotations and a description (their `aria-label`, `title`, text, or the
  `alt` of their images);
- `<blockquote>`, `<q>`, `<article>`, `<section>`, `<figcaption>`, `<code>`, `<kbd>`, `<samp>`,
  `<em>`, `<strong>`, and `<abbr title>` with its expansion;
- changes of language (`lang` on an element);
- other blocks: a paragraph when they hold only text, a division when they hold blocks.

What Fagus marks as artifacts, content a reader skips:

- backgrounds, borders and other decoration;
- margin boxes (headers, footers, page numbers) and running elements, whatever they hold: keep
  content that a reader needs in the page area (see
  [Paged media](paged-media.md#fill-the-whole-page));
- the copies of repeated content: fixed boxes on every page after the first, table headers and
  footers repeated on later pages;
- layout tables: tables without `<th>`, `<caption>`, `<thead>`, `<tfoot>`, `summary` or a
  `role` of `table` or `grid` are divisions, their cells are not table cells;
- images with `alt=""` or `role="presentation"` (or `none`), and elements with
  `aria-hidden="true"`.

### Turn tagging off

A PDF without tags is somewhat smaller and faster to write, which shows with documents that have
many table cells. Turn tagging off with `tagged(false)`:

```php
Fagus::html($html)->tagged(false)->save('report.pdf');
```

In Laravel, the configuration key `tagged` sets the default for all documents. On the command
line, the flag is `--untagged`. `tagged(false)` has no effect when the PDF must be tagged: with
`accessible()` and with PDF/A-2a and PDF/A-3a.

## Make an accessible PDF (PDF/UA-1)

`accessible()` writes a PDF that conforms to PDF/UA-1 (ISO 14289-1), the standard for accessible
PDF. The PDF is tagged, has an outline, and Fagus checks what the standard requires of the
document itself:

- a title: a `<title>` with text;
- a language: `<html lang>`, or the engine's `locale`;
- alternative text for every image: `alt` (or `aria-label`) on `<img>`, a `<title>` or
  `aria-label` in an inline `<svg>`; images that are decoration take `alt=""`, `role="presentation"`
  or `aria-hidden="true"`;
- headings that start at `<h1>` and skip no level on the way down (an `<h3>` may follow an
  `<h1>` only after an `<h2>`).

```php
use Fagus\Exception\RenderException;
use Fagus\Fagus;

try {
    $pdf = Fagus::html($html)->accessible()->render();
} catch (RenderException $exception) {
    // the document does not meet PDF/UA-1: the document has no language (add `lang` to
    // <html>); the image `logo.png` in line 12 has no alternative text (add `alt`, empty if it
    // is decoration)
    echo $exception->getMessage();
}
```

A document that does not meet the standard is refused: rendering fails with a `RenderException`
whose message lists every problem, separated by semicolons. The messages are:

| Problem | Message |
|---|---|
| No title | `the document has no title (add a <title>)` |
| No language | ``the document has no language (add `lang` to <html>)`` |
| Image without alternative text | ``the image `logo.png` in line 12 has no alternative text (add `alt`, empty if it is decoration)`` |
| SVG without alternative text | ``the SVG in line 30 has no alternative text (add a <title> or `aria-label`, or `aria-hidden="true"` if it is decoration)`` |
| Formula without alternative text | ``the formula in line 41 has no alternative text (add `alttext`)`` |
| First heading not an `<h1>` | `the first heading, an <h2> in line 5, is not an <h1> (headings start at h1)` |
| Skipped heading level | `the <h3> in line 9 follows an <h1> (headings skip no level: use an <h2>)` |

The PDF itself is checked as it is written, for PDF/UA as for PDF/A. These problems end in the
same exception:

- ``Inter has no glyph for `☃`, and no other font has one``: a character that no font has, which
  other PDFs show as an empty box, with the warning `font.missing-glyph`. Add a font that has it
  (see [fonts.md](fonts.md)).
- `text in … cannot be mapped to Unicode` and `… shows U+E000 from the private use area, which
  has no meaning`: glyphs without a meaning as text, as in icon fonts.
- `the license of … does not allow embedding it`: the font's license forbids embedding it.
- Attached files that do not qualify (see [Attach files](#attach-files)), for example ``the
  attached file `notes` has no valid MIME type (like `text/csv`), which PDF/A-3 requires``.

`accessible()` works together with PDF/A and with electronic invoices:

```php
Fagus::html($html)->pdfA(Conformance::PdfA3a)->accessible()->facturX($xml)->save('invoice.pdf');
```

## Write PDF/A

PDF/A (ISO 19005) is the standard for archiving. Fagus writes PDF/A-1b, PDF/A-2 and PDF/A-3 in
their levels b, u and a, and PDF/A-4 with its levels e and f:

| Level | `Conformance` | Name | Requires |
|---|---|---|---|
| PDF/A-1b | `PdfA1b` | `'1b'` | PDF 1.4, no transparency, fonts embedded, colors defined |
| PDF/A-2b | `PdfA2b` | `'2b'` | a reliable appearance: fonts embedded, colors defined |
| PDF/A-2u | `PdfA2u` | `'2u'` | 2b, and all text mapped to Unicode |
| PDF/A-2a | `PdfA2a` | `'2a'` | 2u, tagged, with a language and alternative text for images |
| PDF/A-3b | `PdfA3b` | `'3b'` | like 2b, and allows attachments of any type |
| PDF/A-3u | `PdfA3u` | `'3u'` | 3b, and all text mapped to Unicode |
| PDF/A-3a | `PdfA3a` | `'3a'` | 3u, tagged, with a language and alternative text for images |
| PDF/A-4 | `PdfA4` | `'4'` | PDF 2.0, all text mapped to Unicode |
| PDF/A-4e | `PdfA4e` | `'4e'` | 4, for engineering documents (allows 3D content, which Fagus does not write) |
| PDF/A-4f | `PdfA4f` | `'4f'` | 4, and allows attachments of any type |

```php
use Fagus\Conformance;
use Fagus\Fagus;

Fagus::html($html)->pdfA(Conformance::PdfA2b)->save('archive.pdf');
Fagus::html($html)->pdfA('3a')->save('archive.pdf');  // the name, in any case
Fagus::html($html)->pdfA()->save('archive.pdf');      // PDF/A-3b
```

A name that is not a level throws a `Fagus\Exception\InvalidArgumentException` that lists the
levels. On the command line, the flag is `--pdfa 3b`.

Which level to choose: PDF/A-2b or PDF/A-3b where an archive only asks for PDF/A, PDF/A-3 for
attachments and invoices, PDF/A-1b only for an archive that accepts nothing newer, and PDF/A-4
for one that asks for PDF 2.0.

What changes in a PDF/A file:

- The XMP metadata names the part and level (`pdfaid`).
- An sRGB ICC profile is the output intent, and colors are written in that color space. Without
  PDF/A, colors are plain RGB.
- Every character must have a glyph in a font. A character no font has (shown as an empty box
  in other PDFs) makes the document fail: ``Inter has no glyph for `☃`, and no other font has
  one``. The other checks of the written file are listed under PDF/UA above.
- Transparency, blend modes and masks are kept: PDF/A-2, PDF/A-3 and PDF/A-4 allow them.
  PDF/A-1 does not; see below.
- What would need a PostScript function, which PDF/A forbids, is drawn another way: conic
  gradients become images, and repeating linear gradients are written out period by period (one
  with more than 2000 periods in its box becomes its average color). Blurred shadows are images
  that viewers must not smooth.
- PDF/A-2 allows only PDF/A files as attachments, which Fagus does not check, so it refuses
  every attachment: ``PDF/A-2 allows only PDF/A files as attachments; attach `timesheet.csv` to a
  PDF/A-3 document instead``. Use PDF/A-3, which requires a description for each attached file
  (see below). PDF/A-4 refuses them the same way (attach them to PDF/A-4f), and PDF/A-1 allows
  none.
- The levels a are tagged whatever `tagged()` says, and fail without a language or with an
  image without alternative text.

A document that does not meet the level is refused with a `RenderException`, as with PDF/UA:

```text
the document does not meet PDF/A-3a: an image has no alternative text (add `alt`, empty for
decoration); the document has no language (add `lang` to <html>)
```

### PDF/A-1b

PDF/A-1 (ISO 19005-1) is based on PDF 1.4. The file has a classic cross-reference table and no
object streams, so it is somewhat larger. PDF/A-1 forbids transparency: no `opacity` below 1, no
translucent colors, no images with transparent pixels, no blend modes, masks or soft shadows.
Fagus draws what it can without transparency and refuses the rest:

- Content that does not show (`opacity: 0`, a clip that encloses nothing) is clipped away instead
  of drawn transparent.
- `isolation` changes nothing where nothing is transparent, and is left out.
- A PNG, GIF or WebP image with an alpha channel whose pixels are all opaque is embedded without
  that channel. GIF images always have one in the PDF otherwise.
- Everything else that is transparent is refused. Fagus does not flatten it onto what lies below
  (which would mean drawing that, text included, as pixels) and does not leave it out.

The message names what is transparent and the first page where it is:

```text
the document does not meet PDF/A-1b: PDF/A-1 forbids transparency, which the document uses for
`opacity` (first on page 1), translucent colors (first on page 2) and images with transparent
pixels (first on page 2); PDF/A-2b and later allow it
```

The causes are `opacity`, blend modes (`mix-blend-mode`, `background-blend-mode`), masks
(`mask`), translucent colors (also in borders and backgrounds), text in translucent colors or
color fonts (emoji), gradients with translucent colors, images with transparent pixels, SVG
images with transparency, shadows (`box-shadow`, `text-shadow`; a blurred shadow always has
transparent pixels) and filters (`filter`) drawn as pixels. Replace them with opaque colors and
images, or write PDF/A-2b, which keeps them.

PDF/A-1b goes with neither attachments (PDF/A-1 allows none) nor `accessible()`: PDF/UA-1 builds
on PDF 1.7, and the PDF is refused with ``PDF/A-1b and PDF/UA-1 need different PDF versions and
cannot be written together (PDF/UA-1 goes with PDF/A-2 and PDF/A-3)``. A Factur-X invoice is
raised to PDF/A-3b (see below). The PDF is tagged unless `tagged(false)`; Fagus writes no PDF/A-1a.

### PDF/A-4

PDF/A-4 (ISO 19005-4:2020) is based on PDF 2.0. The file starts with `%PDF-2.0`, and the metadata
is only in XMP: PDF 2.0 deprecates the document information dictionary, which PDF/A-4 forbids, so
the title, author and dates are XMP properties alone. The XMP names `pdfaid:part` 4 and
`pdfaid:rev` 2020, and `pdfaid:conformance` only for the levels e (`E`) and f (`F`). PDF/A-4 has
no levels b, u or a: all text must be mapped to Unicode, and tagging is recommended (Fagus tags
unless `tagged(false)`). As with the levels a, characters from the private use area (the icons of
icon fonts) are refused, since they have no meaning as text.

- PDF/A-4f allows attachments of any type, like PDF/A-3. The plain level allows only PDF/A files,
  which Fagus does not check, and refuses attachments: ``PDF/A-4 allows only PDF/A files as
  attachments; attach `timesheet.csv` to a PDF/A-4f document instead``.
- PDF/A-4e is meant for engineering documents with 3D and rich media, which Fagus does not
  write: the file is a PDF/A-4 file that says it is 4e.
- Factur-X 1.07 (ZUGFeRD 2.3) requires PDF/A-3, so an invoice is not written as PDF/A-4, not even
  PDF/A-4f: `facturX()` with a PDF/A-4 level fails with ``the invoice must be PDF/A-3, not
  PDF/A-4f (choose 3b, 3u or 3a)``.
- PDF/UA-1, which `accessible()` writes, builds on PDF 1.7. PDF/UA-2, which goes with PDF 2.0,
  Fagus does not write yet, so `accessible()` with PDF/A-4 fails with ``PDF/A-4 and PDF/UA-1 need
  different PDF versions and cannot be written together (PDF/UA-1 goes with PDF/A-2 and
  PDF/A-3)``. For an accessible archive, use PDF/A-2a or PDF/A-3a with `accessible()`.

## Attach files

`attach()` adds a file to the PDF, which PDF viewers list in their attachments panel:

```php
use Fagus\Fagus;
use Fagus\Relationship;

Fagus::html($html)
    ->pdfA('3b')
    ->attach('timesheet.csv', $csv, 'text/csv', Relationship::Supplement, 'Stundenzettel')
    ->save('invoice.pdf');
```

| Argument | Default | Meaning |
|---|---|---|
| `$name` | | The file name in the PDF |
| `$data` | | The content |
| `$mime` | `'application/octet-stream'` | The MIME type |
| `$relationship` | `Relationship::Unspecified` | How the file relates to the document |
| `$description` | `null` | A description viewers show; the file name if `null` |

The relationship (PDF 2.0 `AFRelationship`, required by PDF/A-3) is one of `Source` (the file
the document was made from), `Data` (data the document shows), `Alternative` (another form of the
same content), `Supplement` (additional information) or `Unspecified`. The file's modification
date is the document's creation date.

PDF/A-3 and PDF/UA require a description for every attached file. Without `$description`, the
file name describes it, in PHP, in Laravel and on the command line alike.
PDF/A-3 also requires a MIME type of the form `type/subtype`; ``the attached file `notes` has no
valid MIME type (like `text/csv`), which PDF/A-3 requires`` names a file without one.

Attachments work in plain PDF, in PDF/A-3 and in PDF/A-4f, not in PDF/A-1, PDF/A-2 and PDF/A-4. On the command line, `--attach
file` attaches a file as a supplement, with its name as description and a MIME type from its
extension (see [cli.md](cli.md)).

## Make an electronic invoice (ZUGFeRD, Factur-X, XRechnung)

ZUGFeRD 2 and Factur-X are the same standard for hybrid invoices: a PDF/A-3 file that people read,
with the invoice as XML (UN/CEFACT Cross Industry Invoice) attached for machines. `facturX()`
makes such a file from your HTML and the XML your application writes:

```php
Fagus::html($html)->facturX($invoiceXml)->save('invoice.pdf');
```

Fagus then:

- attaches the XML as `factur-x.xml` (as `xrechnung.xml` for the XRechnung profile), with the MIME
  type `text/xml`, the description "Factur-X/ZUGFeRD invoice" (unless you give another, see
  below) and the relationship `Alternative` (`Data` for the profiles Minimum and Basic WL, whose
  XML is not a complete invoice);
- writes the Factur-X properties into the XMP metadata, with their PDF/A extension schema:
  `DocumentType` `INVOICE`, `DocumentFileName`, `Version` (`1.0` unless you give another) and
  `ConformanceLevel`;
- writes PDF/A-3b, or the PDF/A-3 level you chose with `pdfA()`. A PDF/A-2 level is raised to
  the PDF/A-3 level of the same letter, PDF/A-1b to PDF/A-3b, with the warning
  `pdf.conformance-raised` ("PDF/A-2b is written as PDF/A-3b because Factur-X attaches its XML,
  which only PDF/A-3 allows"). A PDF/A-4 level is refused: Factur-X 1.07 requires PDF/A-3 (see
  [PDF/A-4](#pdfa-4)).

### The profile

The profile is read from the XML, from the `ID` in its `GuidelineSpecifiedDocumentContextParameter`:

| `InvoiceProfile` | `ConformanceLevel` | Guideline ID |
|---|---|---|
| `Minimum` | `MINIMUM` | ends in `minimum` |
| `BasicWl` | `BASIC WL` | ends in `basicwl` |
| `Basic` | `BASIC` | ends in `basic` |
| `En16931` | `EN 16931` | starts with `urn:cen.eu:en16931:2017` |
| `Extended` | `EXTENDED` | ends in `extended` |
| `XRechnung` | `XRECHNUNG` | contains `xrechnung` |

An XML that declares none is treated as EN 16931, with the warning `invoice.profile-unknown`. You
can name the profile yourself; if the XML declares another one, the warning
`invoice.profile-mismatch` says so:

```php
use Fagus\InvoiceProfile;

Fagus::html($html)->facturX($xml, InvoiceProfile::XRechnung)->save('invoice.pdf');
```

### The version and the description

`version` sets the `Version` of the XMP metadata and `description` the description of the
attached XML:

```php
Fagus::html($html)
    ->facturX($xml, InvoiceProfile::XRechnung, version: '3.0', description: 'XRechnung Invoice')
    ->save('invoice.pdf');
```

For the XRechnung profile, the Factur-X specification (1.07.2, which is ZUGFeRD 2.3.2, section
7.7) describes `Version` as the major and minor version of the invoice data specification, that is
of XRechnung. Fagus does not derive it from the XML: it writes `1.0` unless you give a version.

An empty version or description is refused: `facturX()` throws an `InvalidArgumentException`. One
with control characters fails at rendering ("the document does not meet Factur-X: the invoice
description contains control characters"). On the command line the options are
`--factur-x-version` and `--factur-x-description`, in Rust `factur_x_version()` and
`factur_x_description()`.

Fagus does not check the content of the XML, and it does not compare it with the HTML. Your
application writes both from the same data; validate the result as described below.

### A complete example

This renders an invoice and its XML to a PDF/A-3b file that veraPDF and Mustang accept. The XML
is [tests/corpus/invoice/factur-x.xml](../tests/corpus/invoice/factur-x.xml), an EN 16931 invoice.

`invoice.html`:

```html
<!doctype html>
<html lang="de">
<head>
  <meta charset="utf-8">
  <title>Rechnung RE-2026-0042</title>
  <meta name="author" content="Muster GmbH">
  <meta name="description" content="Rechnung RE-2026-0042 an Kunde AG">
  <style>
    @page {
      size: A4;
      margin: 20mm 20mm 25mm;
      @bottom-right { content: "Seite " counter(page) " von " counter(pages); font: 9pt sans-serif; }
    }
    body { font: 10pt/1.4 sans-serif; color: #1a1a1a; }
    h1 { font-size: 18pt; margin: 0 0 8mm; }
    .parties { display: flex; justify-content: space-between; margin-bottom: 10mm; }
    table { width: 100%; border-collapse: collapse; }
    th, td { padding: 2mm 0; border-bottom: 0.5pt solid #ccc; text-align: left; }
    .amount { text-align: right; }
    tfoot td { border: 0; }
    tfoot tr:last-child td { font-weight: bold; border-top: 1pt solid #1a1a1a; }
  </style>
</head>
<body>
  <h1>Rechnung RE-2026-0042</h1>
  <div class="parties">
    <p>Muster GmbH<br>Hauptstraße 1<br>12345 Musterstadt</p>
    <p>Kunde AG<br>Bahnhofstraße 5<br>54321 Beispielstadt</p>
  </div>
  <p>Rechnungsdatum: 01.11.2026 · Leistungsdatum: 30.09.2026 · Fällig: 15.11.2026</p>
  <table>
    <thead>
      <tr><th>Pos.</th><th>Leistung</th><th class="amount">Menge</th><th class="amount">Preis</th><th class="amount">Betrag</th></tr>
    </thead>
    <tbody>
      <tr><td>1</td><td>Webentwicklung Sprint 1</td><td class="amount">2 h</td><td class="amount">95,00 €</td><td class="amount">190,00 €</td></tr>
      <tr><td>2</td><td>Webentwicklung Sprint 2</td><td class="amount">2 h</td><td class="amount">95,00 €</td><td class="amount">190,00 €</td></tr>
      <tr><td>3</td><td>Webentwicklung Sprint 3</td><td class="amount">2 h</td><td class="amount">95,00 €</td><td class="amount">190,00 €</td></tr>
    </tbody>
    <tfoot>
      <tr><td colspan="4">Nettobetrag</td><td class="amount">570,00 €</td></tr>
      <tr><td colspan="4">Umsatzsteuer 19 %</td><td class="amount">108,30 €</td></tr>
      <tr><td colspan="4">Gesamtbetrag</td><td class="amount">678,30 €</td></tr>
    </tfoot>
  </table>
  <p>Bitte überweisen Sie den Betrag bis zum 15.11.2026 auf DE02 1203 0000 0000 2020 51.</p>
</body>
</html>
```

<details>
<summary><code>invoice.xml</code></summary>

```xml
<?xml version="1.0" encoding="UTF-8"?>
<rsm:CrossIndustryInvoice xmlns:rsm="urn:un:unece:uncefact:data:standard:CrossIndustryInvoice:100"
  xmlns:ram="urn:un:unece:uncefact:data:standard:ReusableAggregateBusinessInformationEntity:100"
  xmlns:udt="urn:un:unece:uncefact:data:standard:UnqualifiedDataType:100">
  <rsm:ExchangedDocumentContext>
    <ram:GuidelineSpecifiedDocumentContextParameter>
      <ram:ID>urn:cen.eu:en16931:2017</ram:ID>
    </ram:GuidelineSpecifiedDocumentContextParameter>
  </rsm:ExchangedDocumentContext>
  <rsm:ExchangedDocument>
    <ram:ID>RE-2026-0042</ram:ID>
    <ram:TypeCode>380</ram:TypeCode>
    <ram:IssueDateTime><udt:DateTimeString format="102">20261101</udt:DateTimeString></ram:IssueDateTime>
  </rsm:ExchangedDocument>
  <rsm:SupplyChainTradeTransaction>
    <ram:IncludedSupplyChainTradeLineItem>
      <ram:AssociatedDocumentLineDocument><ram:LineID>1</ram:LineID></ram:AssociatedDocumentLineDocument>
      <ram:SpecifiedTradeProduct><ram:Name>Webentwicklung Sprint 1</ram:Name></ram:SpecifiedTradeProduct>
      <ram:SpecifiedLineTradeAgreement><ram:NetPriceProductTradePrice><ram:ChargeAmount>95.00</ram:ChargeAmount></ram:NetPriceProductTradePrice></ram:SpecifiedLineTradeAgreement>
      <ram:SpecifiedLineTradeDelivery><ram:BilledQuantity unitCode="HUR">2</ram:BilledQuantity></ram:SpecifiedLineTradeDelivery>
      <ram:SpecifiedLineTradeSettlement>
        <ram:ApplicableTradeTax><ram:TypeCode>VAT</ram:TypeCode><ram:CategoryCode>S</ram:CategoryCode><ram:RateApplicablePercent>19</ram:RateApplicablePercent></ram:ApplicableTradeTax>
        <ram:SpecifiedTradeSettlementLineMonetarySummation><ram:LineTotalAmount>190.00</ram:LineTotalAmount></ram:SpecifiedTradeSettlementLineMonetarySummation>
      </ram:SpecifiedLineTradeSettlement>
    </ram:IncludedSupplyChainTradeLineItem>
    <ram:IncludedSupplyChainTradeLineItem>
      <ram:AssociatedDocumentLineDocument><ram:LineID>2</ram:LineID></ram:AssociatedDocumentLineDocument>
      <ram:SpecifiedTradeProduct><ram:Name>Webentwicklung Sprint 2</ram:Name></ram:SpecifiedTradeProduct>
      <ram:SpecifiedLineTradeAgreement><ram:NetPriceProductTradePrice><ram:ChargeAmount>95.00</ram:ChargeAmount></ram:NetPriceProductTradePrice></ram:SpecifiedLineTradeAgreement>
      <ram:SpecifiedLineTradeDelivery><ram:BilledQuantity unitCode="HUR">2</ram:BilledQuantity></ram:SpecifiedLineTradeDelivery>
      <ram:SpecifiedLineTradeSettlement>
        <ram:ApplicableTradeTax><ram:TypeCode>VAT</ram:TypeCode><ram:CategoryCode>S</ram:CategoryCode><ram:RateApplicablePercent>19</ram:RateApplicablePercent></ram:ApplicableTradeTax>
        <ram:SpecifiedTradeSettlementLineMonetarySummation><ram:LineTotalAmount>190.00</ram:LineTotalAmount></ram:SpecifiedTradeSettlementLineMonetarySummation>
      </ram:SpecifiedLineTradeSettlement>
    </ram:IncludedSupplyChainTradeLineItem>
    <ram:IncludedSupplyChainTradeLineItem>
      <ram:AssociatedDocumentLineDocument><ram:LineID>3</ram:LineID></ram:AssociatedDocumentLineDocument>
      <ram:SpecifiedTradeProduct><ram:Name>Webentwicklung Sprint 3</ram:Name></ram:SpecifiedTradeProduct>
      <ram:SpecifiedLineTradeAgreement><ram:NetPriceProductTradePrice><ram:ChargeAmount>95.00</ram:ChargeAmount></ram:NetPriceProductTradePrice></ram:SpecifiedLineTradeAgreement>
      <ram:SpecifiedLineTradeDelivery><ram:BilledQuantity unitCode="HUR">2</ram:BilledQuantity></ram:SpecifiedLineTradeDelivery>
      <ram:SpecifiedLineTradeSettlement>
        <ram:ApplicableTradeTax><ram:TypeCode>VAT</ram:TypeCode><ram:CategoryCode>S</ram:CategoryCode><ram:RateApplicablePercent>19</ram:RateApplicablePercent></ram:ApplicableTradeTax>
        <ram:SpecifiedTradeSettlementLineMonetarySummation><ram:LineTotalAmount>190.00</ram:LineTotalAmount></ram:SpecifiedTradeSettlementLineMonetarySummation>
      </ram:SpecifiedLineTradeSettlement>
    </ram:IncludedSupplyChainTradeLineItem>
    <ram:ApplicableHeaderTradeAgreement>
      <ram:BuyerReference>K-1007</ram:BuyerReference>
      <ram:SellerTradeParty>
        <ram:Name>Muster GmbH</ram:Name>
        <ram:PostalTradeAddress><ram:PostcodeCode>12345</ram:PostcodeCode><ram:LineOne>Hauptstraße 1</ram:LineOne><ram:CityName>Musterstadt</ram:CityName><ram:CountryID>DE</ram:CountryID></ram:PostalTradeAddress>
        <ram:SpecifiedTaxRegistration><ram:ID schemeID="VA">DE123456789</ram:ID></ram:SpecifiedTaxRegistration>
      </ram:SellerTradeParty>
      <ram:BuyerTradeParty>
        <ram:Name>Kunde AG</ram:Name>
        <ram:PostalTradeAddress><ram:PostcodeCode>54321</ram:PostcodeCode><ram:LineOne>Bahnhofstraße 5</ram:LineOne><ram:CityName>Beispielstadt</ram:CityName><ram:CountryID>DE</ram:CountryID></ram:PostalTradeAddress>
      </ram:BuyerTradeParty>
    </ram:ApplicableHeaderTradeAgreement>
    <ram:ApplicableHeaderTradeDelivery>
      <ram:ActualDeliverySupplyChainEvent><ram:OccurrenceDateTime><udt:DateTimeString format="102">20260930</udt:DateTimeString></ram:OccurrenceDateTime></ram:ActualDeliverySupplyChainEvent>
    </ram:ApplicableHeaderTradeDelivery>
    <ram:ApplicableHeaderTradeSettlement>
      <ram:InvoiceCurrencyCode>EUR</ram:InvoiceCurrencyCode>
      <ram:SpecifiedTradeSettlementPaymentMeans>
        <ram:TypeCode>58</ram:TypeCode>
        <ram:PayeePartyCreditorFinancialAccount><ram:IBANID>DE02120300000000202051</ram:IBANID></ram:PayeePartyCreditorFinancialAccount>
      </ram:SpecifiedTradeSettlementPaymentMeans>
      <ram:ApplicableTradeTax>
        <ram:CalculatedAmount>108.30</ram:CalculatedAmount><ram:TypeCode>VAT</ram:TypeCode><ram:BasisAmount>570.00</ram:BasisAmount><ram:CategoryCode>S</ram:CategoryCode><ram:RateApplicablePercent>19</ram:RateApplicablePercent>
      </ram:ApplicableTradeTax>
      <ram:SpecifiedTradePaymentTerms><ram:DueDateDateTime><udt:DateTimeString format="102">20261115</udt:DateTimeString></ram:DueDateDateTime></ram:SpecifiedTradePaymentTerms>
      <ram:SpecifiedTradeSettlementHeaderMonetarySummation>
        <ram:LineTotalAmount>570.00</ram:LineTotalAmount>
        <ram:TaxBasisTotalAmount>570.00</ram:TaxBasisTotalAmount>
        <ram:TaxTotalAmount currencyID="EUR">108.30</ram:TaxTotalAmount>
        <ram:GrandTotalAmount>678.30</ram:GrandTotalAmount>
        <ram:DuePayableAmount>678.30</ram:DuePayableAmount>
      </ram:SpecifiedTradeSettlementHeaderMonetarySummation>
    </ram:ApplicableHeaderTradeSettlement>
  </rsm:SupplyChainTradeTransaction>
</rsm:CrossIndustryInvoice>
```

</details>

`render.php`:

```php
<?php

require 'vendor/autoload.php';

use Fagus\Fagus;

$pdf = Fagus::html(file_get_contents(__DIR__.'/invoice.html'))
    ->facturX(file_get_contents(__DIR__.'/invoice.xml'))
    ->created(new DateTimeImmutable('2026-11-01 09:00', new DateTimeZone('Europe/Berlin')))
    ->render();

$pdf->save(__DIR__.'/invoice.pdf');

foreach ($pdf->diagnostics() as $diagnostic) {
    echo $diagnostic, "\n";
}
```

The result is a one-page PDF/A-3b file of about 23 KB with `factur-x.xml` attached and the
profile EN 16931 in its metadata. For an invoice that is also accessible, ask for PDF/A-3a and
PDF/UA-1; the HTML above meets both:

```php
use Fagus\Conformance;

Fagus::html($html)
    ->pdfA(Conformance::PdfA3a)
    ->accessible()
    ->facturX($xml)
    ->save('invoice.pdf');
```

In Laravel, the same methods follow the view:

```php
use Fagus\Laravel\Facades\Fagus;

return Fagus::view('invoices.show', ['invoice' => $invoice])
    ->facturX($xml)
    ->download("{$invoice->number}.pdf");
```

On the command line:

```sh
fagus render invoice.html -o invoice.pdf --factur-x invoice.xml
```

### Validate the result

Fagus checks PDF/A and PDF/UA while it writes the file, but not the invoice XML. Two free tools
check the result:

- [veraPDF](https://verapdf.org) checks PDF/A and PDF/UA. It reads the PDF/A level from the
  file's metadata:

  ```sh
  verapdf --format text invoice.pdf            # PDF/A
  verapdf --format text -f ua1 invoice.pdf     # PDF/UA-1
  ```

- [Mustang](https://www.mustangproject.org) checks the PDF/A file and the attached XML against the
  rules of ZUGFeRD, Factur-X and XRechnung:

  ```sh
  java -jar Mustang-CLI.jar --action validate --source invoice.pdf
  ```

  A valid invoice reports `<summary status="valid"/>`.

The repository's script `tools/validate.sh` renders every document of the test corpus as
PDF/A-1b, 2b, 2u, 3b, 3a, 4, 4e and 4f, as PDF/UA-1 and as Factur-X invoices, and checks them
with both tools (documents that Fagus refuses for a level, such as those with transparency for
PDF/A-1b, are left out of it):

```sh
VERAPDF=/path/to/verapdf MUSTANG=/path/to/Mustang-CLI.jar tools/validate.sh /tmp/validate
```

It uses `target/release/fagus`, or the binary in `FAGUS`. With veraPDF 1.30.3 and Mustang
2.26.0 every file passes.

## Protect the PDF with a password

`encrypt()` encrypts the PDF with AES-256, as PDF 2.0 defines it (the standard security handler,
revision 6). It takes a user password, an owner password and permissions:

```php
use Fagus\Fagus;
use Fagus\Permission;

Fagus::html($html)
    ->encrypt(
        userPassword: 'open sesame',
        ownerPassword: 'change me',
        permissions: [Permission::Print, Permission::Copy],
    )
    ->save('salary.pdf');
```

- The **user password** is needed to open the PDF. Without one, the PDF opens without a password,
  but with only the permissions given.
- The **owner password** opens the PDF too, and lifts the restrictions of the permissions. Without
  one, Fagus uses a random password that nobody knows, so the restrictions stay. Giving both the
  same password lifts the restrictions for everyone who can open the file.
- The **permissions** say what a reader with the user password (or with none, if there is none)
  may do. Without the argument, everything is allowed; with an empty list, nothing beyond reading.

At least one of the passwords is needed. `encrypt(ownerPassword: 'change me', permissions: [])`
makes a PDF that opens for everyone but cannot be printed, copied from or changed.

| Permission | Allows |
|---|---|
| `Permission::Print` (`print`) | Printing, in full quality |
| `Permission::PrintLowQuality` (`print-low-quality`) | Printing, only as a low-resolution image |
| `Permission::Modify` (`modify`) | Changing the content |
| `Permission::Copy` (`copy`) | Copying text and images |
| `Permission::Annotate` (`annotate`) | Adding and changing comments, and filling in forms |
| `Permission::FillForms` (`fill-forms`) | Filling in form fields |
| `Permission::Assemble` (`assemble`) | Inserting, deleting and rotating pages, changing bookmarks |

Screen readers and other assistive technology may always extract the text: PDF 2.0 says to allow
it in every encrypted file, and Fagus does.

What to know about encrypted PDFs:

- The permissions are a request to the PDF viewer, which Acrobat and most other viewers honour.
  Anyone who can open the file can read its content, and tools exist that ignore the
  permissions. Only the user password keeps people out.
- Passwords are used as UTF-8, up to 127 bytes; longer ones are cut there. Viewers differ in how
  they treat characters beyond ASCII (PDF 2.0 asks for SASLprep, which Fagus does not apply), so
  passwords of ASCII characters are the safest.
- Everything is encrypted: the content of the pages, fonts, images, attachments, the XMP metadata
  and the strings of the document information (title, author), so a search engine or a file
  manager sees no title either.
- AES-256 needs a PDF 2.0 reader or a PDF 1.7 one with Adobe's extension level 8, which the PDF
  declares: Acrobat 9 and later, and every current viewer, browser and library (pdf.js, PDFium,
  Preview, Poppler, qpdf, pypdf) open it. Older encryption (RC4, AES-128) is not available: it
  is deprecated in PDF 2.0, and RC4 is broken.
- PDF/A forbids encryption, so `encrypt()` cannot go with `pdfA()` or `facturX()` (which
  writes PDF/A-3): rendering fails with ``the document does not meet PDF/A-3b: the PDF cannot be
  encrypted (PDF/A forbids encryption)``. Encrypt a copy of the archived invoice if you need
  one. Fagus refuses `accessible()` with `encrypt()` as well, so that PDF/UA files stay readable
  by every assistive technology.
- The file key, its salts and the initialization vectors are random, from the operating system:
  an encrypted PDF is not byte-reproducible (see
  [Make the output reproducible](#make-the-output-reproducible)), even with `created()`. The Rust
  library has `Encryption::seed()` for tests that compare bytes.

On the command line, `--user-password`, `--owner-password` and `--permissions print,copy` encrypt
the PDF; the passwords can come from files or environment variables, so that they do not show up
in the list of processes (see [cli.md](cli.md#protect-the-pdf-with-a-password)).

To check an encrypted file, `qpdf --password=… --show-encryption file.pdf` shows the encryption
and the permissions, and `qpdf --password=… --decrypt file.pdf plain.pdf` writes it unencrypted.

## Make the output reproducible

A PDF contains its creation date, by default the current time, so two renderings of the same
document differ. With a fixed date, the same input gives a byte-identical file:

```php
Fagus::html($html)
    ->created(new DateTimeImmutable('2026-11-01 09:00', new DateTimeZone('Europe/Berlin')))
    ->save('invoice.pdf');
```

`created()` takes any `DateTimeInterface`; the PHP package converts it to UTC. On the command line,
`--created` takes a date in UTC (`2026-11-01T08:00:00Z`) or with an offset
(`2026-11-01T09:00:00+01:00`), which is converted to UTC (see [cli.md](cli.md)). The PDF holds the
date in UTC.

The output is the same for the same HTML, resources, fonts, options and version of Fagus, on
every platform. Fonts installed on the machine (`systemFonts: true`) make it depend on the
machine, and an encrypted PDF differs every time: its keys are random (see
[Protect the PDF with a password](#protect-the-pdf-with-a-password)).

## File size and compression

Fagus keeps files small without options to set:

- fonts are embedded as subsets, with only the glyphs the document uses;
- page contents, fonts and images are compressed (Flate); all other objects go into compressed
  object streams with a cross-reference stream;
- an image used many times is embedded once, and a background of more than 4096 tiles is one
  tiling pattern;
- JPEG images are embedded as they are, without decoding and compressing them again.

The invoice above is 16 KB as a tagged PDF, 15 KB without tags and 23 KB as PDF/A-3 with its XML
(which also carries an ICC profile). The tags grow with the number of elements, most with large
tables; `tagged(false)` leaves them out where accessibility is not needed.

The binary compresses on two threads of its own while it draws the pages; the environment variable
`RAYON_NUM_THREADS` sets how many.

## Images in the PDF

Fagus reads PNG, JPEG, GIF, WebP, BMP, TIFF and SVG images. Other formats (AVIF, JPEG XL, ICO …)
report `image.unsupported` and are left out; `<picture>` and `srcset` skip AVIF and JPEG XL
sources, as Chrome chooses among the formats it can show. An image of more than 200 megapixels reports
`image.too-large`, one that does not decode `image.broken`.

How images go into the PDF:

- JPEG as it is, compressed as JPEG. A CMYK JPEG without an ICC profile is converted to sRGB as
  Chrome draws it, instead of leaving the conversion to the viewer.
- PNG, GIF and WebP as their pixels, compressed without loss; transparency becomes a soft mask.
  A PNG whose `gAMA` chunk Chrome corrects for is corrected the same way. An animated GIF shows
  its first frame.
- BMP and TIFF as their pixels, decoded by Fagus, compressed without loss. BMP is read as Chrome
  reads it; TIFF, which Chrome does not show, as Safari shows it: its first image, gray, RGB, CMYK
  (converted to RGB as CMYK JPEGs) or YCbCr, uncompressed or compressed with LZW, Deflate,
  PackBits, CCITT Group 4 or JPEG. Palette and Lab TIFFs and CCITT Group 3 report `image.broken`.
- SVG as vector graphics, with their text, gradients and patterns. Filtered SVG and SVG filters
  that PDF cannot express are drawn as pixels at 300 dpi, as Chrome prints them.
- At their full resolution: Fagus does not scale images down. Scale large photos before you hand
  them to the document.
- Without interpolation (`/Interpolate`), as Chrome writes them: viewers decide how to smooth.

## Gradients, filters and differences from Chrome

Fagus draws what it can as vectors, which stay sharp at any zoom and print resolution:

- Linear, radial and conic gradients are PDF shadings. They interpolate between their stops in
  sRGB as Chrome's PDF output does, whatever color space the gradient names, with color hints
  mixed in the named space. Where Chrome prints a gradient with translucent stops in another color
  space (`in oklch`) as an image, Fagus samples it into a shading. A conic gradient with more than
  96 stops (a repeating one of short periods) and every conic gradient in PDF/A is an image.
- `filter`: Chrome prints filtered content as an image. Fagus applies color filters to the colors
  of what the box draws and keeps text and shapes as vectors where they stay sharp; `blur()` and
  SVG filters without a vector form are drawn as pixels.
- `mix-blend-mode`, `background-blend-mode` and `isolation` are PDF blend modes in transparency
  groups; `mask` and `clip-path` are soft masks and clipping paths.
- `@page` backgrounds paint their colors and images but no gradients, as in Chrome.

Other places where Fagus deliberately prints differently from Chrome (the A4 default page size,
the language of form controls, fonts in margin boxes) are listed in [css.md](css.md),
[paged-media.md](paged-media.md) and [fonts.md](fonts.md).
