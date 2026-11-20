<?php

/*
 * The autoloader of the tests: Composer's when the package is installed, a PSR-4 one otherwise,
 * so that the smoke test runs from a checkout without `composer install`.
 */

declare(strict_types=1);

$composer = dirname(__DIR__).'/vendor/autoload.php';
if (is_file($composer)) {
    require $composer;

    return;
}

spl_autoload_register(static function (string $class): void {
    $prefix = 'Fagus\\Ffi\\';
    if (strpos($class, $prefix) !== 0) {
        return;
    }
    $path = dirname(__DIR__).'/src/'.str_replace('\\', '/', substr($class, strlen($prefix))).'.php';
    if (is_file($path)) {
        require $path;
    }
});
