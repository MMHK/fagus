<?php

declare(strict_types=1);

namespace Fagus\Ffi;

use Fagus\Ffi\Exception\EngineException;
use Fagus\Ffi\Exception\RenderException;

/**
 * The C ABI library, loaded once per path, and the calls of its header.
 *
 * PHP 7.4 only accepts PHP strings for `char *`, not for `uint8_t *`, so every input buffer is
 * copied into C data first. A `const char *` comes back as a PHP string in 7.4 and may be C data
 * later, which {@see self::cString()} accepts both of.
 *
 * @internal Use {@see Engine}, not this class.
 */
final class Native
{
    /** The ABI of `include/fagus.h` this package speaks. */
    public const ABI_VERSION = 1;

    /** @var array<string, self> */
    private static $loaded = [];

    /** @var \FFI */
    private $ffi;

    /** @var string */
    private $library;

    private function __construct(\FFI $ffi, string $library)
    {
        $this->ffi = $ffi;
        $this->library = $library;
    }

    /**
     * Loads the library, or returns the one already loaded from the same path.
     *
     * The path is the argument, else the FAGUS_FFI_LIB environment variable, else the name of the
     * library as the system loader finds it.
     *
     * @throws EngineException
     */
    public static function load(?string $library = null): self
    {
        $library = self::locate($library);
        if (isset(self::$loaded[$library])) {
            return self::$loaded[$library];
        }
        if (! extension_loaded('ffi')) {
            throw new EngineException(
                'The ffi extension is not loaded; install it and set ffi.enable=true (or preload the library).'
            );
        }
        try {
            $ffi = \FFI::cdef(self::header(), $library);
        } catch (\Throwable $error) {
            throw new EngineException(sprintf('Cannot load %s: %s', $library, $error->getMessage()), 0, $error);
        }
        $native = new self($ffi, $library);
        $abi = (int) $ffi->fagus_abi_version();
        if ($abi !== self::ABI_VERSION) {
            throw new EngineException(sprintf(
                'The library at %s speaks ABI %d, but this package needs ABI %d.',
                $library,
                $abi,
                self::ABI_VERSION
            ));
        }
        self::$loaded[$library] = $native;

        return $native;
    }

    public function library(): string
    {
        return $this->library;
    }

    public function version(): string
    {
        return $this->cString($this->ffi->fagus_version());
    }

    /**
     * Creates an engine from settings in the shape of the C ABI's `config_json`.
     *
     * @param array<string, mixed> $config
     *
     * @throws EngineException
     */
    public function engine(array $config = []): \FFI\CData
    {
        $json = $config === []
            ? '{}'
            : json_encode($config, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE);
        if ($json === false) {
            throw new EngineException('The engine settings cannot be JSON encoded: '.json_last_error_msg());
        }
        $engine = $this->ffi->fagus_engine_new($this->buffer($json), strlen($json));
        if (\FFI::isNull($engine)) {
            throw new EngineException('Cannot create an engine: '.$this->lastError());
        }

        return $engine;
    }

    /**
     * @param \FFI\CData $engine
     */
    public function freeEngine($engine): void
    {
        $this->ffi->fagus_engine_free($engine);
    }

    /**
     * Renders one document.
     *
     * @param \FFI\CData    $engine
     * @param array<string, mixed> $header the worker protocol's request header, with `blobLengths`
     * @param string[]      $blobs  the data blocks, in the order their indices name
     *
     * @return array{0: string, 1: array<string, mixed>} the PDF and its metadata
     *
     * @throws RenderException
     */
    public function render($engine, array $header, array $blobs): array
    {
        $request = json_encode($header, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE);
        if ($request === false) {
            throw new RenderException('The request cannot be JSON encoded: '.json_last_error_msg());
        }
        $data = implode('', $blobs);
        $requestBuffer = $this->buffer($request);
        $dataBuffer = $this->buffer($data);

        $pdf = $this->ffi->new('uint8_t*');
        $pdfLength = $this->ffi->new('size_t');
        $meta = $this->ffi->new('uint8_t*');
        $metaLength = $this->ffi->new('size_t');

        $status = (int) $this->ffi->fagus_render(
            $engine,
            $requestBuffer,
            strlen($request),
            $dataBuffer,
            strlen($data),
            \FFI::addr($pdf),
            \FFI::addr($pdfLength),
            \FFI::addr($meta),
            \FFI::addr($metaLength)
        );
        if ($status !== 0) {
            throw new RenderException($this->lastError());
        }

        $pdfBytes = (int) $pdfLength->cdata;
        $metaBytes = (int) $metaLength->cdata;
        try {
            $bytes = \FFI::string($pdf, $pdfBytes);
            $answer = json_decode(\FFI::string($meta, $metaBytes), true);
            if (! is_array($answer)) {
                throw new RenderException('The engine answered with invalid metadata: '.json_last_error_msg());
            }
        } finally {
            $this->ffi->fagus_bytes_free($pdf, $pdfBytes);
            $this->ffi->fagus_bytes_free($meta, $metaBytes);
        }

        return [$bytes, $answer];
    }

    public function lastError(): string
    {
        return $this->cString($this->ffi->fagus_last_error());
    }

    /**
     * A NUL-terminated string from C, whether PHP returned a string or C data.
     *
     * @param string|\FFI\CData $value
     */
    private function cString($value): string
    {
        return is_string($value) ? $value : \FFI::string($value);
    }

    /**
     * A C buffer of the bytes of a PHP string, or null for an empty string (a null pointer).
     *
     * @return \FFI\CData|null
     */
    private function buffer(string $data)
    {
        $length = strlen($data);
        if ($length === 0) {
            return null;
        }
        $buffer = $this->ffi->new('uint8_t['.$length.']');
        \FFI::memcpy($buffer, $data, $length);

        return $buffer;
    }

    /**
     * @throws EngineException
     */
    private static function locate(?string $library): string
    {
        if ($library === null || $library === '') {
            $library = getenv('FAGUS_FFI_LIB') ?: null;
        }
        if ($library !== null && $library !== '') {
            $real = realpath($library);
            if ($real === false) {
                throw new EngineException(sprintf('There is no shared library at %s.', $library));
            }

            return $real;
        }
        // The package may ship the binaries of the platforms it supports.
        $packaged = dirname(__DIR__).'/lib/'.self::platform().'/'.self::libraryName();
        if (is_file($packaged)) {
            return realpath($packaged) ?: $packaged;
        }

        return self::libraryName();
    }

    /**
     * The platform directory name of the shipped binaries: `linux-x86_64`, `windows-x86_64` and so
     * on.
     */
    private static function platform(): string
    {
        $machine = strtolower((string) php_uname('m'));
        if (in_array($machine, ['aarch64', 'arm64'], true)) {
            $machine = 'aarch64';
        } else {
            $machine = 'x86_64';
        }
        if (PHP_OS_FAMILY === 'Windows') {
            return 'windows-'.$machine;
        }
        if (PHP_OS_FAMILY === 'Darwin') {
            return 'macos-'.$machine;
        }

        return 'linux-'.$machine;
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

    /**
     * @throws EngineException
     */
    private static function header(): string
    {
        $path = dirname(__DIR__).'/include/fagus.h';
        $header = @file_get_contents($path);
        if ($header === false) {
            throw new EngineException(sprintf('Cannot read the ABI header at %s.', $path));
        }

        return $header;
    }
}
