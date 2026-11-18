<?php

// php engine.php <engine> <document.html> <repeats> [marked|free]
//
// bench.php of ../compare with phase markers for driver.py: renders once, prints "@@cold", waits
// for a line on its input, renders <repeats> times, prints "@@warm", waits again, then prints JSON
// with the times in milliseconds. "free" does not wait (for several processes at once). The PDF is
// written to out/<engine>-<document>.pdf. Engines: fagus, dompdf, mpdf, browsershot.

[, $engine, $file, $repeats] = $argv;
$marked = ($argv[4] ?? 'marked') === 'marked';
require dirname(__DIR__).'/compare/engines.php';

$file = realpath($file);
$render = renderer($engine, $file);

function mark(string $marker, bool $marked): void
{
    echo "@@$marker\n";
    fflush(STDOUT);
    if ($marked) {
        fgets(STDIN);
    }
}

$start = hrtime(true);
$pdf = $render();
$first = (hrtime(true) - $start) / 1e6;
mark('cold', $marked);
$warmStart = microtime(true);
$times = [];
for ($i = 0; $i < (int) $repeats; $i++) {
    $start = hrtime(true);
    $pdf = $render();
    $times[] = (hrtime(true) - $start) / 1e6;
}
$warmEnd = microtime(true);
mark('warm', $marked);
sort($times);

@mkdir(__DIR__.'/out');
file_put_contents(__DIR__.'/out/'.$engine.'-'.basename($file, '.html').'.pdf', $pdf);
echo json_encode([
    'first' => round($first, 1),
    'median' => $times ? round($times[intdiv(count($times), 2)], 1) : null,
    'mean' => $times ? round(array_sum($times) / count($times), 1) : null,
    'n' => count($times),
    'warm_start' => $warmStart,
    'warm_end' => $warmEnd,
]), "\n";
