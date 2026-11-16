<?php

declare(strict_types=1);

namespace Fagus\Exception;

/**
 * The engine could not be found, started or talked to.
 */
final class EngineException extends FagusException
{
    public static function binaryNotFound(): self
    {
        return new self('The fagus binary was not found. Install it with `vendor/bin/fagus-install` (in Laravel: `php artisan fagus:install`), set its path in the Engine or in the FAGUS_BINARY environment variable, or put it on the PATH.');
    }

    public static function notStarted(string $binary, string $reason = ''): self
    {
        return new self(trim("The fagus binary `{$binary}` could not be started. {$reason}"));
    }

    public static function fontDirectoryNotFound(string $directory): self
    {
        return new self("The font directory `{$directory}` does not exist. Create it or remove it from the engine's font directories.");
    }

    public static function stopped(string $output = ''): self
    {
        return new self(trim('The fagus worker stopped. '.$output));
    }

    public static function timedOut(float $seconds): self
    {
        return new self("The fagus worker did not answer within {$seconds} seconds and was stopped.");
    }

    public static function incompatible(mixed $protocol): self
    {
        return new self('The fagus binary speaks protocol '.json_encode($protocol).', this package needs protocol 1. Install the binary of the same release.');
    }

    public static function unexpected(string $kind): self
    {
        return new self("The fagus worker sent an unexpected message `{$kind}`.");
    }
}
