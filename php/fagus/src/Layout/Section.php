<?php

declare(strict_types=1);

namespace Fagus\Layout;

/**
 * The row group a table row is in.
 */
enum Section: string
{
    /** `<thead>` */
    case Header = 'header';

    /** `<tbody>`, or rows outside any row group */
    case Body = 'body';

    /** `<tfoot>` */
    case Footer = 'footer';
}
