# Contributing

Fagus is in early development. Design decisions are recorded in [docs/adr](docs/adr). The Composer package `nforst/fagus` is this repository: its `composer.json` is at the root and its PHP code in `php/`.

- Run `cargo fmt`, `cargo clippy --workspace --all-targets` and `cargo test --workspace` before sending changes.
- For changes to the PHP code, run `composer install` and `vendor/bin/phpunit` in the repository root (after `cargo build --release`, whose binary the tests use).
- No `unsafe` code. Every crate forbids it.
- The workspace builds with the Rust of `rust-toolchain.toml`, and must also build and pass its tests with the oldest one `rust-version` in `Cargo.toml` names (1.97.0), which CI checks: `rustup toolchain install 1.97.0` and `cargo +1.97.0 test --workspace`. Use no standard library function newer than that.
- Trigonometry, powers, logarithms and the like come from the `libm` crate (`libm::sinf`, `libm::powf` …), not from the methods of `f32` and `f64`: those call the system's math library, whose results differ between platforms in the last bit, and the PDF must have the same bytes everywhere. `clippy.toml` refuses the methods.
- Behaviour follows the CSS specifications; appearance is compared with Chrome. Print features that Chrome lacks follow the specifications and WeasyPrint.
- Every bug fix comes with a test.
- Changes that users notice get an entry under `[Unreleased]` in `CHANGELOG.md`. What counts as public API, and what a minor release may change, is in [docs/stability.md](docs/stability.md) ([ADR 5](docs/adr/0005-public-api-and-deprecations.md)). From 1.0 on, breaking changes wait for a major release: deprecate first in a minor release (`@deprecated` and `E_USER_DEPRECATED` in PHP, `#[deprecated]` in Rust, a warning on the command line), with the replacement and an entry under *Deprecated*.
- An incompatible change to the worker protocol raises `PROTOCOL` in `crates/fagus-cli/src/worker.rs` and `Engine::PROTOCOL` together, and only in a major release; compatible changes (new optional fields) keep the number.
- Pull requests also run the [release gates](docs/releasing.md#release-gates): the corpus validated with veraPDF, qpdf and Mustang, rendered to the same bytes on every platform, and timed. After changes to the PDF output, run `tools/validate.sh --subset /tmp/fagus-validate` (it needs Java) and `tools/determinism.sh /tmp/fagus-determinism` locally.
- The documentation is in `docs/` and is published on [fagus.niklasforst.de/docs](https://fagus.niklasforst.de/docs) (`main` under `/docs/next`). Every page needs an entry in `docs/nav.yml`; run `php tools/check-docs.php` after changing the pages, which also checks the links between them.

## Fuzzing

`fuzz/` has [cargo-fuzz](https://rust-fuzz.github.io/book/cargo-fuzz.html) targets for the parts that read untrusted input. It is a workspace of its own and needs a nightly toolchain, which the engine's build and tests do not:

```sh
rustup toolchain install nightly
cargo install cargo-fuzz
fuzz/run.sh render 600    # a target, and for how many seconds
```

| Target | Input |
|---|---|
| `render` | HTML, or XHTML when it starts with `<?xml`, rendered to PDF |
| `css` | a style sheet, parsed and cascaded over a fixed document |
| `font` | a font file (TrueType, OpenType, WOFF, WOFF2), shaped and used through `@font-face` |
| `image` | an image (PNG, JPEG, GIF, WebP, BMP, TIFF) used by a fixed document in several ways |
| `svg` | an SVG image, used as a file and inline in HTML |
| `worker` | the frames a caller sends `fagus worker` ([protocol](docs/protocol.md)) |

`fuzz/run.sh` seeds `fuzz/corpus/<target>` from `fuzz/seeds` and the documents, style sheets, fonts and images of the tests (`fuzz/seed.sh`), then runs the target with its input size, dictionary, timeout and memory limits; options after the seconds go to libFuzzer. The targets are built with overflow checks but without AddressSanitizer, which makes rendering several times slower; `FUZZ_SANITIZER=address fuzz/run.sh …` turns it on for the unsafe code in dependencies. A crash, panic, timeout or running out of memory is saved in `fuzz/artifacts/<target>`; replay it with `cargo +nightly fuzz run <target> <file>` and shrink it with `cargo +nightly fuzz tmin <target> <file>`. A panic counts even where the engine would catch it and answer with an internal error. Fixes come with the shrunk input as a test, usually in `crates/fagus/tests/robustness.rs`. Corpora and artifacts stay out of the repository; only the small seeds in `fuzz/seeds` are committed.

The workflow `fuzz.yml` runs every target for 10 minutes each night and on demand (with a choice of sanitizer), keeps each corpus in the cache between runs and uploads crashes as artifacts.
