# 6. Pages that grow with their content: `-fagus-page-height`

- Status: proposed
- Date: 2026-11-19

## Context

Receipt printers print on rolls, 58mm or 80mm wide, and cut the paper where the document ends. A receipt is one page as long as its content: a page of a fixed height cuts it into pieces or leaves metres of blank paper. CSS has no way to say this. `size` takes a width and a height; `size: 80mm auto` is invalid in CSS Paged Media 3 and dropped by Chrome, and Fagus drops it as well, with a `css.invalid-value` warning. A page box's `height: auto` in `@page` means the height that `size` leaves, not the content's.

What people do today is render the document again and again with other heights in `size` until `pages()` is 1: a binary search that costs a dozen renders and still leaves the page a few millimetres too tall or too short.

## Decision

- A descriptor of `@page`, `-fagus-page-height: auto | fit-content`, as [ADR 4](0004-extensions-only-where-css-has-no-way.md) requires. `auto`, the initial value, is the height `size` gives. With `fit-content`, the page is as tall as its content and its margins (and border and padding).
- Not `size: 80mm auto`: that value is invalid CSS, so a template written with it prints on A4 in Chrome and makes every other tool ignore the whole size. With the descriptor, `size` stays valid: browsers and other tools print the declared size, which is a usable preview, and Fagus grows the page from there. `@supports (-fagus-page-height: fit-content)` tells the two apart.
- The height of `size` is the least the page is tall. Viewport units, percentages of the page area and the containing block of fixed boxes need a definite page height before the content is laid out; the declared one is that height, so `100vh` and `min-height: 100vh` mean the same in Fagus as in the browser preview. A printer that needs a minimum length gets it from `size`; a document without one declares a small height, such as `size: 80mm 40mm`.
- The page is at most 14,400pt (200 inches, about 5 metres) tall, the largest page a PDF holds (ISO 32000-1, Annex C.2), less its bleed and the room for printer's marks. Content that does not fit goes on to a next page that grows in turn. Forced breaks still break: each page grows with the content it holds.
- The content's height is that of the root element's margin box, or of the monolithic content (lines, images) that reaches further. Absolutely positioned boxes do not make the page taller; fixed boxes are placed in the grown page area, so `position: fixed; bottom: 0` lands at the end of the page.
- The page is rounded up to whole points, as the PDF's page size is, so that rounding never cuts off content. The margin boxes, the page's background and border and the page counters follow the grown page as they follow any page.
- In vertical writing modes, where the content runs across the page and its height is the length of the lines, the descriptor is ignored.

## Consequences

A receipt is one render: `@page { size: 80mm 40mm; margin: 4mm; -fagus-page-height: fit-content }`. Applications that searched for the height with `pages()` drop that loop.

The page is laid out with room down to the tallest page, then cut to its content. Margin boxes are laid out after pagination already, so they need no second layout. Each further extension of `@page` follows the same pattern: a `-fagus-` descriptor next to the standard ones, a `css.invalid-value` diagnostic for values it does not take, and an entry in `@supports`.
