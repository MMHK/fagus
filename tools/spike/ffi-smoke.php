<?php

declare(strict_types=1);

/*
 * Loads the C ABI shared library with PHP's FFI extension and renders one document in-process,
 * through the same request the worker takes (docs/plans/2026-11-20-php-ffi-cdylib.md).
 *
 *     php -d ffi.enable=true tools/spike/ffi-smoke.php <libfagus_ffi.so|fagus_ffi.dll>
 */

$lib = $argv[1] ?? (getenv('FAGUS_FFI_LIB') ?: null);
if ($lib === null || ! is_file($lib)) {
    fwrite(STDERR, "usage: php tools/spike/ffi-smoke.php <libfagus_ffi.so|fagus_ffi.dll>\n");
    exit(2);
}
$lib = realpath($lib);

$header = file_get_contents(__DIR__.'/../../crates/fagus-ffi/include/fagus.h');
if ($header === false) {
    fwrite(STDERR, "cannot read fagus.h\n");
    exit(2);
}

printf("php %s, ffi.enable=%s, lib=%s\n", PHP_VERSION, (string) ini_get('ffi.enable'), $lib);

if (! extension_loaded('ffi')) {
    fwrite(STDERR, "the ffi extension is not loaded\n");
    exit(1);
}

$ffi = FFI::cdef($header, $lib);

/* PHP 7.4 returns a `const char *` as a PHP string; later versions may return CData. */
$cString = static function ($value): string {
    return is_string($value) ? $value : FFI::string($value);
};

$fail = static function (string $message) use ($ffi, $cString): void {
    fwrite(STDERR, 'FAIL: '.$message.' ('.$cString($ffi->fagus_last_error()).")\n");
    exit(1);
};

$abi = $ffi->fagus_abi_version();
printf("abi=%d version=%s\n", $abi, $cString($ffi->fagus_version()));
if ($abi !== 1) {
    $fail('unexpected ABI version');
}

$config = '{}';
$engine = $ffi->fagus_engine_new($config, strlen($config));
if (FFI::isNull($engine)) {
    $fail('fagus_engine_new returned null');
}

$html = '<h1>Spike</h1><p>Rendered in-process through PHP FFI.</p>';
$request = json_encode([
    'id' => 1,
    'html' => 0,
    'blobLengths' => [strlen($html)],
    'created' => '2026-11-20T00:00:00Z',
    'layout' => true,
], JSON_THROW_ON_ERROR);

$blob = $ffi->new('uint8_t['.strlen($html).']');
FFI::memcpy($blob, $html, strlen($html));

$pdf = $ffi->new('uint8_t*');
$pdfLen = $ffi->new('size_t');
$meta = $ffi->new('uint8_t*');
$metaLen = $ffi->new('size_t');
$status = $ffi->fagus_render(
    $engine,
    $request, strlen($request),
    $blob, strlen($html),
    FFI::addr($pdf), FFI::addr($pdfLen),
    FFI::addr($meta), FFI::addr($metaLen),
);
if ($status !== 0) {
    $fail('fagus_render returned '.$status);
}

$bytes = FFI::string($pdf, $pdfLen->cdata);
printf("pdf: %d bytes\n", strlen($bytes));
if (substr($bytes, 0, 5) !== '%PDF-') {
    $fail('the PDF does not start with %PDF-');
}

$info = json_decode(FFI::string($meta, $metaLen->cdata), true, 512, JSON_THROW_ON_ERROR);
printf("meta: %d pages, %d diagnostics, layout %s\n",
    $info['pages'] ?? 0,
    count($info['diagnostics'] ?? []),
    isset($info['layout']) ? 'yes' : 'no',
);
if (($info['pages'] ?? 0) < 1) {
    $fail('the document has no pages');
}

$ffi->fagus_bytes_free($pdf, $pdfLen->cdata);
$ffi->fagus_bytes_free($meta, $metaLen->cdata);
$ffi->fagus_engine_free($engine);
echo "PASS\n";
