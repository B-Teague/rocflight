#!/usr/bin/env bash
# Links rocflight into basic-cli's REAL compiled host and runs it.
#
# `host/` is rocflight built as a static library exporting `roc_main`. The platform's
# `main.roc` says how it links (`targets: { x64musl: { inputs: [...] } }`), and its
# `hosted { … }` block says which C symbols the host provides, in dispatch order; a
# table generated from that block is the only platform-specific code, and it is
# generated, never written. Then:
#
#   1. ROCFLIGHT_SELFTEST=1  calls real hosted functions through every ABI shape;
#   2. ROCFLIGHT_APP=ok.roc     runs an app to Ok({}): exit 0;
#   3. ROCFLIGHT_APP=dbg.roc    a `dbg` reaches the host's roc_dbg, which exits 1;
#   4. ROCFLIGHT_APP=exit3.roc  `Err(Exit(3))` becomes exit code 3;
#   5. `rocflight file.roc` itself: links (cold), reuses (warm), runs `Stdout.line!`
#      through the platform's own Roc and host;
#   6. the release binary alone, copied away with nothing beside it.
#
# Needs: zig (`zig cc` is the linker, as in basic-cli's own CI), the Rust musl
# target (`rustup target add x86_64-unknown-linux-musl`), and the platform in roc's
# cache (`roc check tests/host/ok.roc` fetches it). See PLATFORM_HOST_PLAN.md.
#
# Usage: tests/check_host.sh
set -uo pipefail
cd "$(dirname "$0")/.."
TARGET=x86_64-unknown-linux-musl
ZIG=${ZIG:-zig}

red()   { printf '\033[31m%s\033[0m' "$1"; }
green() { printf '\033[32m%s\033[0m' "$1"; }
fail=0
check() { # name, expected exit, actual exit, output-to-show
  if [ "$2" = "$3" ]; then printf '  %s %-28s %s\n' "$(green ok)" "$1" "$4"
  else printf '  %s %-28s exit %s, wanted %s\n%s\n' "$(red FAIL)" "$1" "$3" "$2" "$4"; fail=$((fail+1)); fi
}

url=$(grep -o 'https://[^"]*\.tar\.zst' tests/host/ok.roc | head -1)
hash=$(basename "$url" .tar.zst)
P=$HOME/.cache/roc/packages/$hash/targets/x64musl
if [ ! -f "$P/libhost.a" ]; then
  echo "platform $hash is not in roc's cache — run: roc check tests/host/ok.roc"; exit 2
fi

echo "building librocflight_host.a for $TARGET"
cargo build -p rocflight-host --release --target "$TARGET" 2>&1 | grep -E "^(error|warning)" -A4
lib=target/$TARGET/release/librocflight_host.a
[ -f "$lib" ] || { echo "no $lib"; exit 1; }

# The library must export exactly what the host imports: roc_main and nothing else
# in the host's namespace.
exported=$(nm "$lib" | awk '$2 == "T" && $3 ~ /^roc_/ { print $3 }' | sort -u | tr '\n' ' ')
check "exports roc_main only" "roc_main " "$exported" "$exported"

out=$(mktemp -d); trap 'rm -rf "$out"' EXIT

# The dispatch table, from the platform's own hosted block, in its order.
names=$(awk -F'"' '/^\t\t"hosted_/ { print $2 }' "$HOME/.cache/roc/packages/$hash/main.roc")
{
  for n in $names; do echo "void $n(void);"; done
  echo "static const char *const names[] = {"; for n in $names; do echo "  \"$n\","; done; echo "};"
  echo "static const void *const fns[] = {";   for n in $names; do echo "  (const void *)$n,"; done; echo "};"
  echo "unsigned rocflight_hosted_count(void) { return sizeof(names) / sizeof(names[0]); }"
  echo "const char *rocflight_hosted_name(unsigned i) { return names[i]; }"
  echo "const void *rocflight_hosted_fn(unsigned i) { return fns[i]; }"
} > "$out/table.c"

# The platform's link recipe, `app` filled with the table and the library.
#
# Linked with lld directly, as `roc` does (its linker is an embedded lld). Two Rust
# standard libraries meet here, the host's and rocflight's, and both define
# `rust_eh_personality`; --allow-multiple-definition keeps the first, and `zig cc`'s
# driver does not pass that flag through. -O2 on the table so zig's debug-mode UBSan
# is not linked in.
echo "linking with $P"
if ! "$ZIG" cc -target x86_64-linux-musl -O2 -c -o "$out/table.o" "$out/table.c" 2>"$out/link.err" \
   || ! "$ZIG" ld.lld -static --gc-sections --allow-multiple-definition \
        -o "$out/app" "$P/crt1.o" "$P/libhost.a" "$P/libunwind.a" "$out/table.o" "$lib" "$P/libc.a" 2>>"$out/link.err"; then
  echo "link failed:"; head -30 "$out/link.err"; exit 1
fi
check "links" 0 0 "$(ls -l "$out/app" | awk '{print $5 " bytes"}')"

st=$(ROCFLIGHT_SELFTEST=1 "$out/app" </dev/null 2>&1); code=$?
check "selftest" 0 "$code" "$(printf '%s\n' "$st" | sed 's/^/      /')"
# The host answers an env lookup with the variable's raw bytes: `UnixBytes([49])` is "1".
printf '%s\n' "$st" | grep -q "^selftest hosted_env_var -> Ok(UnixBytes(\[49\]))" || { echo "  env_var did not read ROCFLIGHT_SELFTEST=1"; fail=$((fail+1)); }
printf '%s\n' "$st" | grep -q "^selftest hosted_env_var -> Err(VarNotFound(UnixBytes(" || { echo "  env_var did not report a missing variable"; fail=$((fail+1)); }
printf '%s\n' "$st" | grep -q "^selftest hosted_utc_now -> Ok([0-9]\{15,\})" || { echo "  utc_now did not come back through sret"; fail=$((fail+1)); }
printf '%s\n' "$st" | grep -q "^selftest hosted_stdin_bytes -> Err(EndOfFile)" || { echo "  stdin_bytes at EOF should be Err(EndOfFile)"; fail=$((fail+1)); }

ok=$(ROCFLIGHT_APP="$PWD/tests/host/ok.roc" "$out/app" </dev/null 2>&1); code=$?
check "ok.roc exits 0" 0 "$code" "$(printf '%s' "$ok" | head -3)"

# basic-cli's host exits 1 for a program that called `dbg` (its `rust_main` turns a
# 0 into a 1 once `roc_dbg` has been called), and `roc` agrees; so does this.
db=$(ROCFLIGHT_APP="$PWD/tests/host/dbg.roc" "$out/app" one two </dev/null 2>&1); code=$?
check "dbg.roc exits 1 via roc_dbg" 1 "$code" "$(printf '%s' "$db" | head -3)"
printf '%s\n' "$db" | grep -q "3" || { echo "  dbg of List.len(args) should have reached roc_dbg with 3 (program + two)"; fail=$((fail+1)); }

e3=$(ROCFLIGHT_APP="$PWD/tests/host/exit3.roc" "$out/app" </dev/null 2>&1); code=$?
check "exit3.roc exits 3" 3 "$code" "$(printf '%s' "$e3" | head -3)"

# The driver: `rocflight file.roc` links the platform executable itself (into a
# scratch cache here, so this is a COLD link) and execs it; the second run is warm.
export ROCFLIGHT_LIB=$PWD/$lib XDG_CACHE_HOME=$out/cache
cargo build --quiet 2>&1 | grep -E "^(error|warning)" -A4
t0=$(date +%s%N)
d3=$(./target/debug/rocflight tests/host/exit3.roc </dev/null 2>&1); code=$?
cold=$(( ($(date +%s%N) - t0) / 1000000 ))
check "driver: exit3.roc cold link" 3 "$code" "${cold} ms, $(printf '%s' "$d3" | head -2)"
t0=$(date +%s%N)
d3=$(./target/debug/rocflight tests/host/exit3.roc </dev/null 2>&1); code=$?
warm=$(( ($(date +%s%N) - t0) / 1000000 ))
check "driver: exit3.roc warm" 3 "$code" "${warm} ms"
[ "$(ls "$out/cache/rocflight" | wc -l)" = 1 ] || { echo "  expected one cached executable"; fail=$((fail+1)); }
dd=$(./target/debug/rocflight tests/host/dbg.roc one two </dev/null 2>&1); code=$?
check "driver: dbg.roc via roc_dbg" 1 "$code" "$(printf '%s' "$dd" | head -1)"
printf '%s\n' "$dd" | grep -q "^\[ROC DBG\] 3$" || { echo "  expected [ROC DBG] 3"; fail=$((fail+1)); }
# The whole point: a platform effect runs on the platform's host.
hello=$(./target/debug/rocflight tests/roc/19_platform/basic_cli.roc </dev/null 2>&1); code=$?
check "driver: Stdout.line! on the host" 0 "$code" "$hello"
[ "$hello" = "hello from a real platform" ] || { echo "  wrong output: $hello"; fail=$((fail+1)); }

# The release binary on its own: copied somewhere with nothing beside it, no
# ROCFLIGHT_LIB, an empty cache. It carries the library (build.rs) and extracts it.
cargo build --release --quiet 2>&1 | grep -E "^(error|warning)" -A4
mkdir -p "$out/bin" "$out/cache2" && cp target/release/rocflight "$out/bin/"
alone=$(cd tests/host && env -u ROCFLIGHT_LIB XDG_CACHE_HOME="$out/cache2" "$out/bin/rocflight" exit3.roc </dev/null 2>&1); code=$?
check "standalone release binary" 3 "$code" "$(printf '%s' "$alone" | head -2)"
[ -f "$out"/cache2/rocflight/lib-*/librocflight_host.a ] || { echo "  expected the embedded library extracted into the cache"; fail=$((fail+1)); }

echo
[ "$fail" -eq 0 ] && echo "host gate passed" || echo "$fail failed"
[ "$fail" -eq 0 ]
