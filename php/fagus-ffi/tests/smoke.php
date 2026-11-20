<?php

declare(strict_types=1);

/*
 * Renders documents through the PHP package, against a real shared library:
 *
 *     php -d ffi.enable=true php/fagus-ffi/tests/smoke.php <libfagus_ffi.so|fagus_ffi.dll>
 *
 * Plain PHP, because PHPUnit 12 needs PHP 8.3 and this package supports 7.4.
 */

require __DIR__.'/autoload.php';

use Fagus\Ffi\Document;
use Fagus\Ffi\Engine;
use Fagus\Ffi\Exception\RenderException;

$library = $argv[1] ?? (getenv('FAGUS_FFI_LIB') ?: null);
$temporary = sys_get_temp_dir().'/fagus-ffi-smoke-'.bin2hex(random_bytes(6));
if (! mkdir($temporary, 0700, true) && ! is_dir($temporary)) {
    fwrite(STDERR, "cannot create $temporary\n");
    exit(2);
}

$failures = 0;
$check = static function (bool $condition, string $message) use (&$failures): void {
    if ($condition) {
        return;
    }
    ++$failures;
    fwrite(STDERR, "FAIL: $message\n");
};

try {
    printf("php %s, ffi.enable=%s\n", PHP_VERSION, (string) ini_get('ffi.enable'));

    $engine = new Engine([], $library);
    printf("library %s, version %s\n", $engine->native()->library(), $engine->version());
    $check($engine->version() !== '', 'the library reports a version');

    $html = '<!doctype html><html lang="en"><head><title>Smoke</title></head><body>'
        .'<h1 id="title">Fagus</h1><p class="note">Rendered through PHP FFI.</p></body></html>';

    $pdf = $engine->html($html)
        ->stylesheet('.note { color: teal }', 'smoke.css')
        ->created('2026-11-20T00:00:00Z')
        ->reportLayout()
        ->render();
    $check(substr($pdf->bytes(), 0, 5) === '%PDF-', 'the PDF starts with %PDF-');
    $check($pdf->pages() >= 1, 'the document has a page');
    $check($pdf->size() === strlen($pdf->bytes()), 'the size is the length of the bytes');
    $layout = $pdf->layout();
    $check(
        is_array($layout) && isset($layout['elements'][0]) && $layout['elements'][0]['id'] === 'title',
        'the layout reports the element with an id'
    );
    $check(isset($layout['elements'][0]['boxes'][0]['page']), 'the element has its box on a page');
    $check($pdf->diagnostics() === [], 'a clean document has no diagnostics');
    printf("basic: %d bytes, %d pages, %d diagnostics\n", $pdf->size(), $pdf->pages(), count($pdf->diagnostics()));
    $pdf->save($temporary.'/basic.pdf');
    $check(is_file($temporary.'/basic.pdf') && filesize($temporary.'/basic.pdf') === $pdf->size(), 'the PDF is saved');

    $warned = $engine->html($html)->stylesheet('p { colr: teal }')->render();
    $check(count($warned->diagnostics()) > 0, 'an unknown property makes a diagnostic');
    $check($warned->diagnostics()[0]->code() === 'css.unknown-property', 'the diagnostic has its code');
    printf("diagnostics: %s\n", $warned->diagnostics()[0]->code());

    $attached = $engine->html($html)
        ->pdfA('3b')
        ->attach('data.csv', 'text/csv', "a,b\n1,2\n", 'supplement', 'Raw data')
        ->render();
    $check(substr($attached->bytes(), 0, 5) === '%PDF-', 'the PDF/A-3b document renders');

    $encrypted = $engine->html($html)->encrypt('open sesame', 'change me', ['print'])->render();
    $check(substr($encrypted->bytes(), 0, 5) === '%PDF-', 'the encrypted document renders');

    try {
        $engine->html($html)->pdfA('9z')->render();
        $check(false, 'an unknown PDF/A level is refused');
    } catch (RenderException $error) {
        $check(strpos($error->getMessage(), '9z') !== false, 'the refusal names the level');
        printf("refused: %s\n", $error->getMessage());
    }

    try {
        (new Document())->render($engine);
        $check(false, 'a document without HTML is refused');
    } catch (RenderException $error) {
        $check(strpos($error->getMessage(), 'HTML') !== false, 'the refusal names the HTML');
    }

    $engine->close();
} catch (\Throwable $error) {
    ++$failures;
    fwrite(STDERR, 'FAIL: '.get_class($error).': '.$error->getMessage()."\n");
}

foreach (glob($temporary.'/*') ?: [] as $file) {
    @unlink($file);
}
@rmdir($temporary);

if ($failures > 0) {
    fwrite(STDERR, "$failures failure(s)\n");
    exit(1);
}
echo "PASS\n";
