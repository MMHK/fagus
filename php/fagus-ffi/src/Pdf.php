<?php

declare(strict_types=1);

namespace Fagus\Ffi;

use Fagus\Ffi\Exception\FagusException;

/**
 * A rendered PDF, with what the engine reported about it.
 */
final class Pdf
{
    /** @var string */
    private $bytes;

    /** @var array<string, mixed> */
    private $meta;

    /**
     * @param array<string, mixed> $meta
     */
    public function __construct(string $bytes, array $meta)
    {
        $this->bytes = $bytes;
        $this->meta = $meta;
    }

    public function bytes(): string
    {
        return $this->bytes;
    }

    public function size(): int
    {
        return strlen($this->bytes);
    }

    public function pages(): int
    {
        return (int) ($this->meta['pages'] ?? 0);
    }

    /**
     * Whether the PDF is encrypted.
     */
    public function encrypted(): bool
    {
        return ($this->meta['encrypted'] ?? false) === true;
    }

    /**
     * Whether the PDF is signed.
     */
    public function signed(): bool
    {
        return ($this->meta['signed'] ?? false) === true;
    }

    /**
     * Whether the form controls became the fields of an interactive form.
     */
    public function forms(): bool
    {
        return ($this->meta['forms'] ?? false) === true;
    }

    /**
     * @return Diagnostic[]
     */
    public function diagnostics(): array
    {
        $diagnostics = [];
        foreach ($this->meta['diagnostics'] ?? [] as $diagnostic) {
            $diagnostics[] = Diagnostic::fromArray($diagnostic);
        }

        return $diagnostics;
    }

    /**
     * The timings of the render in milliseconds, by stage.
     *
     * @return array<string, float>
     */
    public function timings(): array
    {
        return $this->meta['timings'] ?? [];
    }

    /**
     * Where the elements with an `id` ended up, if the document asked for it.
     *
     * @return array<string, mixed>|null
     */
    public function layout(): ?array
    {
        return $this->meta['layout'] ?? null;
    }

    /**
     * Everything the engine reported, including `pages`, `diagnostics`, `timings` and `layout`.
     *
     * @return array<string, mixed>
     */
    public function meta(): array
    {
        return $this->meta;
    }

    /**
     * Writes the PDF to a file.
     *
     * @throws FagusException
     */
    public function save(string $path): self
    {
        $written = @file_put_contents($path, $this->bytes);
        if ($written === false || $written !== strlen($this->bytes)) {
            throw new FagusException(sprintf('Cannot write the PDF to %s.', $path));
        }

        return $this;
    }
}
