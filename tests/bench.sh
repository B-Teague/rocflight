#!/usr/bin/env bash
# Benchmark the interpreter, and prove a change actually helped.
#
# Each program in tests/bench/ is run several times; the MEDIAN wall time and the peak
# resident memory are recorded. Results are compared against a saved baseline, so the
# question "did that make it faster?" has an answer instead of an opinion.
#
#   tests/bench.sh            measure, and compare against the baseline if one exists
#   tests/bench.sh --save     measure and write the baseline (do this BEFORE a change)
#   tests/bench.sh --runs 9   more repetitions, for a noisier machine
#   tests/bench.sh calls      just one benchmark
#
# Always measures the RELEASE build: the debug build is slower and its profile is not
# the one that matters. Build it first, or this refuses to guess.
#
# Every benchmark also carries its expected output, checked on every run. A change that
# makes the interpreter fast and wrong fails here rather than looking like a win.
set -uo pipefail
cd "$(dirname "$0")/.."
ROCFLIGHT=${ROCFLIGHT:-$PWD/target/release/rocflight}
BASELINE=""
RUNS=5
save=0
only=()

while [ $# -gt 0 ]; do
  case "$1" in
    --save) save=1 ;;
    --runs) RUNS=$2; shift ;;
    --baseline) BASELINE=$2; shift ;;
    *) only+=("$1") ;;
  esac
  shift
done

BASELINE=${BASELINE:-tests/bench/baseline.tsv}

if [ ! -x "$ROCFLIGHT" ]; then
  echo "error: $ROCFLIGHT not built — run: cargo build --release" >&2
  exit 1
fi

red()   { printf '\033[31m%s\033[0m' "$1"; }
green() { printf '\033[32m%s\033[0m' "$1"; }
grey()  { printf '\033[90m%s\033[0m' "$1"; }

# Median of RUNS timings, in milliseconds. The median rather than the mean because one
# scheduler hiccup should not decide the answer.
median_ms() {
  local file=$1 times=()
  for _ in $(seq "$RUNS"); do
    local start end
    start=$(date +%s%N)
    "$ROCFLIGHT" "$file" >/dev/null 2>&1
    end=$(date +%s%N)
    times+=( $(( (end - start) / 1000000 )) )
  done
  printf '%s\n' "${times[@]}" | sort -n | awk '{a[NR]=$1} END {print a[int((NR+1)/2)]}'
}

# Peak resident memory in kB, sampled from /proc. Coarse, but the failures worth
# catching here are order-of-magnitude ones.
peak_kb() {
  local file=$1
  "$ROCFLIGHT" "$file" >/dev/null 2>&1 &
  local pid=$! peak=0 cur
  while kill -0 "$pid" 2>/dev/null; do
    cur=$(awk '/VmHWM/{print $2}' "/proc/$pid/status" 2>/dev/null)
    [ -n "$cur" ] && [ "$cur" -gt "$peak" ] && peak=$cur
    sleep 0.02
  done
  wait "$pid" 2>/dev/null
  echo "$peak"
}

selected() {
  [ ${#only[@]} -eq 0 ] && return 0
  for w in "${only[@]}"; do [ "$w" = "$1" ] && return 0; done
  return 1
}

# A delta beyond this is reported as a real change; anything inside it is noise.
NOISE=3

results=()
fail=0
printf '%-20s %9s %11s %10s\n' "benchmark" "time" "peak" "vs base"
printf '%-20s %9s %11s %10s\n' "---------" "----" "----" "-------"

for file in tests/bench/*.roc; do
  name=$(basename "$file" .roc)
  selected "$name" || continue

  # Correctness first: a faster interpreter that prints something else is not faster.
  expected=$(grep -oP '(?<=^# expect: ).*' "$file" || true)
  actual=$("$ROCFLIGHT" "$file" 2>&1 | grep -v '^\[Desugaring\]')
  if [ -n "$expected" ] && [ "$actual" != "$expected" ]; then
    printf '  %s %-18s wrong output: %s (wanted %s)\n' "$(red FAIL)" "$name" "$actual" "$expected"
    fail=$((fail+1)); continue
  fi

  ms=$(median_ms "$file")
  kb=$(peak_kb "$file")
  results+=("$name	$ms	$kb")

  delta=""
  if [ "$save" -eq 0 ] && [ -f "$BASELINE" ]; then
    base_ms=$(awk -v n="$name" '$1==n {print $2}' "$BASELINE")
    if [ -n "$base_ms" ] && [ "$base_ms" -gt 0 ]; then
      pct=$(( (ms - base_ms) * 100 / base_ms ))
      if [ "$pct" -le -"$NOISE" ]; then delta=$(green "${pct}%")
      elif [ "$pct" -ge "$NOISE" ]; then delta=$(red "+${pct}%")
      else delta=$(grey "~"); fi
    fi
  fi
  printf '%-20s %7sms %9skB %19s\n' "$name" "$ms" "$kb" "${delta:-$(grey -)}"
done

if [ "$save" -eq 1 ]; then
  printf '%s\n' "${results[@]}" > "$BASELINE"
  echo
  echo "baseline written to $BASELINE"
elif [ ! -f "$BASELINE" ]; then
  echo
  echo "no baseline yet — run: tests/bench.sh --save"
fi

[ "$fail" -eq 0 ]
