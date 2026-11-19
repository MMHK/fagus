# Upgrading

What to change in code, configuration and templates when updating to a new major release. Minor
releases need no changes (see [docs/stability.md](docs/stability.md)); PDFs may still look
different after one, closer to Chrome.

## From 0.x to 1.0

1.0 is not released yet. This section collects the changes since 0.1.0 that can need action, so
that an update from any 0.x release is covered in one place. The [changelog](CHANGELOG.md) has
every change.

- **`element()` with other values in `content`.** A margin box with `content: element(a) element(b)`
  or `content: element(title) " – " counter(page)` is invalid, as CSS Generated Content for Paged
  Media 3 specifies; the declaration is ignored with a `css.invalid-value` diagnostic. Before, the
  box showed the first running element only. Put the pieces into one running element (the page
  number with `counter(page)` in `::after`), or use the neighbouring margin boxes. *(Unreleased)*
- **Strict mode and new diagnostics.** A `size` in `@page` that Chrome does not take (`80mm auto`,
  `A6`, three lengths) now gets a `css.invalid-value` warning. The page keeps its size as before,
  but `strict()` and `--strict` fail on such a document. Write both lengths (`size: 80mm 200mm`,
  `size: 105mm 148mm` for A6). *(Unreleased)*
