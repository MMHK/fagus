<?php

declare(strict_types=1);

namespace Fagus;

use Closure;
use DateTimeImmutable;
use DateTimeInterface;
use DateTimeZone;
use Fagus\Exception\InvalidArgumentException;

/**
 * A digital signature of the PDF (PAdES): the key that signs, and what the signature says. Every
 * method returns a changed copy; the signature itself never changes.
 */
final class Signature
{
    /** The name of the algorithm of RSA keys, as the signer callback gets it. */
    public const RSA_SHA256 = 'RSA-SHA256';

    /** The name of the algorithm of ECDSA keys on the curve P-256, as the signer callback gets it. */
    public const ECDSA_P256_SHA256 = 'ECDSA-P256-SHA256';

    private ?string $name = null;

    private ?string $reason = null;

    private ?string $location = null;

    private ?string $contactInfo = null;

    private ?DateTimeInterface $time = null;

    private ?Certification $certification = null;

    private ?string $element = null;

    /** @var array{page: int, x: float, y: float, width: float, height: float}|null */
    private ?array $area = null;

    /** @var (Closure(string): string)|null */
    private ?Closure $timestamper = null;

    /** @var (Closure(RevocationRequest): list<string>)|null */
    private ?Closure $revocation = null;

    private bool $archiveTimestamp = false;

    /**
     * @param  list<string>  $certificates
     * @param  (Closure(string, string): string)|null  $signer
     */
    private function __construct(
        private readonly ?string $pkcs12,
        #[\SensitiveParameter]
        private readonly ?string $password,
        private readonly array $certificates = [],
        private readonly ?Closure $signer = null,
    ) {}

    /**
     * Signs with the key and certificates of a PKCS#12 file (`.p12`, `.pfx`), RSA or ECDSA on
     * P-256. The file's other certificates go into the signature too.
     *
     * @throws InvalidArgumentException if the file cannot be read
     */
    public static function fromPkcs12(string $path, #[\SensitiveParameter] string $password): self
    {
        $data = is_file($path) ? @file_get_contents($path) : false;
        if ($data === false) {
            throw new InvalidArgumentException("The PKCS#12 file `{$path}` cannot be read.");
        }

        return new self($data, $password);
    }

    /**
     * Signs with the key and certificates of a PKCS#12 file given as its bytes, such as one read
     * from a secrets manager or a database, RSA or ECDSA on P-256. The file's other certificates
     * go into the signature too.
     *
     * @throws InvalidArgumentException for empty data
     */
    public static function fromPkcs12Data(#[\SensitiveParameter] string $data, #[\SensitiveParameter] string $password): self
    {
        if ($data === '') {
            throw new InvalidArgumentException('The PKCS#12 data is empty.');
        }

        return new self($data, $password);
    }

    /**
     * Signs with a key that Fagus does not hold, such as one in a hardware security module or a
     * key management service. `$certificates` are the signer's certificate and then those of its
     * chain, each PEM or DER. The callback gets the SHA-256 digest (32 bytes) to sign and the
     * algorithm ({@see self::RSA_SHA256} or {@see self::ECDSA_P256_SHA256}) and returns the
     * signature of that digest: for RSA the PKCS#1 v1.5 signature, for ECDSA the DER signature or
     * the 64 bytes of r and s.
     *
     * @param  list<string>  $certificates
     * @param  callable(string, string): string  $signer
     *
     * @throws InvalidArgumentException without certificates
     */
    public static function withSigner(array $certificates, callable $signer): self
    {
        if ($certificates === []) {
            throw new InvalidArgumentException('A signature needs the certificate of its signer.');
        }

        return new self(null, null, array_values($certificates), $signer(...));
    }

    /**
     * The name of the signer, as the signature shows it; readers show the name in the
     * certificate otherwise.
     */
    public function name(string $name): self
    {
        $copy = clone $this;
        $copy->name = $name;

        return $copy;
    }

    /**
     * Why the document is signed.
     */
    public function reason(string $reason): self
    {
        $copy = clone $this;
        $copy->reason = $reason;

        return $copy;
    }

    /**
     * Where the document is signed.
     */
    public function location(string $location): self
    {
        $copy = clone $this;
        $copy->location = $location;

        return $copy;
    }

    /**
     * How to reach the signer.
     */
    public function contactInfo(string $contactInfo): self
    {
        $copy = clone $this;
        $copy->contactInfo = $contactInfo;

        return $copy;
    }

    /**
     * The time of signing; the creation date of the document by default.
     */
    public function time(DateTimeInterface $time): self
    {
        $copy = clone $this;
        $copy->time = $time;

        return $copy;
    }

    /**
     * Makes the signature a certification signature (DocMDP): the author's signature, which says
     * what later changes the document allows. It must be the PDF's first signature.
     */
    public function certify(Certification $certification): self
    {
        $copy = clone $this;
        $copy->certification = $certification;

        return $copy;
    }

    /**
     * Makes the signature visible in the box of the element with this `id`: what the element's
     * content paints becomes the appearance of the signature field, while its background and
     * border stay on the page. Rendering fails if the document has no such element or it is not
     * rendered.
     */
    public function visibleOn(string $id): self
    {
        $copy = clone $this;
        $copy->element = $id;
        $copy->area = null;

        return $copy;
    }

    /**
     * Makes the signature visible in an area of a page, which shows what the page has there: the
     * number of the page (from 1), and the area in points from the top left corner of the page,
     * as {@see Layout} gives the boxes of elements.
     *
     * @throws InvalidArgumentException for a page below 1 or an empty area
     */
    public function visibleAt(int $page, float $x, float $y, float $width, float $height): self
    {
        if ($page < 1 || $width <= 0 || $height <= 0) {
            throw new InvalidArgumentException('A signature needs a page from 1 and an area that is not empty.');
        }
        $copy = clone $this;
        $copy->area = ['page' => $page, 'x' => $x, 'y' => $y, 'width' => $width, 'height' => $height];
        $copy->element = null;

        return $copy;
    }

    /**
     * Timestamps the signature (PAdES B-T). The callback gets the SHA-256 digest (32 bytes) of the
     * signature and returns the answer of a time-stamping authority (RFC 3161): the timestamp
     * token or the whole response. {@see self::timestampRequest()} makes the request to send.
     *
     * @param  callable(string): string  $timestamper
     */
    public function timestamp(callable $timestamper): self
    {
        $copy = clone $this;
        $copy->timestamper = $timestamper(...);

        return $copy;
    }

    /**
     * Adds the validation data of the signature to the PDF (PAdES B-LT, with
     * {@see self::timestamp()}): the certificates and their revocation status, so that the
     * signature can be validated without the network, also once the certificates have expired.
     * The callback gets a {@see RevocationRequest} for each certificate of the signature and of
     * its timestamp but the roots, and returns what the OCSP responder or the CRL distribution
     * point answered: OCSP responses and certificate revocation lists (DER), any number of each,
     * or none where the status cannot be had.
     *
     * @param  callable(RevocationRequest): list<string>  $revocation
     */
    public function revocation(callable $revocation): self
    {
        $copy = clone $this;
        $copy->revocation = $revocation(...);

        return $copy;
    }

    /**
     * Adds a document timestamp after the validation data (PAdES B-LTA), from the callback of
     * {@see self::timestamp()}. Needs {@see self::timestamp()} and {@see self::revocation()};
     * rendering fails without them.
     */
    public function archiveTimestamp(): self
    {
        $copy = clone $this;
        $copy->archiveTimestamp = true;

        return $copy;
    }

    /**
     * The RFC 3161 request for a timestamp of a SHA-256 digest, which asks the authority for its
     * certificate too: the body of an HTTP POST to the authority, with the content type
     * `application/timestamp-query`.
     *
     * @throws InvalidArgumentException for a digest that is not 32 bytes long
     */
    public static function timestampRequest(string $digest): string
    {
        if (strlen($digest) !== 32) {
            throw new InvalidArgumentException('A SHA-256 digest is 32 bytes long.');
        }

        return "\x30\x39\x02\x01\x01\x30\x31\x30\x0d\x06\x09\x60\x86\x48\x01\x65\x03\x04\x02\x01\x05\x00\x04\x20"
            .$digest."\x01\x01\xff";
    }

    /**
     * Leaves the key and its password out of `var_dump()` and dumps.
     *
     * @return array<string, mixed>
     */
    public function __debugInfo(): array
    {
        return [
            'source' => $this->pkcs12 !== null ? 'pkcs12' : 'signer',
            'certificates' => count($this->certificates),
            'name' => $this->name,
            'reason' => $this->reason,
            'location' => $this->location,
            'contactInfo' => $this->contactInfo,
            'time' => $this->time,
            'timestamped' => $this->timestamper !== null,
            'certification' => $this->certification,
            'element' => $this->element,
            'area' => $this->area,
            'revocation' => $this->revocation !== null,
            'archiveTimestamp' => $this->archiveTimestamp,
        ];
    }

    /**
     * The `signature` of the render request.
     *
     * @param  Closure(string): int  $add  adds a data block and returns its index
     * @return array<string, mixed>
     *
     * @internal
     */
    public function header(Closure $add): array
    {
        $header = $this->pkcs12 !== null
            ? ['pkcs12' => $add($this->pkcs12), 'password' => $this->password]
            : ['certificates' => array_map($add, $this->certificates)];
        foreach (['name' => $this->name, 'reason' => $this->reason, 'location' => $this->location, 'contactInfo' => $this->contactInfo] as $key => $value) {
            if ($value !== null) {
                $header[$key] = $value;
            }
        }
        if ($this->time !== null) {
            $header['time'] = DateTimeImmutable::createFromInterface($this->time)
                ->setTimezone(new DateTimeZone('UTC'))
                ->format('Y-m-d\TH:i:s\Z');
        }
        if ($this->timestamper !== null) {
            $header['timestamp'] = true;
        }
        if ($this->certification !== null) {
            $header['certify'] = $this->certification->value;
        }
        if ($this->element !== null) {
            $header['element'] = $this->element;
        }
        if ($this->area !== null) {
            $header['area'] = $this->area;
        }
        if ($this->revocation !== null) {
            $header['revocation'] = true;
        }
        if ($this->archiveTimestamp) {
            $header['archiveTimestamp'] = true;
        }

        return $header;
    }

    /**
     * The signature of a digest the engine asks for.
     *
     * @internal
     */
    public function signDigest(string $digest, string $algorithm): string
    {
        if ($this->signer === null) {
            throw new InvalidArgumentException('This signature has no signer callback.');
        }

        return ($this->signer)($digest, $algorithm);
    }

    /**
     * The timestamp of a digest the engine asks for.
     *
     * @internal
     */
    public function timestampDigest(string $digest): string
    {
        if ($this->timestamper === null) {
            throw new InvalidArgumentException('This signature has no timestamp callback.');
        }

        return ($this->timestamper)($digest);
    }

    /**
     * The revocation data of a certificate the engine asks for.
     *
     * @param  array<string, mixed>  $question  the header of the engine's question
     * @param  list<string>  $data  the certificate, its issuer's and the OCSP request
     * @return list<string>
     *
     * @internal
     */
    public function revocationData(array $question, array $data): array
    {
        if ($this->revocation === null) {
            throw new InvalidArgumentException('This signature has no revocation callback.');
        }
        $urls = static fn (mixed $list): array => array_values(array_map('strval', is_array($list) ? $list : []));

        return array_values(($this->revocation)(new RevocationRequest(
            $data[0] ?? '',
            $data[1] ?? '',
            $urls($question['ocspUrls'] ?? []),
            $urls($question['crlUrls'] ?? []),
            $data[2] ?? '',
        )));
    }
}
