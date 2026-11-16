<?php

declare(strict_types=1);

namespace Fagus\Composer;

use Composer\Composer;
use Composer\EventDispatcher\EventSubscriberInterface;
use Composer\IO\IOInterface;
use Composer\Plugin\PluginInterface;
use Composer\Script\ScriptEvents;
use Fagus\Exception\FagusException;
use Fagus\Installer;
use Fagus\Internal\Release;

/**
 * Installs the fagus binary after `composer install` and `composer update`, so that requiring the
 * package is all it takes. The binary is taken from Composer's cache when it was downloaded
 * before; Composer's GitHub token (`github-oauth`) opens a private fork. If the binary
 * cannot be installed, Composer goes on and says why: `vendor/bin/fagus-install` tries again.
 *
 * @internal
 */
final class Plugin implements EventSubscriberInterface, PluginInterface
{
    private ?Composer $composer = null;

    private ?IOInterface $io = null;

    public function activate(Composer $composer, IOInterface $io): void
    {
        $this->composer = $composer;
        $this->io = $io;
    }

    public function deactivate(Composer $composer, IOInterface $io): void {}

    public function uninstall(Composer $composer, IOInterface $io): void {}

    /** @return array<string, string> */
    public static function getSubscribedEvents(): array
    {
        return [
            ScriptEvents::POST_INSTALL_CMD => 'installBinary',
            ScriptEvents::POST_UPDATE_CMD => 'installBinary',
        ];
    }

    public function installBinary(): void
    {
        if ($this->composer === null || $this->io === null) {
            return;
        }
        $package = $this->composer->getRepositoryManager()->getLocalRepository()->findPackage('nforst/fagus', '*');
        $directory = $package === null ? null : $this->composer->getInstallationManager()->getInstallPath($package);
        if ($directory === null) {
            return;
        }
        if (Release::BINARIES === []) {
            $this->io->write('fagus: a development version, without binaries to install.', true, IOInterface::VERBOSE);

            return;
        }
        $config = $this->composer->getConfig();
        $tokens = $config->get('github-oauth');
        $cache = $config->get('cache-dir');
        try {
            $path = (new Installer(
                token: is_array($tokens) && is_string($tokens['github.com'] ?? null) ? $tokens['github.com'] : null,
                cacheDirectory: is_string($cache) && $cache !== '' ? $cache.'/fagus' : null,
            ))->install($directory.'/php/fagus/bin');
            $this->io->write("fagus: the binary is installed at {$path}.", true, IOInterface::VERBOSE);
        } catch (FagusException $exception) {
            $this->io->writeError('<warning>fagus: '.$exception->getMessage().'</warning>');
        }
    }
}
