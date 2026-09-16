#!/usr/bin/env bash
# How much of the language the VM covers, counted rather than guessed.
#
#   tests/vm_coverage.sh            every golden pair and example
#   tests/vm_coverage.sh tests/roc/16_loops/*.roc     just these
#
# Each file is compiled with `--vm`. A file that compiles AND runs is checked against
# the tree-walker's output, byte for byte; a file the VM refuses is counted under the
# construct it refused. The histogram at the end is what decides which phase is worth
# doing next — V2's "match" is 48 files, and no amount of opinion moves that number.
#
# This is a REPORT, not a gate. The gate is `cargo test --test vm_test`, which runs
# programs on both engines and requires the same answer. Until V4 lands, nothing here
# can reach "ran": every golden pair prints with `echo!`, which is a builtin.
set -uo pipefail
cd "$(dirname "$0")/.."
ROCFLIGHT=${ROCFLIGHT:-$PWD/target/release/rocflight}

if [ ! -x "$ROCFLIGHT" ]; then
  echo "error: $ROCFLIGHT not built — run: cargo build --release" >&2
  exit 1
fi

files=("$@")
[ ${#files[@]} -eq 0 ] && files=(tests/roc/*/*.roc tests/roc/examples/*.roc)

ran=0 diverged=0 refused=0
declare -A why

for f in "${files[@]}"; do
  [ -f "$f" ] || continue
  vm=$("$ROCFLIGHT" --vm "$f" 2>&1)
  if [[ "$vm" == *"Error: vm:"* ]]; then
    refused=$((refused + 1))
    reason=$(printf '%s\n' "$vm" | sed -n 's/.*vm: unsupported in V5: //p; s/.*vm: `[^`]*` is not defined.*/a builtin (V4)/p' | head -1)
    why[${reason:-something else}]=$(( ${why[${reason:-something else}]:-0} + 1 ))
  elif [ "$vm" = "$("$ROCFLIGHT" "$f" 2>&1)" ]; then
    ran=$((ran + 1))
  else
    diverged=$((diverged + 1))
    echo "DIVERGED: $f"
  fi
done

echo
printf 'ran identically  %4d\n' "$ran"
printf 'DIVERGED         %4d\n' "$diverged"
printf 'refused          %4d\n' "$refused"
echo
echo "what the VM refused, most blocking first:"
for reason in "${!why[@]}"; do printf '%6d  %s\n' "${why[$reason]}" "$reason"; done | sort -rn

[ "$diverged" -eq 0 ]
