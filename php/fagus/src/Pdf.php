<?php

declare(strict_types=1);

namespace Fagus;

use Fagus\Exception\FagusException;

/**
 * A rendered PDF with what Fagus noticed while rendering it.
 */
final readonly class Pdf
{
    /**
     * @param  list<Diagnostic>  $diagnostics
     * @param  array<string, float>  $timings  milliseconds per phase: parse, style, layout, paint, pdf
     */
    public function __construct(
        private string $bytes,
        private int $pages,
        private array $diagnostics = [],
        private array $timings = [],
    ) {}

    /**
     * @param  array<string, mixed>  $reply
     *
     * @internal
     */
    public static function fromReply(array $reply, string $bytes): self
    {
        return new self(
            $bytes,
            (int) ($reply['pages'] ?? 0),
            array_values(array_map(Diagnostic::fromArray(...), $reply['diagnostics'] ?? [])),
            array_map(floatval(...), $reply['timings'] ?? []),
        );
    }

    /**
     * The PDF file.
     */
    public function bytes(): string
    {
        return $this->bytes;
    }

    public function pages(): int
    {
        return $this->pages;
    }

    /**
     * @return list<Diagnostic>
     */
    public function diagnostics(): array
    {
        return $this->diagnostics;
    }

    /**
     * @return array<string, float>
     */
    public function timings(): array
    {
        return $this->timings;
    }

    public function save(string $path): void
    {
        if (@file_put_contents($path, $this->bytes) === false) {
            throw new FagusException("The PDF could not be written to `{$path}`.");
        }
    }
}
