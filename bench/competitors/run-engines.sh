#!/bin/bash
# Runs inside python:3.13-slim with /bench = engines dir, pinned to one CPU.
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq && apt-get install -y -qq --no-install-recommends fontconfig >/dev/null
mkdir -p /usr/share/fonts/truetype/inter && cp /bench/fonts/*.ttf /usr/share/fonts/truetype/inter/ && fc-cache -f >/dev/null
pip install -q --no-cache-dir --only-binary=:all: pyfulgur ironpress fullbleed plutoprint 2>&1 | grep -vi warning
pip list 2>/dev/null | grep -iE "fulgur|ironpress|fullbleed|plutoprint"
python -c "import plutoprint; print('plutoprint Book:', [n for n in dir(plutoprint.Book) if not n.startswith('_')])" 2>&1 | tail -1
for e in fullbleed fulgur ironpress pluto; do
  echo "== $e"
  for i in 1 2 3; do python bench.py $e /bench/test-3.html 30 2>&1 | grep -v '^ERROR' | tail -2; done
  python bench.py $e /bench/test-150.html 10 2>&1 | grep -v '^ERROR' | tail -2
  python bench.py $e /bench/tiny-font.html 30 2>&1 | grep -v '^ERROR' | tail -2
done
