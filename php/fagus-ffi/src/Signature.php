<?php

declare(strict_types=1);

namespace Fagus\Ffi;

use DateTimeInterface;

/**
 * A digital signature: a PKCS#12 file and where it shows.
 *
 *     $signature = Signature::pkcs12($p12, 'secret')
 *         ->name('Niklas Forst')
 *         ->reason('Invoice 42');
 *
 * Timestamps, revocation data and external signers need the worker protocol, which can call back
 * into the caller; the C ABI cannot.
 */
final class Signature
{
    /** @var string */
    private $data;

    /** @var string|null */
    private $password;

    /** @var array<string, mixed> */
    private $options = [];

    private function __construct(string $data, ?string $password)
    {
        $this->data = $data;
        $this->password = $password;
    }

    public static function pkcs12(string $data, ?string $password = null): self
    {
        return new self($data, $password);
    }

    public function name(string $name): self
    {
        $this->options['name'] = $name;

        return $this;
    }

    public function reason(string $reason): self
    {
        $this->options['reason'] = $reason;

        return $this;
    }

    public function location(string $location): self
    {
        $this->options['location'] = $location;

        return $this;
    }

    public function contactInfo(string $contactInfo): self
    {
        $this->options['contactInfo'] = $contactInfo;

        return $this;
    }

    /**
     * The time of the signature, as a `DateTimeInterface` or a string such as
     * `2026-11-20T12:00:00Z`.
     */
    public function time($time): self
    {
        if ($time instanceof DateTimeInterface) {
            $time = $time->format('Y-m-d\TH:i:sP');
        }
        $this->options['time'] = (string) $time;

        return $this;
    }

    /**
     * What a certification signature allows: `no-changes`, `fill-forms` or `annotate`.
     */
    public function certify(string $certification): self
    {
        $this->options['certify'] = $certification;

        return $this;
    }

    /**
     * The `id` of the element in whose box the signature shows.
     */
    public function visibleOn(string $element): self
    {
        unset($this->options['area']);
        $this->options['element'] = $element;

        return $this;
    }

    /**
     * The area of a page in which the signature shows, in points from its top left corner.
     */
    public function visibleAt(int $page, float $x, float $y, float $width, float $height): self
    {
        unset($this->options['element']);
        $this->options['area'] = [
            'page' => $page,
            'x' => $x,
            'y' => $y,
            'width' => $width,
            'height' => $height,
        ];

        return $this;
    }

    /**
     * @internal For {@see Document}.
     */
    public function bytes(): string
    {
        return $this->data;
    }

    /**
     * The signature as the protocol's JSON, with the block that holds the PKCS#12 file.
     *
     * @internal For {@see Document}.
     *
     * @return array<string, mixed>
     */
    public function request(int $blob): array
    {
        $signature = ['pkcs12' => $blob] + $this->options;
        if ($this->password !== null) {
            $signature['password'] = $this->password;
        }

        return $signature;
    }
}
