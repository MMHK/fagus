<?php

declare(strict_types=1);

namespace Fagus\Ffi;

use Fagus\Ffi\Exception\EngineException;

/**
 * A configured renderer: fonts, locale and limits, loaded once and reused by any number of
 * documents, so that fonts and caches stay warm.
 *
 * The settings are those of the C ABI's `config_json`:
 *
 *     new Engine(['fontDirs' => ['/app/fonts'], 'systemFonts' => true])
 *
 * The library is found by the argument, else the FAGUS_FFI_LIB environment variable, else the
 * system's search path (`libfagus_ffi.so`, `fagus_ffi.dll`).
 */
final class Engine
{
    /** @var self|null */
    private static $shared;

    /** @var Native */
    private $native;

    /** @var \FFI\CData */
    private $handle;

    /**
     * @param array<string, mixed> $config `fontDirs`, `systemFonts`, `locale`, `sansSerif`,
     *                                     `serif`, `monospace`, `memoryBudget`, `timeLimit`
     *
     * @throws EngineException
     */
    public function __construct(array $config = [], ?string $library = null)
    {
        $this->native = Native::load($library);
        $this->handle = $this->native->engine($config);
    }

    public function __destruct()
    {
        $this->close();
    }

    /**
     * The engine that documents without one use.
     */
    public static function shared(): self
    {
        if (self::$shared === null) {
            self::$shared = new self;
        }

        return self::$shared;
    }

    /**
     * The version of the library, as `fagus_version` reports it.
     */
    public function version(): string
    {
        return $this->native->version();
    }

    /**
     * A new document, rendered by this engine.
     */
    public function document(): Document
    {
        return new Document($this);
    }

    /**
     * A new HTML document, rendered by this engine.
     */
    public function html(string $html): Document
    {
        return $this->document()->html($html);
    }

    /**
     * Renders a document that has no engine of its own.
     */
    public function render(Document $document): Pdf
    {
        return $document->render($this);
    }

    /**
     * Frees the engine now rather than at the end of the request.
     */
    public function close(): void
    {
        if (self::$shared === $this) {
            self::$shared = null;
        }
        if ($this->handle !== null) {
            $this->native->freeEngine($this->handle);
            $this->handle = null;
        }
    }

    /**
     * @internal For {@see Document}.
     */
    public function native(): Native
    {
        return $this->native;
    }

    /**
     * @internal For {@see Document}.
     *
     * @return \FFI\CData
     */
    public function handle()
    {
        if ($this->handle === null) {
            throw new EngineException('The engine has been closed.');
        }

        return $this->handle;
    }
}
