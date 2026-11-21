<?php

declare(strict_types=1);

/*
 * Renders a document through the package, so that the PDF can be compared with the one the
 * command line tool writes for the same HTML and the same `--created` (they must be identical):
 *
 *     php -d ffi.enable=true php/fagus-ffi/tests/parity.php \
 *         <libfagus_ffi.so|fagus_ffi.dll> <document.html> <out.pdf> <created>
 *
 * Plain PHP, like smoke.php: PHPUnit 12 needs PHP 8.3 and this package supports 7.4.
 */

require __DIR__.'/autoload.php';

use Fagus\Ffi\Engine;

if ($argc !== 5) {
    fwrite(STDERR, "usage: parity.php <library> <document.html> <out.pdf> <created>\n");
    exit(2);
}

[, $library, $input, $output, $created] = $argv;

$html = file_get_contents($input);
if ($html === false) {
    fwrite(STDERR, "cannot read $input\n");
    exit(1);
}

$engine = new Engine([], $library);
$pdf = $engine->html($html)->created($created)->render();
file_put_contents($output, $pdf->bytes());

printf("%s: %d bytes, %d pages\n", $output, $pdf->size(), $pdf->pages());
