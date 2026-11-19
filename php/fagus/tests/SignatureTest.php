<?php

declare(strict_types=1);

namespace Fagus\Tests;

use DateTimeImmutable;
use DateTimeZone;
use Fagus\Binary;
use Fagus\Document;
use Fagus\Engine;
use Fagus\Exception\InvalidArgumentException;
use Fagus\Exception\RenderException;
use Fagus\Signature;
use PHPUnit\Framework\Attributes\Test;
use PHPUnit\Framework\TestCase;
use RuntimeException;

/**
 * Signs with the test keys of tests/signing, against the fagus binary in FAGUS_BINARY or the
 * release build of this repository.
 */
final class SignatureTest extends TestCase
{
    private const string FIXTURES = __DIR__.'/../../../tests/signing';

    private const string PASSWORD = 'fagus-test-only';

    private static Engine $engine;

    public static function setUpBeforeClass(): void
    {
        $binary = getenv('FAGUS_BINARY') ?: __DIR__.'/../../../target/release/'.Binary::name();
        if (! is_executable($binary)) {
            self::markTestSkipped("No fagus binary at {$binary}; build it with `cargo build --release`.");
        }
        self::$engine = new Engine($binary, [__DIR__.'/../../../tests/fonts']);
    }

    private static function document(): Document
    {
        return Document::html('<title>Rechnung</title><p>Rechnung 2026-0042</p>')
            ->created(new DateTimeImmutable('2026-11-19T09:30:00Z'));
    }

    /**
     * The byte range of the last signature, which must cover the file but the signature.
     *
     * @return array{0: int, 1: int, 2: int, 3: int}
     */
    private function byteRange(string $pdf): array
    {
        $this->assertSame(1, preg_match_all('/\/ByteRange\[0 +(\d+) +(\d+) +(\d+)\]/', $pdf, $matches));
        $range = [0, (int) $matches[1][0], (int) $matches[2][0], (int) $matches[3][0]];
        $this->assertSame(strlen($pdf), $range[2] + $range[3]);
        $this->assertStringEndsWith('/Contents', substr($pdf, 0, $range[1]));

        return $range;
    }

    /**
     * Checks the signature with OpenSSL through tests/signing/verify.py, where both are there.
     */
    private function assertVerifies(string $pdf): void
    {
        $this->byteRange($pdf);
        $python = trim((string) shell_exec('command -v python3'));
        $openssl = trim((string) shell_exec('command -v openssl'));
        if ($python === '' || $openssl === '') {
            return;
        }
        $file = tempnam(sys_get_temp_dir(), 'fagus-signed');
        file_put_contents($file, $pdf);
        try {
            exec(escapeshellarg($python).' '.escapeshellarg(self::FIXTURES.'/verify.py').' '.escapeshellarg($file).' 2>&1', $output, $status);
        } finally {
            unlink($file);
        }
        $this->assertSame(0, $status, implode("\n", $output));
    }

    #[Test]
    public function it_signs_with_pkcs12_files(): void
    {
        $signature = Signature::fromPkcs12(self::FIXTURES.'/rsa.p12', self::PASSWORD)
            ->reason('Rechnung')
            ->location('Berlin')
            ->contactInfo('rechnung@example.com')
            ->name('Rechnungsstelle');
        $pdf = self::document()->sign($signature)->render(self::$engine)->bytes();

        $this->assertVerifies($pdf);
        $this->assertStringContainsString('/M(D:20261119093000Z)/Name(Rechnungsstelle)/Reason(Rechnung)/Location(Berlin)/ContactInfo(rechnung@example.com)', $pdf);
        $this->assertSame($pdf, self::document()->sign($signature)->render(self::$engine)->bytes(), 'the same bytes again');

        $ecdsa = self::document()->sign(Signature::fromPkcs12(self::FIXTURES.'/ecdsa.p12', self::PASSWORD))->pdfA('3b')->render(self::$engine)->bytes();
        $this->assertVerifies($ecdsa);
    }

    #[Test]
    public function it_takes_the_time_of_signing(): void
    {
        $signature = Signature::fromPkcs12(self::FIXTURES.'/rsa.p12', self::PASSWORD)
            ->time(new DateTimeImmutable('2026-11-20 10:00', new DateTimeZone('Europe/Berlin')));
        $pdf = self::document()->sign($signature)->render(self::$engine)->bytes();

        $this->assertStringContainsString('/M(D:20261120090000Z)', $pdf);
    }

    #[Test]
    public function it_asks_a_signer_outside(): void
    {
        if (! function_exists('openssl_private_encrypt')) {
            $this->markTestSkipped('The OpenSSL extension signs for the test.');
        }
        $key = openssl_pkey_get_private((string) file_get_contents(self::FIXTURES.'/rsa.key'));
        $asked = [];
        $signature = Signature::withSigner(
            certificates: [(string) file_get_contents(self::FIXTURES.'/rsa.pem'), (string) file_get_contents(self::FIXTURES.'/ca.pem')],
            signer: function (string $digest, string $algorithm) use ($key, &$asked): string {
                $asked[] = [strlen($digest), $algorithm];
                openssl_private_encrypt(hex2bin('3031300d060960864801650304020105000420').$digest, $signed, $key);

                return $signed;
            },
        );
        $pdf = self::document()->sign($signature)->render(self::$engine)->bytes();

        $this->assertSame([[32, Signature::RSA_SHA256]], $asked);
        $this->assertVerifies($pdf);
        // RSA signatures are deterministic: the same key in the PKCS#12 file signs alike.
        $own = self::document()->sign(Signature::fromPkcs12(self::FIXTURES.'/rsa.p12', self::PASSWORD))->render(self::$engine)->bytes();
        $this->assertSame($own, $pdf);
    }

    #[Test]
    public function it_timestamps_signatures(): void
    {
        $openssl = trim((string) shell_exec('command -v openssl'));
        if ($openssl === '') {
            $this->markTestSkipped('OpenSSL is the time-stamping authority of the test.');
        }
        // OpenSSL's configuration takes backslashes as escapes.
        $directory = str_replace('\\', '/', sys_get_temp_dir()).'/fagus-tsa-'.bin2hex(random_bytes(6));
        $fixtures = str_replace('\\', '/', (string) realpath(self::FIXTURES));
        mkdir($directory);
        file_put_contents("{$directory}/serial", "01\n");
        file_put_contents("{$directory}/tsa.cnf", implode("\n", [
            '[tsa]', 'default_tsa = test', '[test]', "serial = {$directory}/serial",
            "signer_cert = {$fixtures}/tsa.pem", "signer_key = {$fixtures}/tsa.key",
            'signer_digest = sha256', 'default_policy = 1.2.3.4', 'digests = sha256', 'ess_cert_id_alg = sha256', '',
        ]));
        $digests = [];
        $signature = Signature::fromPkcs12(self::FIXTURES.'/rsa.p12', self::PASSWORD)->timestamp(
            function (string $digest) use ($directory, $openssl, &$digests): string {
                $digests[] = $digest;
                file_put_contents("{$directory}/request.tsq", Signature::timestampRequest($digest));
                exec(escapeshellarg($openssl)." ts -reply -config {$directory}/tsa.cnf -queryfile {$directory}/request.tsq -out {$directory}/response.tsr 2>&1", $output, $status);
                if ($status !== 0) {
                    throw new RuntimeException(implode("\n", $output));
                }

                return (string) file_get_contents("{$directory}/response.tsr");
            },
        );
        try {
            $pdf = self::document()->sign($signature)->render(self::$engine)->bytes();
        } finally {
            array_map(unlink(...), (array) glob("{$directory}/*"));
            rmdir($directory);
        }

        $this->assertCount(1, $digests);
        $this->assertSame(32, strlen($digests[0]));
        $this->assertVerifies($pdf);
    }

    #[Test]
    public function it_ends_the_render_with_the_exception_of_a_callback(): void
    {
        $signature = Signature::withSigner([(string) file_get_contents(self::FIXTURES.'/rsa.pem')], static function (): string {
            throw new RuntimeException('Der Schlüsseldienst antwortet nicht.');
        });
        try {
            self::document()->sign($signature)->render(self::$engine);
            $this->fail('The exception of the signer was lost.');
        } catch (RuntimeException $exception) {
            $this->assertSame('Der Schlüsseldienst antwortet nicht.', $exception->getMessage());
        }
        // The engine starts a new worker for the next document.
        $this->assertSame(1, self::document()->render(self::$engine)->pages());
    }

    #[Test]
    public function it_reports_what_keeps_it_from_signing(): void
    {
        try {
            self::document()->sign(Signature::fromPkcs12(self::FIXTURES.'/rsa.p12', 'falsch'))->render(self::$engine);
            $this->fail('A wrong password was taken.');
        } catch (RenderException $exception) {
            $this->assertStringContainsString('the PKCS#12 file cannot be read', $exception->getMessage());
        }

        $wrong = Signature::withSigner([(string) file_get_contents(self::FIXTURES.'/rsa.pem')], static fn (string $digest): string => str_repeat("\0", 256));
        try {
            self::document()->sign($wrong)->render(self::$engine);
            $this->fail('A wrong signature was taken.');
        } catch (RenderException $exception) {
            $this->assertStringContainsString('does not match the certificate', $exception->getMessage());
        }

        $this->expectException(InvalidArgumentException::class);
        Signature::fromPkcs12(self::FIXTURES.'/fehlt.p12', self::PASSWORD);
    }

    #[Test]
    public function it_does_not_sign_encrypted_pdfs_yet(): void
    {
        $this->expectException(RenderException::class);
        $this->expectExceptionMessage('the PDF could not be signed: an encrypted PDF cannot be signed yet; sign it or encrypt it');
        self::document()
            ->encrypt('benutzer')
            ->sign(Signature::fromPkcs12(self::FIXTURES.'/rsa.p12', self::PASSWORD))
            ->render(self::$engine);
    }

    #[Test]
    public function it_hides_the_key_from_dumps(): void
    {
        $dump = print_r(Signature::fromPkcs12(self::FIXTURES.'/rsa.p12', self::PASSWORD)->reason('Rechnung'), true);

        $this->assertStringNotContainsString(self::PASSWORD, $dump);
        $this->assertStringContainsString('Rechnung', $dump);
    }

    #[Test]
    public function it_makes_timestamp_requests(): void
    {
        $request = Signature::timestampRequest(str_repeat("\x07", 32));

        $this->assertSame(59, strlen($request));
        $this->assertSame(strlen($request) - 2, ord($request[1]));

        $this->expectException(InvalidArgumentException::class);
        Signature::timestampRequest('kurz');
    }
}
