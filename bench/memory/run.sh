#!/usr/bin/env zsh
# Memory and CPU time of every engine of ../compare, on macOS. Three runs of each engine on each
# invoice (one PDF, IDLE_COLD seconds of rest, N more PDFs, IDLE_WARM seconds of rest), then four
# renderers at once on the 10-page invoice, then the tables (aggregate.py).
#
#   ./run.sh              everything
#   ./run.sh single       only the runs of one renderer at a time
#   ./run.sh parallel     only four at once
#   N=20 ./run.sh         the number of further PDFs per run (default 20)
set -e
cd ${0:A:h}
repo=${0:A:h:h:h}
python=${PYTHON:-.venv/bin/python3}
docs=../compare/docs
N=${N:-20}
what=${1:-all}

# The invoices of ../compare/run.sh.
mkdir -p $docs/fonts out
cp $repo/tests/fonts/Inter-Regular.ttf $repo/tests/fonts/Inter-Bold.ttf $docs/fonts/
for n in 3 150 600; do $python ../compare/gen-classic.py $n > $docs/classic-$n.html; done
for n in 3 150; do $python $repo/tools/gen.py $n fonts > $docs/modern-$n.html; done

php=(php -d memory_limit=4G -d error_reporting=0 engine.php)

if [[ $what == all || $what == single ]]; then
  typeset -A engines
  all="fagus dompdf mpdf browsershot chrome weasyprint"
  engines=(
    classic-3 $all
    classic-150 $all
    classic-600 $all
    modern-3 "fagus chrome weasyprint"
    modern-150 "fagus chrome weasyprint"
  )
  for i in 1 2 3; do
    for d in classic-3 classic-150 classic-600 modern-3 modern-150; do
      for e in ${=engines[$d]}; do
        case $e in
          chrome) cmd=(node chrome.js $docs/$d.html $N) ;;
          weasyprint) cmd=($python weasy.py $docs/$d.html $N) ;;
          *) cmd=($php $e $docs/$d.html $N) ;;
        esac
        echo "== $i $d $e"
        $python driver.py single "$d $e" $cmd
      done
    done
  done
fi

if [[ $what == all || $what == parallel ]]; then
  # Four PHP or Python processes, or one Chrome with four tabs at a time; each renders as many
  # PDFs as it takes to run for a few seconds.
  doc=$docs/classic-150.html
  for i in 1 2 3; do
    for spec in "fagus 4 100" "mpdf 4 15" "dompdf 4 8" "browsershot 4 5" "weasyprint 4 6" "chrome 1 25"; do
      set -- ${=spec}
      e=$1 w=$2 n=$3
      case $e in
        chrome) cmd=(node chrome.js $doc $n free 4) label="Chrome, 1 browser, 4 tabs at once" ;;
        weasyprint) cmd=($python weasy.py $doc $n free) label="WeasyPrint x4 processes" ;;
        *) cmd=($php $e $doc $n free) label="$e x4 processes" ;;
      esac
      echo "== $i $label"
      RESULTS=parallel.jsonl $python driver.py parallel "$label" $w $cmd
    done
  done
fi

$python aggregate.py | tee out/summary.md
