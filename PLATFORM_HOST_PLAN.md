# Platform host plan

How rocflight runs an app on a real platform by calling the platform's **own compiled
host**, instead of re-implementing its effects in Rust. First target: the `x64musl`
hosts every basic-cli release ships.

Everything below was measured on 2026-09-17 against basic-cli 0.22.0
(`F1JVZPYfWP71s8vk6tHcV1Qx1Ef6CZkwswGoCn8VHZmL`) and the pinned nightly
(`nightly-2026-09-03-62fcb65`), not estimated.

---

## 1. What is wrong today

`src/platform/real.rs` reads a platform's `.roc` sources and then stops: `IMPLEMENTED`
(`real.rs:345`) lists four effects rocflight wrote itself — `Stdout.line!`,
`Stdout.write!`, `Stderr.line!`, `Stderr.write!` — and everything else the platform
declares is reported as a gap "which this interpreter cannot call". The module header
calls that limit architectural.

It is not. `roc --opt=interpreter` is an interpreter that calls this exact host, and it
works here: Snake (`tests/roc/examples/Snake`), with its version pin edited to the
installed nightly, built in **128 ms** into a 4.7 MB statically-linked musl ELF and ran.

So the four native arms are a clone of 4 of the host's 60 functions, and the other 56
are gaps by construction. This plan deletes the clone and calls the host.

## 2. What a platform actually ships

The release tarball, as `roc` extracts it into `~/.cache/roc/packages/<hash>/`:

| Part | Where | What it is |
|---|---|---|
| Public modules | `Stdout.roc`, `Tty.roc`, `Stdin.roc`, … (18) | Ordinary Roc. `Tty.enable_raw_mode! = \|\| Host.tty_enable_raw_mode!()` |
| Host declarations | `Host.roc` | Annotation-only members with the ABI-safe types: `stdin_bytes! : () => Try(List(U8), [EndOfFile, StdinErr(IOErr)])` |
| Symbol map | `main.roc` `hosted { "hosted_stdin_bytes": Host.stdin_bytes!, … }` | 60 entries, in dispatch order |
| Entry point | `main.roc` `provides { "roc_main": main_for_host! }` | Roc: runs `main!`, maps `Ok({})`→0, `Err(Exit(n))`→n, else prints and 1 |
| Link recipe | `main.roc` `targets: { x64musl: { inputs: ["crt1.o", "libhost.a", "libunwind.a", app, "libc.a"] } }` | Per target, in link order; `app` is the slot the Roc side fills |
| The host | `targets/x64musl/libhost.a` (+ `crt1.o`, `libc.a`, `libunwind.a`) | Static musl archives. **No `.so` anywhere.** |

`nm targets/x64musl/libhost.a` is the whole contract:

- **Host defines:** `main`, `roc_alloc`, `roc_dealloc`, `roc_realloc`, `roc_dbg`,
  `roc_expect_failed`, `roc_crashed`, and all 60 `hosted_*` C functions.
- **Host imports exactly one symbol:** `roc_main(args: RocList<OsStr>) -> i32`.

The host is `main`. It builds `args`, calls `roc_main`, and exits with what comes back.
The `app` slot in the link recipe is whatever provides `roc_main`. Under `roc` that is a
shim plus the compiled program; under this plan it is **rocflight itself**.

## 3. The shape this forces

**`libhost.a` cannot be called from a running glibc process.** It is a static archive
built against musl: an archive cannot be `dlopen`ed, and re-linking it into a `.so`
would put musl's std inside a glibc process. `roc` does not try — `roc main.roc` links
a fresh static executable and execs it. rocflight does the same:

```
rocflight main.roc                        the driver: today's binary, glibc, unchanged for
   │                                      apps that name no platform
   ├─ no `platform "https://…"`  →  run in-process, exactly as now
   │
   └─ platform present
        ├─ locate ~/.cache/roc/packages/<hash>/   (already done, real.rs)
        ├─ cache hit on <hash>?  →  exec it
        └─ else link once, per PLATFORM, not per app:
              zig cc -target x86_64-linux-musl -static -nostdlib
                 crt1.o libhost.a libunwind.a  hosted_table.o librocflight.a  libc.a
              -o ~/.cache/rocflight/<hash>/app
           then exec it with ROCFLIGHT_APP=<abs path to main.roc>
```

Three consequences, none avoidable:

1. **rocflight is also a library.** `host/` is a `staticlib` crate over the same
   library, built once for `x86_64-unknown-linux-musl`; `librocflight_host.a` exports
   `roc_main`. The driver finds it beside its own executable (or `ROCFLIGHT_LIB`).
   Prerequisites on a dev machine: `rustup target add x86_64-unknown-linux-musl` and
   `zig` (present at `/usr/bin/zig`; it is what basic-cli's own CI links with). The
   link is `zig ld.lld` directly, as `roc`'s is an embedded lld: the host's and
   rocflight's Rust standard libraries both define `rust_eh_personality`, and
   `--allow-multiple-definition` is a flag `zig cc`'s driver does not pass through.
2. **One `unsafe` crate.** Calling a C function with Roc-layout arguments is FFI.
   `#![forbid(unsafe_code)]` stays on the library; `host/` is the one crate without
   it, and what it does unsafely is listed in its header: dereference addresses the
   host's `roc_alloc` returned, and call function pointers from the host's table.
3. **The linked executable is per platform.** The app path travels in an environment
   variable, so editing a `.roc` file never re-links anything — the edit-run loop stays
   what it is today. `argv` passes through untouched: the host builds `args` from it.

## 4. How the Roc side already fits

The platform's public modules are ordinary Roc, and rocflight already runs ordinary Roc
from a module directory ahead of the app: that is what `Builtin.roc`'s members and
local `import`s go through (`vm::compile::Module`, `src/main.rs` step 2b2/2c). Platform
modules take the same path, rooted at the platform's cache directory instead of the
app's.

`Host.roc` is the interesting one: every member has a type and no body. That is the
**same split `Builtin.roc` has** — an annotation-only member is an intrinsic that Rust
supplies. For `Builtin.roc` the supplier is `src/eval`; for `Host.roc` the supplier is
the `hosted` table. Nothing new in the compiler: a bodiless `Host.x!` compiles to a
call the way a bodiless `Str.concat` does, and the dispatcher routes it by the symbol
its `hosted` entry names.

`roc_main` then does one thing: run `main_for_host!` (named by `provides`) with the
host's `args`, and return its `I32`.

## 5. The ABI, exactly

Sizes are 64-bit. Sources: `roc-compiler/src/builtins/{str,list}.zig`,
`src/layout/{store,field_order}.zig`, and basic-cli's generated
`src/roc_platform_abi.rs`.

| Type | Layout |
|---|---|
| `Str` | 24 B: `bytes: *u8`, `capacity_or_alloc_ptr: usize`, `length: usize` — **in that order**. Small string: the high bit of the *last byte* is set and the low 7 bits of that byte are the length; the 23 preceding bytes hold the text. Big string: `capacity` stored shifted left by one; low bit set means seamless slice. |
| `List(a)` | 24 B: `bytes: *a`, `length: usize`, `capacity_or_alloc_ptr: usize` — **a different order from `Str`**. |
| Refcount | An `isize` stored immediately **before** the allocation `bytes` points at. `1` = uniquely owned (`utils.zig` `rcUnique`); `0` = static, never freed (`REFCOUNT_STATIC_DATA`). Allocate with the host's `roc_alloc(len, align)`; free with `roc_dealloc(ptr, align)`. |
| Records | Fields sorted by **descending alignment, then alphabetical name** (`field_order.zig`). Source order never reaches memory. |
| Tag unions | Tags sorted **alphabetically**; discriminant index is the sorted position. Payload first (a union of the sorted payloads), discriminant **after** it; discriminant width 0/1/2/4/8 bytes by tag count; whole thing padded to its alignment. Payload-less unions are just the discriminant (`[EndOfFile, …]` with no payload → 1 B). |
| `Try(ok, err)` | Is `[Ok(ok), Err(err)]` in `Builtin.roc:5257`, so **`Err` = 0, `Ok` = 1**. |
| `OsStr` | `[Utf8(Str), UnixBytes(List(U8)), WindowsU16s(List(U16))]` → sorted `UnixBytes`=0, `Utf8`=1, `WindowsU16s`=2; 24 B payload + discriminant, padded to **32 B**. |
| `Box(a)` | A pointer (8 B), refcounted like a list. `FileReader`, `SqliteStmt`, `TcpStream` are `Box(U64)` — opaque handles; pass through untouched. |
| Numbers | `I32`/`U64`/`I128`/`F64`/`Dec` at their C sizes; `Bool` is a byte; `{}` is zero-sized. |

The **declared** signature in `Host.roc` drives every layout — never the runtime shape
of a `Value`. That is what keeps this a layout *store* and not a guess.

## 6. Calling a function whose signature is only known at runtime

`roc` uses a fixed assembly trampoline (`src/eval/host_trampoline.zig`, "libffi-style").
rocflight does not need one. On x86-64 SysV:

- any argument or return **larger than 16 bytes** is passed **by pointer**, and returned
  through a hidden first pointer (`sret`);
- anything ≤ 8 bytes travels in one integer register, ≤ 16 in two.

Every `Str`, `List`, record and payload-carrying union above is > 16 B, so the 60
hosted functions collapse onto a handful of `extern "C"` shapes:

```rust
fn(sret: *mut u8)                                   // () => big
fn(sret: *mut u8, a: *const u8)                     // big => big
fn(sret: *mut u8, a: *const u8, b: *const u8)       // big, big => big
fn(a: u64) -> u64                                   // sleep_millis! : U64 => {}  (and friends)
fn() -> u64 / -> [u64; 2]                           // random_seed_u64!, utc_now! : () => I128
```

The classifier is a `match` on `(arg sizes, return size)`; the dispatcher picks the
shape and transmutes the table pointer to it. Phase 3 enumerates all 60 against the
classifier and fails the build if one does not fit.

> `ponytail:` x86-64 SysV only. aarch64 differs (`x8` for sret, homogeneous-aggregate
> rules); the upgrade is a second classifier, or libffi. Floats in SSE registers are
> not needed by basic-cli and are rejected explicitly rather than mis-passed.

**Ownership:** hosted functions **take** their arguments (the host's `RocStr` drops
them) and **give** their results. rocflight allocates fresh args through `roc_alloc`,
never reuses one, converts each result to a `Value`, then `roc_dealloc`s what the
result owned. Phase 2's gate is a round-trip that leaves the host's allocator balanced.

## 7. The per-platform table

rocflight must not know a platform's symbol names at compile time. At link time the
driver writes one C file from the `hosted` block, in dispatch order:

```c
extern void hosted_cmd_host_exec_exit_code(void); /* … 59 more … */
const char  *const rocflight_hosted_names[] = { "hosted_cmd_host_exec_exit_code", /* … */ };
const void  *const rocflight_hosted_fns[]   = {  hosted_cmd_host_exec_exit_code,  /* … */ };
const unsigned    rocflight_hosted_count    = 60;
```

compiled beside the library, exposed as three accessor functions
(`rocflight_hosted_count/name/fn`). The library looks a `Host.x!` member up by the
symbol its `hosted` entry names. This is what `roc`'s own `generatePlatformHostShim`
emits ("hosted dispatch table symbols, ordered by dispatch index"), minus the codegen.

## 8. `dbg`, `expect`, `crash`

Route them to the host, which owns the process: `dbg` → `roc_dbg`; a failed inline
`expect` → `roc_expect_failed` (the host then turns an otherwise-0 exit into 1, as
`roc` does); `crash` → `roc_crashed`, which does not return. Top-level `expect`s stay
`rocflight test`'s business and never reach the host.

## 9. Phases

Each phase lands green on `cargo test`, `check_roc.sh --strict` and `check_examples.sh`.

| | Phase | Gate |
|---|---|---|
| ~~**P0**~~ **done** | The 8 examples skipped for "roc version mismatch" are only a **pin mismatch** (`nightly-2026-09-12` in their headers, `09-03` installed). `check_examples.sh` runs each from a scratch copy with the pin rewritten to `roc version`'s; they are mode `host`, PENDING on mismatch, fatal under `--strict`. | **met** 2026-09-17: `roc` runs all 8; gate is 20 ok, 7 pending, 1 skip. CommandLineArgsFile already matches. 4 of the 7 are blocked before the host — `ImportPackageFromModule`/`RandomNumbers` on package imports, `LoopEffect`/`Parser` on inference — so P5's gate needs those tracks too. |
| ~~**P1**~~ **done** | **Layout store.** `src/platform/layout.rs`: size, alignment, sort class, field order, discriminant offset/width for every type in §5, computed from rocflight's `Type`, with declared names (`IOErr`, `NativeOsStr`) resolved through `Declarations`. Pure Rust, no unsafe. | **met** 2026-09-17: 10 tests. Six pin `roc glue`'s generated sizes — `Str` 24, `OsStr` 32 (tags `UnixBytes`,`Utf8`,`WindowsU16s`), `IOErr` 32, `Try(I32, IOErr)` 40 with `Err`=0, basic-cli's 14-field `Cmd` 160 in exact memory order, `CmdOutputFailure` 56 — and the stdin/main results are rule-derived from those. |
| ~~**P2**~~ **done** | **Marshalling.** `src/platform/marshal.rs`: `write_value`/`read_value`/`release` over a `Heap` trait (`alloc`/`dealloc`/`read`/`write` with `roc_alloc`'s block-and-header convention from `utils.zig`), plus `FakeHeap`, an in-process heap that counts. No unsafe. | **met** 2026-09-17: 12 tests round-trip every §5 shape — small and heap strings, shared and static refcounts, seamless slices, lists of scalars and of strings (element count stored before the refcount), reordered records, tuples, every `Try` and tag form, `List(OsStr)` as `main!` receives it — and the heap is empty after each. |
| ~~**P3**~~ **done** | **FFI + `roc_main`.** `host/`, a separate `staticlib` crate — the ONLY crate without `#![forbid(unsafe_code)]`; the library keeps it. `src/platform/abi.rs` (safe) classifies a signature into register words + a stack image (§6); `host/src/lib.rs` makes the call with two `extern "C"` types, implements `Heap` over `roc_alloc`, exports `roc_main`, and routes `dbg`/`expect` to `roc_dbg`/`roc_expect_failed` and a crash to `roc_crashed` (§8). The pipeline moved into the library as `run::run_file` so `roc_main` and `main.rs` share it. | **met** 2026-09-17, against basic-cli's REAL host: `tests/check_host.sh` builds the musl library (exports exactly `roc_main`), links it with `crt1.o libhost.a libunwind.a <table> libc.a` via lld, and runs it. Selftest calls seven hosted functions through every shape — `Str` on the stack, a 32-byte `NativeOsStr` on the stack, `sret` results with nested unions and strings/lists inside, a 16-aligned `Try(U128, …)`, a register `U64`, `stdin_bytes` at EOF — all correct. `roc_main` runs apps with the host's real `args`: `ok.roc` exits 0, `exit3.roc` exits 3, `dbg.roc` prints `[ROC DBG] 3` through `roc_dbg` and exits 1 — byte-identical to `roc`. `tests/host_abi_test.rs`: all 60 basic-cli signatures classify. |
| ~~**P4**~~ **done** | **Driver.** `src/platform/driver.rs`: an app that names a platform → `librocflight_host.a` beside the binary (or `ROCFLIGHT_LIB`) → cache by package hash + library → generate the hosted table → `zig cc -c` + `zig ld.lld` per the platform's `targets:` recipe → exec with `ROCFLIGHT_APP` and the app's arguments. `src/platform/modules.rs` loads the platform's own modules as Roc, transitively through their imports, only what the app and the entry use; `Host.roc`'s bodiless members register in `platform::hosted` with their laid-out signatures; the platform's `main_for_host!` is the entry inside the host, so the exit code is the platform's Roc. `test` and the debug flags stay in-process. | **met** 2026-09-17: **Snake runs on basic-cli's real host and matches `roc` byte for byte** (`check_examples.sh`); `19_platform/basic_cli` pair passes through the host. Cold link **106 ms**, warm run **8 ms**. Checker fixes it took: `\|\| body` is `{} -> body`; `f! : () => {}` peels its unit parameter; the `{}` pattern is the unit type; a type named before its declaration resolves instead of standing as a shared placeholder; annotated top-level names are bound before any body is checked. The last two were found by PLAYING Snake against `roc` — the examples gate now replays a whole game key by key. 11 of basic-cli's 18 modules load: `Path` (and `Cmd`, `Env`, `File`, `Sqlite` through it) need a string literal to become a nominal via `from_str`, `Locale` a `{ raw: Str }` nominal literal, `Sleep` `seconds * 1000` on an `F64`, `Http` a package. Those are language gaps, and they now show as PENDING with their real error. |
| ~~**P5**~~ **done** | **Delete the clone.** `IMPLEMENTED`, `is_implemented`, `describe_gap`, `host_artifacts`, the four `Stdout`/`Stderr` arms in `src/eval/mod.rs`, their two tests, and the `real.rs` header's "architectural" paragraph — 90 lines. rocflight now implements none of a platform's effects; a declared effect that cannot be laid out says so and points at `--show-platforms`. | **met** 2026-09-17 as far as the host goes: nothing a platform provides is re-implemented, and every effect an example reaches runs on the host. The seven still PENDING are the language gaps P4 listed (string literal as a nominal, package imports, two inference cases) — the original "every example matches" wording promised more than a host plan can deliver, and the examples gate reports each one's real error instead. |

P1 and P2 are the bulk of the work and are pure, testable Rust; P3 is small but is
where every mistake in P1/P2 shows up as a segfault, which is why they come first.

All five landed on 2026-09-17. What is left is not the host's: the language gaps
the platform's own modules exposed, listed under P4.

## 10. What this does not do, on purpose

- **Other targets.** `x64mac` links against `libSystem`, `x64win` needs a COFF linker
  and 15 import libraries. Same plan, different recipe from the `targets:` block; not in
  this pass.
- **Embedding the app.** `roc` bakes the program into the executable; rocflight reads
  it at run time from `ROCFLIGHT_APP`. Per-platform caching is the whole reason the
  edit-run loop stays free.
- **`rocflight test` on a platform app.** Top-level `expect`s do not need the host, so
  `test` stays in-process even when a platform is named — `roc test` behaves the same.
- **Performance of the in-process path.** Untouched: nothing here runs unless the app
  names a platform.
