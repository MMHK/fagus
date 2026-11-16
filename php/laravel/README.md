# php/laravel

The Laravel integration of the Composer package `nforst/fagus` (namespace `Fagus\Laravel\`): the
service provider, the `Fagus` facade, `PendingPdf` with its responses and disks, the configuration
file `config/fagus.php` and the command `fagus:install`. Laravel 13 discovers it on its own.

- [docs/laravel.md](../../docs/laravel.md): views, responses, disks, queues, Octane and the
  configuration.
- [docs/php.md](../../docs/php.md): the document methods it shares with the PHP API.

The tests are in `tests/` and run from the repository root with `vendor/bin/phpunit`.
