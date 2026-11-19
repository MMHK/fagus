<?php

declare(strict_types=1);

namespace Fagus\Layout;

/**
 * The size of a page in points: that of its page box, without bleed and printer's marks.
 */
final readonly class Page
{
    /**
     * @param  int  $number  from 1
     */
    public function __construct(
        public int $number,
        public float $width,
        public float $height,
    ) {}
}
