<?php

declare(strict_types=1);

namespace Fagus\Layout;

/**
 * A row of a table (`<tr>`) and the boxes it got: one on each page it is on.
 */
final readonly class Row
{
    /**
     * @param  int  $index  its place among the rows of its section in the table, from 0
     * @param  list<Box>  $boxes
     */
    public function __construct(
        public Section $section,
        public int $index,
        public ?string $id,
        public array $boxes,
    ) {}

    /**
     * @param  array<string, mixed>  $data
     *
     * @internal
     */
    public static function fromArray(array $data): self
    {
        return new self(
            Section::tryFrom((string) ($data['section'] ?? '')) ?? Section::Body,
            (int) ($data['index'] ?? 0),
            isset($data['id']) ? (string) $data['id'] : null,
            array_values(array_map(Box::fromArray(...), (array) ($data['boxes'] ?? []))),
        );
    }

    /**
     * The numbers of the pages the row is on, in order: two or more for a row that breaks, or
     * a header row that repeats.
     *
     * @return list<int>
     */
    public function pages(): array
    {
        return Element::pagesOf($this->boxes);
    }
}
