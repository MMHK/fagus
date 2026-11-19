<?php

declare(strict_types=1);

namespace Fagus\Tests;

use Fagus\Document;
use Fagus\Engine;
use Fagus\Exception\EngineException;
use Fagus\Internal\Release;
use Fagus\Tests\Fixtures\FakeWorker;
use PHPUnit\Framework\Attributes\Test;
use PHPUnit\Framework\TestCase;

/**
 * The engine against a stand-in for the worker ({@see FakeWorker}), for what the real binary
 * cannot be made to do: speak another protocol, be of another release, report its memory.
 */
final class EngineTest extends TestCase
{
    /** @var list<string> */
    private array $binaries = [];

    protected function setUp(): void
    {
        if (PHP_OS_FAMILY === 'Windows') {
            $this->markTestSkipped('The stand-in binary is a shell script.');
        }
    }

    protected function tearDown(): void
    {
        array_map(unlink(...), $this->binaries);
    }

    #[Test]
    public function it_refuses_a_binary_of_another_protocol(): void
    {
        $binary = $this->fake(protocol: 2, version: '9.0.0');

        try {
            (new Engine($binary))->render(Document::html('<p>x</p>'));
            $this->fail('A binary of protocol 2 was accepted.');
        } catch (EngineException $exception) {
            $this->assertSame(
                "The fagus binary `{$binary}` (version 9.0.0) speaks protocol 2; this package, nforst/fagus ".Release::VERSION.', needs protocol 1. '
                ."Install the binary of the package's release with `vendor/bin/fagus-install` (in Laravel: `php artisan fagus:install`), "
                .'or point FAGUS_BINARY at the binary of version '.Release::VERSION.'.',
                $exception->getMessage(),
            );
        }
    }

    #[Test]
    public function it_warns_once_about_a_binary_of_another_release(): void
    {
        $binary = $this->fake(version: '0.0.1');
        $warnings = [];
        $warn = static function (string $message) use (&$warnings): void {
            $warnings[] = $message;
        };
        $engine = new Engine($binary, onWarning: $warn);

        $this->assertSame('%PDF-fake', substr(Document::html('<p>x</p>')->render($engine)->bytes(), 0, 9), 'it renders');
        $engine->close();
        Document::html('<p>x</p>')->render($engine);
        (new Engine($binary, onWarning: $warn))->version();

        $this->assertCount(1, $warnings, 'once per binary and version in a process');
        $this->assertStringStartsWith("The fagus binary `{$binary}` is version 0.0.1, this package nforst/fagus ".Release::VERSION.'. Both speak protocol 1', $warnings[0]);
        $this->assertStringContainsString('`php artisan fagus:install`', $warnings[0]);
    }

    #[Test]
    public function it_does_not_warn_about_the_binary_of_its_release(): void
    {
        $warnings = [];
        $engine = new Engine($this->fake(version: Release::VERSION), onWarning: static function (string $message) use (&$warnings): void {
            $warnings[] = $message;
        });

        $this->assertSame(Release::VERSION, $engine->version());
        $this->assertSame([], $warnings);
    }

    #[Test]
    public function it_writes_warnings_to_the_error_log_by_default(): void
    {
        $log = tempnam(sys_get_temp_dir(), 'fagus-log-');
        $previous = ini_set('error_log', (string) $log);
        try {
            (new Engine($this->fake(version: '0.0.2')))->version();
            $this->assertStringContainsString('is version 0.0.2, this package nforst/fagus', (string) file_get_contents((string) $log));
        } finally {
            ini_set('error_log', $previous === false ? '' : $previous);
            unlink((string) $log);
        }
    }

    /**
     * @param  list<int>  $rss
     */
    private function fake(int $protocol = 1, string $version = Release::VERSION, array $rss = []): string
    {
        return $this->binaries[] = FakeWorker::create($protocol, $version, $rss);
    }
}
