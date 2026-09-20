#!/usr/bin/env bash
# A rocflight for the eval runner that remembers why a test failed.
#
# The runner execs `<binary> eval [--raw] <file>` and reads stdout; stderr is thrown
# away, so a failing test says `RuntimeError` and nothing more. Run through this
# instead (`tests/check_eval.sh --report`) and every failure leaves a file under
# $EVAL_FAILS: the test's source, then rocflight's stderr, which carries the reason.
#
# stdout passes straight through, so the runner sees exactly what it would have.
real=${ROCFLIGHT_REAL:?set by check_eval.sh}
fails=${EVAL_FAILS:?set by check_eval.sh}
file=${@: -1}
err=$(mktemp)
"$real" "$@" 2>"$err"
code=$?
if [ "$code" -ne 0 ]; then
  {
    echo "=== exit $code"
    cat "$file"
    echo
    echo "--- stderr"
    grep -v '^\[Desugaring\]' "$err"
  } > "$fails/$(basename "$(dirname "$file")").txt"
fi
rm -f "$err"
exit "$code"
