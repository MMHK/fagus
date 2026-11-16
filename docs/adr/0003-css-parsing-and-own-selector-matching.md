# 3. lightningcss for parsing, own selector matching and cascade

- Status: accepted
- Date: 2026-11-02

## Context

lightningcss (MPL-2.0) parses stylesheets into typed values, including modern colors (`oklch()`, `color-mix()`), nesting, `@layer`, `@property`, `@container` and `@page` margin rules. Its selector crate `parcel_selectors` parses `:has()` and the nesting selector `&`, but its matching code treats both as unreachable (`parcel_selectors-0.28.3/matching.rs`, line 816), so calling it with such selectors would panic.

## Decision

- Stylesheets are parsed with lightningcss.
- Selector matching is implemented in `fagus-css` on top of the parsed `parcel_selectors` components: compounds are matched right to left, `&` is resolved against the parent rule, `:has()` is matched by searching the relevant subtree.
- The cascade (origins, layers, importance, specificity, order, inheritance, custom properties) is implemented in `fagus-style`.

## Consequences

Fagus controls matching performance (rule hash buckets, ancestor Bloom filter, sharing) and supports `:has()` and nesting. If lightningcss turns out to lack something important, the fallback is `cssparser` with own property grammars or Stylo; this is re-evaluated after the spike.
