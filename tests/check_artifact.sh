#!/usr/bin/env bash
# The checked-in `Builtin.artifact` is `Builtin.roc` already parsed. Regenerate it and
# check nothing moved — the gate that stops it rotting against the source or the AST.
#
#   tests/check_artifact.sh
#
# `build.rs` already refuses to build against an artifact whose recorded SOURCE hash has
# drifted. This is the other half: that the parser still produces the same trees, which
# a change to the AST or the parser can break without touching Builtin.roc.
set -uo pipefail
cd "$(dirname "$0")/.."

artifact=src/roc/Builtin.artifact
before=$(mktemp) && cp "$artifact" "$before"
trap 'rm -f "$before"' EXIT

if ! cargo run --release --quiet --bin gen-artifact >/dev/null 2>"$before.log"; then
  echo "artifact gate FAILED: gen-artifact did not run" >&2
  cat "$before.log" >&2
  exit 1
fi

if cmp -s "$before" "$artifact"; then
  echo "artifact gate passed: $(wc -c < "$artifact") bytes, unchanged by regeneration"
  exit 0
fi

echo "artifact gate FAILED: src/roc/Builtin.artifact is out of date." >&2
echo "  It has been regenerated in place — review and commit it." >&2
echo "  was $(wc -c < "$before") bytes, now $(wc -c < "$artifact") bytes" >&2
exit 1
