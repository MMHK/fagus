<?php

// php bench.php <engine> <document.html> <repeats>
//
// Renders the document once in this new process, then <repeats> more times, and prints JSON with
// the time of the first render and the median of the repeats in milliseconds. The PDF is written
// to out/<engine>-<document>.pdf. Engines: fagus, dompdf, mpdf, browsershot (see engines.php).

[, $engine, $file, $repeats] = $argv;
require __DIR__.'/engines.php';

$file = realpath($file);
$render = renderer($engine, $file);

$start = hrtime(true);
$pdf = $render();
$first = (hrtime(true) - $start) / 1e6;
$times = [];
for ($i = 0; $i < (int) $repeats; $i++) {
    $start = hrtime(true);
    $pdf = $render();
    $times[] = (hrtime(true) - $start) / 1e6;
}
sort($times);

@mkdir(__DIR__.'/out');
file_put_contents(__DIR__.'/out/'.$engine.'-'.basename($file, '.html').'.pdf', $pdf);
echo json_encode([
    'first' => round($first, 1),
    'repeat' => $times ? round($times[intdiv(count($times), 2)], 1) : null,
]), "\n";
