# Contributing

Fagus is in early development. Design decisions are recorded in [docs/adr](docs/adr). The Composer package `nforst/fagus` is this repository: its `composer.json` is at the root and its PHP code in `php/`.

- Run `cargo fmt`, `cargo clippy --workspace --all-targets` and `cargo test --workspace` before sending changes.
- For changes to the PHP code, run `composer install` and `vendor/bin/phpunit` in the repository root (after `cargo build --release`, whose binary the tests use).
- No `unsafe` code. Every crate forbids it.
- Behaviour follows the CSS specifications; appearance is compared with Chrome. Print features that Chrome lacks follow the specifications and WeasyPrint.
- Every bug fix comes with a test.
- The documentation is in `docs/` and is published on [fagus.niklasforst.de/docs](https://fagus.niklasforst.de/docs) (`main` under `/docs/next`). Every page needs an entry in `docs/nav.yml`; run `php tools/check-docs.php` after changing the pages, which also checks the links between them.
