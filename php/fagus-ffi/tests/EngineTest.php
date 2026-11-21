<?php

declare(strict_types=1);

namespace Fagus\Ffi\Tests;

use Fagus\Ffi\Document;
use Fagus\Ffi\Engine;
use Fagus\Ffi\Exception\EngineException;

final class EngineTest extends TestCase
{
    public function test_the_library_reports_its_version_and_path(): void
    {
        $engine = $this->engine();
        $library = $this->library();

        $this->assertMatchesRegularExpression('/^\d+\.\d+/', $engine->version());
        // The engine resolves the path, so a relative FAGUS_FFI_LIB comes back absolute.
        $this->assertSame(realpath($library) ?: $library, $engine->native()->library());
    }

    public function test_the_engine_takes_settings(): void
    {
        $engine = new Engine(['systemFonts' => true, 'memoryBudget' => 256 * 1024 * 1024], $this->library());
        $pdf = $engine->html('<p id="settings">Settings</p>')->render();
        $this->assertSame('%PDF-', substr($pdf->bytes(), 0, 5));

        $engine->close();
    }

    public function test_a_closed_engine_is_refused(): void
    {
        $engine = new Engine([], $this->library());
        $engine->close();

        $this->expectException(EngineException::class);
        $this->expectExceptionMessage('closed');
        $engine->render((new Document)->html('<p>Closed</p>'));
    }

    public function test_a_missing_library_is_refused(): void
    {
        $this->expectException(EngineException::class);
        $this->expectExceptionMessage('no shared library');

        new Engine([], __DIR__.'/libfagus-ffi-does-not-exist.so');
    }
}
