# Laravel

The package `nforst/fagus` includes its Laravel integration (`Fagus\Laravel\`, in
`php/laravel/src`). Laravel 13 discovers it on its own: `composer require nforst/fagus` registers
the service provider, the `Fagus` facade, the configuration and the `fagus:install` command.

```php
use Fagus\Laravel\Facades\Fagus;

// In a controller: shown in the browser.
return Fagus::view('invoices.show', ['invoice' => $invoice])->name('invoice.pdf');

// As a download, on a disk, or as bytes.
return Fagus::view('invoices.show', $data)->download('invoice.pdf');
Fagus::view('invoices.show', $data)->pdfA('3b')->store('invoices/2026-0042.pdf', 's3');
$bytes = Fagus::html($html)->render()->bytes();
```

Installation is described in [getting-started.md](getting-started.md).

## The facade

`Fagus\Laravel\Facades\Fagus` (also registered as the alias `Fagus`) resolves the service
`fagus`, an instance of `Fagus\Laravel\PdfFactory`. It has four methods:

| Method | Returns |
|---|---|
| `Fagus::view(string $view, array $data = [], array $mergeData = [])` | A `PendingPdf` from a Blade view, rendered with `view()->make($view, $data, $mergeData)`. |
| `Fagus::html(string $html)` | A `PendingPdf` from HTML. |
| `Fagus::xhtml(string $xml)` | A `PendingPdf` from XHTML, parsed as XML (see [php.md](php.md#fagus)). |
| `Fagus::engine()` | The application's `Fagus\Engine`, for `addFont()` and `version()`. |

Every `PendingPdf` starts with the base directory, the root and the tagging of the
configuration.

Do not confuse the facade with the plain PHP class `Fagus\Fagus`. The class does not read
`config/fagus.php` and renders with an engine of its own; in Laravel, import
`Fagus\Laravel\Facades\Fagus`.

You can inject the factory instead of using the facade:

```php
use Fagus\Laravel\PdfFactory;

final class InvoiceMailer
{
    public function __construct(private PdfFactory $pdfs) {}

    public function attachment(Invoice $invoice): string
    {
        return $this->pdfs->view('invoices.show', ['invoice' => $invoice])->render()->bytes();
    }
}
```

`Fagus\Engine` is bound as a singleton too, so `app(Engine::class)` is the same engine as
`Fagus::engine()`.

## `PendingPdf`

`Fagus\Laravel\PendingPdf` wraps a `Fagus\Document` together with the engine. Like the document,
it is immutable: every method returns a changed copy.

### Document methods

These forward to `Fagus\Document` and take the same arguments; [php.md](php.md#document)
describes each one:

`baseDirectory()`, `root()`, `stylesheet()`, `resource()`, `resolver()`, `pdfA()`, `tagged()`,
`accessible()`, `attach()`, `facturX()`, `created()`, `strict()`.

```php
use Fagus\Conformance;
use Illuminate\Support\Facades\Vite;

Fagus::view('invoices.show', ['invoice' => $invoice])
    ->stylesheet(Vite::content('resources/css/pdf.css'))
    ->resource('logo.png', $invoice->company->logo)
    ->pdfA(Conformance::PdfA3b)
    ->facturX($invoice->toFacturX());
```

### `name(string $filename): PendingPdf`

The file name for the browser and for downloads. `document.pdf` by default.

### `inline(?string $filename = null): Response`

Renders the PDF and returns a response that shows it in the browser
(`Content-Disposition: inline`). The file name is `$filename`, else the one from `name()`.

### `download(?string $filename = null): Response`

Renders the PDF and returns a response that downloads it (`Content-Disposition: attachment`).

```php
return Fagus::view('invoices.show', $data)->download("invoice-{$invoice->number}.pdf");
```

Both responses are `Illuminate\Http\Response` with `Content-Type: application/pdf`. File names
with characters outside ASCII are sent as `filename*=utf-8''…` with an ASCII fallback, so
`Übersicht März.pdf` arrives as written.

### `toResponse($request): Response`

`PendingPdf` implements `Responsable`: returned from a controller or route, it renders the PDF and
shows it inline, as `inline()` does.

```php
Route::get('/invoices/{invoice}/pdf', fn (Invoice $invoice) => Fagus::view('invoices.show', [
    'invoice' => $invoice,
])->name("invoice-{$invoice->number}.pdf"));
```

### `store(string $path, ?string $disk = null): Pdf`

Renders the PDF and writes it to a filesystem disk, the default disk if `$disk` is `null`.
Returns the `Fagus\Pdf`.

```php
$pdf = Fagus::view('invoices.show', $data)->store("invoices/{$invoice->number}.pdf", 's3');
$pdf->pages();
```

### `save(string $path): Pdf`

Renders the PDF and writes it to a local path.

### `render(): Pdf`

Renders the PDF and returns the `Fagus\Pdf` with its bytes, page count, diagnostics and timings
(see [php.md](php.md#pdf)).

```php
$pdf = Fagus::view('reports.monthly', $data)->render();

Mail::to($customer)->send(new MonthlyReport($pdf->bytes()));
```

### `document(): Document`

The underlying `Fagus\Document`, for code that works with the plain PHP API.

## Queues

Render PDFs in a job when they take long or when nobody waits for them:

```php
use Fagus\Laravel\Facades\Fagus;
use Illuminate\Contracts\Queue\ShouldQueue;
use Illuminate\Foundation\Queue\Queueable;

final class StoreInvoicePdf implements ShouldQueue
{
    use Queueable;

    public function __construct(public Invoice $invoice) {}

    public function handle(): void
    {
        Fagus::view('invoices.show', ['invoice' => $this->invoice])
            ->pdfA('3b')
            ->store("invoices/{$this->invoice->number}.pdf", 's3');
    }
}
```

The engine is a singleton. In `php artisan queue:work` (and in Horizon's workers), the worker
process starts with the first PDF and serves every following job of that queue worker, so a job
pays only for rendering. `php artisan queue:listen` boots the application for every job, which
starts a new worker process each time.

After deploying a new version of the package, restart the queue workers
(`php artisan queue:restart`) so that they use the new binary.

## Octane and FrankenPHP

Under PHP-FPM, the engine lives as long as the request: the first PDF of each request starts the
worker process (a few milliseconds), and the request's end stops it.

Under Octane (Swoole, RoadRunner or FrankenPHP), the application boots once per Octane worker.
Octane gives each request a copy of the application's container and throws away what a request
resolved when it ends: an engine resolved in a request would start a worker process and stop it
again at the request's end. So when Octane is installed, the service provider adds
`Fagus\Engine` to the `warm` services of `config/octane.php`, which Octane resolves when its
worker boots and shares with every request. One engine, and its worker process, then serves every
PDF of the Octane worker; there is nothing to configure. Listing `\Fagus\Engine::class` in `warm`
yourself does no harm.

More on worker processes, memory and timeouts: [deployment.md](deployment.md#worker-processes).

## Configuration

Publish the configuration file to change it:

```sh
php artisan vendor:publish --tag=fagus-config
```

This writes `config/fagus.php`. Every key, with its environment variable and default:

| Key | Environment variable | Default | Meaning |
|---|---|---|---|
| `binary` | `FAGUS_BINARY` | `null` | The path of the `fagus` binary. If `null`: the one the package installed (with Composer or `php artisan fagus:install`), else the one on the `PATH`. A path that is set must exist. |
| `fonts` | – | `[resource_path('fonts')]` | Directories with fonts for every document: TTF, OTF, TTC, OTC, WOFF and WOFF2 files, in the directory and its subdirectories. Directories that do not exist are skipped, so the default works without a `resources/fonts`; a plain `Fagus\Engine` throws an `EngineException` for them instead. |
| `system_fonts` | `FAGUS_SYSTEM_FONTS` | `false` | Whether the fonts installed on the machine serve too, as in Chrome. Off, so that documents look the same on every machine. |
| `locale` | `FAGUS_LOCALE` | `null` | The language of the texts that form controls show of their own (button labels, date formats) and of the PDF where a document gives no `lang`, such as `de-DE`. American English if `null`. |
| `families` | – | `['sans-serif' => null, 'serif' => null, 'monospace' => null]` | The families that the generic families name. |
| `base_directory` | – | `public_path()` | The directory that relative URLs in documents resolve against. Only files inside it, or inside `root`, are read. |
| `root` | – | `null` | A larger directory that contains `base_directory`, for documents whose URLs lead out of it (`../fonts/…`). URLs that start with `/` resolve against it. |
| `tagged` | – | `true` | Whether PDFs are tagged with the logical structure that screen readers read. Single documents can say otherwise with `tagged()` or ask for PDF/UA with `accessible()`. |
| `timeout` | – | `60` | Seconds to wait for each answer of the worker before it is stopped. Not on Windows, where PHP cannot wait for a pipe with a time limit. |

`binary`, `fonts`, `system_fonts`, `locale`, `families` and `timeout` configure the engine;
`base_directory`, `root` and `tagged` apply to each new `PendingPdf`.

```php
// config/fagus.php
return [
    'binary' => env('FAGUS_BINARY'),
    'fonts' => [resource_path('fonts')],
    'system_fonts' => env('FAGUS_SYSTEM_FONTS', false),
    'locale' => env('FAGUS_LOCALE', 'de-DE'),
    'families' => [
        'sans-serif' => 'Inter',
        'serif' => null,
        'monospace' => 'JetBrains Mono',
    ],
    'base_directory' => resource_path('pdf'),
    'root' => resource_path(),
    'tagged' => true,
    'timeout' => 30,
];
```

With `base_directory` at `public_path()`, a view refers to files of `public/` as a page of the
site would: `<img src="/images/logo.png">` reads `public/images/logo.png`. Absolute URLs from
`asset()`, `url()` or `@vite` start with `http` and are not loaded; see
[resources.md](resources.md#laravel-assets-and-vite) for how to use them.

## `php artisan fagus:install`

Installs the `fagus` binary of the package's release for the platform PHP runs on, as the
Composer plugin does after `composer install`:

```sh
php artisan fagus:install              # where the engine finds it
php artisan fagus:install /opt/fagus   # into another directory
```

It takes `FAGUS_DOWNLOAD_URL` and `FAGUS_GITHUB_TOKEN` (or `GITHUB_TOKEN`) from the environment,
like `vendor/bin/fagus-install`. A binary installed into another directory must be named in
`FAGUS_BINARY` (or the `binary` key) or lie on the `PATH`. See [deployment.md](deployment.md).

## Diagnostics and errors

`render()`, `store()` and `save()` return the `Fagus\Pdf`, whose `diagnostics()` lists CSS that
was ignored and resources that were not found. Log them while you build a template:

```php
$pdf = Fagus::view('invoices.show', $data)->render();

foreach ($pdf->diagnostics() as $diagnostic) {
    Log::warning((string) $diagnostic);
}
```

`inline()` and `download()` render and discard the diagnostics. Errors are exceptions
(`Fagus\Exception\RenderException`, `EngineException`), which Laravel reports as usual. A
`RenderException` keeps the diagnostics found before the document was refused in its
`diagnostics()`. See [troubleshooting.md](troubleshooting.md).

## Testing

The package has no fake. PDFs render in a few milliseconds, so tests can render them with the real
binary: the Composer plugin installs it with `composer install` on the developer's machine and in
CI, as long as `allow-plugins` permits it.

Assert on the response of a route:

```php
public function test_the_invoice_pdf_is_shown(): void
{
    $invoice = Invoice::factory()->create();

    $response = $this->get("/invoices/{$invoice->id}/pdf");

    $response->assertOk();
    $response->assertHeader('Content-Type', 'application/pdf');
    $this->assertStringStartsWith('%PDF-', $response->getContent());
}
```

Fake the disk to check `store()`:

```php
Storage::fake('s3');

StoreInvoicePdf::dispatchSync($invoice);

Storage::disk('s3')->assertExists("invoices/{$invoice->number}.pdf");
```

Make a template fail on every ignored declaration and missing resource with `strict()`:

```php
public function test_the_invoice_template_is_clean(): void
{
    $pdf = Fagus::view('invoices.show', ['invoice' => Invoice::factory()->make()])
        ->strict()
        ->render();

    $this->assertSame(1, $pdf->pages());
}
```

To render with a binary of your own in tests, set `FAGUS_BINARY` in `phpunit.xml`:

```xml
<php>
    <env name="FAGUS_BINARY" value="/usr/local/bin/fagus"/>
</php>
```
