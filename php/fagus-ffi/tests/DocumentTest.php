<?php

declare(strict_types=1);

namespace Fagus\Ffi\Tests;

use Fagus\Ffi\Document;
use Fagus\Ffi\Exception\RenderException;
use Fagus\Ffi\Signature;

final class DocumentTest extends TestCase
{
    public function test_it_renders_a_pdf(): void
    {
        $pdf = $this->engine()->html(self::HTML)->render();

        $this->assertSame('%PDF-', substr($pdf->bytes(), 0, 5));
        $this->assertGreaterThanOrEqual(1, $pdf->pages());
        $this->assertSame(strlen($pdf->bytes()), $pdf->size());
        $this->assertSame([], $pdf->diagnostics());
        $this->assertFalse($pdf->encrypted());
        $this->assertFalse($pdf->signed());
    }

    public function test_the_same_document_renders_the_same_bytes(): void
    {
        $first = $this->engine()->html(self::HTML)->created('2026-11-20T00:00:00Z')->render();
        $second = $this->engine()->html(self::HTML)->created('2026-11-20T00:00:00Z')->render();

        $this->assertSame($first->bytes(), $second->bytes());
    }

    public function test_an_unknown_property_is_a_diagnostic(): void
    {
        $pdf = $this->engine()
            ->html(self::HTML)
            ->stylesheet('p { colr: teal }', 'test.css')
            ->render();

        $diagnostics = $pdf->diagnostics();
        $this->assertNotSame([], $diagnostics);
        $this->assertSame('css.unknown-property', $diagnostics[0]->code());
        $this->assertSame('test.css', $diagnostics[0]->source());
        $this->assertTrue($diagnostics[0]->isWarning());
        $this->assertFalse($diagnostics[0]->isError());
        $this->assertNotSame('', $diagnostics[0]->message());
        $this->assertSame($diagnostics[0]->code(), $diagnostics[0]->toArray()['code']);
    }

    public function test_the_layout_tells_where_elements_went(): void
    {
        $pdf = $this->engine()->html(self::HTML)->reportLayout()->render();

        $layout = $pdf->layout();
        $this->assertIsArray($layout);
        $this->assertArrayHasKey('elements', $layout);
        $this->assertSame('title', $layout['elements'][0]['id']);
        $this->assertArrayHasKey('page', $layout['elements'][0]['boxes'][0]);
    }

    public function test_pdf_a_with_an_attachment(): void
    {
        $pdf = $this->engine()
            ->html(self::HTML)
            ->pdfA('3b')
            ->attach('data.csv', 'text/csv', "a,b\n1,2\n", 'supplement', 'Raw data')
            ->render();

        $this->assertSame('%PDF-', substr($pdf->bytes(), 0, 5));
        $this->assertSame([], $pdf->diagnostics());
    }

    public function test_an_unknown_pdf_a_level_is_refused(): void
    {
        $this->expectException(RenderException::class);
        $this->expectExceptionMessage('9z');
        $this->engine()->html(self::HTML)->pdfA('9z')->render();
    }

    public function test_a_document_without_html_is_refused(): void
    {
        $this->expectException(RenderException::class);
        $this->expectExceptionMessage('HTML');
        (new Document)->render($this->engine());
    }

    public function test_encryption(): void
    {
        $pdf = $this->engine()
            ->html(self::HTML)
            ->encrypt('open sesame', 'change me', ['print'])
            ->render();

        $this->assertSame('%PDF-', substr($pdf->bytes(), 0, 5));
        $this->assertTrue($pdf->encrypted());
    }

    public function test_it_signs_with_pkcs12(): void
    {
        $path = dirname(__DIR__, 3).'/tests/signing/ecdsa.p12';
        if (! is_file($path)) {
            self::markTestSkipped('The test certificate tests/signing/ecdsa.p12 is not there.');
        }
        $pdf = $this->engine()
            ->html('<p id="signature">Signed</p>')
            ->sign(Signature::pkcs12((string) file_get_contents($path), 'fagus-test-only')->reason('Test'))
            ->render();

        $this->assertSame('%PDF-', substr($pdf->bytes(), 0, 5));
        $this->assertTrue($pdf->signed());
    }

    public function test_it_saves_the_pdf(): void
    {
        $directory = sys_get_temp_dir().'/fagus-ffi-test-'.bin2hex(random_bytes(6));
        $this->assertTrue(mkdir($directory, 0700, true));

        try {
            $path = $directory.'/test.pdf';
            $pdf = $this->engine()->html(self::HTML)->save($path);
            $this->assertFileExists($path);
            $this->assertSame($pdf->size(), (int) filesize($path));
        } finally {
            @unlink($directory.'/test.pdf');
            @rmdir($directory);
        }
    }
}
