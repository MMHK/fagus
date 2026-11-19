# 5. Public API, semantic versioning and deprecations

- Status: proposed
- Date: 2026-11-18

## Context

Fagus follows Semantic Versioning (`CHANGELOG.md`), but SemVer says nothing until it is clear what the public API is. People build on several parts of Fagus: the PHP package and its Laravel integration, the Rust crate `fagus`, the `fagus` binary with its options and exit codes, the worker protocol that other languages can speak, the CSS extensions and the diagnostics codes. And there is the output: pages laid out as Chrome prints them, which change with almost every release because the engine gets closer to Chrome. Before 1.0 every minor release may break anything, as 0.1.0 says. From 1.0 on, users need to know what a minor release may change and how they hear about what goes away.

What the code does today, at 0.2.0:

- PHP: the public classes are `final`. Internals live in `Fagus\Internal` or are marked `@internal` (`Binary`, `Composer\Plugin`, `Document::request()` and `resolve()`, `Diagnostic::fromArray()`, `Pdf::fromReply()`). Nothing is deprecated yet, and there is no runtime deprecation.
- The Composer package installs only the binary of its own release, checked against the SHA-256 that `tools/pin-binaries.php` writes into `Internal\Release`. A binary from `FAGUS_BINARY` or the `PATH` may be of any release.
- The worker greets with `H` and `{ "protocol": 1, "version": "0.2.0" }`. `Engine` refuses a `protocol` other than `Engine::PROTOCOL` (1) and does not compare `version`. The worker ignores fields of `R` it does not know (serde without `deny_unknown_fields`); `Engine` throws on message types it does not know. 0.2.0 added `version` and `description` to `facturX` and kept protocol 1, so a 0.1.0 binary ignores them without a word.
- Rust: every crate has `publish = false`; `fagus` is used as a Git dependency pinned to a tag ([rust.md](../rust.md)), and only `fagus` is documented. It re-exports `Diagnostic` and `Severity` from `fagus-css`, `Attachment`, `Conformance`, `Date`, `PdfError` and `Relationship` from `fagus-pdf` and `FontError` from `fagus-text`. No type of a third-party crate appears in its API. None of its enums, and none of its structs with public fields (`Output`, `Timings`, `Failure`, `Diagnostic`, `Attachment`, `Date`), is `#[non_exhaustive]`.
- Diagnostics codes are called stable in [php.md](../php.md) and [rust.md](../rust.md) and listed in [troubleshooting.md](../troubleshooting.md#read-diagnostics). They are string literals in the crates, without a central list.
- The minimums are PHP 8.3, Laravel 13 and Rust 1.97 (`rust-version`). CI tests PHP 8.3 and 8.5, and builds with Rust 1.99.1 only.

## Decision

### Versions

- From 1.0.0 on, only a major release (2.0.0) breaks the public API. A minor release (1.1.0) adds to it and may deprecate parts of it. A patch release (1.0.1) fixes bugs.
- The PHP package, the Laravel integration, the binary, the crate and the protocol are released together, with one version number.
- The public API is what the documentation in `docs/` describes. What it does not describe is not covered: `@internal` code, the namespace `Fagus\Internal`, the crates other than `fagus`, the wording of messages and of `--help`. Where code and documentation disagree, that is a bug; it is fixed in either direction, in any release, with an entry in the changelog.
- Security fixes may break whatever relied on the hole, in any release.
- Before 1.0, a minor release (0.3.0) may break any part of the API. Every such change is listed in `CHANGELOG.md` under *Changed* or *Removed*, with what to do instead. Patch releases (0.2.1) do not break anything. Deprecating first is welcome, not required.

### PHP and Laravel

Covered:

- The public classes, methods, constants and properties of `Fagus\` and `Fagus\Laravel\`, except `@internal` ones and `Fagus\Internal`: `Fagus`, `Document`, `Pdf` (with its public constructor), `Engine` (with `Engine::PROTOCOL`), `Diagnostic`, `Installer`, the enums `Severity`, `Conformance`, `InvoiceProfile` and `Relationship` with their cases and values, and the exceptions: their classes, their hierarchy and when each is thrown.
- Parameter names, since the documentation calls methods with named arguments.
- In Laravel: the facade `Fagus`, `PdfFactory`, `PendingPdf`, the container bindings `fagus` and `Fagus\Engine`, the keys of `config/fagus.php` with their meaning and defaults, the publish tag `fagus-config` and the command `php artisan fagus:install`.
- `vendor/bin/fagus-install`, the Composer plugin's installing, and the environment variables `FAGUS_BINARY`, `FAGUS_SYSTEM_FONTS`, `FAGUS_LOCALE`, `FAGUS_DOWNLOAD_URL`, `FAGUS_GITHUB_TOKEN` and `GITHUB_TOKEN`.

A minor release may add classes, methods, optional parameters after the existing ones, configuration keys and environment variables; add cases to enums (a `match` over an enum of Fagus needs a `default` arm); throw a subclass where the parent class was thrown; and change the messages of exceptions. Since the classes are `final`, extending them is not supported, and adding methods to them breaks nobody.

Breaking, so only in a major release: removing or renaming anything covered, changing a signature in a way that breaks callers (types, the order or names of parameters, a new required parameter), changing a default, removing an enum case or changing its value, throwing another exception class than before (other than a subclass).

### Rust

- Covered is the crate `fagus`: its public items, the types it re-exports and its features (`parallel`, on by default). What counts as breaking follows the [SemVer compatibility rules of Cargo](https://doc.rust-lang.org/cargo/reference/semver.html).
- The other crates (`fagus-dom`, `fagus-css`, `fagus-style`, `fagus-text`, `fagus-layout`, `fagus-paint`, `fagus-pdf`, `fagus-cli`, `fagus-testkit`) are not covered, even though a Git dependency can reach them. They change in any release. Their types count only where `fagus` re-exports them.
- No type of a third-party crate becomes part of the API of `fagus`, so that updating krilla, lightningcss or HarfRust never breaks it.
- From 1.0 on, the enums and the structs with public fields that may grow are `#[non_exhaustive]`, so that a minor release can add a variant or a field. Adding to a type that is not `#[non_exhaustive]` is breaking.

### Command line

- Covered: the commands `fagus render` and `fagus worker`, their options with their values, defaults and meaning, the input formats by file extension, the exit codes (0, 1, 2), the line format of diagnostics on standard error (`source:line:column: severity[code]: message`), `Error:` before the reason of a failure, and `fagus --version` printing `fagus` and the version.
- Not covered: the text of `--help` and of error messages, and the output of `--timings` and `--repeat`, which are written for people.
- A minor release may add commands, options and values. Removing or renaming an option, giving it another meaning or default, or changing what an exit code means is breaking.

### Worker protocol

- The protocol has a number, sent as `protocol` in `H`. It is 1. Within one number, changes are compatible in both directions:
  - A caller may send new optional fields. A worker ignores fields it does not know.
  - A worker may add fields to its answers. Callers ignore fields they do not know.
  - The worker sends a new message type only in answer to something the caller asked for (as `?` only comes with `ask: true`), since callers refuse types they do not know.
  - New values (a PDF/A level, a profile) may be added. A worker that does not know one answers with `E`.
- An incompatible change (a field removed or with another meaning, another default, another frame layout) needs a new protocol number and comes only in a major release. `PROTOCOL` in the worker and `Engine::PROTOCOL` change together.
- Every release of a major speaks the same protocol: a client written for 1.0 works with every 1.x binary. The PHP package supports the binary of its own release. A binary of another release with the same protocol works, but an option newer than that binary is ignored; use the binary of the same release, as [deployment.md](../deployment.md) says.

### CSS extensions and diagnostics

- The extensions in [extensions.md](../extensions.md) are API like a method: `-fagus-repeat` with `auto` and `at-break`, `counter(-fagus-next-page)`, `counter(-fagus-previous-page)`, the meaning of `string()` in such rows and what `@supports` answers for them. New extensions come in minor releases, each with a record as [ADR 4](0004-extensions-only-where-css-has-no-way.md) requires. When a CSS standard covers the need of an extension, the extension is deprecated in favour of the standard.
- A diagnostics code keeps its name and meaning within a major, and so do the sources the documentation names (`<style> 1`, `html`, `resources`, `pdf`). Minor releases may add codes.
- Not stable: the messages, the severity of a diagnostic, and which diagnostics a document gets. A property Fagus learns no longer causes `css.unknown-property`; a check Fagus learns adds diagnostics. Strict mode (`strict()`, `--strict`) may therefore fail after a minor release on a document that passed before. It is meant for tests and CI, where that is the point.

### Rendering output

Not breaking, so in any minor release, and in a patch release where it fixes a bug:

- Layout and painting closer to Chrome, and the reference moving to a newer Chrome.
- Bug fixes, also where a document relied on the bug.
- CSS that was ignored taking effect, and following the specifications more closely, also where that makes a declaration invalid (`content: element(a) element(b)` after 0.2.0).
- Other line and page breaks, another page count, other glyph positions and font subsets.
- Other bytes: object order, compression, identifiers, the PDF version within what the chosen standard allows. A fixed creation date gives the same bytes only with the same binary.
- Raising a limit (nesting depth, frames, pages, image size).

Breaking, so only in a major release:

- Removing support for a feature that works as documented, instead of fixing it.
- Changing a documented default: A4 without `@page { size }`, tagged PDF, the built-in Inter, system fonts off, American English for form controls, the Factur-X version `1.0` and its description.
- Dropping a PDF/A level, PDF/UA or an invoice profile. A PDF that a validator accepted and now rejects is a bug, fixed in a patch release.
- Lowering a limit, except as a security fix.

What is safe by default does not change at all: no network access, no files outside the root, no scripts.

### Minimum versions and platforms

- PHP: raising the minimum (8.3) is allowed in a minor release once PHP no longer gives that version security fixes. Composer keeps projects on an older PHP at the last release that supports it.
- Laravel: support for a new major comes in a minor release. A Laravel major is dropped in a minor release once Laravel no longer gives it security fixes.
- Rust: `rust-version` (1.97) may be raised in a minor release, listed under *Changed*, but only to a Rust release that came out at least three months (two releases) before, so that a toolchain a distribution or CI image ships keeps working.
- Platforms: binaries for Linux (x64 and arm64, glibc 2.17 or newer and musl), macOS (x64 and arm64) and Windows x64. A minor release may add platforms. Dropping one, or raising the glibc minimum, comes only in a major release, unless Rust drops the target or its vendor stopped supporting it.
- Every raised minimum is listed in the changelog under *Changed*.

### Deprecation

- Something is deprecated in a minor release, together with its replacement where there is one, and listed under *Deprecated* in `CHANGELOG.md` with that replacement. It keeps working unchanged for the rest of the major.
- It is removed no earlier than the next major release, and only after at least one released minor version carried the deprecation: never deprecated and removed in the same release. The removal is listed under *Removed* in the changelog and in `UPGRADING.md`, which has a section for each major release with every breaking change and what to do instead.
- PHP: `@deprecated since 1.4.0, use … instead` in the docblock, for IDEs and static analysis, and `@trigger_error('…', E_USER_DEPRECATED)` when the deprecated code runs. The `@` keeps it off the screen, but error handlers see it: Laravel can write it to its `deprecations` log channel (`LOG_DEPRECATIONS_CHANNEL`), PHPUnit reports it. A configuration key or environment variable that is deprecated still works; the service provider triggers the deprecation when it is set.
- Rust: `#[deprecated(since = "1.4.0", note = "use … instead")]`.
- Command line: a deprecated option still works. Using it writes `warning: --… is deprecated; use --… instead` to standard error; `--help` marks it. The exit code does not change, also with `--strict`.
- Worker protocol: a deprecated field is still accepted for as long as the protocol number stays; [protocol.md](../protocol.md) marks it.
- CSS extensions: a diagnostic `css.deprecated` (severity `warning`) at the declaration; the document renders as before. Strict mode fails on it, as on any diagnostic.
- Diagnostics codes are not deprecated: one is renamed or removed only in a major release, listed in `UPGRADING.md`.

## Consequences

Updating to a new minor release needs no change to code, configuration or templates. PDFs may still look different, closer to Chrome. Tests that compare PDF bytes or images need new references after an update, or a pinned version.

Breaking changes wait for a major release, so they are collected before they are merged. Every change to a public surface needs an entry in the changelog, and every deprecation one under *Deprecated*. [stability.md](../stability.md) says the same for users, shorter.

Before 1.0, the code needs:

- A way for a caller to tell which fields a worker knows, so that a newer PHP package with an older binary fails instead of ignoring an option: compare `version` from `H` with `Release::VERSION` in `Engine` (at least warn on another major), or let the worker answer unknown fields with `E`, or list capabilities in `H`. [protocol.md](../protocol.md) should name the release that added each field.
- `#[non_exhaustive]` on the public enums and structs of `fagus` that may grow (`Error`, `PdfError`, `FontError`, `Conformance`, `Relationship`, `FacturXProfile`, `Output`, `Timings`, `Failure`, `Diagnostic`), and constructors for `Attachment` and `Date` if they get it too, since the documentation builds them with struct literals.
- A CI job that builds `fagus` with Rust 1.97, so that `rust-version` is checked.
- A test that every diagnostics code the crates emit is listed in [troubleshooting.md](../troubleshooting.md#read-diagnostics), and the other way round.
- The code `css.deprecated` and the warning for deprecated command line options, when the first deprecation needs them.

`UPGRADING.md` collects the changes since 0.1.0 that can need action, in a section for 1.0, so that an update from any 0.x release is covered in one place.
