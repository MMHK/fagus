# Resources

A document refers to other files: images, stylesheets, fonts, SVG sprites, the documents of
frames. Fagus loads them only from places you allow. It never goes to the network, never reads a
file outside the directory you name, and runs no scripts. This page describes where resources
come from, and so the security model.

## Where a resource comes from

For every URL in a document, Fagus tries these sources in this order and takes the first that has
the resource:

1. **A `data:` URL**: the data in the URL itself, base64 or percent-encoded.
2. **The resources handed over** with `resource($url, $data)`.
3. **The resolver** set with `resolver()`, a PHP callback.
4. **A file inside the root directory**, if a base directory is set.

If none has it, the resource is missing: the document renders without it and the PDF's
diagnostics say so (`resource.not-loaded`).

```php
$pdf = Fagus::html($html)
    ->baseDirectory(__DIR__.'/resources/pdf')
    ->resource('logo.png', $tenant->logo)
    ->resolver(fn (string $url): ?string => $media->find($url))
    ->render();
```

## Read files from a base directory

Without a base directory, Fagus reads no files at all. With one, relative URLs resolve against
it, and only files inside it are read:

```php
Fagus::html('<link rel="stylesheet" href="css/invoice.css"><img src="images/logo.png" alt="ACME">')
    ->baseDirectory(__DIR__.'/resources/pdf');
// reads resources/pdf/css/invoice.css and resources/pdf/images/logo.png
```

A query or fragment (`logo.png?v=2`, `icons.svg#check`) is not part of the file name. Percent
escapes are decoded (`my%20logo.png` is `my logo.png`).

In Laravel the base directory is `public/` unless `base_directory` in `config/fagus.php` says
otherwise.

## Allow a larger directory with `root()`

The root is the directory that files may come from. It is the base directory unless you set it
with `root()`, and it must contain the base directory. Use it when the document's URLs lead out of
its own directory:

```php
Fagus::html($html)
    ->baseDirectory(__DIR__.'/resources/pdf/invoices')
    ->root(__DIR__.'/resources/pdf');
```

```html
<!-- resources/pdf/invoices/invoice.html -->
<link rel="stylesheet" href="../shared/base.css">  <!-- resources/pdf/shared/base.css -->
```

Anything that leads outside the root is refused: `..` past the root, absolute file paths
(`/etc/passwd` is looked up as `<root>/etc/passwd`), and symbolic links that point outside it.

## URLs that start with `/`

A URL that starts with `/` resolves against the root, as on a web server:

```php
Fagus::html('<img src="/images/logo.png" alt="ACME">')
    ->baseDirectory(__DIR__.'/public/invoices')
    ->root(__DIR__.'/public');   // reads public/images/logo.png
```

Without `root()`, it resolves against the base directory. In Laravel, with the base directory at
`public/`, the same markup works in a page of the site and in the PDF.

## Hand over data with `resource()`

`resource($url, $data)` gives Fagus the data for a URL, so it does not have to be a file: a logo
from the database, a chart drawn in PHP, a stylesheet built at runtime.

```php
Fagus::html('<img src="logo.png" alt="ACME"><img src="chart.svg" alt="Revenue by month">')
    ->resource('logo.png', $company->logo_png)
    ->resource('chart.svg', $chart->toSvg());
```

The URL must match the URL that Fagus asks for:

- In the document, that is the URL as written in the attribute: `<img src="logo.png">` asks for
  `logo.png`, `<img src="./logo.png">` for `./logo.png`.
- In a stylesheet that was loaded from a URL (`<link href="css/main.css">`, `@import`), relative
  URLs are resolved against the stylesheet first, as in browsers: `url(img/bg.png)` in
  `css/main.css` asks for `css/img/bg.png`, and `@import "more.css"` there for `css/more.css`.
- In a frame's document (`<iframe src="frames/page.html">`), relative URLs are resolved against
  the frame's URL: `style.css` in `frames/page.html` asks for `frames/style.css`.

Resources handed over work without a base directory. They are also the only way, besides
`data:` URLs and the resolver, to provide resources for a document that has no files.

## Answer requests with `resolver()`

A resolver is a callback that gets each URL that was not handed over, as described above, and
returns its data or `null`:

```php
Fagus::html($html)
    ->baseDirectory(resource_path('pdf'))
    ->resolver(function (string $url): ?string {
        if (! str_starts_with($url, 'media://')) {
            return null;   // let Fagus look for a file
        }

        return Storage::disk('media')->get(substr($url, strlen('media://')));
    });
```

Fagus asks the resolver before it looks for a file; after `null`, it tries the file in the root.
The resolver sees every URL the document refers to that is not a `data:` URL or handed over,
including `https://…` URLs. It decides what to hand out. Fagus does not go to the network, but
your resolver may, under your own rules: check the host against a list, set a timeout, and keep
in mind that a document from user input can name any URL.

The engine waits while the callback runs; the [timeout](deployment.md#timeouts) does not count
that time, as it limits the wait for the worker's answers.

## `data:` URLs

`data:` URLs work everywhere a URL does, base64 or percent-encoded:

```html
<img src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAA…" alt="">
<link rel="stylesheet" href="data:text/css,p%20%7B%20color%3A%20navy%20%7D">
```

In PHP:

```php
$src = 'data:image/png;base64,'.base64_encode($png);
```

A document of a frame that comes from a `data:` URL loads nothing else.

## Images

`<img>`, `<picture>` and `srcset`, CSS backgrounds, `mask`, `list-style-image`, `border-image`,
`content: url()`, and SVG clip paths, masks, filters and `<use href="sprite.svg#id">` in other
files load as resources.

| Format | Notes |
|---|---|
| PNG | Also with `gAMA`, converted to sRGB as Chrome converts them. |
| JPEG | CMYK JPEGs without a profile are converted to sRGB as in Chrome. |
| GIF | |
| WebP | |
| SVG | Drawn as vector graphics. An SVG image does not see the document's `@font-face` fonts; its own `@font-face` rules load fonts only from `data:` URLs, as in Chrome. |

JPEG, PNG and WebP photos stand as their EXIF orientation says. `<picture>` sources and `srcset`
are chosen as Chrome chooses them; AVIF and JPEG XL sources are skipped, as Fagus cannot decode
them.

An image that cannot be used is left out, and a diagnostic says why:

| Code | Message |
|---|---|
| `resource.not-loaded` | image `…` could not be loaded |
| `image.unsupported` | image `…` is not a PNG, JPEG, GIF, WebP or SVG image |
| `image.unsupported` | image `…` is not a valid SVG image |
| `image.too-large` | image `…` has more than 200 megapixels |
| `image.broken` | image `…` could not be decoded and is left out |

## Stylesheets

Stylesheets come from `<style>` elements, `<link rel="stylesheet" href="…">`, `@import` and
`stylesheet()`:

```html
<link rel="stylesheet" href="css/invoice.css">
<style>
  @import url("css/print.css") print;
  @import "css/theme.css" layer(theme);
</style>
```

- A linked stylesheet loads as a resource. One that cannot be loaded is reported:
  `html:4: warning[resource.not-loaded]: stylesheet `css/missing.css` could not be loaded`.
- `@import` loads its stylesheet as a resource, with the media queries, `supports()` condition
  and `layer()` of its rule, up to eight levels deep. One that cannot be loaded is reported as
  "stylesheet `…` could not be imported".
- Relative URLs in a linked or imported stylesheet resolve against the stylesheet's URL.
- `stylesheet($css)` adds CSS from PHP after the document's stylesheets. Its relative URLs
  resolve against the base directory.

## Fonts

`@font-face` rules load their fonts as resources: TTF, OTF, WOFF and WOFF2, with variable fonts
and `unicode-range`.

```css
@font-face {
  font-family: "Corporate Sans";
  src: url("fonts/CorporateSans.woff2") format("woff2");
  font-weight: 100 900;
}
```

Fagus loads the first source that Chrome would load and passes over formats and technologies
Chrome does not load. A font that cannot be loaded is reported ("font `…` could not be loaded"),
one that is not a font Fagus can read too ("`…` is not a font Fagus can read",
`font.unsupported`). Fonts from directories, generic families and system fonts are described in
[fonts.md](fonts.md).

## Frames: `<iframe>`, `<object>` and `<embed>`

An `<iframe>` (its `srcdoc`, else its `src`), and an `<object>` or `<embed>` whose resource is an
HTML document, shows that document as Chrome prints it: laid out at the frame's size, not
paginated, cut off at the frame's edges.

```html
<iframe src="frames/terms.html" style="width: 100%; height: 120mm; border: 0"></iframe>
<iframe srcdoc="<p>Inline document</p>"></iframe>
```

A frame's document loads like any other resource: a `data:` URL, the resources handed over, the
resolver, or a file inside the root. Its own relative URLs resolve against its URL. Scripts in it
do not run. Frames nest at most 8 deep, and one render lays out at most 256 documents in frames
with 8 MiB of markup together; past that limit, frames show nothing and a `frame.limit` warning
says so. The diagnostics of documents in frames are not reported.

Text files and images in frames show as Chrome shows them.

## What is never loaded

- **The network.** URLs with `http:`, `https:`, `ftp:` or a host (`//cdn.example.com/…`) are
  never fetched by Fagus. Only a resolver of yours can answer them.
- **Files outside the root**, through `..`, absolute paths or symbolic links. A URL with a scheme
  (`file:///etc/passwd`) is never read as a file.
- **Files at all**, when no base directory is set.
- **Scripts.** `<script>` does not run, and neither do event handlers or `javascript:` URLs.
  Content that a script would create in a browser, such as a chart drawn by JavaScript, is
  missing. Render it on the server as SVG or an image and hand it over.

Links (`<a href="https://…">`) are different: they become links in the PDF and load nothing.

## Diagnostics for missing resources

Every resource that is missing shows in `$pdf->diagnostics()`:

```php
$pdf = Fagus::html($html)->baseDirectory(__DIR__.'/assets')->render();

foreach ($pdf->diagnostics() as $diagnostic) {
    echo $diagnostic, "\n";
}
```

```text
html:4: warning[resource.not-loaded]: stylesheet `css/missing.css` could not be loaded
resources: warning[resource.not-loaded]: image `https://example.com/a.png` could not be loaded
resources: warning[resource.not-loaded]: image `../outside/x.png` could not be loaded
```

With `strict()`, any diagnostic makes `render()` throw a `RenderException` instead, which helps
to catch missing files in tests. Long `data:` URLs are cut short in the messages.

## Laravel: assets and Vite

`asset()`, `url()`, `Vite::asset()` and `@vite` write absolute URLs with the host of the
application (`https://example.com/build/assets/app-4ed993c7.css`), which Fagus does not fetch.
Use one of these instead:

- Root-relative URLs in the view, which resolve against `public/`:

  ```blade
  <img src="/images/logo.png" alt="ACME">
  ```

- The built CSS, handed over as a stylesheet:

  ```php
  use Illuminate\Support\Facades\Vite;

  Fagus::view('invoices.show', $data)->stylesheet(Vite::content('resources/css/pdf.css'));
  ```

- A resolver that maps the application's URLs to files in `public/`, and only to those:

  ```php
  Fagus::view('invoices.show', $data)->resolver(function (string $url): ?string {
      $prefix = rtrim(config('app.url'), '/').'/';
      if (! str_starts_with($url, $prefix)) {
          return null;
      }
      $public = realpath(public_path());
      $file = realpath(public_path(parse_url($url, PHP_URL_PATH) ?? ''));

      return $file !== false && str_starts_with($file, $public.DIRECTORY_SEPARATOR) && is_file($file)
          ? file_get_contents($file)
          : null;
  });
  ```
