<?php

declare(strict_types=1);

namespace Fagus;

use Stringable;

/**
 * A message about the document: CSS that was ignored, a resource that could not be loaded.
 */
final readonly class Diagnostic implements Stringable
{
    /**
     * @param  string  $code  a stable code, e.g. `css.unknown-property`
     * @param  string  $source  the file or source the message refers to, e.g. `<style> 1`
     * @param  int  $line  1-based, 0 if unknown
     * @param  int  $column  1-based, 0 if unknown
     */
    public function __construct(
        public Severity $severity,
        public string $code,
        public string $message,
        public string $source,
        public int $line = 0,
        public int $column = 0,
    ) {}

    /**
     * @param  array<string, mixed>  $data
     *
     * @internal
     */
    public static function fromArray(array $data): self
    {
        return new self(
            Severity::tryFrom((string) ($data['severity'] ?? '')) ?? Severity::Warning,
            (string) ($data['code'] ?? ''),
            (string) ($data['message'] ?? ''),
            (string) ($data['source'] ?? ''),
            (int) ($data['line'] ?? 0),
            (int) ($data['column'] ?? 0),
        );
    }

    public function __toString(): string
    {
        $position = $this->line > 0 ? ':'.$this->line.($this->column > 0 ? ':'.$this->column : '') : '';

        return "{$this->source}{$position}: {$this->severity->value}[{$this->code}]: {$this->message}";
    }
}
