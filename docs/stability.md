# Stability and upgrades

What you can rely on when you update Fagus, what a release may change and how you hear about
what goes away. The reasons are in [ADR 5](adr/0005-public-api-and-deprecations.md).

## Versions

Fagus follows [Semantic Versioning](https://semver.org). The PHP package, the binary, the Rust
crate and the worker protocol are released together, with one version number.

- **Before 1.0** (now: 0.2.0), a minor release such as 0.3.0 may change the API. The
  [changelog](../CHANGELOG.md) lists every such change under
  *Changed* or *Removed*, with what to do instead. Patch releases such as 0.2.1 only fix bugs.
  Require `^0.2` in Composer to get the patch releases and not the next minor one.
- **From 1.0 on**, only a major release (2.0) breaks the API. Minor releases (1.1) add to it and
  may deprecate parts of it; patch releases (1.0.1) fix bugs. `^1.0` is safe.

Security fixes may break what relied on the hole, in any release.

## What is covered

What these pages describe is the public API:

- **PHP and Laravel:** the classes, methods, enums and exceptions of [PHP API](php.md), with the
  names of their parameters; the facade, `PendingPdf`, the configuration keys and the commands of
  [Laravel](laravel.md); the environment variables (`FAGUS_BINARY`, `FAGUS_SYSTEM_FONTS`,
  `FAGUS_LOCALE`, `FAGUS_SANDBOX`, `FAGUS_DOWNLOAD_URL`, `FAGUS_GITHUB_TOKEN`). Not covered: anything marked
  `@internal` and the namespace `Fagus\Internal`.
- **Rust:** the crate `fagus` with the types it exports and its features ([Rust library](rust.md)).
  The other crates of the repository are internal and change in any release.
- **Command line:** the commands, options, defaults and exit codes of [Command line](cli.md), and
  the format of diagnostics lines. Not the text of `--help`, of error messages or of `--timings`.
- **Worker protocol:** protocol 1 of [Worker protocol](protocol.md). Within a protocol number,
  new fields and values may come; a client ignores fields it does not know. A new number comes
  only with a major release.
- **CSS:** the [extensions](extensions.md) with the `-fagus-` prefix.
- **Diagnostics:** the codes, such as `css.invalid-value`, and what they mean
  ([Troubleshooting](troubleshooting.md#read-diagnostics)). Not their messages or severity.

A minor release may add methods, optional parameters, enum cases, options, configuration keys,
extensions and diagnostics codes. If you `match` over an enum of Fagus, give it a `default` arm.

## What may change in any release

The output. Fagus prints as Chrome does, and every release gets closer:

- Layout, line and page breaks, and the page count may change, because Fagus follows Chrome or the
  specifications more closely or fixes a bug, also where a document relied on the bug.
- CSS that was ignored may take effect.
- The bytes of the PDF change between releases, even with a fixed `created()` date. Within one
  release, the same document, fonts and date give the same bytes ([The PDF](pdf.md)).
- A document may get other diagnostics: fewer as Fagus learns CSS, more as it learns to check
  things. Strict mode (`strict()`, `--strict`) can fail after an update on a document that passed
  before.

If your tests compare PDFs byte by byte or as images, update the references with each release.

What does not change in a minor release: the documented defaults (A4, tagged PDF, the built-in
font, no system fonts, American English for form controls), the PDF/A levels, PDF/UA and the
invoice profiles. What is safe never changes: no network access, no files outside the root, no
scripts.

## Deprecations

Before something is removed in a major release, a minor release deprecates it and names what to
use instead. It keeps working until the next major release. You notice it:

- in PHP, as `@deprecated` in your IDE and static analysis, and at runtime as an
  `E_USER_DEPRECATED` notice that error handlers see (Laravel's `deprecations` log channel,
  PHPUnit), not on the screen;
- in Rust, as a compiler warning from `#[deprecated]`;
- on the command line, as a warning on standard error; the exit code stays the same;
- in CSS, as a `css.deprecated` diagnostic.

The changelog lists deprecations under *Deprecated*. A major release comes with
[UPGRADING.md](https://github.com/nforst/fagus/blob/main/UPGRADING.md), which lists every
breaking change and what to do; its section for 1.0 already collects what changed since 0.1.0.

## Supported versions

| | Now | Raised |
|---|---|---|
| PHP | 8.3 or newer | In a minor release, once PHP stops security fixes for the version |
| Laravel | 13 | New majors in minor releases; an old one dropped once Laravel stops its security fixes |
| Rust | 1.97 or newer | In a minor release, to a Rust release at least three months old, listed in the changelog |
| Platforms | Linux (x64, arm64, glibc 2.17 or newer, musl), macOS (x64, arm64), Windows x64 | Dropped only in a major release |

Composer keeps a project on an older PHP or Laravel at the last release that supports it.
Security fixes go into the newest release ([security policy](../SECURITY.md)).

## Updating

- Read the changelog of every version between yours and the new one.
- Let Composer install the binary: the package works with the binary of its own release. A binary
  of another release through `FAGUS_BINARY` may ignore newer options ([Deployment](deployment.md)).
- Run your templates with `strict()` or `fagus render --strict` in CI to see new diagnostics.
