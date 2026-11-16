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
    public const string VERSION = '0.1.0';

    /** The GitHub repository whose releases hold the binaries. */
    public const string REPOSITORY = 'nforst/fagus';

    /**
     * The SHA-256 of the binary for each target. Empty in a development version, which has no
     * built binaries.
     *
     * @var array<string, string>
     */
    public const array BINARIES = [
        'aarch64-apple-darwin' => '0d6cca221402da87d95d36ab0e2c8146b8dcdecc0dff09db839685bb8ee08b0d',
        'aarch64-unknown-linux-gnu' => 'dce0b52b9a420a9d49cd178e7704eee9210bbc439319f70b76c77f9f52828ba4',
        'aarch64-unknown-linux-musl' => '1a2fd8953276c002e9d54ebf367ab82c2a4d9b64a0836bd72d64bf757b6a91b0',
        'x86_64-apple-darwin' => '1597b5212c99082e1ea77014fceafc936db88fb48fde47a7a66158910ae085f0',
        'x86_64-pc-windows-msvc' => 'ca40aacfd70cfafaeae34c47ec3299277c60bb0c30b7483f72e1de74bb530b83',
        'x86_64-unknown-linux-gnu' => 'c8eab7031f361883fc10463826a4c641db26a3f2e5be3001ccf37031e43aefe7',
        'x86_64-unknown-linux-musl' => '8ab2c6df17e1c8277e71ce6909c22b40a3fe5fdec95efadcf07d01b4acf4d7e0',
    ];
}
