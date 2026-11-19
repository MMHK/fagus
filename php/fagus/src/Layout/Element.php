<?php

declare(strict_types=1);

namespace Fagus\Layout;

/**
 * An element with an `id` and the boxes it got: one for each page it is on and, for inline
 * elements, for each line. An element that is not rendered (`display: none`) has none.
 */
final readonly class Element
{
    /**
     * @param  string  $tag  its tag name, e.g. `td`
     * @param  list<Box>  $boxes  in page order
     * @param  list<Row>  $rows  the rows of a table, in document order; empty for other elements
     */
    public function __construct(
        public string $id,
        public string $tag,
        public array $boxes,
        public array $rows = [],
    ) {}

    /**
     * @param  array<string, mixed>  $data
     *
     * @internal
     */
    public static function fromArray(array $data): self
    {
        return new self(
            (string) ($data['id'] ?? ''),
            (string) ($data['tag'] ?? ''),
            array_values(array_map(Box::fromArray(...), (array) ($data['boxes'] ?? []))),
            array_values(array_map(Row::fromArray(...), (array) ($data['rows'] ?? []))),
        );
    }

    /**
     * The numbers of the pages the element is on, in order.
     *
     * @return list<int>
     */
    public function pages(): array
    {
        return self::pagesOf($this->boxes);
    }

    /**
     * The number of the first page the element is on, or null if it has no box.
     */
    public function firstPage(): ?int
    {
        return $this->boxes[0]->page ?? null;
    }

    /**
     * The number of the last page the element is on, or null if it has no box.
     */
    public function lastPage(): ?int
    {
        return $this->boxes === [] ? null : $this->boxes[count($this->boxes) - 1]->page;
    }

    /**
     * The rows of a table that are on page `$page`.
     *
     * @return list<Row>
     */
    public function rowsOnPage(int $page): array
    {
        return array_values(array_filter(
            $this->rows,
            static fn (Row $row): bool => in_array($page, $row->pages(), true),
        ));
    }

    /**
     * @param  list<Box>  $boxes
     * @return list<int>
     *
     * @internal
     */
    public static function pagesOf(array $boxes): array
    {
        $pages = [];
        foreach ($boxes as $box) {
            if (end($pages) !== $box->page) {
                $pages[] = $box->page;
            }
        }

        return $pages;
    }
}
