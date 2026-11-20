# AGENTS.md

Fagus renders HTML/CSS to PDF like Chrome prints, without a browser. A Rust workspace holds the
engine and CLI; the repository root is also the Composer package `nforst/fagus`, with the PHP code
in `php/fagus` (`Fagus\`) and `php/laravel` (`Fagus\Laravel\`).

## Commands

Run from the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo run -p fagus-cli -- render tests/corpus/invoice/invoice-3.html -o invoice.pdf
```

CI sets `RUSTFLAGS=-D warnings`, so clippy warnings fail. For PHP changes:

```sh
cargo build --release -p fagus-cli   # PHP tests use target/release/fagus (or FAGUS_BINARY)
composer install
vendor/bin/phpunit
vendor/bin/pint --test               # style in pint.json; bench/ is excluded
```

Run one Rust test with `cargo test -p fagus-layout <test_name>` (or `--test <file>` for files in
`crates/*/tests`). Most engine tests are inline `#[cfg(test)]` modules; layout tests are in
`crates/fagus-layout/src/tests/`. On Windows prefer `E:\temp\...` over the `/tmp` paths in
`CONTRIBUTING.md` and docs.

## Local tooling

- **Do not use PowerShell for text or character handling.** `pwsh` mangles UTF-8, quoting and
  escaping when it rewrites files or passes non-ASCII output between commands. Prefer a short
  Node.js script (`node -e`, `fs`, `child_process`) for that work, and keep shell calls to plain
  process invocation.
- **Use Yarn 4, never npm.** JavaScript dependencies (for example `bench/compare`) are installed
  with `corepack enable && yarn install`. Do not run `npm install`, and do not commit
  `package-lock.json`; Yarn writes `yarn.lock`.
- **Record plans before non-trivial work.** Write or amend a plan under `docs/plans/` (see
  [Documentation and changelog](#documentation-and-changelog)) rather than keeping the plan only in
  the conversation.

## Constraints that are easy to get wrong

- **No `unsafe`.** The workspace denies it; the only exception is the glibc `malloc_trim` call in
  `crates/fagus-cli/src/memory.rs`.
- **Use `libm` for transcendental math** (`libm::sinf`, `libm::powf`, …), never `f32`/`f64`
  methods such as `.sin()`, `.ln()`, `.powf()`: they call the platform libm. `clippy.toml` rejects
  them. Output must be byte-identical on Linux, macOS and Windows, so do not introduce platform
  math, locale, time or random state into rendering (`--created` exists to pin dates).
- The pinned toolchain is `1.99.1` (`rust-toolchain.toml`), but CI also builds and tests with the
  MSRV `1.97` from `Cargo.toml`. Do not use newer standard-library APIs.
- Rust 2024 edition; `rustfmt` uses `max_width = 110`.
- In `crates/fagus`, growable public enums/structs with public fields are `#[non_exhaustive]`;
  user-constructed structs get a constructor. Deprecate before removing — see
  `docs/stability.md` and ADR 5.
- An incompatible worker-protocol change bumps **both** `PROTOCOL` in
  `crates/fagus-cli/src/worker.rs` and `Engine::PROTOCOL` in `php/fagus/src/Engine.php`, only in a
  major release. Compatible additions keep the number.
- Prose and comments use British English (`behaviour`, `colour`, `licence`).
- The root `composer.json` is the published package; `.gitattributes` excludes `crates/`, `tests/`,
  `tools/` and the Rust manifests from Composer archives. Keep PHP code under `php/` and Rust
  code under `crates/`.

## Architecture

Rendering pipeline: `fagus-dom` (HTML arena DOM) → `fagus-css` (parse/match) → `fagus-style`
(cascade/computed values) → `fagus-text` (fonts, shaping, line breaking) → `fagus-layout` (boxes,
fragmentation, paged media) → `fagus-paint` (display lists) → `fagus-pdf` (PDF output). The public
Rust API is `crates/fagus`; the `fagus` binary is `crates/fagus-cli`; conformance tooling is
`crates/fagus-testkit`.

PHP never renders in-process: it starts the binary and speaks the worker protocol
(`docs/protocol.md`, implemented in `crates/fagus-cli/src/worker.rs` and
`php/fagus/src/Engine.php`). `php/fagus/src/Fagus.php` is the fluent entrypoint; the Laravel
service provider is in `php/laravel/src/`.

Behaviour follows the CSS specifications and is compared visually with Chrome. Print features
Chrome lacks follow the specs and WeasyPrint. Design decisions are recorded in `docs/adr/` — read
the relevant ADR before changing layout or extension behaviour.

## Testing and conformance

- Every bug fix needs a test. Put robustness regressions in `crates/fagus/tests/robustness.rs`;
  layout behaviour generally goes beside the existing tests in `crates/fagus-layout/src/tests/`.
- `tests/corpus` is shared test material (documents, fonts, images, invoices). Normal
  `cargo test` does not need network or extra services.
- WPT reftests: `tools/wpt/run.sh [--filter <path>]`; failures must be listed in
  `tools/wpt/expectations.txt`. If a fix makes a listed test pass, run
  `tools/wpt/run.sh --update --docs docs/compatibility.md` and commit both changes.
- Chrome comparison: `tools/chrome-corpus.sh`; expected differences live in
  `tools/chrome-expectations.txt`.
- Release gates, needed when changing PDF output: `tools/validate.sh` (veraPDF/Mustang/qpdf, needs
  Java), `tools/determinism.sh`, `python3 tools/bench-smoke.py`. These are slower and
  platform-dependent; do not run them for unrelated changes.
- Fuzzing is a separate nightly workspace in `fuzz/` (`fuzz/run.sh <target> <seconds>`). CI runs
  `cargo +nightly check --bins` inside `fuzz/`; when `worker.rs` starts using a new module, add it
  to the `worker` fuzz target too.

## Documentation and changelog

- Every page under `docs/` needs an entry in `docs/nav.yml`. After editing docs run
  `php tools/check-docs.php`; it also checks internal links.
- Plans and design notes live in `docs/plans/`, named `YYYY-MM-DD-<slug>.md` after the date they
  are written, for example `docs/plans/2026-11-20-php-ffi-cdylib.md`; the slug is short
  kebab-case. Amend the existing plan instead of adding a dated copy per revision. Plans are
  pages under `docs/`, so they need a `docs/nav.yml` entry too (`group: Internals`,
  `optional: true`).
- User-visible changes get an entry under `[Unreleased]` in `CHANGELOG.md`. API changes follow
  `docs/stability.md`; planned removals go under *Next major* in `UPGRADING.md`.
- `CONTRIBUTING.md` is the source for contribution policy; `docs/releasing.md` explains release
  gates and the binary-pinning flow.
