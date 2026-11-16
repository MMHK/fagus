<?php

declare(strict_types=1);

namespace Fagus;

/**
 * A PDF/A level. PDF/A-3 allows attachments of any type, as ZUGFeRD and Factur-X need; the
 * levels ending in `a` are tagged for accessibility.
 */
enum Conformance: string
{
    case PdfA2b = '2b';
    case PdfA2u = '2u';
    case PdfA2a = '2a';
    case PdfA3b = '3b';
    case PdfA3u = '3u';
    case PdfA3a = '3a';
}
