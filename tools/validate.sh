#!/usr/bin/env bash
# Renders the corpus as PDF/A, PDF/UA and Factur-X invoices and checks the files with veraPDF
# (https://verapdf.org) and Mustang (https://www.mustangproject.org).
#
# Usage: VERAPDF=path/to/verapdf MUSTANG=path/to/Mustang-CLI.jar tools/validate.sh OUT_DIR
#
# Pages that Fagus refuses for PDF/UA or PDF/A level a (images without alternative text, headings
# that do not start at h1) are left out; the message says why.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:?usage: tools/validate.sh OUT_DIR}
fagus=${FAGUS:-$root/target/release/fagus}
verapdf=${VERAPDF:?set VERAPDF to the verapdf script}
mustang=${MUSTANG:?set MUSTANG to the Mustang CLI jar}
fonts=$root/tests/fonts
xml=$root/tests/corpus/invoice/factur-x.xml
mkdir -p "$out"

pdfa=()
ua=()
for page in "$root"/tests/corpus/*.html "$root"/tests/corpus/*/*.html; do
  name=$(basename "$(dirname "$page")")-$(basename "$page" .html)
  for level in 2b 2u 3b 3a; do
    if "$fagus" render "$page" -o "$out/$name.$level.pdf" --font-dir "$fonts" --pdfa "$level" 2>"$out/$name.$level.log"; then
      pdfa+=("$out/$name.$level.pdf")
    else
      echo "refused as PDF/A-$level: $name: $(head -c 160 "$out/$name.$level.log")"
    fi
  done
  if "$fagus" render "$page" -o "$out/$name.ua.pdf" --font-dir "$fonts" --accessible 2>"$out/$name.ua.log"; then
    ua+=("$out/$name.ua.pdf")
  else
    echo "refused as PDF/UA: $name: $(head -c 160 "$out/$name.ua.log")"
  fi
done
invoices=()
for page in "$root"/tests/corpus/invoice/*.html; do
  name=factur-x-$(basename "$page" .html)
  "$fagus" render "$page" -o "$out/$name.pdf" --font-dir "$fonts" --pdfa 3b --factur-x "$xml" 2>/dev/null
  "$fagus" render "$page" -o "$out/$name.ua.pdf" --font-dir "$fonts" --pdfa 3a --accessible --factur-x "$xml" 2>/dev/null
  invoices+=("$out/$name.pdf" "$out/$name.ua.pdf")
  pdfa+=("$out/$name.pdf" "$out/$name.ua.pdf")
  ua+=("$out/$name.ua.pdf")
done

failed=0
# The flavour of PDF/A comes from each file's metadata.
"$verapdf" --format text "${pdfa[@]}" > "$out/verapdf-pdfa.txt" || true
"$verapdf" --format text -v -f ua1 "${ua[@]}" > "$out/verapdf-pdfua.txt" || true
for report in "$out/verapdf-pdfa.txt" "$out/verapdf-pdfua.txt"; do
  echo "$(basename "$report"): $(grep -c '^PASS' "$report") passed, $(grep -c '^FAIL' "$report" || true) failed"
  if grep -q '^FAIL' "$report"; then
    grep -A3 '^FAIL' "$report"
    failed=1
  fi
done
for invoice in "${invoices[@]}"; do
  report="$out/$(basename "$invoice" .pdf).mustang.xml"
  java -jar "$mustang" --disable-file-logging --action validate --source "$invoice" > "$report" 2>/dev/null || true
  if grep -q '<summary status="valid"/>' "$report" && ! grep -q '<summary status="invalid"/>' "$report"; then
    echo "Mustang: $(basename "$invoice") valid"
  else
    echo "Mustang: $(basename "$invoice") invalid, see $report"
    failed=1
  fi
done
exit $failed
