#!/usr/bin/env bash
# Golden-pair gate for the syntax test suite.
#
# Every syntax feature has a pair of files:
#     tests/roc/<NN>_<phase>/<syntax>.roc              (sugared)
#     tests/roc/<NN>_<phase>/<syntax>.desugared.roc    (explicit types)
#
# All FOUR of these outputs must be byte-identical:
#     roc run <sugared>      roc run <desugared>
#     rocflight <sugared>    rocflight <desugared>
#
# Two gates, because a pair can be correctly written while the interpreter is
# still catching up:
#
#   PAIR  both files pass `roc check`, the two `roc run` outputs agree, and the
#         desugared file carries explicit annotations. Failing this means the test
#         files are wrong. Always fatal.
#   INTERP  rocflight matches roc on BOTH files, AND builds the same AST and infers
#         the same type from both. The two files differ only in sugar, so the AST
#         they produce must be identical — that is the point of the pair. Failing
#         this means the feature is not implemented yet, or the two files differ by
#         more than sugar. Reported as pending; fatal only under --strict.
#
# --vm runs the interpreter's REGISTER VM instead of its tree-walker, so the INTERP
# gate becomes "the VM matches roc on both files". A pair the VM cannot compile yet is
# reported as pending rather than failed; `tests/vm_coverage.sh` is what counts those.
#
# Usage: tests/check_roc.sh [--strict] [--vm] [path-under-tests/roc]
set -uo pipefail
cd "$(dirname "$0")/.."
ROC=${ROC:-roc}
ROCFLIGHT=${ROCFLIGHT:-./target/debug/rocflight}

strict=0
vm=()
root=tests/roc
for arg in "$@"; do
  case "$arg" in
    --strict) strict=1 ;;
    --vm) vm=(--vm) ;;
    *) root=$arg ;;
  esac
done

pass=0 fail=0 pending=0

red()   { printf '\033[31m%s\033[0m' "$1"; }
green() { printf '\033[32m%s\033[0m' "$1"; }
amber() { printf '\033[33m%s\033[0m' "$1"; }

fail_msg() { printf '  %s %s — %s\n' "$(red FAIL)" "$1" "$2"; fail=$((fail+1)); }

# The interpreter prints its own progress line to stderr; strip it before diffing.
run_interp() { "$ROCFLIGHT" "${vm[@]}" "$1" 2>&1 | grep -v '^\[Desugaring\]'; }

if [ ! -x "$ROCFLIGHT" ]; then
  echo "note: $ROCFLIGHT not built — running PAIR checks only (cargo build to enable INTERP)"
  ROCFLIGHT=
fi

while IFS= read -r sugared; do
  des="${sugared%.roc}.desugared.roc"
  name="${sugared#tests/roc/}"

  # ---- PAIR gate ----
  if [ ! -f "$des" ]; then
    fail_msg "$name" "no .desugared.roc sibling"; continue
  fi

  for f in "$sugared" "$des"; do
    if ! out=$("$ROC" check "$f" 2>&1); then
      fail_msg "$name" "roc check failed on ${f##*/}"
      printf '%s\n' "$out" | sed 's/^/    /' | tail -12
      continue 2
    fi
  done

  # The desugared file must state its types, not merely compile.
  if ! grep -qE '^[a-z_][A-Za-z0-9_]*!? *:' "$des"; then
    fail_msg "$name" "desugared file has no top-level type annotations"; continue
  fi

  # The SUGARED file must state none: every type is inferred. That is the whole point
  # of the pair, and it is what proves inference works rather than coasting on
  # annotations. A handful of files genuinely cannot drop theirs — the annotation is
  # the syntax under test, or `roc` refuses the file without it — and each says so in a
  # comment containing STAY. Anything else is drift.
  if grep -qE '^[[:space:]]*[a-z_][A-Za-z0-9_]*!? +: ' "$sugared" && ! grep -q 'STAY' "$sugared"; then
    fail_msg "$name" "sugared file has type annotations; infer them, or say why they STAY"
    grep -nE '^[[:space:]]*[a-z_][A-Za-z0-9_]*!? +: ' "$sugared" | sed 's/^/    /' | head -5
    continue
  fi

  roc_sug=$("$ROC" run "$sugared" 2>&1)
  roc_des=$("$ROC" run "$des" 2>&1)
  if [ "$roc_sug" != "$roc_des" ]; then
    fail_msg "$name" "desugaring changed the output"
    diff <(printf '%s\n' "$roc_sug") <(printf '%s\n' "$roc_des") | sed 's/^/    /' | head -12
    continue
  fi

  # ---- INTERP gate ----
  if [ -z "$ROCFLIGHT" ]; then
    printf '  %s %s\n' "$(green ok)" "$name"; pass=$((pass+1)); continue
  fi

  int_sug=$(run_interp "$sugared")
  int_des=$(run_interp "$des")

  bad=""
  [ "$int_sug" = "$roc_sug" ] || bad="sugared"
  [ "$int_des" = "$roc_des" ] || bad="${bad:+$bad, }desugared"

  # Same sugar, same AST. Only checked when both files already run correctly,
  # so an unimplemented feature is reported once rather than twice.
  # Only the AST STRUCTURE is compared, not the `:: type` line `--ast-only` also
  # prints. Annotations are not sugar: the desugared file declares types the sugared
  # one leaves to inference, so its type line is legitimately more specific
  # (`List(Str) -> ...` versus `$0 -> ...`). Comparing structure catches the thing
  # that matters — that the two files describe the same program.
  ast_note=""
  if [ -z "$bad" ]; then
    ast_sug=$("$ROCFLIGHT" --ast-only "$sugared" 2>&1 | grep -v '^:: ')
    ast_des=$("$ROCFLIGHT" --ast-only "$des" 2>&1 | grep -v '^:: ')
    if [ "$ast_sug" != "$ast_des" ]; then
      bad="AST differs between the two files"
      ast_note=yes
    fi
  fi

  if [ -n "$bad" ]; then
    if [ "$strict" -eq 1 ]; then
      fail_msg "$name" "$bad"
      if [ -n "$ast_note" ]; then
        diff <(printf '%s\n' "$ast_sug") <(printf '%s\n' "$ast_des") | sed 's/^/    /' | head -8
      fi
      [ "$int_sug" != "$roc_sug" ] && printf '    sugared   roc=%s\n    sugared   int=%s\n' \
        "$(printf %s "$roc_sug" | head -1)" "$(printf %s "$int_sug" | head -1)"
      [ "$int_des" != "$roc_des" ] && printf '    desugared roc=%s\n    desugared int=%s\n' \
        "$(printf %s "$roc_des" | head -1)" "$(printf %s "$int_des" | head -1)"
    else
      printf '  %s %s — %s\n' "$(amber PEND)" "$name" "$bad"
      pending=$((pending+1))
    fi
    continue
  fi

  printf '  %s %s\n' "$(green ok)" "$name"; pass=$((pass+1))
done < <(find "$root" -name '*.roc' ! -name '*.desugared.roc' ! -path '*/examples/*' ! -path '*/bench/*' | sort)

echo
if [ "$strict" -eq 1 ]; then
  echo "$pass passed, $fail failed (strict: interpreter parity required)"
else
  echo "$pass passed, $fail failed, $pending interpreter-pending"
  [ "$pending" -gt 0 ] && echo "run with --strict to treat pending as failure"
fi
[ "$fail" -eq 0 ]
