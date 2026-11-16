FROM php:8.4-cli
RUN apt-get update && apt-get install -y --no-install-recommends gcc libc6-dev libffi-dev && docker-php-ext-install ffi && rm -rf /var/lib/apt/lists/*
RUN printf 'ffi.enable=1\n' > /usr/local/etc/php/conf.d/ffi.ini
WORKDIR /bench
