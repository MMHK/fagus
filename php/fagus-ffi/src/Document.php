<?php

declare(strict_types=1);

namespace Fagus\Ffi;

use Fagus\Ffi\Exception\RenderException;

/**
 * One document to render: the HTML, its style sheets and resources, PDF/A and PDF/UA, attachments,
 * Factur-X, encryption and limits.
 *
 *     $pdf = (new Engine())->html('<h1>Invoice</h1>')
 *         ->stylesheet('h1 { color: teal }')
 *         ->pdfA('3b')
 *         ->render();
 *
 * The options are those of the worker protocol (`docs/protocol.md`); signatures are not available
 * through the C ABI yet.
 */
final class Document
{
    /** @var Engine|null */
    private $engine;

    /** @var string */
    private $html = '';

    /** @var bool */
    private $xhtml = false;

    /** @var array<string, mixed> */
    private $options = [];

    /**
     * The data blocks. Index 0 is the HTML, which the request fills in when it is built.
     *
     * @var string[]
     */
    private $blobs = [''];

    public function __construct(?Engine $engine = null)
    {
        $this->engine = $engine;
    }

    public function html(string $html): self
    {
        $this->html = $html;
        $this->xhtml = false;

        return $this;
    }

    /**
     * XHTML, parsed as XML as browsers parse `.xhtml` files.
     */
    public function xhtml(string $xhtml): self
    {
        $this->html = $xhtml;
        $this->xhtml = true;

        return $this;
    }

    /**
     * The directory that relative URLs resolve against; files are read from it.
     */
    public function baseDirectory(string $directory): self
    {
        $this->options['base'] = $directory;

        return $this;
    }

    /**
     * The directory that URLs starting with `/` resolve against; it must contain the base directory.
     */
    public function root(string $directory): self
    {
        $this->options['root'] = $directory;

        return $this;
    }

    /**
     * A style sheet added after those of the document; `name` is what diagnostics call it.
     */
    public function stylesheet(string $css, string $name = 'stylesheet'): self
    {
        $this->options['stylesheets'][] = ['name' => $name, 'blob' => $this->addBlob($css)];

        return $this;
    }

    /**
     * Data the document may use, by the URL written in the document.
     */
    public function resource(string $url, string $data): self
    {
        $this->options['resources'][] = ['url' => $url, 'blob' => $this->addBlob($data)];

        return $this;
    }

    /**
     * PDF/A: `1b`, `2b`, `2u`, `2a`, `3b`, `3u`, `3a`, `4`, `4e` or `4f`.
     */
    public function pdfA(string $level = '3b'): self
    {
        $this->options['conformance'] = $level;

        return $this;
    }

    /**
     * Whether the PDF is tagged, with the logical structure of the document (on by default).
     */
    public function tagged(bool $tagged = true): self
    {
        $this->options['tagged'] = $tagged;

        return $this;
    }

    /**
     * Whether the PDF conforms to PDF/UA-1 (which needs a title, a language and alternative text
     * for every image).
     */
    public function accessible(bool $accessible = true): self
    {
        $this->options['accessible'] = $accessible;

        return $this;
    }

    /**
     * Whether form controls become the fields of an interactive form.
     */
    public function forms(bool $forms = true): self
    {
        $this->options['forms'] = $forms;

        return $this;
    }

    /**
     * An attached file. `relationship` is `source`, `data`, `alternative`, `supplement` or
     * `unspecified`; PDF/A-3 and PDF/UA need a description, which falls back to the file name.
     */
    public function attach(
        string $name,
        string $mime,
        string $data,
        ?string $relationship = null,
        ?string $description = null
    ): self {
        $attachment = ['name' => $name, 'mime' => $mime, 'blob' => $this->addBlob($data)];
        if ($relationship !== null) {
            $attachment['relationship'] = $relationship;
        }
        if ($description !== null) {
            $attachment['description'] = $description;
        }
        $this->options['attachments'][] = $attachment;

        return $this;
    }

    /**
     * The XML of a ZUGFeRD or Factur-X invoice, attached and described in the PDF's metadata.
     */
    public function facturX(
        string $xml,
        ?string $profile = null,
        ?string $version = null,
        ?string $description = null
    ): self {
        $invoice = ['blob' => $this->addBlob($xml)];
        if ($profile !== null) {
            $invoice['profile'] = $profile;
        }
        if ($version !== null) {
            $invoice['version'] = $version;
        }
        if ($description !== null) {
            $invoice['description'] = $description;
        }
        $this->options['facturX'] = $invoice;

        return $this;
    }

    /**
     * The creation date, as a `DateTimeInterface` or a string such as `2026-11-20T12:00:00Z`.
     */
    public function created($date): self
    {
        if ($date instanceof \DateTimeInterface) {
            $date = $date->format('Y-m-d\TH:i:sP');
        }
        $this->options['created'] = (string) $date;

        return $this;
    }

    /**
     * AES-256 encryption: `$userPassword` opens the PDF, `$ownerPassword` lifts the restrictions.
     * `$permissions` lists what is allowed without the owner password: `print`,
     * `print-low-quality`, `modify`, `copy`, `annotate`, `fill-forms` and `assemble`.
     */
    public function encrypt(
        ?string $userPassword = null,
        ?string $ownerPassword = null,
        ?array $permissions = null
    ): self {
        $encryption = [];
        if ($userPassword !== null) {
            $encryption['userPassword'] = $userPassword;
        }
        if ($ownerPassword !== null) {
            $encryption['ownerPassword'] = $ownerPassword;
        }
        if ($permissions !== null) {
            $encryption['permissions'] = array_values($permissions);
        }
        $this->options['encryption'] = $encryption;

        return $this;
    }

    /**
     * The memory budget of the document in bytes (0 or null for none).
     */
    public function memoryBudget(?int $bytes): self
    {
        $this->options['memoryBudget'] = $bytes === null ? 0 : $bytes;

        return $this;
    }

    /**
     * The time limit of the document in milliseconds (0 or null for none).
     */
    public function timeLimit(?float $milliseconds): self
    {
        $this->options['timeLimit'] = $milliseconds === null ? 0 : $milliseconds;

        return $this;
    }

    /**
     * Whether the answer reports where the elements with an `id` ended up.
     */
    public function reportLayout(bool $reportLayout = true): self
    {
        $this->options['layout'] = $reportLayout;

        return $this;
    }

    /**
     * The request of the worker protocol, and its data blocks in order.
     *
     * @return array{0: array<string, mixed>, 1: string[]}
     *
     * @throws RenderException
     */
    public function request(int $id = 1): array
    {
        if ($this->html === '') {
            throw new RenderException('The document has no HTML.');
        }
        $blobs = $this->blobs;
        $blobs[0] = $this->html;
        $header = ['id' => $id, 'html' => 0] + $this->options;
        if ($this->xhtml) {
            $header['xhtml'] = true;
        }
        $lengths = [];
        foreach ($blobs as $blob) {
            $lengths[] = strlen($blob);
        }
        $header['blobLengths'] = $lengths;

        return [$header, $blobs];
    }

    /**
     * @throws RenderException
     */
    public function render(?Engine $engine = null): Pdf
    {
        $engine = $engine ?: $this->engine ?: Engine::shared();
        list($header, $blobs) = $this->request();
        list($bytes, $meta) = $engine->native()->render($engine->handle(), $header, $blobs);

        return new Pdf($bytes, $meta);
    }

    /**
     * Renders the document into a file.
     *
     * @throws RenderException
     */
    public function save(string $path, ?Engine $engine = null): Pdf
    {
        return $this->render($engine)->save($path);
    }

    private function addBlob(string $data): int
    {
        $this->blobs[] = $data;

        return count($this->blobs) - 1;
    }
}
