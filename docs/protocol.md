# Worker protocol

`fagus worker` renders documents for a caller over standard input and output. One process serves
any number of documents one after another, with fonts loaded and caches warm, so a document costs
the time it takes to render it and nothing more. The PHP package uses this protocol; any other
language can too.

## Frames

Every message is a frame:

| Bytes | Content |
|---|---|
| 4 | length of the rest of the frame, unsigned, little-endian |
| 1 | message type |
| 4 | length of the JSON header, unsigned, little-endian |
| n | JSON header (UTF-8) |
| … | data blocks, one after another |

The header's `blobs` array lists the lengths of the data blocks that follow it; frames without
data have no `blobs`. Data travels as raw bytes, never base64. Frames are at most 1 GiB. A frame
the worker cannot read (too long, a header that is not JSON, data blocks longer than the frame)
ends the worker with exit code 1 and the reason on its error output.

## Messages

| Type | Direction | Header | Data |
|---|---|---|---|
| `H` hello | worker → caller | `protocol`, `version` | – |
| `R` render | caller → worker | the document, see below | HTML, stylesheets, resources, attachments, invoice XML |
| `?` ask | worker → caller | `id`, `url` | – |
| `r` resource | caller → worker | `found` | the resource, if found |
| `P` PDF | worker → caller | `id`, `pages`, `diagnostics`, `timings` | the PDF |
| `E` error | worker → caller | `id` (if known), `message`, `diagnostics` | – |
| `F` font | caller → worker | – | a font file (TTF, OTF, TTC, WOFF, WOFF2) |
| `K` done | worker → caller | `faces` | – |

The worker sends `H` when it starts; its `protocol` is 1 and its `version` that of the binary.
A caller that does not know the protocol version stops there. The worker ends with
exit code 0 when its input closes. A message of an unknown type gets an `E` without `id`.

### Rendering

The caller sends `R` and reads frames until it gets `P` or `E`. Meanwhile the worker may ask for
resources it needs with `?`, if the request allows it (`ask`); the caller answers each one with
`r`, with the data or `found: false`. Nothing else may come in between.

The header of `R`; fields other than `id` and `html` may be left out:

```json
{
  "id": 1,
  "html": 0,
  "xhtml": false,
  "base": "/app/resources/pdf",
  "root": "/app/resources",
  "stylesheets": [{ "name": "app.css", "blob": 1 }],
  "resources": [{ "url": "logo.png", "blob": 2 }],
  "ask": true,
  "conformance": "3b",
  "tagged": true,
  "accessible": false,
  "attachments": [{ "name": "data.csv", "mime": "text/csv", "relationship": "supplement", "description": "Rohdaten", "blob": 3 }],
  "facturX": { "blob": 4, "profile": "en16931", "version": "1.0", "description": "Factur-X/ZUGFeRD invoice" },
  "created": "2026-11-02T12:00:00Z",
  "blobs": [5120, 812, 20431, 96, 4187]
}
```

- `html`, `blob`: the index of a data block. The HTML and style sheets must be UTF-8.
- `xhtml`: whether the markup is XHTML, parsed as XML as browsers parse `.xhtml` files (`false`
  if left out).
- `base`: the directory that relative URLs resolve against; files are read from there (and from
  `root`, if given, which must contain it). URLs that start with `/` resolve against `root` (or
  `base`), as on a web server. Without `base` the worker reads no files.
- `resources`: data the document may use, by URL as written in the document. The documents of
  frames (`<iframe src>`, `<object data>`) ask for their resources by their URLs resolved against
  the frame's URL, relative to the document (`sub/style.css` for `style.css` in `sub/page.html`).
- `stylesheets`: style sheets added after those of the document; `name` is the source that
  diagnostics name.
- `ask`: whether the worker asks for resources it does not have (`false` if left out). A resource
  is looked for as a `data:` URL, in `resources`, by asking the caller, then as a file, in that
  order.
- `conformance`: `2b`, `2u`, `2a`, `3b`, `3u` or `3a` for PDF/A.
- `tagged`: whether the PDF is tagged, with the logical structure of the document (`true` if
  left out).
- `accessible`: whether the PDF conforms to PDF/UA-1 (and is tagged). The document needs a
  title, a language and alternative text for every image; otherwise the answer is `E`.
- `attachments`: `name` and `mime` are required. `relationship` is `source`, `data`,
  `alternative`, `supplement` or `unspecified` (`unspecified` if left out). `description` may be
  left out (or be `null` or empty): PDF/A-3 and PDF/UA require one, so the worker then describes
  the file by its `name`, as `fagus render --attach` does.
- `facturX`: the invoice XML of a ZUGFeRD or Factur-X invoice; `profile` (`minimum`, `basicwl`,
  `basic`, `en16931`, `extended`, `xrechnung`) is read from the XML if left out (`en16931` if
  the XML declares none). `version` is the `Version` of the Factur-X XMP metadata (`1.0` if left
  out or `null`), `description` the description of the attached XML ("Factur-X/ZUGFeRD invoice"
  if left out or `null`). An empty `version` or `description`, or one with control characters,
  is answered with `E`.
- `created`: the creation date, as `2026-11-02T12:00:00Z` or with an offset from UTC
  (`2026-11-02T13:00:00+01:00`), which the worker applies; the current time if left out. A date
  without an offset is taken as UTC.

A request the worker cannot use (a missing data block, an unknown PDF/A level, relationship or
profile, an invalid date) is answered with `E` and its reason, as is a document that does not
meet PDF/A or PDF/UA.

The header of `P`:

```json
{
  "id": 1,
  "pages": 3,
  "diagnostics": [
    { "severity": "warning", "code": "css.unknown-property", "message": "unknown property `colr`; the declaration is ignored", "source": "<style> 1", "line": 12, "column": 3 }
  ],
  "timings": { "parse": 0.12, "style": 0.31, "layout": 0.44, "paint": 0.02, "pdf": 0.71 },
  "blobs": [48213]
}
```

Times are in milliseconds. `severity` is `info`, `warning` or `error`; `line` and `column` are 0
when unknown.

The header of `E` has the `message` and a `diagnostics` array of the same form. For a document
that could not be rendered (it does not meet PDF/A or PDF/UA, or the PDF could not be written),
it holds the diagnostics found until then, which often explain the failure; for a request the
worker cannot use, an `F` it cannot read and a message of an unknown type, it is empty.

### Fonts

`F` adds a font to the worker for all later documents; the worker answers with `K` and the number
of faces it found, or `E` (without `id`). Fonts can also be given on the command line
(`fagus worker --font-dir fonts`, and `--system-fonts` for the fonts installed on the machine) or
come with a document through `@font-face`. `--sans-serif`, `--serif` and `--monospace` name the
families of the generic families.

### Language of form controls

`fagus worker --locale de-DE` sets the language of the texts form controls show of their own
(button labels, the placeholders and formats of date fields) in documents that give a control no
`lang`; without it they are American English, as in Chrome by default. Text without `lang` is
shaped in that language as well, as Chrome shapes it in the language of its user interface.
