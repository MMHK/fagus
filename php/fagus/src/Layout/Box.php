<?php

declare(strict_types=1);

namespace Fagus\Layout;

/**
 * The border box of an element, or of a table row, on one page, in points from the top left
 * corner of the page.
 */
final readonly class Box
{
    /**
     * @param  int  $page  the number of the page, from 1
     */
    public function __construct(
        public int $page,
        public float $x,
        public float $y,
        public float $width,
        public float $height,
    ) {}

    /**
     * @param  array<string, mixed>  $data
     *
     * @internal
     */
    public static function fromArray(array $data): self
    {
        return new self(
            (int) ($data['page'] ?? 0),
            (float) ($data['x'] ?? 0),
            (float) ($data['y'] ?? 0),
            (float) ($data['width'] ?? 0),
            (float) ($data['height'] ?? 0),
        );
    }

    /**
     * How far below the top of the page the box ends, in points.
     */
    public function bottom(): float
    {
        return $this->y + $this->height;
    }

    /**
     * How far right of the left edge of the page the box ends, in points.
     */
    public function right(): float
    {
        return $this->x + $this->width;
    }
}
