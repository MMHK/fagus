<?php

declare(strict_types=1);

namespace Fagus;

/**
 * What the revocation status of one certificate of a signature is asked with, for
 * {@see Signature::revocation()}: the certificate, its issuer's certificate, where to ask, and
 * the OCSP request to send.
 */
final readonly class RevocationRequest
{
    /**
     * @param  string  $certificate  the certificate (DER)
     * @param  string  $issuer  the certificate of its issuer (DER)
     * @param  list<string>  $ocspUrls  the OCSP responders the certificate names: POST
     *                                  `$ocspRequest` to them with the content type
     *                                  `application/ocsp-request`
     * @param  list<string>  $crlUrls  the certificate revocation lists the certificate names, to GET
     * @param  string  $ocspRequest  the OCSP request for the certificate (DER)
     */
    public function __construct(
        public string $certificate,
        public string $issuer,
        public array $ocspUrls,
        public array $crlUrls,
        public string $ocspRequest,
    ) {}
}
