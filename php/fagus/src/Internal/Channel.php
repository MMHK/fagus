<?php

declare(strict_types=1);

namespace Fagus\Internal;

use Fagus\Exception\EngineException;
use JsonException;
use stdClass;

/**
 * The frames of the worker protocol (docs/protocol.md) over the pipes of the worker process.
 *
 * @internal
 */
final class Channel
{
    private const MAX_FRAME = 1 << 30;

    private const CHUNK = 65536;

    /**
     * @param  resource  $input  the worker's standard output
     * @param  resource  $output  the worker's standard input
     * @param  float  $timeout  seconds to wait for the worker before giving up
     */
    public function __construct(
        private $input,
        private $output,
        private readonly float $timeout,
    ) {}

    /**
     * @param  array<string, mixed>  $header
     * @param  list<string>  $blobs
     */
    public function write(string $kind, array $header, array $blobs = []): void
    {
        if ($blobs !== []) {
            $header['blobs'] = array_map(strlen(...), $blobs);
        }
        try {
            $json = json_encode($header === [] ? new stdClass : $header, JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE);
        } catch (JsonException $exception) {
            throw new EngineException('The request could not be encoded: '.$exception->getMessage(), 0, $exception);
        }
        $length = 1 + 4 + strlen($json) + array_sum(array_map(strlen(...), $blobs));
        $this->send(pack('V', $length).$kind.pack('V', strlen($json)).$json);
        foreach ($blobs as $blob) {
            $this->send($blob);
        }
    }

    /**
     * @return array{0: string, 1: array<string, mixed>, 2: list<string>}
     */
    public function read(): array
    {
        $length = unpack('V', $this->receive(4))[1];
        if ($length < 5 || $length > self::MAX_FRAME) {
            throw new EngineException("The worker sent a frame of {$length} bytes.");
        }
        $body = $this->receive($length);
        $headerLength = unpack('V', substr($body, 1, 4))[1];
        try {
            $header = json_decode(substr($body, 5, $headerLength), true, 512, JSON_THROW_ON_ERROR);
        } catch (JsonException $exception) {
            throw new EngineException('The worker sent a header that is not JSON.', 0, $exception);
        }
        if (! is_array($header)) {
            throw new EngineException('The worker sent a header that is not an object.');
        }
        $blobs = [];
        $offset = 5 + $headerLength;
        foreach ($header['blobs'] ?? [] as $blobLength) {
            $blobs[] = substr($body, $offset, (int) $blobLength);
            $offset += (int) $blobLength;
        }

        return [$body[0], $header, $blobs];
    }

    private function send(string $data): void
    {
        $length = strlen($data);
        for ($offset = 0; $offset < $length; $offset += $written) {
            $written = @fwrite($this->output, substr($data, $offset, self::CHUNK));
            if ($written === false || $written === 0) {
                throw EngineException::stopped();
            }
        }
    }

    private function receive(int $length): string
    {
        $data = '';
        while (strlen($data) < $length) {
            // Pipes cannot be selected on Windows; reads block there.
            if (PHP_OS_FAMILY !== 'Windows') {
                $read = [$this->input];
                $write = null;
                $except = null;
                $seconds = (int) $this->timeout;
                $ready = @stream_select($read, $write, $except, $seconds, (int) (($this->timeout - $seconds) * 1_000_000));
                if ($ready === 0) {
                    throw EngineException::timedOut($this->timeout);
                }
            }
            $chunk = fread($this->input, $length - strlen($data));
            if ($chunk === false || ($chunk === '' && feof($this->input))) {
                throw EngineException::stopped();
            }
            $data .= $chunk;
        }

        return $data;
    }
}
