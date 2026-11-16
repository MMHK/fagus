<?php

declare(strict_types=1);

namespace Fagus\Exception;

use Fagus\Diagnostic;

/**
 * A document could not be rendered, or had diagnostics in strict mode. Either way,
 * {@see self::diagnostics()} lists the diagnostics found.
 */
final class RenderException extends FagusException
{
    /**
     * @param  list<Diagnostic>  $diagnostics
     */
    public function __construct(string $message, private readonly array $diagnostics = [])
    {
        parent::__construct($message);
    }

    /**
     * @param  list<Diagnostic>  $diagnostics
     */
    public static function strict(array $diagnostics): self
    {
        $lines = implode("\n", array_map(strval(...), $diagnostics));

        $count = count($diagnostics);
        $noun = $count === 1 ? 'diagnostic' : 'diagnostics';

        return new self("{$count} {$noun} in strict mode:\n{$lines}", $diagnostics);
    }

    /**
     * The diagnostics of the document: those that strict mode refused, or those found before
     * rendering failed, which often explain the failure.
     *
     * @return list<Diagnostic>
     */
    public function diagnostics(): array
    {
        return $this->diagnostics;
    }
}
