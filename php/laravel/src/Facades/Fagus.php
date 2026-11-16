<?php

declare(strict_types=1);

namespace Fagus\Laravel\Facades;

use Fagus\Engine;
use Fagus\Laravel\PdfFactory;
use Fagus\Laravel\PendingPdf;
use Illuminate\Support\Facades\Facade;

/**
 * @method static PendingPdf html(string $html)
 * @method static PendingPdf xhtml(string $xml)
 * @method static PendingPdf view(string $view, array<string, mixed> $data = [], array<string, mixed> $mergeData = [])
 * @method static Engine engine()
 *
 * @see PdfFactory
 */
final class Fagus extends Facade
{
    protected static function getFacadeAccessor(): string
    {
        return 'fagus';
    }
}
