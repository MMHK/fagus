<?php

declare(strict_types=1);

namespace Fagus;

use Fagus\Exception\EngineException;

/**
 * Finds the fagus binary.
 *
 * @internal
 */
final class Binary
{
    /**
     * The binary at `$path` if one is given, else the one in FAGUS_BINARY, else the one the
     * installer put into this package (bin/), else the one on the PATH. A binary that is given,
     * in `$path` or FAGUS_BINARY, must exist.
     */
    public static function locate(?string $path = null): string
    {
        $environment = getenv('FAGUS_BINARY');
        $given = $path ?? ($environment === false || $environment === '' ? null : $environment);
        if ($given !== null) {
            if (is_file($given) && is_executable($given)) {
                return $given;
            }
            throw EngineException::notStarted($given, 'It does not exist or is not executable.');
        }
        $name = self::name();
        $candidates = [dirname(__DIR__).DIRECTORY_SEPARATOR.'bin'.DIRECTORY_SEPARATOR.$name];
        foreach (explode(PATH_SEPARATOR, (string) getenv('PATH')) as $directory) {
            // Windows allows quotes around the directories of the PATH.
            $directory = trim($directory, '"');
            if ($directory !== '') {
                $candidates[] = rtrim($directory, '/\\').DIRECTORY_SEPARATOR.$name;
            }
        }
        foreach ($candidates as $candidate) {
            if (is_file($candidate) && is_executable($candidate)) {
                return $candidate;
            }
        }

        throw EngineException::binaryNotFound();
    }

    /**
     * The file name of the binary on this platform.
     */
    public static function name(): string
    {
        return PHP_OS_FAMILY === 'Windows' ? 'fagus.exe' : 'fagus';
    }
}
