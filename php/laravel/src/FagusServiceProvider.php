<?php

declare(strict_types=1);

namespace Fagus\Laravel;

use Closure;
use Fagus\Engine;
use Fagus\Laravel\Console\InstallCommand;
use Fagus\Laravel\LaravelPdf\FagusDriver;
use Illuminate\Contracts\Foundation\Application;
use Illuminate\Support\ServiceProvider;
use Spatie\LaravelPdf\Drivers\PdfDriver;

/**
 * Registers the engine (one worker process for the life of the application: per request under
 * PHP-FPM, across requests under Octane and across jobs in queue workers) and the PDF factory.
 * Under Octane, the engine is warmed: resolved when the Octane worker boots, so that the
 * requests share it instead of each starting and stopping a worker process of its own.
 * With spatie/laravel-pdf installed, it also registers Fagus as laravel-pdf's `fagus` driver.
 */
final class FagusServiceProvider extends ServiceProvider
{
    public function register(): void
    {
        $this->mergeConfigFrom(__DIR__.'/../config/fagus.php', 'fagus');

        $this->app->singleton(Engine::class, static function (Application $app): Engine {
            /** @var array<string, mixed> $config */
            $config = $app['config']['fagus'];
            $families = (array) ($config['families'] ?? []);

            return new Engine(
                binary: $config['binary'] ?? null,
                // The default directory, resources/fonts, is optional: those missing are skipped.
                fontDirectories: array_values(array_filter((array) ($config['fonts'] ?? []), is_dir(...))),
                sansSerif: $families['sans-serif'] ?? null,
                serif: $families['serif'] ?? null,
                monospace: $families['monospace'] ?? null,
                timeout: (float) ($config['timeout'] ?? 60),
                systemFonts: (bool) ($config['system_fonts'] ?? false),
                locale: isset($config['locale']) && $config['locale'] !== '' ? (string) $config['locale'] : null,
                // A binary of another release than the package, for example, goes to the log.
                onWarning: static fn (string $message) => $app->make('log')->warning($message),
            );
        });

        $this->app->singleton(PdfFactory::class, static fn (Application $app): PdfFactory => new PdfFactory(
            $app->make(Engine::class),
            $app->make('view'),
            (array) $app['config']['fagus'],
        ));
        $this->app->alias(PdfFactory::class, 'fagus');

        if (interface_exists(PdfDriver::class)) {
            $this->app->singleton('laravel-pdf.driver.fagus', static fn (Application $app): FagusDriver => new FagusDriver(
                $app->make(PdfFactory::class),
            ));
        }
    }

    public function boot(): void
    {
        $this->warmUnderOctane();
        $this->selectableAsLaravelPdfDriver();
        if ($this->app->runningInConsole()) {
            $this->publishes([__DIR__.'/../config/fagus.php' => config_path('fagus.php')], 'fagus-config');
            $this->commands([InstallCommand::class]);
        }
    }

    /**
     * Adds the engine to Octane's `warm` services, if Octane is installed. Octane gives each
     * request a copy of the container and throws away what the request resolved, so an engine
     * resolved in a request would stop its worker process at the request's end. Services in
     * `warm` are resolved once, when the Octane worker boots (after the providers boot), and
     * shared by its requests. Merged here rather than in register(), where Octane's own default
     * configuration may not be merged yet.
     */
    private function warmUnderOctane(): void
    {
        $config = $this->app['config'];
        $warm = $config->get('octane.warm');
        if (is_array($warm) && ! in_array(Engine::class, $warm, true)) {
            $config->set('octane.warm', [...$warm, Engine::class]);
        }
    }

    /**
     * Lets `laravel-pdf.driver` (LARAVEL_PDF_DRIVER) name the `fagus` driver. laravel-pdf binds
     * its default driver to a closure that knows only its own drivers and throws for any other
     * name; this binding answers for `fagus` and hands every other name to that closure. Done
     * here rather than in register(), where laravel-pdf may not be registered yet. A binding of
     * the application's own, made later, still wins.
     */
    private function selectableAsLaravelPdfDriver(): void
    {
        $previous = $this->app->getBindings()[PdfDriver::class]['concrete'] ?? null;
        if (! $previous instanceof Closure) {
            return;
        }

        $this->app->singleton(PdfDriver::class, static fn (Application $app): PdfDriver => $app['config']->get('laravel-pdf.driver') === 'fagus'
            ? $app->make('laravel-pdf.driver.fagus')
            : $previous($app, []));
    }
}
