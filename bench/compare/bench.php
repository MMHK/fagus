<?php

// php bench.php <engine> <document.html> <repeats>
//
// Renders the document once in this new process, then <repeats> more times, and prints JSON with
// the time of the first render and the median of the repeats in milliseconds. The PDF is written
// to out/<engine>-<document>.pdf. Engines: fagus, dompdf, mpdf, browsershot.

[, $engine, $file, $repeats] = $argv;
$repo = dirname(__DIR__, 2);
require __DIR__.'/vendor/autoload.php';
require $repo.'/vendor/autoload.php';

$file = realpath($file);
$html = file_get_contents($file);
$directory = dirname($file);
$chrome = getenv('CHROME') ?: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

$render = match ($engine) {
    'fagus' => (function () use ($html, $directory, $repo) {
        $fagus = new Fagus\Engine($repo.'/target/release/fagus');

        return fn () => Fagus\Fagus::html($html)->baseDirectory($directory)->render($fagus)->bytes();
    })(),
    'dompdf' => function () use ($html, $directory) {
        $options = new Dompdf\Options();
        $options->setChroot($directory);
        $options->setFontCache(sys_get_temp_dir());
        $dompdf = new Dompdf\Dompdf($options);
        $dompdf->setBasePath($directory.'/');
        $dompdf->loadHtml($html);
        $dompdf->render();

        return $dompdf->output();
    },
    'mpdf' => function () use ($html, $directory) {
        $mpdf = new Mpdf\Mpdf(['tempDir' => sys_get_temp_dir().'/mpdf']);
        $mpdf->SetBasePath($directory.'/');
        $mpdf->WriteHTML($html);

        return $mpdf->Output('', 'S');
    },
    'browsershot' => fn () => Spatie\Browsershot\Browsershot::htmlFromFilePath($file)
        ->allowLocalFileAccess()
        ->setChromePath($chrome)
        ->setNodeModulePath(__DIR__.'/node_modules')
        ->showBackground()
        ->pdf(),
};

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
