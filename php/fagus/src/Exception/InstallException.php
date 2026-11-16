<?php

declare(strict_types=1);

namespace Fagus\Exception;

/**
 * The fagus binary could not be installed.
 */
final class InstallException extends FagusException
{
    public static function developmentVersion(string $version): self
    {
        return new self("This version of nforst/fagus ({$version}) is a development version without built binaries. Build the binary with `cargo build --release` and set FAGUS_BINARY to it.");
    }

    public static function unsupportedPlatform(string $platform): self
    {
        return new self("There is no fagus binary for {$platform}. Build it from source with `cargo build --release` and set FAGUS_BINARY to it.");
    }

    public static function downloadFailed(string $url, string $reason): self
    {
        return new self(trim("The fagus binary could not be downloaded from {$url}. {$reason}"));
    }

    public static function checksumMismatch(string $asset, string $expected, string $actual): self
    {
        return new self("The downloaded {$asset} is not the binary of this release: its SHA-256 is {$actual}, expected {$expected}. It was not installed.");
    }

    public static function notWritten(string $path): self
    {
        return new self("The fagus binary could not be written to `{$path}`. Check that the directory is writable; on Windows, also stop the processes that run the binary (queue workers, Octane), which keep it locked.");
    }
}
