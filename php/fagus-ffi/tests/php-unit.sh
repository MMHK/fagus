#!/bin/sh

# Runs the package's PHPUnit suite against a real library inside an official php:*-cli container,
# which ships neither FFI nor Composer:
#
#     docker run --rm -v "$PWD:/w" -w /w php:8.5-cli \
#         sh /w/php/fagus-ffi/tests/php-unit.sh /w/php/fagus-ffi \
#         /w/target/x86_64-unknown-linux-gnu/release/libfagus_ffi.so
#
# Composer picks PHPUnit 9.6 on PHP 7.4 and 8.0 and PHPUnit 12 on 8.3 and later, so running it on
# both ends of the range shows that the suite itself works there. smoke.php stays the check that
# needs nothing but PHP: it runs on every version, and on the assembled package where there is no
# vendor directory.

set -eu

package=$1
library=$2

apt-get update -qq
apt-get install -y -qq --no-install-recommends libffi-dev unzip >/dev/null
docker-php-ext-install ffi >/dev/null

if ! command -v composer >/dev/null 2>&1; then
    curl -sS https://getcomposer.org/installer \
        | php -- --install-dir=/usr/local/bin --filename=composer
fi

cd "$package"
# A lock file written under another PHP version would pin a PHPUnit that cannot run here; the
# package commits none, so start clean.
rm -rf vendor composer.lock
composer install --no-interaction --no-progress --prefer-dist
FAGUS_FFI_LIB="$library" php -d ffi.enable=true vendor/bin/phpunit
