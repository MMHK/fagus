#!/bin/sh

# Checks the two ways a deployment can let PHP-FPM use the package (docs/php.md): `ffi.enable` is
# `preload` by default, so either it is set to `true`, or a file that OPcache preloads renders
# before the pool serves a request. Runs inside php:*-fpm, which ships neither FFI nor a FastCGI
# client:
#
#     docker run --rm -v "$PWD:/w" -w /w php:8.4-fpm \
#         sh /w/php/fagus-ffi/tests/fpm-preload.sh /w/php/fagus-ffi \
#         /w/target/x86_64-unknown-linux-gnu/release/libfagus_ffi.so
#
# `php -d ffi.enable=true` covers the command line (php-smoke.sh); this covers the pool.

set -eu

package=$1
library=$2
work=/tmp/fagus-fpm
failures=0

fail() {
    echo "FAIL: $*" >&2
    failures=$((failures + 1))
}

apt-get update -qq
apt-get install -y -qq --no-install-recommends libffi-dev libfcgi-bin >/dev/null
docker-php-ext-install ffi opcache >/dev/null

rm -rf "$work"
mkdir -p "$work"
# The paths come from files rather than from the environment, because the pool drops the
# environment of the master by default and the preload file does not see a request's at all.
printf '%s' "$package" > "$work/package"
printf '%s' "$library" > "$work/library"

cat > "$work/render.php" <<'PHP'
<?php

require trim(file_get_contents('/tmp/fagus-fpm/package')).'/tests/autoload.php';

$pdf = (new Fagus\Ffi\Engine([], trim(file_get_contents('/tmp/fagus-fpm/library'))))
    ->html('<h1>FPM</h1>')
    ->render();
printf("ffi.enable=%s %s\n", ini_get('ffi.enable'), strncmp($pdf->bytes(), '%PDF-', 5) === 0 ? 'rendered' : 'garbage');
PHP

cat > "$work/refused.php" <<'PHP'
<?php

require trim(file_get_contents('/tmp/fagus-fpm/package')).'/tests/autoload.php';

try {
    new Fagus\Ffi\Engine([], trim(file_get_contents('/tmp/fagus-fpm/library')));
} catch (Throwable $error) {
    printf("ffi.enable=%s refused\n", ini_get('ffi.enable'));
    exit(0);
}
printf("ffi.enable=%s accepted\n", ini_get('ffi.enable'));
exit(1);
PHP

cat > "$work/preload.php" <<'PHP'
<?php

require trim(file_get_contents('/tmp/fagus-fpm/package')).'/tests/autoload.php';

$pdf = (new Fagus\Ffi\Engine([], trim(file_get_contents('/tmp/fagus-fpm/library'))))
    ->html('<h1>Preloaded</h1>')
    ->render();
file_put_contents('/tmp/fagus-fpm/preloaded', strncmp($pdf->bytes(), '%PDF-', 5) === 0 ? "rendered\n" : "garbage\n");
PHP

# One FastCGI request to the pool, with curl's output stripped of the response headers.
request() {
    SCRIPT_FILENAME="$work/$1" SCRIPT_NAME="/$1" REQUEST_METHOD=GET \
        cgi-fcgi -bind -connect 127.0.0.1:9000 2>/dev/null | tr -d '\r'
}

# Starts the pool with the given php.ini lines, asks for the script until the pool answers, and
# stops it again. Prints what the request said.
serve() {
    printf '%s\n' "$@" > /usr/local/etc/php/conf.d/zz-fagus.ini
    rm -f "$work/preloaded"
    php-fpm >"$work/fpm.log" 2>&1 &
    master=$!
    answer=''
    tries=0
    while [ "$tries" -lt 50 ]; do
        if answer=$(request "$script") && [ -n "$answer" ]; then
            break
        fi
        tries=$((tries + 1))
        sleep 0.2
    done
    kill "$master" 2>/dev/null || true
    wait "$master" 2>/dev/null || true
    if [ -z "$answer" ]; then
        cat "$work/fpm.log" >&2
    fi
}

expect() {
    name=$1
    expected=$2
    case $answer in
        *"$expected"*) printf '%s: %s\n' "$name" "$answer" ;;
        *) fail "$name: expected '$expected', got '$answer'" ;;
    esac
}

# 1. The setting the README asks for: the request renders.
script=render.php
serve 'ffi.enable=true'
expect 'ffi.enable=true' 'rendered'

# 2. The default, with no preload file: a request cannot call the library. This is why the
#    requirement above exists; a deployment that relies on the default and preloads nothing gets
#    an error rather than a silently missing renderer.
script=refused.php
serve 'ffi.enable=preload'
expect 'ffi.enable=preload without a preload file' 'refused'

# 3. The default with a preload file: the file renders as the pool starts. What a request may do
#    with the engine then is PHP's business; that the preloaded path works is what the package
#    promises here.
script=refused.php
serve 'ffi.enable=preload' 'opcache.preload=/tmp/fagus-fpm/preload.php' 'opcache.preload_user=root'
preloaded=$(cat "$work/preloaded" 2>/dev/null || true)
if [ "$preloaded" = "rendered" ]; then
    echo "ffi.enable=preload with a preload file: $preloaded"
else
    fail "ffi.enable=preload with a preload file: expected the preload to render, got '$preloaded'"
    cat "$work/fpm.log" >&2
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)" >&2
    exit 1
fi
echo "PASS"
