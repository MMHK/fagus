<?php

declare(strict_types=1);

namespace Fagus;

use Fagus\Layout\Element;
use Fagus\Layout\Page;

/**
 * Where the elements with an `id` ended up on the pages of a rendered PDF, in PDF points from the
 * top left corner of each page. Rendered with {@see Document::reportLayout()}.
 */
final readonly class Layout
{
    /**
     * @param  list<Page>  $pages  the pages, in order
     * @param  list<Element>  $elements  the elements with an `id`, in document order
     */
    public function __construct(
        public array $pages,
        public array $elements,
    ) {}

    /**
     * @param  array<string, mixed>  $data
     *
     * @internal
     */
    public static function fromArray(array $data): self
    {
        $pages = [];
        foreach (array_values((array) ($data['pages'] ?? [])) as $index => $page) {
            $pages[] = new Page(
                $index + 1,
                (float) ($page['width'] ?? 0),
                (float) ($page['height'] ?? 0),
            );
        }

        return new self(
            $pages,
            array_values(array_map(Element::fromArray(...), (array) ($data['elements'] ?? []))),
        );
    }

    /**
     * The first element with the `id`, or null if there is none.
     */
    public function element(string $id): ?Element
    {
        foreach ($this->elements as $element) {
            if ($element->id === $id) {
                return $element;
            }
        }

        return null;
    }

    /**
     * A page by its number, from 1.
     */
    public function page(int $number): ?Page
    {
        return $this->pages[$number - 1] ?? null;
    }
}
