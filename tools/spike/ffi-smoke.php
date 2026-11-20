<?php

declare(strict_types=1);

/*
 * SPIKE: loads the C ABI shared library with PHP's FFI extension and checks the surface that the
 * plan (docs/plans/2026-11-20-php-ffi-cdylib.md) depends on. Rendering is not implemented yet;
 * fagus_spike_probe() renders one tiny document in-process.
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

$abi = $ffi->fagus_abi_version();
printf("abi=%d version=%s\n", $abi, $cString($ffi->fagus_version()));
if ($abi !== 1) {
    fwrite(STDERR, "unexpected ABI version\n");
    exit(1);
}

$engine = $ffi->fagus_engine_new(null, 0);
if (FFI::isNull($engine)) {
    fwrite(STDERR, "fagus_engine_new returned null\n");
    exit(1);
}

$pdf = $ffi->new('uint8_t*');
$pdfLen = $ffi->new('size_t');
$meta = $ffi->new('uint8_t*');
$metaLen = $ffi->new('size_t');
$status = $ffi->fagus_render(
    $engine,
    null, 0,
    null, 0,
    FFI::addr($pdf), FFI::addr($pdfLen),
    FFI::addr($meta), FFI::addr($metaLen),
);
printf("render stub: status=%d message=%s\n", $status, $cString($ffi->fagus_last_error()));
if ($status !== -1) {
    fwrite(STDERR, "unexpected status from the stub\n");
    exit(1);
}

$bytes = $ffi->fagus_spike_probe();
printf("probe: %d bytes of PDF\n", $bytes);
if ($bytes < 5) {
    fwrite(STDERR, "the engine did not render\n");
    exit(1);
}

$ffi->fagus_engine_free($engine);
echo "PASS\n";
