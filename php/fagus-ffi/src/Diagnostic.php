<?php

declare(strict_types=1);

namespace Fagus\Ffi;

/**
 * One diagnostic of a render: a warning about CSS, a failure of PDF/A or PDF/UA, a limit.
 */
final class Diagnostic
{
    /** @var string */
    private $severity;

    /** @var string */
    private $code;

    /** @var string */
    private $message;

    /** @var string */
    private $source;

    /** @var int */
    private $line;

    /** @var int */
    private $column;

    public function __construct(
        string $severity,
        string $code,
        string $message,
        string $source = '',
        int $line = 0,
        int $column = 0
    ) {
        $this->severity = $severity;
        $this->code = $code;
        $this->message = $message;
        $this->source = $source;
        $this->line = $line;
        $this->column = $column;
    }

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        return new self(
            (string) ($data['severity'] ?? 'info'),
            (string) ($data['code'] ?? ''),
            (string) ($data['message'] ?? ''),
            (string) ($data['source'] ?? ''),
            (int) ($data['line'] ?? 0),
            (int) ($data['column'] ?? 0)
        );
    }

    public function severity(): string
    {
        return $this->severity;
    }

    public function code(): string
    {
        return $this->code;
    }

    public function message(): string
    {
        return $this->message;
    }

    public function source(): string
    {
        return $this->source;
    }

    public function line(): int
    {
        return $this->line;
    }

    public function column(): int
    {
        return $this->column;
    }

    public function isError(): bool
    {
        return $this->severity === 'error';
    }

    public function isWarning(): bool
    {
        return $this->severity === 'warning';
    }

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        return [
            'severity' => $this->severity,
            'code' => $this->code,
            'message' => $this->message,
            'source' => $this->source,
            'line' => $this->line,
            'column' => $this->column,
        ];
    }
}
