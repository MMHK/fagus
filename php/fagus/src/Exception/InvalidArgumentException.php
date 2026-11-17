<?php

declare(strict_types=1);

namespace Fagus\Exception;

use Fagus\Conformance;

/**
 * A value given to Fagus that it cannot use, such as an unknown PDF/A level.
 */
final class InvalidArgumentException extends FagusException
{
    public static function unknownConformance(string $level): self
    {
        $levels = implode(', ', array_map(static fn (Conformance $level): string => "'{$level->value}'", Conformance::cases()));

        return new self("Unknown PDF/A level '{$level}'. The levels are {$levels}.");
    }

    public static function emptyInvoiceField(string $field): self
    {
        return new self("The invoice {$field} must not be empty.");
    }
}
