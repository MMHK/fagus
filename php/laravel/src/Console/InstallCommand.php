<?php

declare(strict_types=1);

namespace Fagus\Laravel\Console;

use Fagus\Exception\FagusException;
use Fagus\Installer;
use Illuminate\Console\Command;

/**
 * Installs the fagus binary of the release that nforst/fagus belongs to, for this platform.
 */
final class InstallCommand extends Command
{
    protected $signature = 'fagus:install {directory? : Where to put the binary; by default where the engine finds it}';

    protected $description = 'Install the fagus binary for this platform';

    public function handle(): int
    {
        $directory = $this->argument('directory');
        try {
            $path = (new Installer)->install(is_string($directory) ? $directory : null);
        } catch (FagusException $exception) {
            $this->components->error($exception->getMessage());

            return self::FAILURE;
        }
        $this->components->info("The fagus binary is installed at {$path}.");

        return self::SUCCESS;
    }
}
