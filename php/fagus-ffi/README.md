# Fagus for PHP through FFI

Renders HTML and CSS to PDF in-process through the C ABI of the Fagus engine: no browser and no
worker process. This package is the low-level layer for **PHP 7.4 and later**; the `nforst/fagus`
package is the full API for PHP 8.3 and later, over the worker protocol.

## Requirements

- PHP 7.4 or later, non-thread-safe, with `ext-ffi`
- `ffi.enable=true` in `php.ini` for CLI and FPM (`preload` by default; a preload file works too)
- the shared library for the platform: `libfagus_ffi.so` (Linux, glibc 2.17 or later, x86_64 and
  aarch64) or `fagus_ffi.dll` (Windows, x64, one DLL for PHP 7.4 to 8.4)

Point `FAGUS_FFI_LIB` at the library, or pass its path to `new Engine()`.

## Usage

```php
use Fagus\Ffi\Engine;

$engine = new Engine([
    'fontDirs' => [__DIR__.'/fonts'],
    'systemFonts' => true,
]);

$pdf = $engine->html('<h1>Invoice</h1><p>Rendered in-process.</p>')
    ->stylesheet('h1 { color: teal }')
    ->pdfA('3b')
    ->render();

$pdf->save(__DIR__.'/invoice.pdf');
printf("%d pages, %d diagnostics\n", $pdf->pages(), count($pdf->diagnostics()));
```

`Engine` keeps fonts and caches warm, so reuse it for many documents. `Document` has the options
of the worker protocol: `baseDirectory()`, `root()`, `stylesheet()`, `resource()`, `pdfA()`,
`tagged()`, `accessible()`, `forms()`, `attach()`, `facturX()`, `created()`, `encrypt()`, `sign()`,
`memoryBudget()`, `timeLimit()` and `reportLayout()`. `Pdf` exposes the bytes, the page count, the
diagnostics, the timings and the layout.

Signing is available with a PKCS#12 file:

```php
use Fagus\Ffi\Signature;

$pdf = $engine->html($html)
    ->sign(Signature::pkcs12(file_get_contents('certificate.p12'), 'secret')->reason('Invoice 42'))
    ->render();
```

Limits of the C ABI for now:

- Signatures are PKCS#12 only: external signers, timestamps and revocation data need callbacks
  into PHP, which the worker protocol provides instead.
- Resources must be given with `resource()`; the engine cannot call back into PHP to ask for one.
- Strict mode is not part of the engine settings.
- The library has no sandbox: a library loaded into PHP runs in the PHP process, unlike the
  worker, which can be sandboxed with Landlock and seccomp on Linux.

## Tests

```sh
php -d ffi.enable=true php/fagus-ffi/tests/smoke.php path/to/libfagus_ffi.so
```
