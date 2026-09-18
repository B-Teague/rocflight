#!/usr/bin/env bash
# Compare rocflight against roc's own interpreter on the programs in tests/bench/.
#
# Three engines, same programs, same measurement as tests/bench.sh (median wall time
# over RUNS runs, output checked against `# expect:`):
#
#   rocflight   target/release/rocflight <file>
#   roc-interp  roc --opt=interpreter <file>   roc's LIR interpreter (roc-compiler/src/eval)
#   roc-dev     roc <file>                     roc's dev backend, for scale
#
# A program roc cannot run (script-style files, or ones it rejects) shows `n/a` in
# that column rather than failing the comparison. The last column is roc-interp's
# time over rocflight's: >1 means rocflight is faster.
#
#   tests/bench_compare.sh              all benchmarks
#   tests/bench_compare.sh --runs 9     more repetitions
#   tests/bench_compare.sh calls loop   just those
set -uo pipefail
cd "$(dirname "$0")/.."
ROCFLIGHT=${ROCFLIGHT:-$PWD/target/release/rocflight}
ROC=${ROC:-roc}
RUNS=5
only=()

while [ $# -gt 0 ]; do
  case "$1" in
    --runs) RUNS=$2; shift ;;
    *) only+=("$1") ;;
  esac
  shift
done

if [ ! -x "$ROCFLIGHT" ]; then
  echo "error: $ROCFLIGHT not built — run: cargo build --release" >&2
  exit 1
fi
command -v "$ROC" >/dev/null || { echo "error: $ROC not found" >&2; exit 1; }

# Median wall time in ms of `cmd...` (the last argument is the file). roc caches
# builds, so its first run may include compilation; the median absorbs that.
median_ms() {
  local times=() start end
  for _ in $(seq "$RUNS"); do
    start=$(date +%s%N)
    timeout 120 "$@" >/dev/null 2>&1
    end=$(date +%s%N)
    times+=( $(( (end - start) / 1000000 )) )
  done
  printf '%s\n' "${times[@]}" | sort -n | awk '{a[NR]=$1} END {print a[int((NR+1)/2)]}'
}

# Prints the median in ms, or `n/a` when the engine cannot run the file correctly.
measure() {
  local expected=$1; shift
  local actual
  actual=$(timeout 120 "$@" 2>&1 | grep -v '^\[Desugaring\]')
  if [ "$actual" != "$expected" ]; then echo n/a; return; fi
  median_ms "$@"
}

ms() { [ "$1" = n/a ] && echo n/a || echo "${1}ms"; }

selected() {
  [ ${#only[@]} -eq 0 ] && return 0
  for w in "${only[@]}"; do [ "$w" = "$1" ] && return 0; done
  return 1
}

printf '%-16s %11s %11s %11s %9s\n' benchmark rocflight roc-interp roc-dev "interp/rf"
printf '%-16s %11s %11s %11s %9s\n' --------- --------- ---------- ------- ---------
for file in tests/bench/*.roc; do
  name=$(basename "$file" .roc)
  selected "$name" || continue
  expected=$(grep -oP '(?<=^# expect: ).*' "$file" || true)
  rf=$(measure "$expected" "$ROCFLIGHT" "$file")
  ri=$(measure "$expected" "$ROC" --opt=interpreter "$file")
  rd=$(measure "$expected" "$ROC" "$file")
  ratio=-
  [ "$rf" != n/a ] && [ "$ri" != n/a ] && [ "$rf" -gt 0 ] && ratio=$(awk -v a="$ri" -v b="$rf" 'BEGIN{printf "%.1fx", a/b}')
  printf '%-16s %11s %11s %11s %9s\n' "$name" "$(ms "$rf")" "$(ms "$ri")" "$(ms "$rd")" "$ratio"
done
