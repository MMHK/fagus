# php/fagus

The PHP code of the Composer package `nforst/fagus` (namespace `Fagus\`): the document builder,
the engine that runs the `fagus` binary as a worker process, the installer and the Composer plugin
that downloads the binary, and `bin/fagus-install`. The package's `composer.json` is at the root
of the repository; the Laravel integration is in [`php/laravel`](../laravel).

- [docs/getting-started.md](../../docs/getting-started.md): installation and the first PDF.
- [docs/php.md](../../docs/php.md): the PHP API.
- [docs/deployment.md](../../docs/deployment.md): installing the binary on servers.

The tests are in `tests/` and run from the repository root with `vendor/bin/phpunit`, against
`target/release/fagus` or the binary in `FAGUS_BINARY`.
