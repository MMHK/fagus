#!/bin/sh

# Runs the package's PHPUnit suite against a real library inside an official php:*-cli container,
# which ships neither FFI nor Composer (and whose Debian lists no longer hold the unzip the
# images would need): the PHPUnit phar needs none of that, and tests/autoload.php is the PSR-4
# autoloader when there is no vendor directory.
#
#     docker run --rm -v "$PWD:/w" -w /w php:8.5-cli \
#         sh /w/php/fagus-ffi/tests/php-unit.sh /w/php/fagus-ffi \
#         /w/target/x86_64-unknown-linux-gnu/release/libfagus_ffi.so
#
# PHPUnit 9.6 on PHP 7.4 and 8.0, PHPUnit 12 on 8.3 and later, as composer.json allows; running it
# at both ends of the range shows that the suite itself works there. smoke.php stays the check that
# needs nothing but PHP: it runs on every version, and on the assembled package where there is no
# vendor directory.

set -eu

package=$1
library=$2

apt-get update -qq
apt-get install -y -qq --no-install-recommends libffi-dev >/dev/null
docker-php-ext-install ffi >/dev/null

if php -r 'exit(version_compare(PHP_VERSION, "8.3.0", ">=") ? 0 : 1);'; then
    phpunit=12.5
else
    phpunit=9.6
fi
curl -sSLo /tmp/phpunit.phar "https://phar.phpunit.de/phpunit-$phpunit.phar"

cd "$package"
FAGUS_FFI_LIB="$library" php -d ffi.enable=true /tmp/phpunit.phar -c phpunit.xml.dist
