<?php

declare(strict_types=1);

namespace Fagus;

/**
 * A PDF/A level. PDF/A-1b is PDF 1.4 without transparency; PDF/A-3 allows attachments of any
 * type, as ZUGFeRD and Factur-X need; PDF/A-4 is PDF 2.0, and PDF/A-4f allows attachments of any
 * type too. The levels ending in `a` are tagged for accessibility.
 */
enum Conformance: string
{
    case PdfA1b = '1b';
    case PdfA2b = '2b';
    case PdfA2u = '2u';
    case PdfA2a = '2a';
    case PdfA3b = '3b';
    case PdfA3u = '3u';
    case PdfA3a = '3a';
    case PdfA4 = '4';
    case PdfA4e = '4e';
    case PdfA4f = '4f';
}
