<?php

declare(strict_types=1);

namespace Fagus\Tests\Fixtures;

use RuntimeException;

/**
 * A stand-in for `fagus worker`: it greets with the protocol and version it is given, answers
 * every document with a PDF that names its process and how many documents it has rendered, and
 * reports the memory it is given. {@see self::create()} writes a binary that runs it.
 */
final class FakeWorker
{
    /**
     * Writes an executable that runs this worker with the given greeting and memory and returns
     * its path. `$memory` maps fields of the answers (`rss`, `maxRss`) to their values in bytes,
     * one per document of a process, the last one repeated.
     *
     * @param  array<string, list<int>>  $memory
     */
    public static function create(int $protocol = 1, string $version = '0.0.0', array $memory = []): string
    {
        $binary = sys_get_temp_dir().DIRECTORY_SEPARATOR.'fagus-fake-'.bin2hex(random_bytes(6));
        $settings = escapeshellarg((string) json_encode(['protocol' => $protocol, 'version' => $version, 'memory' => (object) $memory]));
        file_put_contents($binary, "#!/bin/sh\nexec ".escapeshellarg(PHP_BINARY).' '.escapeshellarg(__FILE__)." {$settings} \"\$@\"\n");
        chmod($binary, 0755);

        return $binary;
    }

    /**
     * The process and the number of the document in it, from a PDF this worker wrote.
     *
     * @return array{0: int, 1: int}
     */
    public static function parse(string $pdf): array
    {
        if (preg_match('/^%PDF-fake pid=(\d+) n=(\d+)(\n.*)?$/s', $pdf, $match) !== 1) {
            throw new RuntimeException("Not a PDF of the fake worker: {$pdf}");
        }

        return [(int) $match[1], (int) $match[2]];
    }

    /**
     * The limits of the request that the stand-in answered with `$pdf` (`memoryBudget`, `timeLimit`).
     *
     * @return array<string, int|float>
     */
    public static function limits(string $pdf): array
    {
        return (array) json_decode(explode("\n", $pdf, 2)[1] ?? '{}', true);
    }

    /**
     * @param  array{protocol: int, version: string, memory: array<string, list<int>>}  $settings
     */
    public static function serve(array $settings): void
    {
        $input = STDIN;
        $output = STDOUT;
        self::write($output, 'H', ['protocol' => $settings['protocol'], 'version' => $settings['version']]);
        $documents = 0;
        while (($frame = self::read($input)) !== null) {
            [$kind, $header] = $frame;
            if ($kind === 'R') {
                $documents++;
                $reply = ['id' => $header['id'] ?? 0, 'pages' => 1, 'diagnostics' => [], 'timings' => []];
                foreach ($settings['memory'] as $field => $values) {
                    $reply[$field] = $values[min($documents, count($values)) - 1];
                }
                $limits = array_intersect_key($header, ['memoryBudget' => true, 'timeLimit' => true]);
                self::write($output, 'P', $reply, ['%PDF-fake pid='.getmypid()." n={$documents}\n".json_encode($limits)]);
            } elseif ($kind === 'F') {
                self::write($output, 'K', ['faces' => 1]);
            } else {
                self::write($output, 'E', ['message' => "unknown message `{$kind}`", 'diagnostics' => []]);
            }
        }
    }

    /**
     * @param  resource  $stream
     * @return ?array{0: string, 1: array<string, mixed>}
     */
    private static function read($stream): ?array
    {
        $length = self::exactly($stream, 4);
        if ($length === null) {
            return null;
        }
        $body = (string) self::exactly($stream, unpack('V', $length)[1]);
        $headerLength = unpack('V', substr($body, 1, 4))[1];

        return [$body[0], (array) json_decode(substr($body, 5, $headerLength), true)];
    }

    /**
     * @param  resource  $stream
     */
    private static function exactly($stream, int $length): ?string
    {
        $data = '';
        while (strlen($data) < $length) {
            $chunk = fread($stream, $length - strlen($data));
            if ($chunk === false || $chunk === '') {
                return null;
            }
            $data .= $chunk;
        }

        return $data;
    }

    /**
     * @param  resource  $stream
     * @param  array<string, mixed>  $header
     * @param  list<string>  $blobs
     */
    private static function write($stream, string $kind, array $header, array $blobs = []): void
    {
        if ($blobs !== []) {
            $header['blobs'] = array_map(strlen(...), $blobs);
        }
        $json = (string) json_encode($header === [] ? (object) [] : $header);
        $body = $kind.pack('V', strlen($json)).$json.implode('', $blobs);
        fwrite($stream, pack('V', strlen($body)).$body);
        fflush($stream);
    }
}

if (PHP_SAPI === 'cli' && realpath($_SERVER['SCRIPT_FILENAME'] ?? '') === __FILE__) {
    FakeWorker::serve(json_decode($argv[1], true));
}
