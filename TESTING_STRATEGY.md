# Testing Strategy

Two gates define parity, and one of them is not written here at all.

## roc's own eval tests, with rocflight as a backend

`roc-compiler/src/eval/test/` holds roc's data-driven eval tests — 2,299 of them on the
pinned checkout, each a source with an expected `Str.inspect` string, an expected
compile problem, or an expected crash. roc's runner (`parallel_runner.zig`) runs every
one through its interpreter, its dev backend and wasm, in forked children, and requires
the strings to agree. rocflight is wired in as a **fifth backend**: `--rocflight
<binary>` (or `$ROCFLIGHT`) makes the runner exec `rocflight eval` on each test's
source and hold it to the same comparison. Nothing in this repository decides what
"agrees with roc" means; roc's harness does, on roc's tests.

```bash
tests/check_eval.sh              # build the runner if needed, run all, report rocflight's tally
tests/check_eval.sh --strict     # full parity required: rocflight passes every test
tests/check_eval.sh --filter Dict
```

**Full feature parity is `--strict` green — and it is.** The tally as of 2026-09-19:
**1,953 of 1,953** backend-run tests pass through rocflight (957 before the plan's
phases), and all 72 problem tests are rejected as roc rejects them.
`tests/check_eval.sh --report` says why each miss missed. The gaps are catalogued by kind in
[IMPLEMENTATION_PHASES.md → Phase 23](IMPLEMENTATION_PHASES.md#phase-23-rocs-eval-tests-as-a-backend),
and [EVAL_PARITY_PLAN.md](EVAL_PARITY_PLAN.md) is the phased plan for closing them:
phases 8 to 17, one per remaining family, each with its own `--filter` check.

`rocflight eval FILE` is the harness's entry point: it prints `Str.inspect` of the
module's value and exits 0, or prints an error NAME and exits 2 — `Crash` for a crash
test, `CompileError` for a problem test. A `.module` test gets a bare `main` appended,
because a module's value in rocflight is its last top-level statement.

## Golden pairs

The per-feature protocol is in one place:

**[PHASE_IMPLEMENTATION_GUIDE.md → The golden-pair rule](PHASE_IMPLEMENTATION_GUIDE.md#the-golden-pair-rule)**

In short: every syntax feature gets its own pair of `.roc` files —

```
tests/roc/<NN>_<phase>/<syntax>.roc              # sugared
tests/roc/<NN>_<phase>/<syntax>.desugared.roc    # explicit types, no sugar
```

— and **all four of these outputs must be byte-identical**:

```
        roc run <sugared>   ═══   roc run <desugared>
              ║                          ║
     rocflight <sugared>   ═══   rocflight <desugared>
```

Both files must also pass `roc check`. The **desugared** file carries explicit type
annotations; the **sugared** file carries none — every type is inferred. Enforced by:

```bash
tests/check_roc.sh            # test-file correctness fatal, interpreter gaps as PEND
tests/check_roc.sh --strict   # interpreter parity required — the definition of done
```

Plus `cargo test --quiet` for the Rust side, and

```bash
tests/check_examples.sh       # every example from roc-lang.org/examples, roc vs rocflight
```

which is the outside-in check: code written by the language's own authors, not for
this interpreter. See [Phase 22](IMPLEMENTATION_PHASES.md#phase-22-the-languages-own-examples).

Performance has its own gate, measured rather than argued:

```bash
cargo build --release
tests/bench.sh --save    # baseline before a change
tests/bench.sh           # what the change did
tests/bench_compare.sh   # the same programs under roc's own interpreter and dev backend
```

See [OPTIMIZATION_PLAN.md](OPTIMIZATION_PLAN.md).

---

## Why this file shrank

It described a flat `tests/roc/phaseN_*_test.roc` layout, one file per *phase*, with
`expect` assertions inside `main!`. Three problems:

1. Every one of those files failed `roc check`. They were never compiled.
2. One file per phase cannot satisfy "each syntax feature gets its own test file".
3. It duplicated the workflow in PHASE_IMPLEMENTATION_GUIDE.md, and the two drifted
   apart.

It also contained example `roc repl` transcripts that do not match the real REPL
(`:type` output, `Try(Ok(5), Err("error"))` as a value). Pin types with the LSP or
by reading `roc check`'s expected-type output instead — see the guide.
