#!/bin/sh

# Runs inside an official php:*-cli container: installs the FFI extension (the images do not ship
# it) and runs the smoke test against the shared library given as the first argument.

set -eu

apt-get update -qq
apt-get install -y -qq libffi-dev >/dev/null
docker-php-ext-install ffi >/dev/null

php -d ffi.enable=true /w/tools/spike/ffi-smoke.php "$1"
