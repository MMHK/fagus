# 7. Layers for screen and print, and form fields as an option

- Status: accepted
- Date: 2026-11-19

## Context

Two features of PDF have no counterpart in how Chrome prints a page:

- **Layers** (optional content groups): content that a reader shows and hides, and content that
  shows on screen only or in print only. Invoices want a note such as "Pay online with the link
  below" that a printout leaves out, or a "Copy" stamp that only the printout has.
- **Interactive forms** (AcroForm): fields that a reader fills in in the PDF viewer. Chrome
  prints form controls as they look, and so does Fagus; existing documents with controls must
  keep their bytes.

[ADR 4](0004-extensions-only-where-css-has-no-way.md) allows an extension only where CSS has no
way to reach the result.

## Decision

### Layers

- CSS has no way to put content on a layer. `@media screen` and `@media print` look like one,
  but they choose the styles of a whole rendering: Fagus renders for print, and rules in
  `@media screen` describe how a browser shows the page, often a different layout (navigation, a
  wider column), not content the PDF should show on a screen. Taking them for layers would change
  the PDFs of existing documents and could not work where the two media lay out differently.
  `visibility` and `display` hide content for every reader.
- So there is an extension, one property: `-fagus-layer: none | <string> [ screen | print ]?`.
  The string names the layer; elements with the same name share it. `screen` keeps the layer out
  of print, `print` hides it on screen. One property rather than a name and a second property for
  the medium, since the medium belongs to the layer, not to each element, and a value that names
  both is harder to get wrong.
- What the element paints is on the layer, with its descendants, as `opacity` groups them: the
  property makes the element a stacking context.
- `@supports (-fagus-layer: none)` is true in Fagus.
- PDF/A-2 to PDF/A-4 and PDF/UA-1 forbid the automatic states that make a layer depend on
  printing (the `AS` entry of an optional content configuration). There the layers stay and show
  in both media, with a warning. PDF/A-1 (PDF 1.4) has no optional content: the content is
  printed without layers, with a warning. Rendering does not fail, since the content is all
  there.

### Form fields

- HTML already says what a field is: `<input>`, `<textarea>`, `<select>`, with `name`, `value`,
  `checked`, `selected`, `readonly`, `disabled`, `required` and `maxlength`. No extension is
  needed for what a field is.
- Whether controls become fields is an option of the document, off by default: `forms()` in PHP
  and Laravel, `Document::forms` in Rust, `--forms` on the command line, `forms` in the worker
  protocol. It is not a property, because the choice belongs to the PDF as a whole (a form to
  fill in, or a printout of a filled-in form), not to the style of an element, and because CSS
  has no property for interactivity that a printout could honour.
- Where one control in a form should stay as it looks (a sample filled in for illustration), HTML
  has a way: a control in an `inert` element cannot be interacted with, so it stays as printed.
  No extension is needed for that either.

## Consequences

Documents without `-fagus-layer` and rendered without `forms()` keep their bytes. `-fagus-layer`
is documented in [extensions.md](../extensions.md) and form fields in [pdf.md](../pdf.md). If CSS
gets a way to place content on layers, `-fagus-layer` is deprecated in its favour, as
[ADR 5](0005-public-api-and-deprecations.md) describes.
