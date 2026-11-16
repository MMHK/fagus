<?php

declare(strict_types=1);

namespace Fagus\Exception;

use Fagus\Conformance;

/**
 * A value given to Fagus that it does not know, such as a PDF/A level.
 */
final class InvalidArgumentException extends FagusException
{
    public static function unknownConformance(string $level): self
    {
        $levels = implode(', ', array_map(static fn (Conformance $level): string => "'{$level->value}'", Conformance::cases()));

        return new self("Unknown PDF/A level '{$level}'. The levels are {$levels}.");
    }
}
