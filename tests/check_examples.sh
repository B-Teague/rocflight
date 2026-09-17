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
# Usage: tests/check_examples.sh [name ...]
set -uo pipefail
cd "$(dirname "$0")/.."
ROC=${ROC:-roc}
ROCFLIGHT=${ROCFLIGHT:-$PWD/target/debug/rocflight}
ROOT=tests/roc/examples

# name entry mode
#   run       compare bare `roc <entry>` against `rocflight <entry>`
#   test      a module with no entry point; roc runs its `expect`s via `roc test`
#   skip:...  roc itself cannot run it with the installed compiler; reason follows
#
# Every example `roc` can run here matches byte for byte. The nine below are skipped
# because `roc` ITSELF refuses them with this compiler build — a platform or package
# built for a different version — so there is nothing to compare against, and the gap
# is not the interpreter's.
#
# Getting the other 19 there took `Builtin.roc` itself: `Dict` and `Set` run the
# compiler's own open-addressing table, `Json` and `EncodeDecode` a codec a type can
# override with its own `encoder_for`, and `SafeMath` a real fixed-point `Dec`. The
# numeric ones also needed roc's numeral default — an unconstrained literal is
# fractional, so `15` prints `15.0`. See BUILTIN_PLAN.md.
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
CommandLineArgsFile     main.roc                skip:needs a basic-cli platform built for another compiler (roc version mismatch)
Commands                main.roc                skip:needs a basic-cli platform built for another compiler (roc version mismatch)
ErrorHandlingRealWorld  main.roc                skip:needs a basic-cli platform built for another compiler (roc version mismatch)
ImportPackageFromModule main.roc                skip:needs a package built for another compiler (roc version mismatch)
LoopEffect              main.roc                skip:needs a basic-cli platform built for another compiler (roc version mismatch)
Parser                  main.roc                skip:needs a basic-cli platform built for another compiler (roc version mismatch)
RandomNumbers           main.roc                skip:needs a package built for another compiler (roc version mismatch)
Snake                   main.roc                skip:needs a basic-cli platform built for another compiler (roc version mismatch)
EOF
)

pass=0 fail=0 skip=0
red()   { printf '\033[31m%s\033[0m' "$1"; }
green() { printf '\033[32m%s\033[0m' "$1"; }
grey()  { printf '\033[90m%s\033[0m' "$1"; }

want=("$@")
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

  dir="$ROOT/$name"
  if [ ! -f "$dir/$entry" ]; then
    printf '  %s %-24s missing %s\n' "$(red FAIL)" "$name" "$entry"; fail=$((fail+1)); continue
  fi

  # `roc test` runs a module's top-level `expect`s and nothing else; `roc run` runs
  # its entry point and skips them. rocflight splits the same way, on `--test`.
  case "$mode" in
    test) roc_out=$(cd "$dir" && timeout 120 "$ROC" test "$entry" 2>&1 </dev/null) ;;
    # Bare `roc <file>`, which is what every example's README shows. It is NOT the
    # same as `roc run <file>`: the multi-file examples resolve their imports under the
    # bare form and are rejected with "expected app header" under `roc run`.
    *)    roc_out=$(cd "$dir" && timeout 120 "$ROC" "$entry" 2>&1 </dev/null) ;;
  esac
  # `roc test` reports its own timing, which is not reproducible.
  roc_out=$(printf '%s\n' "$roc_out" | sed -E 's/ in [0-9.]+ m?s\.?( \(cached\))?$//')

  # `--test` makes rocflight report its `expect` tally the way `roc test` does.
  int_flags=""
  [ "$mode" = test ] && int_flags=--test
  int_out=$(cd "$dir" && timeout 120 "$ROCFLIGHT" $int_flags "$entry" 2>&1 </dev/null | grep -v '^\[Desugaring\]')
  int_out=$(printf '%s\n' "$int_out" | sed -E 's/ in [0-9.]+ m?s\.?( \(cached\))?$//')

  if [ "$roc_out" = "$int_out" ]; then
    printf '  %s %-24s %s\n' "$(green ok)" "$name" "$(printf '%s' "$roc_out" | head -1 | cut -c1-44)"
    pass=$((pass+1))
  else
    printf '  %s %-24s\n' "$(red FAIL)" "$name"
    diff <(printf '%s\n' "$roc_out") <(printf '%s\n' "$int_out") | sed 's/^/      /' | head -10
    fail=$((fail+1))
  fi
done <<< "$MANIFEST"

echo
echo "$pass passed, $fail failed, $skip skipped (roc cannot run those here)"
[ "$fail" -eq 0 ]
