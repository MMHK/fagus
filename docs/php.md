# PHP API

The PHP API lives in the namespace `Fagus\` (`php/fagus/src`). It has four parts:

- `Fagus`: static entry points and the shared engine.
- `Document`: a document to render, built step by step.
- `Pdf`: the rendered PDF with its page count, diagnostics and timings.
- `Engine`: the `fagus` binary, running as a worker process.

In Laravel, use the facade described in [laravel.md](laravel.md); it offers the same document
methods and renders with the engine of `config/fagus.php`.

```php
use Fagus\Conformance;
use Fagus\Fagus;
use Fagus\Relationship;

$pdf = Fagus::html($html)
    ->baseDirectory(__DIR__.'/resources/pdf')   // relative URLs and the files they may read
    ->root(__DIR__.'/resources')                 // a larger directory files may come from (../fonts/…)
    ->stylesheet($css)                           // after the document's own styles
    ->resource('logo.png', $logo)                // data the document refers to
    ->resolver(fn (string $url): ?string => $cache->get($url))
    ->pdfA(Conformance::PdfA3b)
    ->attach('data.csv', $csv, 'text/csv', Relationship::Supplement, 'Raw data')
    ->facturX($invoiceXml)                       // ZUGFeRD / Factur-X; the profile comes from the XML
    ->created(new DateTimeImmutable('2026-11-02 12:00')) // a fixed date makes the PDF reproducible
    ->strict()                                   // diagnostics become exceptions
    ->render();

$pdf->bytes();
$pdf->pages();
$pdf->diagnostics();  // CSS that was ignored, resources that are missing, with their position
$pdf->timings();      // milliseconds per phase: parse, style, layout, paint, pdf
$pdf->save('invoice.pdf');
```

## `Fagus`

`Fagus\Fagus` is a final class with static methods only.

### `Fagus::html(string $html): Document`

Starts a document from HTML, parsed as browsers parse HTML. Same as `Document::html()`.

```php
Fagus::html('<h1>Invoice</h1>')->save('invoice.pdf');
```

### `Fagus::xhtml(string $xml): Document`

Starts a document from XHTML, parsed as XML as browsers parse `.xhtml` files: CDATA sections and
empty-element tags (`<div/>`) work, names are case-sensitive and nothing is implied (no `<tbody>`
added, no tags closed for you). Same as `Document::xhtml()`.

```php
Fagus::xhtml('<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Invoice</p><div/></body></html>')
    ->save('invoice.pdf');
```

### `Fagus::engine(): Engine`

The engine that documents render with unless one is passed to `render()` or `save()`: the one set
with `useEngine()`, else one created on first use with the default settings (the binary from
`FAGUS_BINARY`, the installed one or the `PATH`, no font directories, no system fonts).

```php
Fagus::engine()->version(); // "0.1.0"
```

### `Fagus::useEngine(?Engine $engine): void`

Sets the shared engine, for example to configure fonts once at bootstrap. `null` drops it; the
next call to `engine()` creates a new default engine.

```php
use Fagus\Engine;
use Fagus\Fagus;

Fagus::useEngine(new Engine(
    fontDirectories: [__DIR__.'/resources/fonts'],
    locale: 'de-DE',
));
```

## `Document`

`Fagus\Document` is immutable: every method returns a changed copy and the document itself never
changes. You can build a base document once and derive variants from it.

```php
$base = Fagus::html($html)->baseDirectory(__DIR__.'/resources/pdf');

$base->save('invoice.pdf');
$base->pdfA('3b')->save('invoice-archive.pdf');
```

### `Document::html(string $html): Document`

A document from HTML. The constructor is private; use this or `Fagus::html()`.

### `Document::xhtml(string $xml): Document`

A document from XHTML, parsed as XML (see `Fagus::xhtml()`).

### `baseDirectory(string $directory): Document`

The directory that relative URLs (stylesheets, images, fonts, frames) resolve against. Only files
inside it are read, unless `root()` allows a larger directory. Without a base directory, Fagus
reads no files at all. See [resources.md](resources.md).

```php
Fagus::html('<img src="images/logo.png" alt="ACME">')
    ->baseDirectory(__DIR__.'/resources/pdf');   // reads resources/pdf/images/logo.png
```

### `root(string $directory): Document`

The directory that files may be read from. It must contain the base directory. Use it when the
document's URLs lead out of the base directory (`../fonts/Inter.woff2`). URLs that start with `/`
resolve against the root, as on a web server. Without `root()`, the root is the base directory.

```php
Fagus::html($html)
    ->baseDirectory(__DIR__.'/resources/pdf/invoices')
    ->root(__DIR__.'/resources/pdf');   // allows ../fonts/… and /images/logo.png
```

### `stylesheet(string $css, string $name = 'stylesheet'): Document`

Adds a stylesheet after the document's own stylesheets, so its rules win over the document's
rules of the same specificity. It can be called several times; the stylesheets apply in that
order. `$name` is the source that diagnostics name for it.

```php
Fagus::html($html)->stylesheet(file_get_contents(__DIR__.'/pdf.css'), 'pdf.css');
```

Relative URLs in it resolve against the base directory.

### `resource(string $url, string $data): Document`

Hands over the data of a resource that the document refers to, by its URL exactly as written in
the document. Use it for data that is not in a file, such as a logo from a database or a chart
generated in PHP. A later call with the same URL replaces the data.

```php
Fagus::html('<img src="logo.png" alt="ACME"><img src="chart.svg" alt="Revenue">')
    ->resource('logo.png', $tenant->logo)
    ->resource('chart.svg', $chartSvg);
```

URLs in stylesheets are resolved against the stylesheet first: `url(img/bg.png)` in
`css/main.css` asks for `css/img/bg.png`. See [resources.md](resources.md).

### `resolver(callable $resolver): Document`

Sets a callback that answers requests for resources that are not handed over with `resource()`.
It gets the URL (resolved as described for `resource()`) and returns the data, or `null` when it
does not have it. Fagus asks the callback before it looks for a file inside the root; after
`null`, it tries the file. The engine waits while the callback runs.

```php
Fagus::html($html)->resolver(function (string $url): ?string {
    if (str_starts_with($url, 'media://')) {
        return Storage::get('media/'.substr($url, 8));
    }

    return null;
});
```

Fagus itself never goes to the network. A resolver may, under your own rules. Only one resolver
is kept; a second call replaces the first.

### `pdfA(Conformance|string $level = Conformance::PdfA3b): Document`

Writes the PDF as PDF/A. `$level` is a `Conformance` case or its value: `'2b'`, `'2u'`, `'2a'`,
`'3b'`, `'3u'` or `'3a'` (case-insensitive). Any other string throws a
`Fagus\Exception\InvalidArgumentException` that lists the levels ("Unknown PDF/A level '4z'.
The levels are '2b', '2u', '2a', '3b', '3u', '3a'."). The levels ending in `a` are tagged for
accessibility. See [pdf.md](pdf.md).

```php
Fagus::html($html)->pdfA();      // PDF/A-3b
Fagus::html($html)->pdfA('2u');
```

### `tagged(bool $tagged = true): Document`

Whether the PDF is tagged with the logical structure of the document (headings, paragraphs,
lists, tables, images and links in reading order), which screen readers read. Tagging is on by
default. Without it, a PDF with many table cells is somewhat smaller and faster to write.
`accessible()` and the PDF/A levels ending in `a` tag the PDF whatever `tagged()` says.

```php
Fagus::html($html)->tagged(false);
```

### `accessible(bool $accessible = true): Document`

Writes the PDF as PDF/UA-1, the standard for accessible PDF. The document needs a `<title>`, a
language (`<html lang>`), alternative text for every image (`alt`, empty for decoration) and
headings that start at `<h1>` and skip no level. Without them, `render()` throws a
`RenderException` that lists what is missing:

```text
the document does not meet PDF/UA-1: the document has no title (add a <title>); the image `logo.png` in line 12 has no alternative text (add `alt`, empty if it is decoration)
```

`accessible()` also works together with PDF/A-2a, PDF/A-3a and Factur-X. See [pdf.md](pdf.md).

### `attach(string $name, string $data, string $mime = 'application/octet-stream', Relationship $relationship = Relationship::Unspecified, ?string $description = null): Document`

Attaches a file to the PDF. `$relationship` says how the file relates to the document (PDF 2.0
`AFRelationship`, see `Relationship` below). PDF/A-3 and PDF/UA require a description of every
attached file; without `$description`, the file name describes it. Call it once per file.

```php
use Fagus\Relationship;

Fagus::html($html)
    ->pdfA('3b')
    ->attach('timesheet.csv', $csv, 'text/csv', Relationship::Supplement, 'Hours worked');
```

PDF/A-2 allows only PDF/A files as attachments: with `pdfA('2b')` and a CSV attachment, `render()`
throws a `RenderException` ("the document does not meet PDF/A-2b: PDF/A-2 allows only PDF/A files
as attachments; attach `timesheet.csv` to a PDF/A-3 document instead"). Use PDF/A-3 for other
files.

### `facturX(string $xml, ?InvoiceProfile $profile = null, ?string $version = null, ?string $description = null): Document`

Makes the PDF an electronic invoice of ZUGFeRD or Factur-X: the XML is attached under the name
its profile requires (`factur-x.xml`, or `xrechnung.xml` for XRechnung), described in the XMP
metadata, and the PDF conforms to PDF/A-3 (PDF/A-3b unless another PDF/A-3 level is chosen). The
profile is the one the XML declares in its guideline parameter (EN 16931 if it declares none),
unless `$profile` is given. `$version` is the `Version` of the XMP metadata (`'1.0'` by default),
`$description` the description of the attached XML (`'Factur-X/ZUGFeRD invoice'` by default); an
empty one throws an `InvalidArgumentException`.

```php
use Fagus\InvoiceProfile;

Fagus::html($html)->facturX($xml);                            // profile from the XML
Fagus::html($html)->facturX($xml, InvoiceProfile::XRechnung); // profile given
Fagus::html($html)->facturX($xml, version: '3.0', description: 'XRechnung Invoice');
```

Diagnostics report a profile that differs from the XML's (`invoice.profile-mismatch`), an XML
without a profile (`invoice.profile-unknown`) and a PDF/A-2 level raised to PDF/A-3
(`pdf.conformance-raised`). See [pdf.md](pdf.md).

### `created(DateTimeInterface $date): Document`

The creation date written into the PDF; the current time by default. Fagus converts it to UTC. A
fixed date makes the output reproducible: the same document, fonts and binary give the same bytes.

```php
Fagus::html($html)->created(new DateTimeImmutable('2026-11-02 12:00', new DateTimeZone('Europe/Berlin')));
```

### `strict(bool $strict = true): Document`

Fails with a `RenderException` when the PDF has diagnostics (CSS that is ignored, resources that
are missing), instead of returning it. Useful in tests and CI.

```php
Fagus::html($html)->strict()->render();
```

```text
2 diagnostics in strict mode:
<style> 1:3:5: warning[css.unknown-property]: unknown property `colr`; the declaration is ignored
resources: warning[resource.not-loaded]: image `logo.png` could not be loaded
```

### `render(?Engine $engine = null): Pdf`

Renders the document with `$engine`, or with `Fagus::engine()`. Throws a `RenderException` when
the document cannot be rendered (or has diagnostics in strict mode) and an `EngineException` when
the engine cannot be started or stops.

```php
$pdf = Fagus::html($html)->render();
$pdf = Fagus::html($html)->render($myEngine);
```

### `save(string $path, ?Engine $engine = null): Pdf`

Renders the document and writes the PDF to `$path`. Returns the `Pdf`, so you can still read its
page count and diagnostics.

```php
$pdf = Fagus::html($html)->save(storage_path('invoices/2026-0042.pdf'));
```

### What comes from the document

There are no methods for page size, margins, headers, footers or metadata. As in a browser, they
come from the document:

| PDF | From |
|---|---|
| Page size and margins | `@page { size: A4; margin: 20mm }` ([paged-media.md](paged-media.md)) |
| Headers, footers, page numbers | `@page` margin boxes |
| Title | `<title>` |
| Language | `<html lang>`, else the engine's `locale` |
| Author | `<meta name="author" content="…">` |
| Subject | `<meta name="description" content="…">` |
| Keywords | `<meta name="keywords" content="…">`, separated by commas |
| Outline (bookmarks) | the headings (`bookmark-level`, `bookmark-label`, `bookmark-state`) |

## `Pdf`

`Fagus\Pdf` is a final readonly class: a rendered PDF with what Fagus noticed while rendering it.

| Method | Returns |
|---|---|
| `bytes(): string` | The PDF file. |
| `pages(): int` | The number of pages. |
| `diagnostics(): list<Diagnostic>` | CSS that was ignored, resources that could not be loaded, and other notes, in the order they were found. Empty for a clean document. |
| `timings(): array<string, float>` | Milliseconds per phase, with the keys `parse`, `style`, `layout`, `paint` and `pdf`. |
| `save(string $path): void` | Writes the PDF to `$path`. Throws a `FagusException` ("The PDF could not be written to `…`.") if it cannot. |

```php
$pdf = Fagus::html($html)->render();

file_put_contents('invoice.pdf', $pdf->bytes());
echo array_sum($pdf->timings()), " ms\n";

foreach ($pdf->diagnostics() as $diagnostic) {
    error_log((string) $diagnostic);
}
```

The constructor is public, `new Pdf(string $bytes, int $pages, array $diagnostics = [], array $timings = [])`,
so tests can make a `Pdf` without rendering.

## `Engine`

`Fagus\Engine` is the `fagus` binary, running as a worker process. Configure one when the
defaults do not fit and make it the shared engine with `Fagus::useEngine()`, or pass it to
`render()`.

```php
use Fagus\Engine;
use Fagus\Fagus;

Fagus::useEngine(new Engine(
    binary: '/usr/local/bin/fagus',           // FAGUS_BINARY, the installed binary or the PATH if null
    fontDirectories: [__DIR__.'/resources/fonts'],
    sansSerif: 'Inter',                       // the families of sans-serif, serif and monospace
    serif: 'Source Serif 4',
    monospace: 'JetBrains Mono',
    timeout: 60.0,                            // seconds to wait for each answer of the worker
    systemFonts: false,
    locale: 'de-DE',
));
```

### Constructor

All arguments are optional and named as below.

| Argument | Type, default | Meaning |
|---|---|---|
| `binary` | `?string`, `null` | The path of the `fagus` binary. If `null`: `FAGUS_BINARY`, else the binary the plugin installed into the package, else `fagus` (`fagus.exe` on Windows) on the `PATH`. A binary that is given here or in `FAGUS_BINARY` must exist and be executable. |
| `fontDirectories` | `list<string>`, `[]` | Directories with fonts for every document: TTF, OTF, TTC, OTC, WOFF and WOFF2 files, in the directory and its subdirectories. Each directory must exist: before it starts the worker, the engine throws an `EngineException` that names a missing one ("The font directory `…` does not exist."). |
| `sansSerif` | `?string`, `null` | The family that `sans-serif` names. |
| `serif` | `?string`, `null` | The family that `serif` names. |
| `monospace` | `?string`, `null` | The family that `monospace` names. |
| `timeout` | `float`, `60.0` | Seconds to wait for each answer of the worker. A worker that does not answer in time is stopped and an `EngineException` is thrown. Not on Windows, where PHP cannot wait for a pipe with a time limit. |
| `systemFonts` | `bool`, `false` | Whether the fonts installed on the machine serve too, as in Chrome. The output then depends on the machine. |
| `locale` | `?string`, `null` | The language of the texts that form controls show of their own (button labels, date formats) and of the PDF where a document has no `lang`, such as `'de-DE'`. American English if `null`. |

Form controls write their texts (the labels of submit and reset buttons, "Choose File", the
placeholders and formats of date and time fields, the decimal separator of number fields) as
Chrome does, but in the language of the control: its `lang` (inherited, as from
`<html lang="de">`), else `locale`, else American English. Chrome itself takes the language of
its user interface. Fagus has Chrome's texts for English (`en-US`, `en-GB`), German, French,
Spanish, Italian, Dutch, Portuguese (`pt-BR`, `pt-PT`) and Polish. Text without `lang` is shaped
in the `locale` language too.

Fonts, generic families and system fonts are described in [fonts.md](fonts.md).

The constructor does not start the worker. The worker starts with the first call that needs it
(`render()`, `addFont()`, `version()`).

### `render(Document $document): Pdf`

Renders a document. `$document->render($engine)` calls this.

### `addFont(string $data): int`

Adds a font (TTF, OTF, TTC, OTC, WOFF or WOFF2 data) for all later documents of this engine and returns the number
of faces in it. The engine keeps the data and hands it to every new worker it starts. Data that is
not a font throws an `EngineException` ("the data is not a supported font").

```php
Fagus::engine()->addFont(file_get_contents(__DIR__.'/resources/fonts/CorporateSans.otf'));
```

### `version(): string`

The version of the running binary, such as `0.1.0`. Starts the worker if it is not running.

### `close(): void`

Stops the worker process. It ends when its input closes; one that has not ended after two seconds
is terminated. The next document starts a new worker, which gets the fonts of `addFont()` again.
The destructor calls `close()`.

### `Engine::PROTOCOL`

The version of the worker protocol this package speaks: `1`. A binary that speaks another
protocol is refused ([protocol.md](protocol.md)).

### The worker's lifetime

The worker process starts with the first document and keeps running, with its fonts and caches
warm, until the engine is closed or destroyed:

- Under PHP-FPM, the shared engine lives as long as the request, so the worker serves the
  documents of one request.
- In queue workers, under Octane or FrankenPHP's worker mode and in long-running scripts, it
  serves every document of the process.

When a render fails with an `EngineException` (a timeout, a worker that exited), the worker is
stopped and the next document starts a new one. A `RenderException` leaves the worker running.
More in [deployment.md](deployment.md#worker-processes).

## `Diagnostic` and `Severity`

`Fagus\Diagnostic` is a final readonly class with public properties:

| Property | Type | Meaning |
|---|---|---|
| `severity` | `Severity` | `Severity::Info`, `Severity::Warning` or `Severity::Error`. |
| `code` | `string` | A stable code, such as `css.unknown-property` or `resource.not-loaded`. |
| `message` | `string` | What happened, in English. |
| `source` | `string` | Where: `<style> 1` (the first `<style>` element), the URL of a linked stylesheet, the name given to `stylesheet()`, `html`, `resources` or `pdf`. |
| `line` | `int` | 1-based line in the source, 0 if unknown. |
| `column` | `int` | 1-based column, 0 if unknown. |

Cast it to a string for one line in the format `source:line:column: severity[code]: message`:

```php
foreach ($pdf->diagnostics() as $diagnostic) {
    if ($diagnostic->code === 'resource.not-loaded') {
        $logger->warning((string) $diagnostic);
    }
}
// resources: warning[resource.not-loaded]: image `logo.png` could not be loaded
```

`Fagus\Severity` is a string-backed enum: `Info = 'info'`, `Warning = 'warning'`,
`Error = 'error'`. The codes are listed in
[troubleshooting.md](troubleshooting.md#read-diagnostics).

## `Conformance`

`Fagus\Conformance` is a string-backed enum of PDF/A levels, for `pdfA()`:

| Case | Value | Level |
|---|---|---|
| `Conformance::PdfA2b` | `'2b'` | PDF/A-2b |
| `Conformance::PdfA2u` | `'2u'` | PDF/A-2u |
| `Conformance::PdfA2a` | `'2a'` | PDF/A-2a |
| `Conformance::PdfA3b` | `'3b'` | PDF/A-3b |
| `Conformance::PdfA3u` | `'3u'` | PDF/A-3u |
| `Conformance::PdfA3a` | `'3a'` | PDF/A-3a |

PDF/A-3 allows attachments of any type, as ZUGFeRD and Factur-X need. The levels ending in `a`
are tagged for accessibility.

## `InvoiceProfile`

`Fagus\InvoiceProfile` is a string-backed enum of ZUGFeRD and Factur-X profiles, for
`facturX()`:

| Case | Value |
|---|---|
| `InvoiceProfile::Minimum` | `'minimum'` |
| `InvoiceProfile::BasicWl` | `'basicwl'` |
| `InvoiceProfile::Basic` | `'basic'` |
| `InvoiceProfile::En16931` | `'en16931'` |
| `InvoiceProfile::Extended` | `'extended'` |
| `InvoiceProfile::XRechnung` | `'xrechnung'` |

## `Relationship`

`Fagus\Relationship` is a string-backed enum of how an attached file relates to the document
(PDF 2.0 `AFRelationship`), for `attach()`:

| Case | Value | Use for |
|---|---|---|
| `Relationship::Source` | `'source'` | The source the document was made from. |
| `Relationship::Data` | `'data'` | Data the document shows, such as a table as CSV. |
| `Relationship::Alternative` | `'alternative'` | Another representation of the same content. |
| `Relationship::Supplement` | `'supplement'` | Additional material. |
| `Relationship::Unspecified` | `'unspecified'` | None of these; the default. |

## Exceptions

All exceptions extend `Fagus\Exception\FagusException`, which extends PHP's `RuntimeException`.
Catch `FagusException` to handle anything that goes wrong in Fagus.

```php
use Fagus\Exception\EngineException;
use Fagus\Exception\RenderException;

try {
    $pdf = Fagus::html($html)->accessible()->render();
} catch (RenderException $exception) {
    // The document cannot be rendered as asked, or has diagnostics in strict mode.
    report($exception);
    foreach ($exception->diagnostics() as $diagnostic) {
        // those strict mode refused, or those found before rendering failed
    }
} catch (EngineException $exception) {
    // The binary was not found, could not be started, timed out or stopped.
}
```

| Exception | Thrown when |
|---|---|
| `FagusException` | Base class. Thrown itself by `Pdf::save()` when the file cannot be written. |
| `RenderException` | The document cannot be rendered (for example PDF/UA requirements that are not met), or has diagnostics in strict mode. `diagnostics(): list<Diagnostic>` returns the diagnostics strict mode refused, or those found before rendering failed (CSS that was ignored, resources that are missing), which often explain the failure. |
| `EngineException` | The binary was not found or could not be started, a font directory does not exist, the worker did not answer within the timeout, it stopped, it speaks another protocol, or a font passed to `addFont()` cannot be read. When the worker has exited, the message ends with its exit code and what it wrote to its error output. |
| `InvalidArgumentException` | A value Fagus does not know, such as a PDF/A level passed to `pdfA()` as a string that is not one. |
| `InstallException` | The binary could not be installed: a development version without binaries, an unsupported platform, a failed download, a checksum mismatch, or a directory that cannot be written. Thrown by `Installer`, `vendor/bin/fagus-install` and `php artisan fagus:install`; the Composer plugin turns it into a warning. |

The exact messages are listed in [troubleshooting.md](troubleshooting.md).

## `Installer`

`Fagus\Installer` installs the binary of the package's release for the platform PHP runs on. The
Composer plugin, `vendor/bin/fagus-install` and `php artisan fagus:install` use it; you need it
only to install from your own code.

```php
use Fagus\Installer;

$path = (new Installer(
    token: null,          // GitHub token for a private repository; FAGUS_GITHUB_TOKEN or GITHUB_TOKEN if null
    source: null,         // a directory or URL with the files of the release; FAGUS_DOWNLOAD_URL if null
    cacheDirectory: null, // where downloaded binaries are kept for the next installation
))->install();            // or ->install('/opt/fagus') for another directory

Installer::target();          // "x86_64-unknown-linux-musl" on Alpine on x64
Installer::asset('x86_64-pc-windows-msvc'); // "fagus-x86_64-pc-windows-msvc.exe"
```

`install(?string $directory = null): string` returns the path of the binary. A binary that is
already there with the right SHA-256 is kept. See [deployment.md](deployment.md).
