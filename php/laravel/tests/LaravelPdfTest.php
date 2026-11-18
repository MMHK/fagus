<?php

declare(strict_types=1);

namespace Fagus\Laravel\Tests;

use Closure;
use Fagus\Laravel\Facades\Fagus;
use Fagus\Laravel\FagusServiceProvider;
use Fagus\Laravel\LaravelPdf\FagusDriver;
use Fagus\Laravel\PdfFactory;
use Fagus\Laravel\PendingPdf;
use Illuminate\Support\Facades\Route;
use Illuminate\Support\Facades\Storage;
use Orchestra\Testbench\Attributes\DefineEnvironment;
use PHPUnit\Framework\Attributes\Test;
use Spatie\LaravelPdf\Drivers\PdfDriver;
use Spatie\LaravelPdf\Enums\Format;
use Spatie\LaravelPdf\Enums\Unit;
use Spatie\LaravelPdf\Exceptions\CouldNotGeneratePdf;
use Spatie\LaravelPdf\Exceptions\InvalidDriver;
use Spatie\LaravelPdf\Facades\Pdf;
use Spatie\LaravelPdf\PdfBuilder;
use Spatie\LaravelPdf\PdfOptions;
use Spatie\LaravelPdf\PdfServiceProvider;

final class LaravelPdfTest extends TestCase
{
    private const string TWO_PAGES = '<!doctype html><html lang="de"><head><style>body { font-family: Inter }</style></head>'
        .'<body><h1>Rechnung</h1><p style="break-before: page">Seite zwei</p></body></html>';

    protected function getPackageProviders($app): array
    {
        return [PdfServiceProvider::class, FagusServiceProvider::class];
    }

    #[Test]
    public function it_renders_views_with_the_fagus_driver(): void
    {
        $bytes = Pdf::view('invoice', ['number' => 'RE-1', 'lines' => ['Beratung']])->driver('fagus')->generatePdfContent();

        $this->assertStringStartsWith('%PDF-', $bytes);
        $this->assertSame(self::drawn(Fagus::view('invoice', ['number' => 'RE-1', 'lines' => ['Beratung']])), self::drawn($bytes));
    }

    #[Test]
    #[DefineEnvironment('withFagusAsDefault')]
    public function the_configuration_can_make_it_the_default_driver(): void
    {
        $this->assertInstanceOf(FagusDriver::class, $this->app->make(PdfDriver::class));
        $this->assertSame($this->app->make('laravel-pdf.driver.fagus'), $this->app->make(PdfDriver::class));
        $this->assertStringStartsWith('%PDF-', Pdf::html('<p>Hallo</p>')->generatePdfContent());
    }

    protected function withFagusAsDefault($app): void
    {
        $app['config']->set('laravel-pdf.driver', 'fagus');
    }

    #[Test]
    #[DefineEnvironment('withUnknownDriver')]
    public function other_driver_names_stay_with_laravel_pdf(): void
    {
        $this->expectException(InvalidDriver::class);

        $this->app->make(PdfDriver::class);
    }

    protected function withUnknownDriver($app): void
    {
        $app['config']->set('laravel-pdf.driver', 'unknown');
    }

    #[Test]
    public function it_leaves_the_page_of_the_document_alone_without_options(): void
    {
        $html = '<style>@page { size: A5; margin: 10mm }</style><p>Hallo</p>';

        $this->assertSame(['0 0 420 594.96'], self::mediaBoxes(Pdf::html($html)->driver('fagus')->generatePdfContent()));
        $this->assertSame('', $this->stylesheetOf(Pdf::html($html)));
    }

    #[Test]
    public function the_options_of_the_builder_win_over_the_page_of_the_document(): void
    {
        $html = '<style>@page { size: A5; margin: 10mm }</style><p>Hallo</p>';

        $this->assertSame(['0 0 841.92 594.96'], self::mediaBoxes(Pdf::html($html)->driver('fagus')->format(Format::A4)->landscape()->generatePdfContent()));
        $this->assertSame(['0 0 612 792'], self::mediaBoxes(Pdf::html($html)->driver('fagus')->format('Letter')->portrait()->generatePdfContent()));
        $this->assertSame(['0 0 297.12 420'], self::mediaBoxes(Pdf::html($html)->driver('fagus')->format(Format::A6)->generatePdfContent()));
        $this->assertSame(['0 0 162 1416.96'], self::mediaBoxes(Pdf::html($html)->driver('fagus')->paperSize(57, 500)->generatePdfContent()));
        $this->assertSame(['0 0 1416.96 162'], self::mediaBoxes(Pdf::html($html)->driver('fagus')->paperSize(57, 500)->landscape()->generatePdfContent()));
        $this->assertSame(['0 0 841.92 594.96'], self::mediaBoxes(Pdf::html('<p>Hallo</p>')->driver('fagus')->landscape()->generatePdfContent()), 'A4, turned');
    }

    #[Test]
    public function it_writes_the_options_as_a_page_rule(): void
    {
        $builder = Pdf::html('<p>Hallo</p>')->paperSize(80.5, 200, Unit::Millimeter)->margins(10, 5.25, 12, 5.25);

        $this->assertSame("@page { size: 80.5mm 200mm; margin: 10mm 5.25mm 12mm 5.25mm; }\n", $this->stylesheetOf($builder));
        $this->assertSame("@page { margin: 0.5in 0in 0in 0in; }\n", $this->stylesheetOf(Pdf::html('<p>Hallo</p>')->margins(0.5, unit: 'in')));

        $html = '<p>Hallo</p>';
        $this->assertSame(
            self::drawn(Fagus::html($html)->stylesheet('@page { size: 80.5mm 200mm; margin: 10mm 5.25mm 12mm 5.25mm }')),
            self::drawn($builder->driver('fagus')->generatePdfContent()),
        );
    }

    #[Test]
    public function it_shows_the_header_and_footer_on_every_page(): void
    {
        $builder = Pdf::html(self::TWO_PAGES)
            ->driver('fagus')
            ->margins(30, 20, 25, 20)
            ->headerView('header', ['city' => 'Berlin'])
            ->footerView('footer');

        // What the documentation of Fagus writes for a header and footer with HTML.
        $expected = Fagus::html(str_replace('<body>', '<body>'
            .'<div class="head"><div class="letterhead"><strong>ACME GmbH</strong><span>Berlin</span></div></div>'
            .'<div class="foot"><div style="font-size: 8pt; text-align: right">Seite <span class="n"></span> von <span class="t"></span></div></div>', self::TWO_PAGES))
            ->stylesheet('@page { margin: 30mm 20mm 25mm 20mm; @top-center { content: element(head) } @bottom-center { content: element(foot) } }'
                .'.head { position: running(head); width: 100% } .foot { position: running(foot); width: 100% }'
                .'.letterhead { display: flex; justify-content: space-between; font-size: 9pt }'
                .'.n::before { content: counter(page) } .t::before { content: counter(pages) }');

        $pdf = $this->pendingOf($builder)->strict()->render();
        $this->assertSame(2, $pdf->pages(), 'the header and footer are not in the flow');
        $this->assertSame(self::drawn($expected), self::drawn($pdf->bytes()));
        $this->assertNotSame(
            self::drawn(Pdf::html(self::TWO_PAGES)->driver('fagus')->margins(30, 20, 25, 20)->generatePdfContent()),
            self::drawn($pdf->bytes()),
        );
    }

    #[Test]
    public function it_places_the_header_and_footer_in_documents_without_a_body_tag(): void
    {
        foreach (['<p>Hallo</p>', '<!doctype html><html lang="de"><p>Hallo</p>', '<!doctype html><head><title>T</title></head><p>Hallo</p>'] as $html) {
            $pdf = $this->pendingOf(Pdf::html($html)->headerHtml('<p>Kopf</p>')->footerHtml('<p>Fuß @pageNumber</p>'))->strict()->render();

            $this->assertSame(1, $pdf->pages(), $html);
        }
    }

    #[Test]
    public function it_saves_to_paths_and_disks(): void
    {
        Storage::fake('local');
        $path = sys_get_temp_dir().'/fagus-laravel-pdf-'.bin2hex(random_bytes(4)).'.pdf';

        try {
            Pdf::view('invoice', ['number' => 'RE-2', 'lines' => []])->driver('fagus')->save($path);
            $this->assertStringStartsWith('%PDF-', (string) file_get_contents($path));
        } finally {
            @unlink($path);
        }

        Pdf::view('invoice', ['number' => 'RE-2', 'lines' => []])->driver('fagus')->disk('local')->save('rechnungen/re-2.pdf');
        Storage::disk('local')->assertExists('rechnungen/re-2.pdf');
        $this->assertStringStartsWith('%PDF-', (string) Storage::disk('local')->get('rechnungen/re-2.pdf'));
    }

    #[Test]
    public function it_saves_in_queued_jobs(): void
    {
        Storage::fake('local');

        Pdf::view('invoice', ['number' => 'RE-3', 'lines' => []])->driver('fagus')->disk('local')->saveQueued('rechnungen/re-3.pdf');

        Storage::disk('local')->assertExists('rechnungen/re-3.pdf');
    }

    #[Test]
    public function it_downloads_and_shows_pdfs(): void
    {
        Route::get('/download', fn (): PdfBuilder => Pdf::view('invoice', ['number' => 'RE-4', 'lines' => []])->driver('fagus')->download('re-4.pdf'));
        Route::get('/inline', fn (): PdfBuilder => Pdf::view('invoice', ['number' => 'RE-4', 'lines' => []])->driver('fagus')->name('re-4'));

        $download = $this->get('/download');
        $download->assertOk();
        $download->assertHeader('Content-Type', 'application/pdf');
        $download->assertHeader('Content-Disposition', 'attachment; filename="re-4.pdf"');
        $this->assertStringStartsWith('%PDF-', (string) $download->getContent());

        $inline = $this->get('/inline');
        $inline->assertOk();
        $inline->assertHeader('Content-Disposition', 'inline; filename="re-4.pdf"');
        $this->assertStringStartsWith('%PDF-', (string) $inline->getContent());
    }

    #[Test]
    public function it_tags_and_describes_pdfs(): void
    {
        $this->app['config']->set('fagus.tagged', false);
        $this->app->forgetInstance('fagus');
        $this->app->forgetInstance(PdfFactory::class);
        $this->app->forgetInstance('laravel-pdf.driver.fagus');

        $tagged = static fn (string $bytes): bool => str_contains(self::decoded($bytes), '/StructTreeRoot');
        $this->assertFalse($tagged(Pdf::html('<p>Hallo</p>')->driver('fagus')->generatePdfContent()));
        $this->assertTrue($tagged(Pdf::html('<p>Hallo</p>')->driver('fagus')->tagged()->generatePdfContent()));

        $bytes = Pdf::html('<p>Hallo</p>')->driver('fagus')->meta(title: 'Rechnung RE-5', author: 'ACME GmbH')->generatePdfContent();
        $this->assertStringContainsString('/Title (Rechnung RE-5)', $bytes);
    }

    #[Test]
    public function it_reads_files_from_the_base_directory(): void
    {
        $this->app['config']->set('fagus.base_directory', __DIR__.'/../../../tests');
        $this->app->forgetInstance('fagus');
        $this->app->forgetInstance(PdfFactory::class);
        $this->app->forgetInstance('laravel-pdf.driver.fagus');

        $pdf = $this->pendingOf(Pdf::html('<img src="corpus/images/logo.png" width="40">'))->strict()->render();

        $this->assertSame([], $pdf->diagnostics());
    }

    #[Test]
    public function it_refuses_options_that_it_does_not_support(): void
    {
        $refused = function (PdfBuilder $builder, string $message): void {
            try {
                $builder->driver('fagus')->generatePdfContent();
                $this->fail("Expected an exception: {$message}");
            } catch (CouldNotGeneratePdf $exception) {
                $this->assertStringContainsString($message, $exception->getMessage());
            }
        };

        $refused(Pdf::html('<p>Hallo</p>')->scale(0.8), 'does not support scale()');
        $refused(Pdf::html('<p>Hallo</p>')->pageRanges('1-2'), 'does not support pageRanges()');
        $refused(Pdf::html('<p>Hallo</p>')->waitUntilReady(), 'does not support waiting for readiness');
        $refused(Pdf::html('<p>Hallo</p>')->format('b5'), 'does not know the format [b5]');
        $refused(Pdf::html('<p>Hallo</p>')->margins(10, unit: 'em'), 'does not know the unit [em]');

        $this->assertStringStartsWith('%PDF-', Pdf::html('<p>Hallo</p>')->driver('fagus')->scale(1.0)->generatePdfContent());
    }

    /**
     * The Fagus document that the driver makes of a builder.
     */
    private function pendingOf(PdfBuilder $builder): PendingPdf
    {
        $driver = $this->app->make('laravel-pdf.driver.fagus');
        $options = (fn (): PdfOptions => $this->buildOptions())->call($builder);
        $pending = fn (): PendingPdf => $this->pending($builder->getHtml(), $builder->getHeaderHtml(), $builder->getFooterHtml(), $options);

        return Closure::bind($pending, $driver, FagusDriver::class)();
    }

    /**
     * The stylesheet that the driver adds for a builder, empty when it adds none.
     */
    private function stylesheetOf(PdfBuilder $builder): string
    {
        /** @var list<array{name: string, css: string}> $stylesheets */
        $stylesheets = (fn (): array => $this->stylesheets)->call($this->pendingOf($builder)->document());

        return implode('', array_column($stylesheets, 'css'));
    }

    /**
     * @return list<string>
     */
    private static function mediaBoxes(string $pdf): array
    {
        preg_match_all('~/MediaBox\s*\[([^\]]*)\]~', self::decoded($pdf), $boxes);

        return array_map(static fn (string $box): string => implode(' ', array_map(
            static fn (string $number): string => (string) round((float) $number, 2),
            preg_split('/\s+/', trim($box)) ?: [],
        )), $boxes[1]);
    }

    /**
     * The bytes of a PDF followed by the decoded contents of its object streams.
     */
    private static function decoded(string $pdf): string
    {
        preg_match_all('~/Type/ObjStm/N \d+/First \d+/Filter/FlateDecode/Length (\d+)>>\nstream\n~', $pdf, $streams, PREG_OFFSET_CAPTURE);
        $decoded = $pdf;
        foreach ($streams[0] as $index => [$header, $offset]) {
            $decoded .= (string) gzuncompress(substr($pdf, $offset + strlen($header), (int) $streams[1][$index][0]));
        }

        return $decoded;
    }

    /**
     * The streams of a PDF that hold what it draws: all but the metadata, the object streams and
     * the cross-reference stream.
     *
     * @return list<string>
     */
    private static function drawn(PendingPdf|string $pdf): array
    {
        if ($pdf instanceof PendingPdf) {
            $pdf = $pdf->render()->bytes();
        }
        preg_match_all('~<<(.*?)>>\nstream\n~s', $pdf, $streams, PREG_OFFSET_CAPTURE);
        $drawn = [];
        foreach ($streams[0] as $index => [$header, $offset]) {
            $dictionary = $streams[1][$index][0];
            if (preg_match('~/Type/(Metadata|ObjStm|XRef)\b~', $dictionary) === 1 || preg_match('~/Length (\d+)~', $dictionary, $length) !== 1) {
                continue;
            }
            $drawn[] = substr($pdf, $offset + strlen($header), (int) $length[1]);
        }

        return $drawn;
    }
}
