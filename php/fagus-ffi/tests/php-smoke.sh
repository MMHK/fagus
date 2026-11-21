#!/bin/sh

# Runs inside an official php:*-cli container: installs the FFI extension (the images do not ship
# it), lints the package and renders through it. The shared library is the second argument, the
# path of the package the first.
#
#     docker run --rm -v "$PWD:/w" -w /w php:7.4-cli \
#         sh php/fagus-ffi/tests/php-smoke.sh php/fagus-ffi /w/target/x86_64-unknown-linux-gnu/release/libfagus_ffi.so

set -eu

package=$1
library=$2

apt-get update -qq
apt-get install -y -qq libffi-dev >/dev/null
docker-php-ext-install ffi >/dev/null

find "$package/src" "$package/tests" -name '*.php' -exec php -l {} \; >/dev/null

php -d ffi.enable=true "$package/tests/smoke.php" "$library"
