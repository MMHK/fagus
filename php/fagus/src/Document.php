<?php

declare(strict_types=1);

namespace Fagus;

use Closure;
use DateTimeImmutable;
use DateTimeInterface;
use DateTimeZone;
use Fagus\Exception\InvalidArgumentException;
use Fagus\Exception\RenderException;

/**
 * A document to render. Every method returns a changed copy; the document itself never changes.
 */
final class Document
{
    private ?string $base = null;

    private ?string $root = null;

    /** @var list<array{name: string, css: string}> */
    private array $stylesheets = [];

    /** @var array<string, string> */
    private array $resources = [];

    /** @var (Closure(string): ?string)|null */
    private ?Closure $resolver = null;

    private ?Conformance $conformance = null;

    private bool $tagged = true;

    private bool $accessible = false;

    /** @var list<array{name: string, data: string, mime: string, relationship: Relationship, description: ?string}> */
    private array $attachments = [];

    private ?string $invoice = null;

    private ?InvoiceProfile $profile = null;

    private ?DateTimeInterface $created = null;

    private bool $strict = false;

    private function __construct(
        private readonly string $html,
        private readonly bool $xhtml = false,
    ) {}

    public static function html(string $html): self
    {
        return new self($html);
    }

    /**
     * An XHTML document, parsed as XML as browsers parse `.xhtml` files: CDATA sections and
     * empty-element tags (`<div/>`) work, names are case-sensitive and nothing is implied.
     */
    public static function xhtml(string $xml): self
    {
        return new self($xml, xhtml: true);
    }

    /**
     * The directory that relative URLs (stylesheets, images, fonts) resolve against. Only files
     * inside it are read, unless {@see self::root()} allows a larger directory.
     */
    public function baseDirectory(string $directory): self
    {
        $copy = clone $this;
        $copy->base = $directory;

        return $copy;
    }

    /**
     * The directory that files may be read from; it must contain the base directory.
     */
    public function root(string $directory): self
    {
        $copy = clone $this;
        $copy->root = $directory;

        return $copy;
    }

    /**
     * Adds a stylesheet after those of the document.
     */
    public function stylesheet(string $css, string $name = 'stylesheet'): self
    {
        $copy = clone $this;
        $copy->stylesheets[] = ['name' => $name, 'css' => $css];

        return $copy;
    }

    /**
     * Hands over the data of a resource the document refers to, by its URL as written there.
     */
    public function resource(string $url, string $data): self
    {
        $copy = clone $this;
        $copy->resources[$url] = $data;

        return $copy;
    }

    /**
     * Answers requests for resources that are not handed over with {@see self::resource()}: the
     * callback gets the URL as written in the document and returns the data, or null. It is
     * asked before a file is looked for; after null, the file inside the base directory (or
     * root) is read. Fagus itself never goes to the network.
     *
     * @param  callable(string): ?string  $resolver
     */
    public function resolver(callable $resolver): self
    {
        $copy = clone $this;
        $copy->resolver = $resolver(...);

        return $copy;
    }

    /**
     * Writes the PDF as PDF/A. A level given as a string is a value of {@see Conformance}
     * ('2b', '2u', '2a', '3b', '3u' or '3a', case-insensitive).
     *
     * @throws InvalidArgumentException for a string that is no level
     */
    public function pdfA(Conformance|string $level = Conformance::PdfA3b): self
    {
        if (is_string($level)) {
            $level = Conformance::tryFrom(strtolower($level)) ?? throw InvalidArgumentException::unknownConformance($level);
        }
        $copy = clone $this;
        $copy->conformance = $level;

        return $copy;
    }

    /**
     * Whether the PDF is tagged: with the logical structure of the document (headings,
     * paragraphs, lists, tables, images and links in reading order), which screen readers read.
     * On by default; without it, a PDF with many table cells is somewhat smaller and faster.
     */
    public function tagged(bool $tagged = true): self
    {
        $copy = clone $this;
        $copy->tagged = $tagged;

        return $copy;
    }

    /**
     * Writes the PDF as PDF/UA-1, the standard for accessible PDF. The document needs a
     * `<title>`, a language (`<html lang>`) and alternative text for every image (`alt`, empty
     * for decoration); without them, rendering fails with a {@see RenderException}.
     */
    public function accessible(bool $accessible = true): self
    {
        $copy = clone $this;
        $copy->accessible = $accessible;

        return $copy;
    }

    /**
     * Attaches a file to the PDF. Without a description, the name describes it (PDF/A-3 and
     * PDF/UA require one).
     */
    public function attach(
        string $name,
        string $data,
        string $mime = 'application/octet-stream',
        Relationship $relationship = Relationship::Unspecified,
        ?string $description = null,
    ): self {
        $copy = clone $this;
        $copy->attachments[] = compact('name', 'data', 'mime', 'relationship', 'description');

        return $copy;
    }

    /**
     * Makes the PDF an electronic invoice of ZUGFeRD or Factur-X: the XML is attached under the
     * name its profile requires and described in the XMP metadata, and the PDF conforms to
     * PDF/A-3. The profile is the one the XML declares unless it is given.
     */
    public function facturX(string $xml, ?InvoiceProfile $profile = null): self
    {
        $copy = clone $this;
        $copy->invoice = $xml;
        $copy->profile = $profile;

        return $copy;
    }

    /**
     * The creation date written into the PDF (now by default). A fixed date makes the output
     * reproducible.
     */
    public function created(DateTimeInterface $date): self
    {
        $copy = clone $this;
        $copy->created = $date;

        return $copy;
    }

    /**
     * Fails with a {@see RenderException} when the document has diagnostics (CSS that is
     * ignored, resources that are missing).
     */
    public function strict(bool $strict = true): self
    {
        $copy = clone $this;
        $copy->strict = $strict;

        return $copy;
    }

    public function render(?Engine $engine = null): Pdf
    {
        $pdf = ($engine ?? Fagus::engine())->render($this);
        if ($this->strict && $pdf->diagnostics() !== []) {
            throw RenderException::strict($pdf->diagnostics());
        }

        return $pdf;
    }

    public function save(string $path, ?Engine $engine = null): Pdf
    {
        $pdf = $this->render($engine);
        $pdf->save($path);

        return $pdf;
    }

    /**
     * The render request of the worker protocol: its header and data blocks.
     *
     * @return array{0: array<string, mixed>, 1: list<string>}
     *
     * @internal
     */
    public function request(int $id): array
    {
        $blobs = [$this->html];
        $add = static function (string $data) use (&$blobs): int {
            $blobs[] = $data;

            return count($blobs) - 1;
        };
        $header = ['id' => $id, 'html' => 0, 'ask' => $this->resolver !== null];
        if ($this->xhtml) {
            $header['xhtml'] = true;
        }
        if ($this->base !== null) {
            $header['base'] = $this->base;
        }
        if ($this->root !== null) {
            $header['root'] = $this->root;
        }
        foreach ($this->stylesheets as $stylesheet) {
            $header['stylesheets'][] = ['name' => $stylesheet['name'], 'blob' => $add($stylesheet['css'])];
        }
        foreach ($this->resources as $url => $data) {
            $header['resources'][] = ['url' => (string) $url, 'blob' => $add($data)];
        }
        if ($this->conformance !== null) {
            $header['conformance'] = $this->conformance->value;
        }
        $header['tagged'] = $this->tagged;
        if ($this->accessible) {
            $header['accessible'] = true;
        }
        foreach ($this->attachments as $attachment) {
            $header['attachments'][] = [
                'name' => $attachment['name'],
                'mime' => $attachment['mime'],
                'relationship' => $attachment['relationship']->value,
                'description' => $attachment['description'],
                'blob' => $add($attachment['data']),
            ];
        }
        if ($this->invoice !== null) {
            $header['facturX'] = ['blob' => $add($this->invoice)];
            if ($this->profile !== null) {
                $header['facturX']['profile'] = $this->profile->value;
            }
        }
        if ($this->created !== null) {
            $header['created'] = DateTimeImmutable::createFromInterface($this->created)
                ->setTimezone(new DateTimeZone('UTC'))
                ->format('Y-m-d\TH:i:s\Z');
        }

        return [$header, $blobs];
    }

    /**
     * The data of a resource the engine asks for.
     *
     * @internal
     */
    public function resolve(string $url): ?string
    {
        return $this->resolver === null ? null : ($this->resolver)($url);
    }
}
