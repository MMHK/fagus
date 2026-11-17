# Command line

The `fagus` binary renders HTML files to PDF on the command line. It is the same binary the PHP
package runs as its worker, so it renders exactly as the PHP package does. Use it to try a
template, in shell scripts and build jobs, or from languages other than PHP.

```sh
fagus render invoice.html -o invoice.pdf
```

## Get the binary

Composer installs the binary of the package's release into the package's `bin` directory
(`vendor/nforst/fagus/php/fagus/bin/fagus`, `fagus.exe` on Windows); see
[getting-started.md](getting-started.md). A checkout of the repository builds it with
`cargo build --release` as `target/release/fagus`.

```sh
fagus --version     # fagus 0.1.0
fagus --help
fagus render --help
```

## Render an HTML file

```sh
fagus render <INPUT> --output <OUTPUT> [OPTIONS]
```

`fagus render` reads the HTML file `INPUT` and writes the PDF to `OUTPUT`. Relative URLs in the
document (style sheets, images, fonts) resolve against the directory of the input file, and files
are only read from inside that directory, or from inside `--root`. Nothing is loaded from the
network.

A file ending in `.xhtml` or `.xht` is parsed as XML, as browsers parse XHTML files; any other
file as HTML.

| Option | Default | Description |
|---|---|---|
| `-o`, `--output <FILE>` | required | The PDF file to write |
| `--font-dir <DIR>` | none | A directory with font files (TTF, OTF, TTC, OTC, WOFF, WOFF2), read with its subdirectories; repeatable |
| `--system-fonts` | off | Use the fonts installed on the machine too, as Chrome does; the output then depends on the machine |
| `--locale <TAG>` | none | The language of the texts form controls show (button labels, date formats) where the document gives no `lang`, e.g. `de-DE`; without it they are American English. Text without `lang` is shaped in it, and it is the PDF's language when `<html>` has no `lang` |
| `--sans-serif <FAMILY>` | none | The family used for `sans-serif` |
| `--serif <FAMILY>` | none | The family used for `serif` |
| `--monospace <FAMILY>` | none | The family used for `monospace` |
| `--root <DIR>` | the input's directory | A larger directory the document may read files from, which contains the input's directory; URLs starting with `/` resolve against it |
| `--pdfa <LEVEL>` | none | Write PDF/A: `2b`, `2u`, `2a`, `3b`, `3u` or `3a` |
| `--untagged` | off | Write a PDF without tags (the logical structure screen readers read) |
| `--accessible` | off | Write PDF/UA-1; needs a title, a language and alternative text for images |
| `--attach <FILE>` | none | Attach a file; repeatable |
| `--factur-x <FILE>` | none | Make a ZUGFeRD / Factur-X invoice with this XML |
| `--profile <PROFILE>` | the XML's | The invoice profile: `minimum`, `basicwl`, `basic`, `en16931`, `extended` or `xrechnung` |
| `--factur-x-version <VERSION>` | `1.0` | The `Version` of the Factur-X XMP metadata |
| `--factur-x-description <TEXT>` | `Factur-X/ZUGFeRD invoice` | The description of the attached invoice XML |
| `--created <DATE>` | now | The creation date, e.g. `2026-11-13T12:00:00Z` or `2026-11-13T14:00:00+02:00`, for reproducible files |
| `--strict` | off | Fail on any diagnostic (not only warnings), without writing the PDF |
| `--timings` | off | Print the page count and the time of each phase |
| `--repeat <N>` | `1` | Render N times and report the median time (for benchmarks) |
| `-h`, `--help` | | Print help |

The options mean what the methods of the PHP package mean; [pdf.md](pdf.md) describes PDF/A,
PDF/UA, attachments, invoices and reproducible output in detail.

Some details of the command line:

- `--attach` attaches the file under its file name, with the file name as description and the
  relationship `Supplement`. The MIME type comes from the extension: `xml` (`text/xml`), `pdf`,
  `csv`, `json`, `txt`, `png`, `jpg` and `jpeg`; any other file is `application/octet-stream`.
- `--profile`, `--factur-x-version` and `--factur-x-description` only apply together with
  `--factur-x`.
- `--created` reads the date, the time and an offset from UTC (`Z`, `+01:00`, `-0530`) and
  writes the date in UTC: `2026-11-13T12:00:00+02:00` becomes `2026-11-13T10:00:00Z`. A date
  without an offset is taken as UTC; a fraction of a second is dropped. Anything else after the
  seconds makes the date invalid.
- `--untagged` has no effect with `--accessible` or with `--pdfa 2a` and `3a`, which need tags.
- With `--repeat`, the PDF of the last run is written.

## Read and write

The input is a file path; `fagus render` does not read from standard input. Write the HTML to a
file first, or use the worker or the PHP package.

On Linux and macOS, the PDF can go to standard output through `/dev/stdout`:

```sh
fagus render invoice.html -o /dev/stdout | aws s3 cp - s3://invoices/2026-0042.pdf
```

Diagnostics and timings go to standard error, one per line:

```text
<style> 1:12:3: warning[css.unknown-property]: unknown property `colr`; the declaration is ignored
resources: warning[resource.not-loaded]: image `logo.png` could not be loaded
```

Each line has the source (a file name, `<style> 1` for the first `<style>` element, `resources`,
`pdf`), the line and column where known, the severity, a stable code and the message. The page
is written in spite of warnings unless `--strict` is given, which fails on any diagnostic
("1 diagnostic in strict mode"). When a document is refused (PDF/A, PDF/UA), the diagnostics found
before are printed too, ahead of the error.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | The PDF was written, or help or the version was printed |
| `1` | Rendering failed: the input or a font directory could not be read, the document was refused (PDF/A, PDF/UA), `--strict` found diagnostics, `--created` is not a date, or the PDF could not be written. The reason goes to standard error, after `Error:` |
| `2` | The command line is wrong: an unknown option, a missing `--output` or input, a value that is not allowed (`--pdfa 1b`) |

```text
$ fagus render letter.html -o letter.pdf --accessible
Error: the document does not meet PDF/UA-1: the document has no title (add a <title>); the document has no language (add `lang` to <html>)
$ echo $?
1
```

## Examples

A PDF with fonts of your own:

```sh
fagus render invoice.html -o invoice.pdf --font-dir resources/fonts
```

A document whose style sheets and images lie in a parent directory (`../css/print.css`):

```sh
fagus render resources/pdf/invoice.html -o invoice.pdf --root resources
```

An archivable PDF with a file attached:

```sh
fagus render report.html -o report.pdf --pdfa 3b --attach data.csv
```

A ZUGFeRD / Factur-X invoice that is also accessible, with a fixed date:

```sh
fagus render invoice.html -o invoice.pdf \
  --factur-x invoice.xml --pdfa 3a --accessible \
  --created 2026-11-01T08:00:00Z
```

A check in a build job that fails on any ignored CSS or missing resource:

```sh
fagus render template.html -o /tmp/template.pdf --strict
```

How long each phase takes, as the median of 20 runs:

```sh
$ fagus render invoice.html -o invoice.pdf --timings --repeat 20
1 pages, median 1.77 ms (parse 0.03, style 0.79, layout 0.24, paint 0.02, pdf 0.49)
```

The phase times are those of the last run. Each `fagus render` starts a new process that loads
its fonts first; the PHP package keeps one worker running, which is faster when you render many
documents.

## Threads

The binary compresses the PDF on two threads while it draws the pages. The environment variable
`RAYON_NUM_THREADS` sets another number:

```sh
RAYON_NUM_THREADS=1 fagus render invoice.html -o invoice.pdf
```

## The worker

```sh
fagus worker [OPTIONS]
```

`fagus worker` renders documents for another program: it reads requests from standard input and
writes PDFs to standard output, in the binary frames of [protocol.md](protocol.md), until its
input closes. The PHP package starts it and talks to it; it is not meant to be used by hand. Write
a client of your own only to use Fagus from another language.

| Option | Default | Description |
|---|---|---|
| `--font-dir <DIR>` | none | A directory with font files, as for `render`; repeatable |
| `--system-fonts` | off | Use the fonts installed on the machine too |
| `--locale <TAG>` | none | The language of the texts of form controls, as for `render` |
| `--sans-serif <FAMILY>` | none | The family used for `sans-serif` |
| `--serif <FAMILY>` | none | The family used for `serif` |
| `--monospace <FAMILY>` | none | The family used for `monospace` |

Everything else (PDF/A, attachments, resources, the creation date) comes with each request. The
worker ends with exit code 0 when its input closes, and with 1 when it reads a frame that breaks
the protocol.
