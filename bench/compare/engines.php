<?php

// The PHP engines of the comparison, for bench.php and ../memory/engine.php: renderer() returns a
// function that renders the document with the engine and returns the PDF. Engines: fagus, dompdf,
// mpdf, browsershot.

require __DIR__.'/vendor/autoload.php';
require dirname(__DIR__, 2).'/vendor/autoload.php';

function renderer(string $engine, string $file): Closure
{
    $repo = dirname(__DIR__, 2);
    $html = file_get_contents($file);
    $directory = dirname($file);
    $chrome = getenv('CHROME') ?: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

    return match ($engine) {
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
}
