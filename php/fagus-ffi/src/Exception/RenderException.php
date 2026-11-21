<?php

declare(strict_types=1);

namespace Fagus\Ffi\Exception;

use Fagus\Ffi\Diagnostic;
use Throwable;

/**
 * A document could not be rendered: a missing data block, an unknown PDF/A level, a document that
 * does not meet PDF/A or PDF/UA, or a panic inside the engine.
 */
class RenderException extends FagusException
{
    /** @var Diagnostic[] */
    private $diagnostics;

    /**
     * @param Diagnostic[] $diagnostics
     */
    public function __construct(string $message, array $diagnostics = [], ?Throwable $previous = null)
    {
        parent::__construct($message, 0, $previous);
        $this->diagnostics = $diagnostics;
    }

    /**
     * @return Diagnostic[]
     */
    public function diagnostics(): array
    {
        return $this->diagnostics;
    }
}
