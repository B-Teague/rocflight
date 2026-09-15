# Testing Strategy

Moved. The testing protocol is now one thing, in one place:

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
