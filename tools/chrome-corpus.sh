#!/usr/bin/env bash
# Prints every document of tests/corpus with Chrome and with Fagus, draws both PDFs with Poppler's
# pdftoppm at 50 dpi and compares them: a document matches when both have the same number of
# pages and on no page more than 0.5 % of the pixels differ beyond antialiasing and glyphs a pixel
# apart. It fails when a document differs that tools/chrome-expectations.txt does not list, and
# warns about listed documents that match now; the share of matching documents is in the summary.
# Documents that use print features Chrome lacks (running elements, named strings, cross
# references, leaders, footnotes, blank pages for left and right page breaks), Fagus' layers,
# TIFF images or web fonts with font-display: optional (which Chrome takes only if they load in
# time) are not compared. Chrome gets each document's language (LANGUAGE on Linux, --lang
# elsewhere), as Fagus writes the texts of form controls in it. Chrome on Linux is the reference
# (docs/css.md); on other systems the fonts and so the results differ.
#
#   tools/chrome-corpus.sh OUT_DIR [fagus-conformance corpus options]
#
# CHROME names the Chrome binary. Without it, the script downloads Chrome for Testing at the
# version below (Linux x64 only), checks its SHA-256 and keeps it in target/chrome, and gives
# Chrome and Fagus the same fixed set of fonts (see set_up_fonts). With CHROME, Fagus renders with
# tests/fonts and the fonts installed on the machine, as Chrome uses those. FAGUS_CONFORMANCE names a
# harness binary built before (otherwise cargo builds it). The PDFs go to OUT_DIR/chrome and
# OUT_DIR/fagus, the result of each document to OUT_DIR/results.txt, the summary to
# OUT_DIR/summary.md (and the job summary on GitHub Actions) and the expectations this run would
# write to OUT_DIR/expectations.txt; --update writes them into tools/chrome-expectations.txt.
set -euo pipefail

CHROME_VERSION=157.0.8103.78
CHROME_SHA256=9223832977a13db3646201b1341c1f9cf08afa3ceae92540116d7b0c062364fb

root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:?usage: tools/chrome-corpus.sh OUT_DIR [options]}
shift
mkdir -p "$out"

# The fonts Chrome and Fagus both see on Linux: the test fonts, the metric-compatible Liberation
# fonts, DejaVu, and one Noto family for each other script of the corpus, from the packages of
# Ubuntu 24.04 (fonts-liberation2, fonts-dejavu-core, fonts-dejavu-extra, fonts-noto-core,
# fonts-noto-cjk). A fontconfig configuration of their own (FONTCONFIG_FILE, which Chrome's
# fontconfig and Fagus read) lists only these, with the system's rules (aliases such as Arial to
# Liberation Sans), so that both fall back to the same font for a script.
set_up_fonts() {
  local fonts=$root/target/chrome-fonts names=() files=() name file
  for name in Sans Serif Mono; do
    names+=(Liberation"$name"-{Regular,Bold,Italic,BoldItalic}.ttf)
  done
  names+=(DejaVu{Sans,SansMono}{,-Bold,-Oblique,-BoldOblique}.ttf DejaVuSerif{,-Bold,-Italic,-BoldItalic}.ttf)
  for name in Thai Devanagari Bengali Tamil Ethiopic; do
    names+=(NotoSans"$name"-{Regular,Bold}.ttf)
  done
  names+=(NotoSansCJK-{Regular,Bold}.ttc)
  files+=("$root"/tests/fonts/*.ttf)
  for name in "${names[@]}"; do
    file=$(find /usr/share/fonts -name "$name" -print -quit)
    if [[ -z $file ]]; then
      echo "missing font $name; install the packages named in tools/chrome-corpus.sh. Installed:" >&2
      find /usr/share/fonts -type f | sort >&2
      exit 2
    fi
    files+=("$file")
  done
  rm -rf "$fonts"
  mkdir -p "$fonts/files" "$fonts/cache"
  cp "${files[@]}" "$fonts/files/"
  cat > "$fonts/fonts.conf" <<CONF
<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <dir>$fonts/files</dir>
  <cachedir>$fonts/cache</cachedir>
  <include ignore_missing="yes">/etc/fonts/conf.d</include>
</fontconfig>
CONF
  export FONTCONFIG_FILE=$fonts/fonts.conf
  fc-cache -f > /dev/null 2>&1 || true
  echo "fonts for Chrome and Fagus:"
  (cd "$fonts/files" && sha256sum ./*)
}

chrome=${CHROME:-}
arguments=()
if [[ -z $chrome ]]; then
  if [[ $(uname -s)-$(uname -m) != Linux-x86_64 ]]; then
    echo "Chrome for Testing is downloaded for Linux x64 only; set CHROME to a Chrome binary" >&2
    exit 2
  fi
  directory=$root/target/chrome/$CHROME_VERSION
  if [[ ! -x $directory/chrome-linux64/chrome ]]; then
    mkdir -p "$directory"
    curl -fsSL -o "$directory/chrome.zip" \
      "https://storage.googleapis.com/chrome-for-testing-public/$CHROME_VERSION/linux64/chrome-linux64.zip"
    echo "$CHROME_SHA256  $directory/chrome.zip" | sha256sum -c -
    unzip -q -o "$directory/chrome.zip" -d "$directory"
    rm "$directory/chrome.zip"
  fi
  chrome=$directory/chrome-linux64/chrome
  # The sandbox needs user namespaces, which Ubuntu 24.04 restricts; the corpus is trusted.
  arguments+=(--chrome-arg=--no-sandbox)
  set_up_fonts
fi

harness=${FAGUS_CONFORMANCE:-}
if [[ -z $harness ]]; then
  (cd "$root" && cargo build --release --locked -p fagus-testkit)
  harness=$root/target/release/fagus-conformance
fi
exec "$harness" corpus --chrome "$chrome" --tests "$root/tests" --out "$out" \
  --expectations "$root/tools/chrome-expectations.txt" ${arguments[@]+"${arguments[@]}"} "$@"
