<?php

declare(strict_types=1);

namespace Fagus\Internal;

/**
 * The release of the fagus binary that this package works with. `tools/pin-binaries.php` writes
 * this file when the binaries of a release are built.
 *
 * @internal
 */
final class Release
{
    /** The version of the binary. */
    public const string VERSION = '0.2.0';

    /** The GitHub repository whose releases hold the binaries. */
    public const string REPOSITORY = 'nforst/fagus';

    /**
     * The SHA-256 of the binary for each target. Empty in a development version, which has no
     * built binaries.
     *
     * @var array<string, string>
     */
    public const array BINARIES = [
        'aarch64-apple-darwin' => '79a684b0139237c4e72e530f66816ffeb69137f2e9836d20987651201a665bf9',
        'aarch64-unknown-linux-gnu' => '42efb1edbfd7101c46e29dc3639ee580e8c3e9e74de2cf5a5a7ec80467aed882',
        'aarch64-unknown-linux-musl' => '81a47cecd61d627bfef38b010c94f78b5cde8907f4f4968c85c2bf9b0b3d981c',
        'x86_64-apple-darwin' => '82cde22d5168f5d6e75c01b68f6aaac2006ed43d78e21f75ea2e6a2e1e949e91',
        'x86_64-pc-windows-msvc' => 'a6dff9aed17e5f403037ebacf4fd8a1edcfbc5b0795b98f67527eeb6e74c1336',
        'x86_64-unknown-linux-gnu' => '2147121ba5b4cf01c5e278db225b5cb81c174bbcbf0011f740d5b7adc1027861',
        'x86_64-unknown-linux-musl' => 'f53f86e6bd49a2293d9f8c3c4e372a7ac1d5a1b464bfb0de82403fab6b22280d',
    ];
}
