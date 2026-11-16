#!/usr/bin/env zsh
# Renders the test invoices with every engine (3 new processes each, every one rendering the
# document once and then repeatedly) and the feature probes once, then prints the medians, the
# page counts and which probes each engine passed.
set -e
cd ${0:A:h}
repo=${0:A:h:h:h}
python=${PYTHON:-python3}
# WeasyPrint finds Homebrew's Pango only through this (macOS drops it when it starts a shell).
[[ $(uname) == Darwin ]] && export DYLD_FALLBACK_LIBRARY_PATH=${DYLD_FALLBACK_LIBRARY_PATH:-/opt/homebrew/lib}

mkdir -p docs/fonts out
cp $repo/tests/fonts/Inter-Regular.ttf $repo/tests/fonts/Inter-Bold.ttf docs/fonts/
for n in 3 150 600; do $python gen-classic.py $n > docs/classic-$n.html; done
for n in 3 150; do $python $repo/tools/gen.py $n fonts > docs/modern-$n.html; done

typeset -A engines
all="fagus dompdf mpdf browsershot chrome weasyprint"
engines=(
  classic-3 $all
  classic-150 $all
  classic-600 $all
  modern-3 "fagus chrome weasyprint"
  modern-150 "fagus chrome weasyprint"
)

render() { # engine file repeats
  case $1 in
    chrome) node chrome.js $2 $3 ;;
    weasyprint) $python weasy.py $2 $3 ;;
    *) php -d memory_limit=4G -d error_reporting=0 bench.php $1 $2 $3 ;;
  esac
}

: > out/results.txt
for d in classic-3 classic-150 classic-600 modern-3 modern-150; do
  case $d in *600) n=3 ;; *150) n=5 ;; *) n=20 ;; esac
  for e in ${=engines[$d]}; do
    m=$n; [[ $e == browsershot ]] && m=3
    for i in 1 2 3; do echo "$d $e $(render $e docs/$d.html $m | tail -1)" | tee -a out/results.txt; done
  done
done

for p in probes/*.html; do
  for e in fagus dompdf mpdf chrome weasyprint; do render $e $p 0 > /dev/null 2>&1 || true; done
done

$python check.py
