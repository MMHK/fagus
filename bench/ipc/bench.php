<?php
declare(strict_types=1);
// Usage: php bench.php <engine-binary> <lib.so|-> <in_bytes> <out_bytes> <runs> [fat_mb]
[$_, $bin, $lib, $inBytes, $outBytes, $runs] = $argv + [6 => 0];
$inBytes = (int) $inBytes; $outBytes = (int) $outBytes; $runs = (int) $runs;
$fat = isset($argv[6]) ? str_repeat('x', (int) $argv[6] * 1048576) : '';   // simulate a fat worker (resident memory)
$input = str_repeat('<div class="p-4">Rechnung</div>', intdiv($inBytes, 31) + 1);
$input = substr($input, 0, $inBytes);

function median(array $t): float { sort($t); return $t[intdiv(count($t), 2)]; }
function readn($h, int $n): string { $b = ''; while (strlen($b) < $n) { $c = fread($h, $n - strlen($b)); if ($c === '' || $c === false) break; $b .= $c; } return $b; }

// 1) spawn per call (proc_open)
$t = [];
for ($i = 0; $i < $runs; $i++) {
    $s = hrtime(true);
    $p = proc_open([$bin, (string) $outBytes], [0 => ['pipe', 'r'], 1 => ['pipe', 'w']], $pipes);
    fwrite($pipes[0], $input); fclose($pipes[0]);
    $out = stream_get_contents($pipes[1]); fclose($pipes[1]);
    proc_close($p);
    $t[] = (hrtime(true) - $s) / 1e6;
    if (strlen($out) !== $outBytes) { fwrite(STDERR, "bad output\n"); exit(1); }
}
printf("spawn per call     : median %6.3f ms\n", median($t));

// 2) persistent worker (one process, length-prefixed frames)
$p = proc_open([$bin, '--worker', (string) $outBytes], [0 => ['pipe', 'r'], 1 => ['pipe', 'w']], $pipes);
$t = [];
for ($i = 0; $i < $runs; $i++) {
    $s = hrtime(true);
    fwrite($pipes[0], pack('V', strlen($input)).$input);
    $len = unpack('V', readn($pipes[1], 4))[1];
    $out = readn($pipes[1], $len);
    $t[] = (hrtime(true) - $s) / 1e6;
}
fclose($pipes[0]); fclose($pipes[1]); proc_close($p);
printf("persistent worker  : median %6.3f ms\n", median($t));

// 3) FFI (in-process call incl. copying input and output)
if ($lib !== '-' && extension_loaded('ffi')) {
    $ffi = FFI::cdef('size_t engine_render(const char *in, size_t in_len, size_t out_len, char **out); void engine_free(char *p);', $lib);
    $t = [];
    for ($i = 0; $i < $runs; $i++) {
        $s = hrtime(true);
        $outPtr = $ffi->new('char*');
        $n = $ffi->engine_render($input, strlen($input), $outBytes, FFI::addr($outPtr));
        $out = FFI::string($outPtr, $n);
        $ffi->engine_free($outPtr);
        $t[] = (hrtime(true) - $s) / 1e6;
    }
    printf("FFI in-process     : median %6.3f ms\n", median($t));
}
