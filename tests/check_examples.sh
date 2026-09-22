#!/usr/bin/env bash
# Runs every example from https://www.roc-lang.org/examples/ under both the roc
# compiler and rocflight, and requires the two outputs to be byte-identical.
#
# Unlike the golden pairs in tests/roc/<NN>_<phase>/, these are not written for the
# interpreter — they are the language's own showcase, copied verbatim. They are the
# outside-in check on everything the phases built.
#
# stdout and stderr are compared together, because several examples write to stderr
# (`dbg`, `expect`) and an example's exit status is part of what it demonstrates —
# CommandLineArgs exits non-zero on purpose when given no arguments.
#
# Usage: tests/check_examples.sh [--strict] [name ...]
set -uo pipefail
cd "$(dirname "$0")/.."
ROC=${ROC:-roc}
ROCFLIGHT=${ROCFLIGHT:-$PWD/target/debug/rocflight}
# An app on a real platform runs on that platform's host: rocflight links
# `librocflight_host.a` into it (Learning.md §13). Built by
# `tests/check_host.sh`; without it, platform apps stop at their first effect.
HOST_LIB=$PWD/target/x86_64-unknown-linux-musl/release/librocflight_host.a
[ -z "${ROCFLIGHT_LIB:-}" ] && [ -f "$HOST_LIB" ] && export ROCFLIGHT_LIB=$HOST_LIB
ROOT=tests/roc/examples

# Every example is run from a scratch COPY with its `roc: "nightly-…"` pin rewritten to
# the installed compiler's version. The pins in the checked-in examples are whatever
# roc-lang.org shipped, a few days ahead of the pinned nightly, and `roc` refuses a
# header whose pin is not its own. That refusal is about the version string, not the
# platform: the platform tarball a URL names is the same bytes whatever pins it.
# Rewriting the pin in a copy is what lets `roc` be the oracle for the platform apps
# without touching the sources.
SCRATCH=$(mktemp -d)
trap 'rm -rf "$SCRATCH"' EXIT
ROC_VERSION=$("$ROC" version 2>/dev/null | awk '{print $NF}')

# name entry mode
#   run       compare bare `roc <entry>` against `rocflight <entry>`
#   test      a module with no entry point; roc runs its `expect`s via `roc test`
#   host      runs on a real platform's compiled host, which rocflight links itself
#             into (Learning.md §13; needs librocflight_host.a, see below).
#             `roc` is the oracle; a mismatch is a language gap met on the way and
#             is reported as PENDING, fatal only under --strict
#   skip:...  roc itself cannot run it with the installed compiler; reason follows
#
# Every `run` and `test` example matches byte for byte. The eight `host` examples all
# run under `roc` here (2026-09-17: all eight, once the pin is rewritten). They are
# apps on basic-cli's prebuilt host, which rocflight will link against and call rather
# than re-implement; until then a mismatch is the measure of that gap, not a failure.
# A PENDING line shows rocflight's LAST line of output, which is the thing it is
# actually blocked on. Snake runs on the host and matches. The rest are language
# gaps met while loading the platform's own modules or the app: a string literal
# standing for a nominal (`Path`, which CommandLineArgsFile, Commands and
# ErrorHandlingRealWorld import), package imports (`Grapheme.split`,
# `Random.seed`), and inference (`LoopEffect`, `Parser`).
#
# Getting the 19 there took `Builtin.roc` itself: `Dict` and `Set` run the
# compiler's own open-addressing table, `Json` and `EncodeDecode` a codec a type can
# override with its own `encoder_for`, and `SafeMath` a real fixed-point `Dec`. The
# numeric ones also needed roc's numeral default — an unconstrained literal is
# fractional, so `15` prints `15.0`. See Learning.md §11.
MANIFEST=$(cat <<'EOF'
HelloWorld              main.roc                run
FizzBuzz                main.roc                run
AllSyntax               main.roc                run
Tuples                  main.roc                run
PatternMatching         PatternMatching.roc     run
ErrorHandlingBasic      ErrorHandlingBasic.roc  run
BasicDict               BasicDict.roc           run
IngestFiles             main.roc                run
Json                    main.roc                run
CommandLineArgs         main.roc                run
TryOperatorDesugaring   main.roc                run
SafeMath                main.roc                run
EncodeDecode            main.roc                run
CustomInspect           OpaqueTypes.roc         run
LeastSquares            main.roc                run
GraphTraversal          Graph.roc               test
MultipleRocFiles        main.roc                run
RecordBuilder           DateParser.roc          test
TowersOfHanoi           Hanoi.roc               test
ImportFromDirectory     main.roc                skip:roc rejects it here — "the value hello is not exposed by the module Dir/Hello"
CommandLineArgsFile     main.roc                host
Commands                main.roc                host
ErrorHandlingRealWorld  main.roc                host
ImportPackageFromModule main.roc                host
LoopEffect              main.roc                host
Parser                  main.roc                host
RandomNumbers           main.roc                host
Snake                   main.roc                host
EOF
)

pass=0 fail=0 pending=0 skip=0
red()   { printf '\033[31m%s\033[0m' "$1"; }
green() { printf '\033[32m%s\033[0m' "$1"; }
amber() { printf '\033[33m%s\033[0m' "$1"; }
grey()  { printf '\033[90m%s\033[0m' "$1"; }

strict=0
want=()
for arg in "$@"; do
  case "$arg" in
    --strict) strict=1 ;;
    *) want+=("$arg") ;;
  esac
done
selected() {
  [ ${#want[@]} -eq 0 ] && return 0
  for w in "${want[@]}"; do [ "$w" = "$1" ] && return 0; done
  return 1
}

while read -r name entry mode; do
  [ -z "$name" ] && continue
  selected "$name" || continue

  case "$mode" in
    skip:*)
      printf '  %s %-24s %s\n' "$(grey SKIP)" "$name" "$(grey "${mode#skip:}")"
      skip=$((skip+1)); continue ;;
  esac

  if [ ! -f "$ROOT/$name/$entry" ]; then
    printf '  %s %-24s missing %s\n' "$(red FAIL)" "$name" "$entry"; fail=$((fail+1)); continue
  fi
  # The scratch copy, pin rewritten (see the note by SCRATCH).
  dir="$SCRATCH/$name"
  cp -r "$ROOT/$name" "$dir"
  grep -rl 'roc: "nightly-' "$dir" --include='*.roc' 2>/dev/null \
    | xargs -r sed -i "s/roc: \"nightly-[0-9a-f-]*\"/roc: \"$ROC_VERSION\"/"

  # `roc test` runs a module's top-level `expect`s and nothing else; `roc run` runs
  # its entry point and skips them. rocflight splits the same way, on `test`.
  case "$mode" in
    test) roc_out=$(cd "$dir" && timeout 120 "$ROC" test "$entry" 2>&1 </dev/null) ;;
    # Bare `roc <file>`, which is what every example's README shows. It is NOT the
    # same as `roc run <file>`: the multi-file examples resolve their imports under the
    # bare form and are rejected with "expected app header" under `roc run`.
    *)    roc_out=$(cd "$dir" && timeout 120 "$ROC" "$entry" 2>&1 </dev/null) ;;
  esac
  # `roc test` reports its own timing, which is not reproducible.
  roc_out=$(printf '%s\n' "$roc_out" | sed -E 's/ in [0-9.]+ m?s\.?( \(cached\))?$//')

  # `rocflight test` reports its `expect` tally the way `roc test` does.
  int_flags=""
  [ "$mode" = test ] && int_flags=test
  int_out=$(cd "$dir" && timeout 120 "$ROCFLIGHT" $int_flags "$entry" 2>&1 </dev/null | grep -v '^\[Desugaring\]')
  int_out=$(printf '%s\n' "$int_out" | sed -E 's/ in [0-9.]+ m?s\.?( \(cached\))?$//')

  if [ "$roc_out" = "$int_out" ]; then
    printf '  %s %-24s %s\n' "$(green ok)" "$name" "$(printf '%s' "$roc_out" | head -1 | cut -c1-44)"
    pass=$((pass+1))
  elif [ "$mode" = host ] && [ "$strict" -eq 0 ]; then
    printf '  %s %-24s %s\n' "$(amber PENDING)" "$name" "$(grey "$(printf '%s' "$int_out" | tail -1 | cut -c1-60)")"
    pending=$((pending+1))
  else
    printf '  %s %-24s\n' "$(red FAIL)" "$name"
    diff <(printf '%s\n' "$roc_out") <(printf '%s\n' "$int_out") | sed 's/^/      /' | head -10
    fail=$((fail+1))
  fi
done <<< "$MANIFEST"

# Snake, PLAYED: the manifest run above ends at the first read of stdin. This drives
# a whole game under both, one key per drawn frame — a key is sent only after the
# frame's `Score:` line, so both programs see the same key on the same turn — and
# compares every byte. The keys steer onto the food (5 right, 5 down), eat it, and
# march into a wall. That is the path that found the interpreter defaulting the
# numerals of a forward-referenced type to `Dec` and crashing on `%`.
lockstep() { # OUT KEY... -- CMD...: one key per frame into CMD, output to OUT
  local out=$1; shift; local keys=(); while [ "$1" != "--" ]; do keys+=("$1"); shift; done; shift
  : > "$out"
  coproc GAME { "$@" 2>&1; }
  local pid=$GAME_PID line k
  for k in "${keys[@]}"; do
    while IFS= read -r -u "${GAME[0]}" line; do
      printf '%s\n' "$line" >> "$out"
      case "$line" in Score:*) break ;; esac
    done
    printf '%s' "$k" >&"${GAME[1]}"
  done
  exec {GAME[1]}>&-
  cat <&"${GAME[0]}" >> "$out"
  wait "$pid"
}
if selected Snake && [ -n "${ROCFLIGHT_LIB:-}" ] && [ -d "$SCRATCH/Snake" ]; then
  keys=(d d d d d s s s s s d d d d w w w w a a a a a a a a a a a a)
  ( cd "$SCRATCH/Snake" && timeout 120 bash -c "$(declare -f lockstep); lockstep roc.play ${keys[*]} -- '$ROC' main.roc" )
  ( cd "$SCRATCH/Snake" && timeout 120 bash -c "$(declare -f lockstep); lockstep int.play ${keys[*]} -- '$ROCFLIGHT' main.roc" )
  if cmp -s "$SCRATCH/Snake/roc.play" "$SCRATCH/Snake/int.play"; then
    printf '  %s %-24s %s\n' "$(green ok)" "Snake, played" "$(grep -ac '^Score' "$SCRATCH/Snake/roc.play") frames identical, score $(grep -a '^Score' "$SCRATCH/Snake/roc.play" | tail -1 | tr -d '\r' | awk '{print $2}')"
    pass=$((pass+1))
  else
    printf '  %s %-24s\n' "$(red FAIL)" "Snake, played"
    diff "$SCRATCH/Snake/roc.play" "$SCRATCH/Snake/int.play" | sed 's/^/      /' | head -10
    fail=$((fail+1))
  fi
fi

echo
echo "$pass passed, $fail failed, $pending pending (platform apps blocked on a language gap), $skip skipped (roc cannot run those here)"
[ "$fail" -eq 0 ]
