<?php

declare(strict_types=1);

namespace Fagus\Laravel\Tests;

use Fagus\Binary;
use Fagus\Laravel\FagusServiceProvider;
use Orchestra\Testbench\TestCase as Orchestra;

abstract class TestCase extends Orchestra
{
    protected function getPackageProviders($app): array
    {
        return [FagusServiceProvider::class];
    }

    protected function defineEnvironment($app): void
    {
        $binary = getenv('FAGUS_BINARY') ?: __DIR__.'/../../../target/release/'.Binary::name();
        if (! is_executable($binary)) {
            $this->markTestSkipped("No fagus binary at {$binary}; build it with `cargo build --release`.");
        }
        $app['config']->set('fagus.binary', $binary);
        $app['config']->set('fagus.fonts', [__DIR__.'/../../../tests/fonts', __DIR__.'/does-not-exist']);
        $app['config']->set('view.paths', [__DIR__.'/views']);
    }
}
