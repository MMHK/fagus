<?php

declare(strict_types=1);

namespace Fagus\Laravel\LaravelPdf;

use Fagus\Laravel\PdfFactory;
use Fagus\Laravel\PendingPdf;
use Spatie\LaravelPdf\Drivers\PdfDriver;
use Spatie\LaravelPdf\Exceptions\CouldNotGeneratePdf;
use Spatie\LaravelPdf\PdfOptions;

/**
 * Fagus as a driver of spatie/laravel-pdf: `Pdf::view(...)->driver('fagus')`, or
 * `LARAVEL_PDF_DRIVER=fagus` for every PDF. Documents start with the base directory, root and
 * tagging of config/fagus.php and render with the application's engine.
 *
 * The format, paper size, orientation and margins of the builder become an `@page` rule after the
 * document's own stylesheets, so they win over the document's `@page` rule; options that are not
 * set leave the document's `@page` alone. The header and footer become running elements in the
 * top and bottom page margins.
 */
final readonly class FagusDriver implements PdfDriver
{
    /**
     * The paper formats of laravel-pdf, in inches, as Chrome prints them.
     */
    private const array FORMATS = [
        'letter' => [8.5, 11],
        'legal' => [8.5, 14],
        'tabloid' => [11, 17],
        'ledger' => [17, 11],
        'a0' => [33.1, 46.8],
        'a1' => [23.4, 33.1],
        'a2' => [16.54, 23.4],
        'a3' => [11.7, 16.54],
        'a4' => [8.27, 11.7],
        'a5' => [5.83, 8.27],
        'a6' => [4.13, 5.83],
    ];

    private const string HEADER = 'fagus-laravel-pdf-header';

    private const string FOOTER = 'fagus-laravel-pdf-footer';

    public function __construct(private PdfFactory $pdfs) {}

    public function generatePdf(string $html, ?string $headerHtml, ?string $footerHtml, PdfOptions $options): string
    {
        return $this->pending($html, $headerHtml, $footerHtml, $options)->render()->bytes();
    }

    public function savePdf(string $html, ?string $headerHtml, ?string $footerHtml, PdfOptions $options, string $path): void
    {
        $this->pending($html, $headerHtml, $footerHtml, $options)->save($path);
    }

    /**
     * The Fagus document for what laravel-pdf hands over.
     */
    private function pending(string $html, ?string $headerHtml, ?string $footerHtml, PdfOptions $options): PendingPdf
    {
        if ($options->scale !== null && $options->scale !== 1.0) {
            throw new CouldNotGeneratePdf('The [fagus] driver does not support scale(): Fagus lays the document out at the size its CSS gives. Use the CSS property `zoom` instead, for example `html { zoom: 0.8 }`.');
        }
        if ($options->pageRanges !== null && trim($options->pageRanges) !== '') {
            throw new CouldNotGeneratePdf('The [fagus] driver does not support pageRanges(): it renders every page of the document. Leave out what should not be printed, or hide it with `@media print`.');
        }

        $header = self::fragment($headerHtml);
        $footer = self::fragment($footerHtml);
        $css = self::pageRule($options, $header !== null, $footer !== null);

        $pdf = $this->pdfs->html(self::withRunningElements($html, $header, $footer));
        if ($css !== '') {
            $pdf = $pdf->stylesheet($css, 'laravel-pdf');
        }

        // laravel-pdf's tagged() asks for tags; without it, the configuration decides (tagged by default).
        return $options->tagged ? $pdf->tagged() : $pdf;
    }

    /**
     * The `@page` rule and the styles of the running elements, empty when nothing is set.
     */
    private static function pageRule(PdfOptions $options, bool $header, bool $footer): string
    {
        $declarations = [];
        $size = self::size($options);
        if ($size !== null) {
            $declarations[] = "size: {$size};";
        }
        if ($options->margins !== null) {
            $unit = self::unit($options->margins['unit'] ?? 'mm');
            $margins = array_map(
                static fn (string $side): string => self::number((float) ($options->margins[$side] ?? 0)).$unit,
                ['top', 'right', 'bottom', 'left'],
            );
            $declarations[] = 'margin: '.implode(' ', $margins).';';
        }
        if ($header) {
            $declarations[] = '@top-center { content: element('.self::HEADER.'); }';
        }
        if ($footer) {
            $declarations[] = '@bottom-center { content: element('.self::FOOTER.'); }';
        }
        if ($declarations === []) {
            return '';
        }

        $css = '@page { '.implode(' ', $declarations)." }\n";
        foreach (array_keys(array_filter([self::HEADER => $header, self::FOOTER => $footer])) as $name) {
            // The classes that Chrome fills in its header and footer templates (@pageNumber, @totalPages).
            $css .= ".{$name} { position: running({$name}); width: 100%; }\n"
                .".{$name} .pageNumber::before { content: counter(page); }\n"
                .".{$name} .totalPages::before { content: counter(pages); }\n";
        }

        return $css;
    }

    /**
     * The value of `size`: the paper size, else the format, turned to the orientation; the
     * orientation alone turns the page size of the document.
     */
    private static function size(PdfOptions $options): ?string
    {
        $orientation = $options->orientation === null ? null : strtolower($options->orientation);
        if ($orientation !== null && $orientation !== 'portrait' && $orientation !== 'landscape') {
            throw new CouldNotGeneratePdf("The [fagus] driver does not know the orientation [{$options->orientation}]. Use Portrait or Landscape.");
        }

        if ($options->paperSize !== null) {
            $unit = self::unit($options->paperSize['unit'] ?? 'mm');
            $width = (float) ($options->paperSize['width'] ?? 0);
            $height = (float) ($options->paperSize['height'] ?? 0);
        } elseif ($options->format !== null) {
            $unit = 'in';
            [$width, $height] = self::FORMATS[strtolower($options->format)]
                ?? throw new CouldNotGeneratePdf("The [fagus] driver does not know the format [{$options->format}]. The formats are ".implode(', ', array_keys(self::FORMATS)).'; use paperSize() for others.');
        } else {
            return $orientation;
        }

        if (($orientation === 'landscape' && $width < $height) || ($orientation === 'portrait' && $width > $height)) {
            [$width, $height] = [$height, $width];
        }

        return self::number((float) $width).$unit.' '.self::number((float) $height).$unit;
    }

    private static function unit(mixed $unit): string
    {
        $unit = strtolower((string) $unit);
        if (! in_array($unit, ['px', 'in', 'cm', 'mm', 'pt', 'pc', 'q'], true)) {
            throw new CouldNotGeneratePdf("The [fagus] driver does not know the unit [{$unit}]. Use px, in, cm, mm, pt or pc.");
        }

        return $unit;
    }

    /**
     * A number as CSS writes it, whatever the locale.
     */
    private static function number(float $number): string
    {
        $formatted = rtrim(rtrim(sprintf('%.4F', $number), '0'), '.');

        return $formatted === '-0' ? '0' : $formatted;
    }

    /**
     * The header or footer as a fragment of the document. Chrome renders them as documents of
     * their own, so header and footer views may be whole documents: their doctype, `html`, `head`
     * and `body` tags, title and meta elements are dropped, their styles kept.
     */
    private static function fragment(?string $html): ?string
    {
        if ($html === null || trim($html) === '') {
            return null;
        }
        $html = preg_replace('~<title\b[^>]*>.*?</title\s*>~is', '', $html) ?? $html;

        return preg_replace('~<!doctype[^>]*>|</?(?:html|head|body)\b[^>]*>|<meta\b[^>]*>~i', '', $html) ?? $html;
    }

    /**
     * The document with the header and footer at the start of its body, where running elements
     * show from the first page on.
     */
    private static function withRunningElements(string $html, ?string $header, ?string $footer): string
    {
        $elements = ($header === null ? '' : '<div class="'.self::HEADER.'">'.$header.'</div>')
            .($footer === null ? '' : '<div class="'.self::FOOTER.'">'.$footer.'</div>');
        if ($elements === '') {
            return $html;
        }

        foreach (['~<body\b[^>]*>~i', '~</head\s*>~i', '~^\s*(?:<!doctype[^>]*>)?\s*(?:<html\b[^>]*>)?~i'] as $pattern) {
            if (preg_match($pattern, $html, $match, PREG_OFFSET_CAPTURE) === 1) {
                $end = $match[0][1] + strlen($match[0][0]);

                return substr($html, 0, $end).$elements.substr($html, $end);
            }
        }

        return $elements.$html;
    }
}
