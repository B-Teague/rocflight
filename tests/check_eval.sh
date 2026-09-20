#!/usr/bin/env bash
# Parity gate: roc's own eval tests, with rocflight as one more backend.
#
# `roc-compiler/src/eval/test/` holds ~2,300 data-driven eval tests. roc's runner
# (`parallel_runner.zig`) runs each one through its interpreter, its dev backend and
# wasm, and requires the `Str.inspect` strings to agree. With `--rocflight <binary>`
# it runs rocflight too, as a fifth backend, over the same pipe protocol and with the
# same comparison — so "does rocflight agree with roc" is answered by roc's own
# harness, on roc's own tests, not by anything written here.
#
# Full parity is every test passing with rocflight enabled. Until then this reports
# rocflight's tally and lists what it gets wrong; `--strict` makes any miss fatal.
#
#   tests/check_eval.sh                  build if needed, run everything
#   tests/check_eval.sh --strict         exit non-zero unless rocflight passes all
#   tests/check_eval.sh --filter "Dict"  only tests whose name or source matches
#   tests/check_eval.sh --report         also say WHY: every failure's source and
#                                        stderr are kept, and bucketed by reason
#   tests/check_eval.sh --keep           leave the failure files in place (with --report)
#   tests/check_eval.sh -- --verbose     anything after -- goes to the runner
#
# The runner is built from roc-compiler with `zig build build-test-eval-runner`
# (ReleaseFast — a Debug runner is several times slower on the interpreter side).
# Building it takes a few minutes the first time; it is cached after that.
set -uo pipefail
cd "$(dirname "$0")/.."
ROCFLIGHT=${ROCFLIGHT:-$PWD/target/release/rocflight}
RUNNER=roc-compiler/zig-out/bin/eval-test-runner
strict=0
report=0
keep=0
runner_args=()
while [ $# -gt 0 ]; do
  case "$1" in
    --strict) strict=1 ;;
    --report) report=1 ;;
    --keep) keep=1; report=1 ;;
    --filter) runner_args+=(--filter "$2"); shift ;;
    --) shift; runner_args+=("$@"); break ;;
    *) runner_args+=("$1") ;;
  esac
  shift
done

if [ ! -x "$ROCFLIGHT" ]; then
  echo "error: $ROCFLIGHT not built — run: cargo build --release" >&2
  exit 1
fi
if [ ! -x "$RUNNER" ] || [ roc-compiler/src/eval/test/parallel_runner.zig -nt "$RUNNER" ]; then
  echo "building $RUNNER (this takes a few minutes the first time)..."
  (cd roc-compiler && zig build build-test-eval-runner -Doptimize=ReleaseFast) || exit 1
fi

log=$(mktemp)
trap 'rm -f "$log"' EXIT
binary=$ROCFLIGHT
if [ "$report" -eq 1 ]; then
  # The runner discards a backend's stderr; the stand-in keeps it, per failure.
  export ROCFLIGHT_REAL=$ROCFLIGHT
  export EVAL_FAILS=${EVAL_FAILS:-$PWD/target/eval-fails}
  rm -rf "$EVAL_FAILS"; mkdir -p "$EVAL_FAILS"
  binary=$PWD/tests/eval_stand_in.sh
fi
"$RUNNER" --rocflight "$binary" "${runner_args[@]}" 2>&1 | tee "$log" | grep -v '^  \(PASS\|RUN\|SKIP\) ' | grep -v '^        \(interpreter\|dev\|wasm\|llvm\):'

if [ "$report" -eq 1 ]; then
  echo
  echo "=== rocflight failures by roc test file ==="
  # A failing test's name, looked up in the test tables.
  grep -E '^  (FAIL|HANG) ' "$log" | sed -E 's/^  (FAIL|HANG)  //; s/  \([0-9.]+ms( total)?\)$//' \
    | while IFS= read -r name; do
        grep -l -F ".name = \"$name\"" roc-compiler/src/eval/test/eval_*.zig 2>/dev/null | head -1 | xargs -r basename
      done | sort | uniq -c | sort -rn
  echo
  echo "=== rocflight failures by reason (first error line; NO-MESSAGE is a wrong value) ==="
  for f in "$EVAL_FAILS"/*.txt; do
    [ -e "$f" ] || break
    awk '/^--- stderr/{f=1;next} f' "$f" | grep -m1 -E 'rror|Undefined|ambiguous|vm:' || echo NO-MESSAGE
  done | sed -E 's/ at [^ ]+\.roc:[0-9]+:[0-9]+//g; s/(Unknown function [A-Za-z0-9]+\.[a-z_0-9]+).*/\1/; s/(No match arm matched).*/\1 <value>/; s/(cannot hold).*/\1 <value>/; s/(is ambiguous:).*/\1 <names>/' \
    | sort | uniq -c | sort -rn | head -60
  echo
  echo "=== wrong values (expected | got) ==="
  awk '/^        expected:/{e=$0; sub(/^        expected: +/,"",e)} /^        rocflight:      WRONG/{g=$0; sub(/.*got /,"",g); sub(/ \([0-9.]+ms\)$/,"",g); print e " | " g}' "$log" \
    | sort | uniq -c | sort -rn | head -40
  if [ "$keep" -eq 1 ]; then
    cp "$log" "$EVAL_FAILS/runner.log"
    echo; echo "failure files kept under $EVAL_FAILS (the runner's output is runner.log)"
  else
    rm -rf "$EVAL_FAILS"
  fi
fi

# The runner's own exit code says whether EVERY backend passed everything; the
# rocflight row of the per-backend tally is the number this gate is about.
tally=$(grep -E '^  rocflight:' "$log" | tail -1)
echo
echo "${tally:-rocflight: no tests ran}"
if [ "$strict" -eq 1 ]; then
  [[ "$tally" =~ ([0-9]+)\ of\ ([0-9]+) ]] && [ "${BASH_REMATCH[1]}" = "${BASH_REMATCH[2]}" ]
else
  true
fi
