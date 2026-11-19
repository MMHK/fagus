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
     * @param  ?Layout  $layout  where the elements ended up, if the document asked for it
     */
    public function __construct(
        private string $bytes,
        private int $pages,
        private array $diagnostics = [],
        private array $timings = [],
        private ?Layout $layout = null,
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
            is_array($reply['layout'] ?? null) ? Layout::fromArray($reply['layout']) : null,
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

    /**
     * Where the elements with an `id` ended up: their pages and boxes, and the rows of their
     * tables.
     *
     * @throws FagusException when the document was rendered without {@see Document::reportLayout()}
     */
    public function layout(): Layout
    {
        return $this->layout ?? throw new FagusException(
            'The layout was not reported; render the document with reportLayout() to read it, with a fagus binary of this release.',
        );
    }

    public function save(string $path): void
    {
        if (@file_put_contents($path, $this->bytes) === false) {
            throw new FagusException("The PDF could not be written to `{$path}`.");
        }
    }
}
