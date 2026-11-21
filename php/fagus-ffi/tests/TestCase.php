<?php

declare(strict_types=1);

namespace Fagus\Ffi\Tests;

use Fagus\Ffi\Engine;
use PHPUnit\Framework\TestCase as BaseTestCase;

/**
 * The tests render with a real library, so they need one: set `FAGUS_FFI_LIB`, or build it with
 * `cargo build -p fagus-ffi`, which puts it in the workspace's `target/`. A test is skipped rather
 * than failed when there is none, so that the suite runs wherever Composer does.
 */
abstract class TestCase extends BaseTestCase
{
    /**
     * The HTML of the documents the tests render.
     */
    protected const HTML = '<!doctype html><html lang="en"><head><title>Test</title></head><body>'
        .'<h1 id="title">Fagus</h1><p class="note">Rendered through PHP FFI.</p></body></html>';

    /** @var Engine|null */
    private $engine;

    /**
     * An engine with defaults, kept for the whole test.
     */
    protected function engine(): Engine
    {
        if ($this->engine === null) {
            $this->engine = new Engine([], $this->library());
        }

        return $this->engine;
    }

    /**
     * The path of the library to load.
     */
    protected function library(): string
    {
        $library = self::find();
        if ($library === null) {
            self::markTestSkipped(
                'No fagus_ffi library: set FAGUS_FFI_LIB, or build one with `cargo build -p fagus-ffi`.'
            );
        }

        return $library;
    }

    /**
     * The library of `FAGUS_FFI_LIB`, of the package's `lib/<platform>/`, or a build of the
     * workspace (`target/release`, `target/debug`, `target/<target>/<profile>`).
     */
    private static function find(): ?string
    {
        $name = self::libraryName();
        $root = dirname(__DIR__, 3);
        $paths = [];
        $environment = getenv('FAGUS_FFI_LIB');
        if (is_string($environment) && $environment !== '') {
            $paths[] = $environment;
        }
        foreach (glob(dirname(__DIR__).'/lib/*/'.$name) ?: [] as $packaged) {
            $paths[] = $packaged;
        }
        foreach (['release', 'debug'] as $profile) {
            $paths[] = $root.'/target/'.$profile.'/'.$name;
        }
        foreach (glob($root.'/target/*/'.$name) ?: [] as $built) {
            $paths[] = $built;
        }
        foreach (glob($root.'/target/*/*/'.$name) ?: [] as $built) {
            $paths[] = $built;
        }
        foreach ($paths as $path) {
            if (is_file($path)) {
                return $path;
            }
        }

        return null;
    }

    private static function libraryName(): string
    {
        if (PHP_OS_FAMILY === 'Windows') {
            return 'fagus_ffi.dll';
        }
        if (PHP_OS_FAMILY === 'Darwin') {
            return 'libfagus_ffi.dylib';
        }

        return 'libfagus_ffi.so';
    }
}
