#!/usr/bin/env bash
# Runs the reftests of web-platform-tests with Fagus: checks out the tests (tools/wpt/fetch.sh),
# builds the harness and runs it. Tests and references are both rendered by Fagus, drawn by
# Poppler's pdftoppm at 96 dpi and compared with the fuzziness each test allows. It needs git
# and pdftoppm (poppler-utils).
#
#   tools/wpt/run.sh                          # every directory of tools/wpt/dirs.txt
#   tools/wpt/run.sh --filter css/css-cascade # only tests whose path contains this
#   tools/wpt/run.sh --update                 # also rewrite tools/wpt/expectations.txt
#   tools/wpt/run.sh --docs docs/compatibility.md   # also rewrite the table of pass rates
#
# The results of each directory go to target/wpt/results/results/<directory>.txt, the numbers
# to target/wpt/results/summary.md and summary.json, and the expectations this run would write
# to target/wpt/results/expectations.txt. It fails if a test fails that tools/wpt/expectations.txt
# does not expect to fail; tests that pass although they are expected to fail are listed.
#
# FAGUS_CONFORMANCE names a harness binary built before (otherwise cargo builds it), JOBS the
# number of tests at once, PDFTOPPM another pdftoppm. More options: fagus-conformance wpt --help.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
tools/wpt/fetch.sh target/wpt/checkout
harness=${FAGUS_CONFORMANCE:-}
if [[ -z $harness ]]; then
  cargo build --release --locked -p fagus-testkit
  harness=target/release/fagus-conformance
fi
jobs=()
[[ -z ${JOBS:-} ]] || jobs=(--jobs "$JOBS")
exec "$harness" wpt --checkout target/wpt/checkout ${jobs[@]+"${jobs[@]}"} "$@"
