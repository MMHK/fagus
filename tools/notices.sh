#!/usr/bin/env bash
# Writes THIRD-PARTY-NOTICES: the code and data of others that the fagus binary contains, with
# their licenses. Run it before a release (docs/releasing.md) and commit the result; the release
# attaches the file, and the Composer package ships it. Needs cargo-about
# (cargo install --locked cargo-about --features cli).
set -euo pipefail
cd "$(dirname "$0")/.."

section() { # title, file
  printf '\n################################################################################\n%s\n################################################################################\n\n' "$1"
  cat "$2"
}

{
  cat <<'TEXT'
THIRD-PARTY NOTICES

Fagus is licensed under the MIT license or the Apache License 2.0 (LICENSE-MIT, LICENSE-APACHE).
The fagus binary also contains the code and data of others listed here, each under its own
license: parts ported from Chromium and Blink, data of Unicode, a part ported from HarfBuzz, the
user agent style sheet of the HTML Standard, the font Inter, the hyphenation dictionaries of Chromium, and the Rust crates it is built from.
TEXT
  section "Chromium (Blink): the predefined counter styles and the algorithms of the complex ones, the marks of form controls, the distribution of the height of cells spanning table rows, the spacing of East Asian punctuation and text (fagus-layout, fagus-paint, fagus-style, fagus-text)" crates/fagus-layout/CHROMIUM-LICENSE
  section "Blink's shapes (Adobe): the geometry of shape-outside (fagus-layout)" crates/fagus-layout/ADOBE-SHAPES-LICENSE
  section "Unicode Character Database: the vertical orientation of characters and their classes for the spacing of East Asian text (fagus-layout)" crates/fagus-layout/UNICODE-LICENSE
  section "HarfBuzz: reading Apple's hvgl table (fagus-text)" crates/fagus-text/HARFBUZZ-LICENSE
  printf '\n################################################################################\n%s\n################################################################################\n\n' "HTML Standard: the user agent style sheet follows its rendering section (fagus-style)"
  cat <<'TEXT'
Copyright © WHATWG (Apple, Google, Mozilla, Microsoft). The HTML Standard is licensed under a
Creative Commons Attribution 4.0 International License; to the extent portions of it are
incorporated into source code, such portions in the source code are licensed under the BSD
3-Clause License:

Redistribution and use in source and binary forms, with or without modification, are permitted
provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this list of conditions
   and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice, this list of
   conditions and the following disclaimer in the documentation and/or other materials provided
   with the distribution.
3. Neither the name of the copyright holder nor the names of its contributors may be used to
   endorse or promote products derived from this software without specific prior written
   permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR
IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR
CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER
IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT
OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
TEXT
  section "Inter, the built-in font (fagus-text)" crates/fagus-text/fonts/Inter-LICENSE.txt
  section "Hyphenation dictionaries of Chromium (fagus-text)" crates/fagus-text/hyphenation/LICENSE.txt
  printf '\n################################################################################\nRust crates the fagus binary is built from\n################################################################################\n'
  cargo about generate --frozen -c tools/notices/about.toml -m crates/fagus-cli/Cargo.toml tools/notices/about.hbs
} > THIRD-PARTY-NOTICES
echo "THIRD-PARTY-NOTICES: $(wc -l < THIRD-PARTY-NOTICES) lines"
