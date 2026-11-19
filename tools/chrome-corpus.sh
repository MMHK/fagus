#!/usr/bin/env bash
# Prints every document of tests/corpus with Chrome and with Fagus, draws both PDFs with Poppler's
# pdftoppm at 50 dpi and compares them: a document matches when both have the same number of
# pages and on no page more than 0.5 % of the pixels differ beyond antialiasing and glyphs a pixel
# apart. It fails when a document differs that tools/chrome-expectations.txt does not list, and
# warns about listed documents that match now; the share of matching documents is in the summary.
# Documents that use print features Chrome lacks (running elements, named strings, cross
# references, leaders, footnotes, blank pages for left and right page breaks), Fagus' layers or
# TIFF images are not compared. Chrome gets each document's language (--lang), as Fagus writes the
# texts of form controls in it. Chrome on Linux is the reference (docs/css.md); on other systems
# the fonts and so the results differ.
#
#   tools/chrome-corpus.sh OUT_DIR [fagus-conformance corpus options]
#
# CHROME names the Chrome binary. Without it, the script downloads Chrome for Testing at the
# version below (Linux x64 only), checks its SHA-256 and keeps it in target/chrome. Fagus renders
# with tests/fonts and the fonts installed on the machine, as Chrome uses those; on CI the fonts
# of tests/fonts are installed for Chrome too, so both have the same. FAGUS_CONFORMANCE names a
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
fi

harness=${FAGUS_CONFORMANCE:-}
if [[ -z $harness ]]; then
  (cd "$root" && cargo build --release --locked -p fagus-testkit)
  harness=$root/target/release/fagus-conformance
fi
exec "$harness" corpus --chrome "$chrome" --tests "$root/tests" --out "$out" \
  --expectations "$root/tools/chrome-expectations.txt" ${arguments[@]+"${arguments[@]}"} "$@"
