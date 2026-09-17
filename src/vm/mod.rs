//! A register VM for Roc, in safe Rust. The engine.
//!
//! It ran alongside a tree-walking interpreter for six phases, gated against it at
//! every step; `OPTIMIZATION_PLAN.md` has the measurements and the order they landed
//! in. The tree-walker is gone, and this runs every program.
//!
//! Four properties are load-bearing and worth stating before the code:
//!
//! 1. **No `unsafe`.** The crate is `#![forbid(unsafe_code)]`. Register access is
//!    bounds-checked indexing, dispatch is a `match`, and frames are indices rather
//!    than pointers. A wrong opcode is a panic with a message, not memory corruption.
//! 2. **Names are resolved at compile time.** A local is a register index, a captured
//!    variable is a capture index, a top-level value is a slot, a top-level function is
//!    a chunk id. Nothing compares a string at run time, which is the whole reason this
//!    is faster than walking the tree.
//! 3. **Calls do not recurse in Rust.** `frames` is an ordinary `Vec`, so Roc recursion
//!    costs ~32 bytes a level on the heap instead of a Rust stack frame, and going too
//!    deep is a Roc-level error rather than a stack overflow with no diagnostic. The
//!    exception is a builtin's callback, which re-enters through `call_closure` — see
//!    there.
//! 4. **A tail call reuses its frame.** Tail-recursive Roc runs in constant memory, so
//!    the idiomatic functional loop stops being a depth risk at all.

pub mod compile;

pub use compile::{compile, compile_unit};

use crate::ast::BinOp;
use crate::error::EvalError;
use crate::eval::Value;
use std::cell::RefCell;
use std::rc::Rc;

/// Top-level values, by slot.
///
/// Shared rather than owned: a builtin's callback (`xs.map(f)`) re-enters the VM
/// through `call_closure`, and that second machine has to see the same top level.
type Globals = Rc<RefCell<Vec<Option<Value>>>>;

thread_local! {
    /// The program and globals of the VM currently running, so `eval::apply` can call
    /// a `Value::Closure` from inside a builtin. A stack, because a callback can
    /// itself call a builtin that takes a callback.
    static RUNNING: RefCell<Vec<(Rc<Program>, Globals)>> = const { RefCell::new(Vec::new()) };
}

/// Run `f` against the program currently running, if there is one.
///
/// A borrow rather than a clone of the `Rc`: these lookups happen inside `Str.inspect`
/// and `==`, and the program is not going anywhere while an instruction of its own is
/// executing.
fn with_running<R>(f: impl FnOnce(&Program) -> R) -> Option<R> {
    RUNNING.with(|r| r.borrow().last().map(|(program, _)| f(program)))
}

/// Every `Type.method` function in the running program, for a given method name.
///
/// A nominal's method block compiles to top-level functions whose names carry a dot,
/// and the compiler indexed those by bare method name in `Program::methods_by_name` —
/// so this is a table lookup, not a scan of every chunk with a formatted suffix.
///
/// Used by the two builtins that dispatch on a user's own method: `Str.inspect` looking
/// for a `to_inspect`, and operator dispatch looking for `plus`/`is_eq`/…
pub fn methods_named(method: &str, receiver: &Value) -> Vec<(&'static str, Value)> {
    with_running(|program| {
        let Some(defined) = program.methods_by_name.get(method) else { return Vec::new() };
        defined
            .iter()
            // Only the nominals whose shape does not RULE the receiver out. Without
            // this a lone `Try.is_eq` in scope answered every `==`, tuples and unrelated
            // tags included, and `(1, "x") == (1, "x")` failed with "No match arm
            // matched".
            .filter(|(qualified, _)| match qualified.rsplit_once('.') {
                Some((owner, _)) => program
                    .nominal_shapes
                    .get(owner)
                    .is_none_or(|shape| shape.admits(receiver)),
                None => true,
            })
            .map(|(qualified, chunk)| {
                (*qualified, Value::Closure(Rc::clone(&program.chunks[*chunk as usize].bare)))
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Call a VM closure from outside the VM — from a builtin's callback.
///
/// `List.map` and friends are written against `eval::call_function`, so a callback
/// re-enters the VM here. The cost is that a callback nests a Rust frame: "calls do not
/// recurse in Rust" holds for Roc calls but NOT across a builtin's callback boundary,
/// so deeply nested `map`-inside-`map` is bounded by the Rust stack again. Lowering the
/// callback-taking builtins into bytecode is what removes the last such bound.
pub fn call_closure(closure: &Rc<Closure>, args: Vec<Value>) -> Result<Value, EvalError> {
    let context = RUNNING.with(|r| r.borrow().last().cloned());
    let (program, globals) = context.ok_or_else(|| EvalError {
        message: "vm: a closure was called with no VM running".to_string(),
    })?;
    // A fresh register file per callback. Pooling them across callbacks was measured
    // on `iter_range` (two million of these) and saved nothing: the allocation is not
    // where a callback's time goes. Lowering `fold` and `map` into bytecode is.
    let mut vm = Vm { program, regs: Vec::new(), frames: Vec::new(), globals, entry_args: None };
    vm.call(closure, args)
}

/// A register, relative to the frame's base.
pub type Reg = u16;

/// An index into `Program::chunks`.
pub type ChunkId = u32;

/// One instruction. Three-address: operands in, one destination out.
///
/// `Bin` carries its `BinOp` rather than there being one opcode per operator. Splitting
/// it into `AddInt`/`AddFrac`/`AddAny` using what the type checker proved is a
/// *measured* step for later; doing it now would mean two implementations of Roc's
/// arithmetic semantics before there is a single benchmark to prove it pays.
#[derive(Debug, Clone, Copy)]
pub enum Op {
    /// `dst = consts[k]`
    LoadK { dst: Reg, k: u32 },
    /// `dst = src`
    Move { dst: Reg, src: Reg },
    /// `dst = globals[idx]`
    LoadGlob { dst: Reg, idx: u32 },
    /// `globals[idx] = src`
    StoreGlob { idx: u32, src: Reg },
    /// `dst = this closure's captures[idx]`
    LoadCap { dst: Reg, idx: u16 },
    /// `dst = the closure that is running`.
    ///
    /// How a block-local function calls itself. The tree-walker rebinds a rebuilt
    /// closure into every call frame to tie that knot; the running closure is already
    /// in the frame, so this is a refcount bump and no analysis.
    LoadSelf { dst: Reg },
    /// `dst = a op b`
    Bin { dst: Reg, a: Reg, b: Reg, op: BinOp },
    /// `dst = a op b`, where the checker proved both operands are integers.
    ///
    /// Skips the dispatch on operand shapes that `Bin` pays on every execution: the
    /// answer to "what are these?" was known once, at compile time. Emitted only where
    /// `TypeChecker::integer_binops` says so, and it falls back to the generic path if
    /// a value turns out not to be an integer after all — being right matters more
    /// than the assumption being kept.
    BinInt { dst: Reg, a: Reg, b: Reg, op: BinOp },
    /// `ip = to`
    Jump { to: u32 },
    /// `if cond == False { ip = to }`. The condition must be a `Bool`, and `kind`
    /// decides only what a non-`Bool` is told — roc words the three cases differently
    /// and the tree-walker follows it, so the VM has to as well.
    JumpFalse { cond: Reg, to: u32, kind: CondKind },
    /// `dst = closure(chunk, regs[base..base + n])` — the captures are already in
    /// consecutive registers, put there by the enclosing function.
    MakeClosure { dst: Reg, chunk: ChunkId, base: Reg, n: u16 },
    /// `dst = chunk(regs[base..base + argc])`, the callee known at compile time.
    ///
    /// The arguments are already in consecutive registers, and the callee's frame
    /// starts at `base` — so a call passes no argument list at all, which is the
    /// per-call `Vec<Value>` the tree-walker allocates. A top-level function needs no
    /// closure value either, so a direct call allocates nothing whatsoever.
    CallFn { dst: Reg, chunk: ChunkId, base: Reg, argc: u16 },
    /// `dst = regs[func](regs[base..base + argc])`, the callee a value in a register.
    Call { dst: Reg, func: Reg, base: Reg, argc: u16 },
    /// A call in tail position: reuse this frame instead of pushing one.
    ///
    /// This is what makes a tail-recursive Roc function a loop. The callee is `chunk`
    /// when it is known at compile time, and the value in `func` otherwise.
    TailCall { func: Reg, chunk: Option<ChunkId>, base: Reg, argc: u16 },
    /// Return `src` to the caller.
    Ret { src: Reg },

    // ---- aggregates: built from values already in consecutive registers ----
    /// `dst = [regs[base .. base + n]]`
    MakeList { dst: Reg, base: Reg, n: u16 },
    /// `dst = (regs[base .. base + n])`
    MakeTuple { dst: Reg, base: Reg, n: u16 },
    /// `regs[list].push(regs[src])`, in place when nothing else holds the list —
    /// which is so for the list a compiled `map` is building. `src` is left `Unit`.
    ListPush { list: Reg, src: Reg },
    /// `dst = Name(regs[base .. base + n])`, the name from this chunk's `names`.
    MakeTag { dst: Reg, name: u16, base: Reg, n: u16 },
    /// `dst = { names[name .. name + n]: regs[base .. base + n] }`, in that order.
    MakeRecord { dst: Reg, name: u16, base: Reg, n: u16 },
    /// `dst = { ..regs[obj], names[name .. name + n]: regs[base .. base + n] }`.
    ///
    /// A new record; roc has no mutation. Updating a field the record does not have is
    /// an error, which is why this is not just a sequence of stores.
    UpdateRecord { dst: Reg, obj: Reg, name: u16, base: Reg, n: u16 },
    /// `dst = regs[obj].field`, the field name from `names`.
    GetField { dst: Reg, obj: Reg, name: u16 },
    /// `dst = Ok(regs[obj].field)`, or `Err(MissingField)` when it is absent.
    GetOptField { dst: Reg, obj: Reg, name: u16 },
    /// `dst = regs[obj].i` — a tuple's positional element.
    GetIndex { dst: Reg, obj: Reg, i: u16 },

    // ---- pattern tests: each jumps to `to` when the value does NOT match ----
    /// A literal pattern from this chunk's `pats`.
    TestLit { obj: Reg, pat: u16, to: u32 },
    /// A tag with this name and this many payload elements.
    TestTag { obj: Reg, name: u16, n: u16, to: u32 },
    /// A tuple of exactly this length.
    TestTuple { obj: Reg, n: u16, to: u32 },
    /// A record — which fields it needs is checked by `GetFieldOr`.
    TestRecord { obj: Reg, to: u32 },
    /// A list, of exactly `n` elements, or at least `n` when `exact` is false.
    TestList { obj: Reg, n: u16, exact: bool, to: u32 },
    /// No arm matched `obj`. Always an error; roc's own exhaustiveness check is what
    /// normally makes this unreachable.
    NoMatch { obj: Reg },

    // ---- destructuring: valid once the matching test above has passed ----
    /// `dst = regs[obj]`'s tag payload element `i`.
    GetPayload { dst: Reg, obj: Reg, i: u16 },
    /// `dst = regs[obj].field`, or jump to `to` when the record has no such field.
    GetFieldOr { dst: Reg, obj: Reg, name: u16, to: u32 },
    /// `dst =` the record's fields that `names[name .. name + n]` does not name.
    GetRest { dst: Reg, obj: Reg, name: u16, n: u16 },
    /// `dst = regs[obj][i]`, counting from the END when `from_end`.
    GetElem { dst: Reg, obj: Reg, i: u16, from_end: bool },
    /// `dst =` the list minus `front` elements at the start and `back` at the end —
    /// what `[a, .. as middle, b]` binds.
    GetSlice { dst: Reg, obj: Reg, front: u16, back: u16 },

    // ---- loops ----
    /// `dst = start..end`, inclusive or not. A range is NOT a list: roc keeps it
    /// opaque, and `IterNext` walks it without ever building one.
    MakeRange { dst: Reg, start: Reg, end: Reg, inclusive: bool },
    // ---- builtins, dispatch, interpolation ----
    /// `dst = Module.name(regs[base .. base + argc])`, a builtin of the interpreter's.
    ///
    /// The implementations live in `crate::eval`, take and return `Value`s, and hold
    /// no interpreter state — which is why they outlived the tree-walker they were
    /// written for.
    CallBuiltin { dst: Reg, name: u16, base: Reg, argc: u16 },
    /// `dst = name(regs[base .. base + argc])`, an effect of the default host.
    CallHost { dst: Reg, name: u16, base: Reg, argc: u16 },
    /// `dst = Module.name` as a VALUE — `xs.map(Str.inspect)`.
    MakeBuiltin { dst: Reg, name: u16 },
    /// `dst = regs[base].method(regs[base + 1 .. base + argc])`.
    ///
    /// Which builtin that is depends on the receiver's type at RUN time, so unlike a
    /// nominal's own method — which the compiler resolves to a chunk — this one cannot
    /// be settled earlier.
    DispatchMethod { dst: Reg, name: u16, base: Reg, argc: u16 },
    /// `dst =` the literals `names[name .. name + n + 1]` with `regs[base .. base + n]`
    /// interleaved between them: `"a${x}b"` is two literals and one value.
    Interp { dst: Reg, name: u16, base: Reg, n: u16 },
    /// `dst = a op b`, but a nominal's own operator method first.
    ///
    /// Only emitted for a program that actually defines one (`plus`, `is_eq`, …), so
    /// the ordinary `Bin` stays a jump into the operator table and nothing else.
    BinDispatch { dst: Reg, a: Reg, b: Reg, op: BinOp },

    // ---- statements ----
    /// `expect cond` inside a function body: a runtime assertion. Reports a failure to
    /// stderr and carries on — roc does not abort on a failed expectation — and makes
    /// the run exit non-zero. It is not a test, so it is never tallied.
    Expect { cond: Reg },
    /// A TOP-LEVEL `expect`, which is a test. Only emitted under `--test`, and tallied
    /// there; a normal run does not compile it at all.
    TestExpect { cond: Reg },
    /// `dbg value`, to stderr.
    Dbg { src: Reg },
    /// `crash message`. Always an error.
    Crash { src: Reg },

    /// The next element of `iter`, with `idx` holding the position reached so far.
    ///
    /// `dst` gets the element and `idx` advances; when there is nothing left, jump to
    /// `to`. Over a range this computes the element instead of reading one, so
    /// `for i in 0..<10_000_000` allocates nothing — the tree-walker's own fix for that
    /// was to special-case ranges in `for`, and this is the same idea as an opcode.
    IterNext { dst: Reg, iter: Reg, idx: Reg, to: u32 },
}

/// Which construct a conditional jump came from, so its error can say so.
#[derive(Debug, Clone, Copy)]
pub enum CondKind {
    If,
    Guard,
    While,
}

impl CondKind {
    fn expected_bool(self, got: &Value) -> EvalError {
        EvalError {
            message: match self {
                CondKind::If => format!("An if condition must be a Bool, got {}", got),
                CondKind::Guard => format!("A match guard must be a Bool, got {}", got),
                CondKind::While => format!("A `while` condition must be a Bool, got {}", got),
            },
        }
    }
}

/// One compiled function: flat code, its own constants, a known frame size.
#[derive(Debug)]
pub struct Chunk {
    pub code: Vec<Op>,
    pub consts: Vec<Value>,
    /// Frame size. Known at compile time, so a call grows the register file once.
    pub n_regs: u16,
    pub arity: u16,
    /// Parameter names. Shared with the AST node, and used only to render a function
    /// value as `<lambda |x, y|>` — the same text the tree-walker produces.
    pub params: Rc<Vec<&'static str>>,
    /// Field and tag names, by index. Names are compared by content at run time —
    /// resolving a field to a SLOT needs the record's type at the access site, which
    /// means threading the checker's types through the compiler. A later phase.
    pub names: Vec<&'static str>,
    /// Literal patterns, by index. Only `Int`, `Float` and `Str` ever land here: every
    /// other pattern is compiled into tests and destructuring ops.
    pub pats: Vec<crate::ast::Pattern>,
    /// For errors and `dbg` only — never looked up by.
    pub name: &'static str,
    /// The AST node each instruction came from, parallel to `code`.
    ///
    /// This is what a runtime error is located by: the instruction that failed knows
    /// its node, the node knows its offset, and the offset knows its line. It was
    /// impossible until the AST had node identity — there was nothing for an
    /// instruction to point at.
    pub spans: Vec<crate::ast::NodeId>,
    /// This chunk as a closure over nothing, made once at compile time.
    ///
    /// A top-level function captures nothing, so the closure it runs under is the same
    /// object every time. Building it per call — which is what the first draft did —
    /// put a heap allocation on the hot path and cost 4% of `fib`.
    pub bare: Rc<Closure>,
}

/// A function value: which chunk to run, and what it captured.
///
/// Captures are by VALUE and decided at compile time, so there is no environment, no
/// `RefCell` chain and no scope vector — the three things that made the tree-walker's
/// closures expensive. A `var` that is captured *and* assigned needs a shared cell
/// instead; `var` is V3, and so is that.
#[derive(Debug)]
pub struct Closure {
    pub chunk: ChunkId,
    /// Copied from the chunk, so rendering a function value needs no program.
    pub params: Rc<Vec<&'static str>>,
    pub captures: Vec<Value>,
}

/// A whole compiled program.
#[derive(Debug)]
pub struct Program {
    pub chunks: Vec<Chunk>,
    /// Top-level bindings whose value is not a function.
    pub n_globals: usize,
    /// The chunk holding the top level itself.
    pub top: ChunkId,
    /// The app's entry point, if it declared one, and its arity.
    pub entry: Option<(ChunkId, u16)>,
    /// Top-level methods by `(module, method)` — `("Dict", "insert")`.
    ///
    /// The compiler resolves a dispatch itself when the checker names the receiver's
    /// module. When it cannot, only the running value knows, and this is what it looks
    /// the method up in.
    pub methods: std::collections::HashMap<(&'static str, &'static str), ChunkId>,
    /// The same methods by bare name. A user nominal is a plain record at run time —
    /// roc erases nominals — so a receiver whose module cannot be named falls back to
    /// a method of that name, provided only one type defines it.
    pub methods_by_name: std::collections::HashMap<&'static str, Vec<(&'static str, ChunkId)>>,
    /// What each nominal's backing type LOOKS like, by name.
    ///
    /// roc erases nominals, so a value cannot say which one it is. Its shape can rule
    /// one out, though, and that is enough: a tuple is not a `Try`, so `Try.is_eq` must
    /// not answer `(1, "x") == (1, "x")`. This is the runtime half of nominal identity
    /// — the compiler resolves it from the type wherever the checker knows one.
    pub nominal_shapes: std::collections::HashMap<&'static str, NominalShape>,
    /// The shapes of the nominals declared with `::`, the OPAQUE form.
    ///
    /// roc shows one as `<opaque>` instead of its backing value, and that is the only
    /// place opacity is observable inside a single file.
    pub opaque_shapes: Vec<NominalShape>,
}

/// The shape of a nominal's backing type, as far as a runtime value can be compared.
#[derive(Debug, Clone)]
pub enum NominalShape {
    Tags(Vec<String>),
    Fields(Vec<String>),
    Tuple(usize),
    /// A backing this cannot rule anything out from — a type variable, or another
    /// nominal whose own shape is unknown. Never filters.
    Unknown,
}

/// A top-level method by its qualified name, as a callable value.
///
/// Unlike `methods_named` this needs no receiver: `Json.parse` looks up a nominal's
/// `parser_for` knowing only the type it must produce.
pub fn method_by_name(module: &str, method: &str) -> Option<Value> {
    with_running(|program| {
        let chunk = *program.methods.get(&(module, method))?;
        Some(Value::Closure(Rc::clone(&program.chunks[chunk as usize].bare)))
    })
    .flatten()
}

/// Is `value` exactly the shape of a nominal the running program declared opaque?
///
/// Asked for every record, tag and tuple `Str.inspect` renders, nested ones included,
/// so it borrows the shapes rather than copying them out each time.
pub fn is_opaque(value: &Value) -> bool {
    with_running(|program| program.opaque_shapes.iter().any(|shape| shape.is_exactly(value)))
        .unwrap_or(false)
}

impl NominalShape {
    /// Is `value` EXACTLY this shape — the same fields, no more?
    ///
    /// Stricter than `admits`, which only rules a value out. Opacity is decided from
    /// the shape because roc erases nominals and the value cannot say which one it is,
    /// so a record that happens to have the same fields as an opaque nominal reads as
    /// opaque too. `ponytail: the alternative is a `Value` that carries its nominal,
    /// which every other operation would then have to see through.`
    pub fn is_exactly(&self, value: &Value) -> bool {
        match (self, value) {
            (NominalShape::Fields(declared), Value::Record(fields)) => {
                declared.len() == fields.len()
                    && declared.iter().all(|want| fields.iter().any(|(have, _)| have == want))
            }
            (NominalShape::Tags(names), Value::Tag(tag, _)) => names.iter().any(|n| n == tag),
            (NominalShape::Tuple(n), Value::Tuple(items)) => items.len() == *n,
            _ => false,
        }
    }

    /// Could `value` be of this nominal? `false` only when the shape RULES IT OUT.
    pub fn admits(&self, value: &Value) -> bool {
        match (self, value) {
            (NominalShape::Tags(names), Value::Tag(tag, _)) => names.iter().any(|n| n == tag),
            (NominalShape::Tags(_), _) => false,
            (NominalShape::Fields(declared), Value::Record(fields)) => {
                declared.iter().all(|want| fields.iter().any(|(have, _)| have == want))
            }
            (NominalShape::Fields(_), _) => false,
            (NominalShape::Tuple(n), Value::Tuple(items)) => items.len() == *n,
            (NominalShape::Tuple(_), _) => false,
            (NominalShape::Unknown, _) => true,
        }
    }
}

/// The shape of a type, for `NominalShape`.
pub fn shape_of(ty: &crate::types::Type) -> NominalShape {
    use crate::types::Type;
    match ty {
        Type::TagUnion { tags, .. } => {
            NominalShape::Tags(tags.iter().map(|(name, _)| name.clone()).collect())
        }
        Type::Record { fields, .. } => {
            NominalShape::Fields(fields.iter().map(|(name, _)| name.clone()).collect())
        }
        Type::Tuple(items) => NominalShape::Tuple(items.len()),
        Type::Nominal { backing, .. } => shape_of(backing),
        _ => NominalShape::Unknown,
    }
}

/// A suspended caller: where to resume, and where to put the result.
///
/// The tree-walker spends many Rust frames per Roc call, which is why it needs a 256 MB
/// stack to recurse a few hundred levels; this is a `Vec` push.
#[derive(Debug)]
struct Frame {
    chunk: ChunkId,
    ip: u32,
    base: u32,
    dst: Reg,
    /// The closure that was running, for `LoadCap` and `LoadSelf` after the return.
    closure: Rc<Closure>,
}

/// How deep Roc recursion may go before it is an error rather than an ambition.
///
/// This exists so that runaway recursion produces a message. It is not a stack limit —
/// frames are heap-allocated — so it can be generous: a million frames is ~32 MB. A
/// tail-recursive function does not count against it at all.
const MAX_FRAMES: usize = 1_000_000;

/// Compile an AST and run it, in one call.
///
/// The ordinary way to run a single-file program: the compile step is not separately
/// interesting to a caller who just wants the answer.
pub fn eval(ast: &crate::ast::Expr) -> Result<Value, EvalError> {
    let program = Rc::new(compile(ast, None).map_err(|message| EvalError { message })?);
    run(&program)
}

/// Run a compiled program's top level, then its entry point if it has one.
///
/// The return value is the entry point's, or the top level's if there is none — which
/// is what a module is.
pub fn run(program: &Rc<Program>) -> Result<Value, EvalError> {
    run_with_args(program, None)
}

/// Run, handing the entry point `args` — what a platform's host passes to `main!`.
/// `None` is an empty argument list, which is all a platformless run can offer.
pub fn run_with_args(program: &Rc<Program>, args: Option<Value>) -> Result<Value, EvalError> {
    let mut vm = Vm::new(program);
    vm.entry_args = args;
    // Installed for as long as this program runs, so a builtin's callback can find the
    // machine to run a closure on.
    RUNNING.with(|r| r.borrow_mut().push((Rc::clone(program), Rc::clone(&vm.globals))));
    let result = vm.run_program();
    RUNNING.with(|r| {
        r.borrow_mut().pop();
    });
    result
}

impl Vm {
    fn run_program(&mut self) -> Result<Value, EvalError> {
        let (top, entry) = (self.program.top, self.program.entry);
        let value = self.call_chunk(top, Vec::new())?;
        match entry {
            None => Ok(value),
            // `main! : List(Str) => ...`, and the arity-0 spelling is also accepted.
            // The arguments are the host's to give; without one there are none.
            Some((chunk, arity)) => {
                let args = if arity == 0 {
                    Vec::new()
                } else {
                    vec![self.entry_args.take().unwrap_or_else(|| Value::list(Vec::new()))]
                };
                self.call_chunk(chunk, args)
            }
        }
    }
}

/// The machine. Registers, frames and globals.
pub struct Vm {
    program: Rc<Program>,
    /// One contiguous register file. A frame is the window `[base, base + n_regs)`.
    regs: Vec<Value>,
    frames: Vec<Frame>,
    /// Top-level values, by slot. `None` until the top level assigns it, which is how
    /// a use-before-definition becomes a message instead of a wrong answer.
    globals: Globals,
    /// What the entry point is called with, when a host supplied it. Taken on use.
    entry_args: Option<Value>,
}

impl Vm {
    pub fn new(program: &Rc<Program>) -> Self {
        Vm {
            program: Rc::clone(program),
            regs: Vec::new(),
            frames: Vec::new(),
            globals: Rc::new(RefCell::new(vec![None; program.n_globals])),
            entry_args: None,
        }
    }

    /// Call a chunk that captures nothing — the top level, or a top-level function.
    pub fn call_chunk(&mut self, chunk: ChunkId, args: Vec<Value>) -> Result<Value, EvalError> {
        let closure = self.chunk(chunk)?.bare.clone();
        self.call(&closure, args)
    }

    /// Call `closure` with `args` and run until it returns.
    pub fn call(&mut self, closure: &Rc<Closure>, args: Vec<Value>) -> Result<Value, EvalError> {
        let base = self.regs.len();
        let n_regs = self.chunk(closure.chunk)?.n_regs as usize;
        self.regs.resize(base + n_regs.max(args.len()), Value::Unit);
        for (i, arg) in args.into_iter().enumerate() {
            self.regs[base + i] = arg;
        }
        let result = self.exec(closure.clone(), base);
        // Leave the register file as it was found, whether or not the call succeeded:
        // an error propagating out of here must not leave a frame's worth of registers
        // behind for the next call to inherit.
        self.regs.truncate(base);
        self.frames.clear();
        result
    }

    fn chunk(&self, id: ChunkId) -> Result<&Chunk, EvalError> {
        self.program.chunks.get(id as usize).ok_or_else(|| EvalError {
            message: format!("vm: no chunk {}", id),
        })
    }

    /// The interpreter loop.
    ///
    /// `ip`, `base`, `code` and `cur` are locals, not fields: they change on every
    /// instruction, and reading them through `self.frames.last()` each time would be
    /// both slower and harder to read. They are written into a `Frame` only on a call.
    fn exec(&mut self, start: Rc<Closure>, start_base: usize) -> Result<Value, EvalError> {
        // Split the borrows up front: `code` comes out of `program`, and the register
        // file is mutated all the way through. Without this the two would conflict.
        let program = Rc::clone(&self.program);
        let globals = Rc::clone(&self.globals);
        let Vm { regs, frames, .. } = self;

        let mut cur = start;
        let mut chunk_id = cur.chunk;
        let mut code: &[Op] = &program.chunks[chunk_id as usize].code;
        let mut base = start_base;
        let mut ip = 0usize;

        loop {
            let op = *code.get(ip).ok_or_else(|| EvalError {
                message: format!(
                    "vm: ran off the end of `{}`",
                    program.chunks[chunk_id as usize].name
                ),
            })?;
            ip += 1;

            match op {
                Op::LoadK { dst, k } => {
                    regs[base + dst as usize] =
                        program.chunks[chunk_id as usize].consts[k as usize].clone();
                }
                Op::Move { dst, src } => {
                    regs[base + dst as usize] = regs[base + src as usize].clone();
                }
                Op::LoadGlob { dst, idx } => {
                    let value = globals.borrow()[idx as usize].clone().ok_or_else(|| EvalError {
                        message: "Used before it was defined".to_string(),
                    })?;
                    regs[base + dst as usize] = value;
                }
                Op::StoreGlob { idx, src } => {
                    globals.borrow_mut()[idx as usize] = Some(regs[base + src as usize].clone());
                }
                Op::LoadCap { dst, idx } => {
                    regs[base + dst as usize] = cur.captures[idx as usize].clone();
                }
                Op::LoadSelf { dst } => {
                    regs[base + dst as usize] = Value::Closure(cur.clone());
                }
                Op::Bin { dst, a, b, op } => {
                    // The one implementation of Roc's operators, shared with the
                    // tree-walker. The operands are borrowed, not cloned: cloning two
                    // 32-byte `Value`s to add two integers was 40% of `fib`.
                    //
                    // Nominal operator overloading (`dispatch_operator`) is not
                    // consulted because the compiler cannot yet compile a nominal's
                    // method block, so no value reaching here can have one.
                    let value = crate::eval::apply_binop(
                        op,
                        &regs[base + a as usize],
                        &regs[base + b as usize],
                    )
                    .map_err(|e| locate_error(&program, chunk_id, ip, e))?;
                    regs[base + dst as usize] = value;
                }
                Op::BinInt { dst, a, b, op } => {
                    let value = match (&regs[base + a as usize], &regs[base + b as usize]) {
                        (Value::Int(x), Value::Int(y)) => match crate::eval::int_binop(op, *x, *y)
                        {
                            Some(result) => result,
                            // `and`/`or` are the only ops `int_binop` declines, and the
                            // compiler never specialises those.
                            None => crate::eval::apply_binop(
                                op,
                                &regs[base + a as usize],
                                &regs[base + b as usize],
                            ),
                        },
                        (left, right) => crate::eval::apply_binop(op, left, right),
                    }
                    .map_err(|e| locate_error(&program, chunk_id, ip, e))?;
                    regs[base + dst as usize] = value;
                }
                Op::Jump { to } => ip = to as usize,
                Op::JumpFalse { cond, to, kind } => {
                    // Strictly a `Bool`, and the same message as the tree-walker for
                    // anything else: roc has no truthiness and neither does this.
                    match &regs[base + cond as usize] {
                        Value::Bool(true) => {}
                        Value::Bool(false) => ip = to as usize,
                        other => return Err(locate_error(&program, chunk_id, ip, kind.expected_bool(other))),
                    }
                }
                Op::MakeClosure { dst, chunk, base: cap_base, n } => {
                    let mut captures = Vec::with_capacity(n as usize);
                    for i in 0..n as usize {
                        captures.push(regs[base + cap_base as usize + i].clone());
                    }
                    let callee = &program.chunks[chunk as usize];
                    regs[base + dst as usize] = if n == 0 {
                        // Captures nothing, so the closure made at compile time will do.
                        Value::Closure(callee.bare.clone())
                    } else {
                        Value::Closure(Rc::new(Closure {
                            chunk,
                            params: callee.params.clone(),
                            captures,
                        }))
                    };
                }
                Op::CallFn { dst, chunk, base: arg_base, argc } => {
                    let callee = &program.chunks[chunk as usize];
                    check_arity(callee.arity, argc).map_err(|e| locate_error(&program, chunk_id, ip, e))?;
                    if frames.len() >= MAX_FRAMES {
                        return Err(locate_error(&program, chunk_id, ip, too_deep(callee.name)));
                    }
                    let new_base = base + arg_base as usize;
                    grow(regs, new_base + callee.n_regs as usize);
                    frames.push(Frame {
                        chunk: chunk_id,
                        ip: ip as u32,
                        base: base as u32,
                        dst,
                        closure: cur,
                    });
                    // A top-level function captures nothing, so the closure it runs
                    // under is the one made at compile time: a refcount bump, not an
                    // allocation, on every call.
                    cur = callee.bare.clone();
                    chunk_id = chunk;
                    code = &callee.code;
                    base = new_base;
                    ip = 0;
                }
                Op::Call { dst, func, base: arg_base, argc } => {
                    // A BUILTIN held as a value and then called: `parse_with(U64.from_str)`
                    // stores it, and the parser it returns calls it. `eval::call_function`
                    // already knew how; the opcode insisted on a closure.
                    if let Value::Builtin(qualified, _) = &regs[base + func as usize] {
                        let qualified = *qualified;
                        let args = collect(regs, base + arg_base as usize, argc);
                        let value = crate::eval::call_function(
                            Value::Builtin(qualified, args.len()),
                            args,
                        )
                        .map_err(|e| locate_error(&program, chunk_id, ip, e))?;
                        regs[base + dst as usize] = value;
                        continue;
                    }
                    let callee = as_closure(&regs[base + func as usize])
                            .map_err(|e| locate_error(&program, chunk_id, ip, e))?;
                    let target = &program.chunks[callee.chunk as usize];
                    check_arity(target.arity, argc).map_err(|e| locate_error(&program, chunk_id, ip, e))?;
                    if frames.len() >= MAX_FRAMES {
                        return Err(locate_error(&program, chunk_id, ip, too_deep(target.name)));
                    }
                    let new_base = base + arg_base as usize;
                    grow(regs, new_base + target.n_regs as usize);
                    frames.push(Frame {
                        chunk: chunk_id,
                        ip: ip as u32,
                        base: base as u32,
                        dst,
                        closure: cur,
                    });
                    cur = callee;
                    chunk_id = cur.chunk;
                    code = &target.code;
                    base = new_base;
                    ip = 0;
                }
                Op::TailCall { func, chunk, base: arg_base, argc } => {
                    // No frame is pushed and none is popped: the arguments move down
                    // to where this frame's own parameters are, and execution starts
                    // again at the top of the callee. A tail-recursive function is
                    // therefore a loop, in constant memory.
                    let callee = match chunk {
                        Some(id) => program.chunks[id as usize].bare.clone(),
                        None => as_closure(&regs[base + func as usize])
                            .map_err(|e| locate_error(&program, chunk_id, ip, e))?,
                    };
                    let target = &program.chunks[callee.chunk as usize];
                    check_arity(target.arity, argc).map_err(|e| locate_error(&program, chunk_id, ip, e))?;
                    if arg_base != 0 {
                        // Increasing order, and every destination is below its source,
                        // so nothing is overwritten before it has been read.
                        for i in 0..argc as usize {
                            let arg = std::mem::replace(
                                &mut regs[base + arg_base as usize + i],
                                Value::Unit,
                            );
                            regs[base + i] = arg;
                        }
                    }
                    grow(regs, base + target.n_regs as usize);
                    cur = callee;
                    chunk_id = cur.chunk;
                    code = &target.code;
                    ip = 0;
                }
                // ---- aggregates ----
                Op::MakeList { dst, base: b, n } => {
                    let items = collect(regs, base + b as usize, n);
                    regs[base + dst as usize] = Value::list(items);
                }
                Op::MakeTuple { dst, base: b, n } => {
                    let items = collect(regs, base + b as usize, n);
                    regs[base + dst as usize] = Value::Tuple(items);
                }
                Op::ListPush { list, src } => {
                    let item = std::mem::replace(&mut regs[base + src as usize], Value::Unit);
                    match &mut regs[base + list as usize] {
                        Value::List(items) => Rc::make_mut(items).push(item),
                        other => {
                            return Err(locate_error(&program, chunk_id, ip, EvalError {
                                message: format!("vm: pushed onto {}, which is not a list", other),
                            }))
                        }
                    }
                }
                Op::MakeTag { dst, name, base: b, n } => {
                    let items = collect(regs, base + b as usize, n);
                    let tag = program.chunks[chunk_id as usize].names[name as usize];
                    regs[base + dst as usize] = Value::tag(tag, items);
                }
                Op::MakeRecord { dst, name, base: b, n } => {
                    let names = &program.chunks[chunk_id as usize].names;
                    let mut fields = Vec::with_capacity(n as usize);
                    for i in 0..n as usize {
                        fields.push((names[name as usize + i], regs[base + b as usize + i].clone()));
                    }
                    regs[base + dst as usize] = Value::Record(fields);
                }
                Op::UpdateRecord { dst, obj, name, base: b, n } => {
                    let mut fields = match &regs[base + obj as usize] {
                        Value::Record(fields) => fields.clone(),
                        other => {
                            return Err(locate_error(&program, chunk_id, ip, EvalError {
                                message: format!("Cannot update `{}`: it is not a record", other),
                            }))
                        }
                    };
                    let names = &program.chunks[chunk_id as usize].names;
                    for i in 0..n as usize {
                        let field = names[name as usize + i];
                        match fields.iter_mut().find(|(f, _)| *f == field) {
                            Some(slot) => slot.1 = regs[base + b as usize + i].clone(),
                            None => {
                                return Err(locate_error(&program, chunk_id, ip, EvalError {
                                    message: format!("Record has no field `{}` to update", field),
                                }))
                            }
                        }
                    }
                    regs[base + dst as usize] = Value::Record(fields);
                }
                Op::GetField { dst, obj, name } => {
                    let field = program.chunks[chunk_id as usize].names[name as usize];
                    let value = match &regs[base + obj as usize] {
                        Value::Record(fields) => fields
                            .iter()
                            .find(|(f, _)| *f == field)
                            .map(|(_, v)| v.clone())
                            .ok_or_else(|| EvalError {
                                message: format!("Record has no field '{}'", field),
                            })?,
                        other => {
                            return Err(locate_error(&program, chunk_id, ip, EvalError {
                                message: format!("Cannot access field '{}' on {}", field, other),
                            }))
                        }
                    };
                    regs[base + dst as usize] = value;
                }
                Op::GetOptField { dst, obj, name } => {
                    let field = program.chunks[chunk_id as usize].names[name as usize];
                    let value = match &regs[base + obj as usize] {
                        Value::Record(fields) => match fields.iter().find(|(f, _)| *f == field) {
                            Some((_, v)) => Value::tag("Ok", vec![v.clone()]),
                            // Absent, which is the point of an optional field.
                            None => Value::tag("Err", vec![Value::tag("MissingField", vec![])]),
                        },
                        other => {
                            return Err(locate_error(&program, chunk_id, ip, EvalError {
                                message: format!(
                                    "Cannot read optional field `{}` on {}",
                                    field, other
                                ),
                            }))
                        }
                    };
                    regs[base + dst as usize] = value;
                }
                Op::GetIndex { dst, obj, i } => {
                    let value = match &regs[base + obj as usize] {
                        Value::Tuple(items) => {
                            items.get(i as usize).cloned().ok_or_else(|| EvalError {
                                message: format!(
                                    "Tuple has {} element(s), so .{} is out of range",
                                    items.len(),
                                    i
                                ),
                            })?
                        }
                        other => {
                            return Err(locate_error(&program, chunk_id, ip, EvalError {
                                message: format!("Cannot index .{} on {}", i, other),
                            }))
                        }
                    };
                    regs[base + dst as usize] = value;
                }

                // ---- pattern tests ----
                Op::TestLit { obj, pat, to } => {
                    let pattern = &program.chunks[chunk_id as usize].pats[pat as usize];
                    if !crate::eval::literal_pattern_matches(pattern, &regs[base + obj as usize]) {
                        ip = to as usize;
                    }
                }
                Op::TestTag { obj, name, n, to } => {
                    let want = program.chunks[chunk_id as usize].names[name as usize];
                    match &regs[base + obj as usize] {
                        Value::Tag(tag, payload)
                            if *tag == want && payload.len() == n as usize => {}
                        _ => ip = to as usize,
                    }
                }
                Op::TestTuple { obj, n, to } => match &regs[base + obj as usize] {
                    Value::Tuple(items) if items.len() == n as usize => {}
                    _ => ip = to as usize,
                },
                Op::TestRecord { obj, to } => {
                    // `{}` the value is `Unit`, and the pattern `{}` names no fields,
                    // so it matches; a pattern with fields then fails on the read.
                    if !matches!(regs[base + obj as usize], Value::Record(_) | Value::Unit) {
                        ip = to as usize;
                    }
                }
                Op::TestList { obj, n, exact, to } => match &regs[base + obj as usize] {
                    Value::List(items)
                        if (exact && items.len() == n as usize)
                            || (!exact && items.len() >= n as usize) => {}
                    _ => ip = to as usize,
                },
                Op::NoMatch { obj } => {
                    return Err(locate_error(&program, chunk_id, ip, EvalError {
                        message: format!("No match arm matched {}", regs[base + obj as usize]),
                    }))
                }

                // ---- destructuring ----
                Op::GetPayload { dst, obj, i } => {
                    let value = match &regs[base + obj as usize] {
                        Value::Tag(_, payload) => payload[i as usize].clone(),
                        other => unreachable_shape("a tag", other)?,
                    };
                    regs[base + dst as usize] = value;
                }
                Op::GetFieldOr { dst, obj, name, to } => {
                    let field = program.chunks[chunk_id as usize].names[name as usize];
                    let found = match &regs[base + obj as usize] {
                        Value::Record(fields) => {
                            fields.iter().find(|(f, _)| *f == field).map(|(_, v)| v.clone())
                        }
                        other => unreachable_shape("a record", other)?,
                    };
                    match found {
                        Some(value) => regs[base + dst as usize] = value,
                        // A record pattern naming a field the value does not have is
                        // simply not a match, not an error.
                        None => ip = to as usize,
                    }
                }
                Op::GetRest { dst, obj, name, n } => {
                    let names = &program.chunks[chunk_id as usize].names;
                    let named = &names[name as usize..name as usize + n as usize];
                    let value = match &regs[base + obj as usize] {
                        Value::Record(fields) => Value::Record(
                            fields
                                .iter()
                                .filter(|(f, _)| !named.contains(f))
                                .cloned()
                                .collect(),
                        ),
                        other => unreachable_shape("a record", other)?,
                    };
                    regs[base + dst as usize] = value;
                }
                Op::GetElem { dst, obj, i, from_end } => {
                    let value = match &regs[base + obj as usize] {
                        Value::List(items) => {
                            let at = if from_end { items.len() - 1 - i as usize } else { i as usize };
                            items[at].clone()
                        }
                        other => unreachable_shape("a list", other)?,
                    };
                    regs[base + dst as usize] = value;
                }
                Op::GetSlice { dst, obj, front, back } => {
                    let value = match &regs[base + obj as usize] {
                        Value::List(items) => {
                            Value::list(items[front as usize..items.len() - back as usize].to_vec())
                        }
                        other => unreachable_shape("a list", other)?,
                    };
                    regs[base + dst as usize] = value;
                }

                // ---- loops ----
                Op::MakeRange { dst, start, end, inclusive } => {
                    let bound = |v: &Value| match v {
                        Value::Int(n) => Ok(*n),
                        other => Err(EvalError {
                            message: format!("A range needs whole numbers, got {}", other),
                        }),
                    };
                    let start = bound(&regs[base + start as usize])?;
                    let end = bound(&regs[base + end as usize])?;
                    regs[base + dst as usize] = Value::Range { start, end, inclusive };
                }
                Op::IterNext { dst, iter, idx, to } => {
                    let at = match &regs[base + idx as usize] {
                        Value::Int(n) => *n,
                        other => {
                            return Err(locate_error(&program, chunk_id, ip, EvalError {
                                message: format!("vm: loop counter held {}", other),
                            }))
                        }
                    };
                    let next = match &regs[base + iter as usize] {
                        Value::Range { start, end, inclusive } => {
                            let last = if *inclusive { *end } else { *end - 1 };
                            let current = start + at;
                            (current <= last).then_some(Value::Int(current))
                        }
                        Value::List(items) => items.get(at as usize).cloned(),
                        other => {
                            return Err(locate_error(&program, chunk_id, ip, EvalError {
                                message: format!(
                                    "`for` needs a List or a range to iterate, got {}",
                                    other
                                ),
                            }))
                        }
                    };
                    match next {
                        None => ip = to as usize,
                        Some(value) => {
                            regs[base + dst as usize] = value;
                            regs[base + idx as usize] = Value::Int(at + 1);
                        }
                    }
                }

                // ---- builtins, dispatch, interpolation ----
                Op::CallBuiltin { dst, name, base: b, argc } => {
                    let names = &program.chunks[chunk_id as usize].names;
                    let (module, func) = (names[name as usize], names[name as usize + 1]);
                    let args = collect(regs, base + b as usize, argc);
                    let value = crate::eval::call_builtin_values(module, func, args)
                        .map_err(|e| locate_error(&program, chunk_id, ip, e))?;
                    regs[base + dst as usize] = value;
                }
                Op::CallHost { dst, name, base: b, argc } => {
                    let effect = program.chunks[chunk_id as usize].names[name as usize];
                    let args = collect(regs, base + b as usize, argc);
                    regs[base + dst as usize] = crate::eval::host_effect(effect, args).map_err(|e| locate_error(&program, chunk_id, ip, e))?;
                }
                Op::MakeBuiltin { dst, name } => {
                    // The qualified name is built once, at compile time.
                    let qualified = program.chunks[chunk_id as usize].names[name as usize];
                    // Arity 1: every builtin used as a value takes its subject and
                    // nothing else.
                    regs[base + dst as usize] = Value::Builtin(qualified, 1);
                }
                Op::DispatchMethod { dst, name, base: b, argc } => {
                    let method = program.chunks[chunk_id as usize].names[name as usize];
                    // A method the PROGRAM defines for this value's own module wins over
                    // the interpreter's own: once `Builtin.roc` is loaded, `List.map` is
                    // Roc, and the Rust one is only there for what Roc has not defined.
                    let target = match crate::eval::module_for(&regs[base + b as usize]) {
                        // The value names its module, so the answer is that module's
                        // method or nothing. Falling through to another type's method of
                        // the same name is what made a loaded `Stream` answer a List.
                        Some(module) => program.methods.get(&(module, method)).copied(),
                        // No module: a record or a tag, which is all a nominal is at run
                        // time. If exactly one type defines the method, that is the one
                        // meant; if several do, only the checker could have known.
                        None => match program.methods_by_name.get(method) {
                            Some(defined) if defined.len() == 1 => Some(defined[0].1),
                            // Several types define it and the value cannot say which it
                            // is. Guessing would silently run the wrong one, so name
                            // them and let the caller be explicit.
                            Some(defined) => {
                                let names: Vec<&str> =
                                    defined.iter().map(|(name, _)| *name).collect();
                                return Err(locate_error(&program, chunk_id, ip, EvalError {
                                    message: format!(
                                        "`{}` is ambiguous: {} all define it. Call it explicitly.",
                                        method,
                                        names.join(", ")
                                    ),
                                }));
                            }
                            None => None,
                        },
                    };
                    match target {
                        Some(chunk) => {
                            let callee = &program.chunks[chunk as usize];
                            check_arity(callee.arity, argc)
                                .map_err(|e| locate_error(&program, chunk_id, ip, e))?;
                            if frames.len() >= MAX_FRAMES {
                                return Err(locate_error(&program, chunk_id, ip, too_deep(callee.name)));
                            }
                            let new_base = base + b as usize;
                            grow(regs, new_base + callee.n_regs as usize);
                            frames.push(Frame {
                                chunk: chunk_id,
                                ip: ip as u32,
                                base: base as u32,
                                dst,
                                closure: cur,
                            });
                            cur = callee.bare.clone();
                            chunk_id = chunk;
                            code = &callee.code;
                            base = new_base;
                            ip = 0;
                        }
                        None => {
                            let mut args = collect(regs, base + b as usize, argc);
                            let receiver = args.remove(0);
                            let value = crate::eval::dispatch_builtin(receiver, method, args)
                                .map_err(|e| locate_error(&program, chunk_id, ip, e))?;
                            regs[base + dst as usize] = value;
                        }
                    }
                }
                Op::Interp { dst, name, base: b, n } => {
                    let names = &program.chunks[chunk_id as usize].names;
                    let mut out = String::new();
                    for i in 0..n as usize {
                        out.push_str(names[name as usize + i]);
                        out.push_str(&crate::eval::interpolated(&regs[base + b as usize + i]));
                    }
                    out.push_str(names[name as usize + n as usize]);
                    regs[base + dst as usize] = Value::Str(Rc::from(out));
                }
                Op::BinDispatch { dst, a, b, op } => {
                    let left = regs[base + a as usize].clone();
                    let right = regs[base + b as usize].clone();
                    let value = match crate::eval::dispatch_operator(op, &left, &right)
                        .map_err(|e| locate_error(&program, chunk_id, ip, e))?
                    {
                        Some(from_method) => from_method,
                        None => crate::eval::apply_binop(op, &left, &right)
                            .map_err(|e| locate_error(&program, chunk_id, ip, e))?,
                    };
                    regs[base + dst as usize] = value;
                }

                // ---- statements ----
                Op::Expect { cond } => {
                    crate::eval::run_expect(&regs[base + cond as usize]).map_err(|e| locate_error(&program, chunk_id, ip, e))?;
                }
                Op::TestExpect { cond } => {
                    crate::eval::run_test_expect(&regs[base + cond as usize]).map_err(|e| locate_error(&program, chunk_id, ip, e))?;
                }
                Op::Dbg { src } => crate::eval::run_dbg(&regs[base + src as usize]),
                Op::Crash { src } => {
                    return Err(locate_error(
                        &program,
                        chunk_id,
                        ip,
                        crate::eval::crash_error(&regs[base + src as usize]),
                    ))
                }

                Op::Ret { src } => {
                    // Take the value rather than cloning it: this register is dead.
                    let value = std::mem::replace(&mut regs[base + src as usize], Value::Unit);
                    match frames.pop() {
                        None => return Ok(value),
                        Some(caller) => {
                            chunk_id = caller.chunk;
                            code = &program.chunks[chunk_id as usize].code;
                            base = caller.base as usize;
                            ip = caller.ip as usize;
                            cur = caller.closure;
                            regs[base + caller.dst as usize] = value;
                        }
                    }
                }
            }
        }
    }
}

/// Take `n` values out of consecutive registers, leaving `Unit` behind.
///
/// A move rather than a clone: these registers are the aggregate's own arguments and
/// are dead the instant it is built.
fn collect(regs: &mut [Value], from: usize, n: u16) -> Vec<Value> {
    (0..n as usize)
        .map(|i| std::mem::replace(&mut regs[from + i], Value::Unit))
        .collect()
}

/// A destructuring op ran on a value whose shape a test should already have rejected.
///
/// Reachable only through a compiler bug, so it reports one rather than panicking: a
/// wrong opcode in safe Rust is a message, which is the whole argument for this being
/// safe Rust.
fn unreachable_shape<T>(expected: &str, got: &Value) -> Result<T, EvalError> {
    Err(EvalError {
        message: format!("vm: destructured {} as {}", got, expected),
    })
}

/// Say where a failing instruction was, if its node knows.
///
/// The message gains " at file:line:column". A node with no registered source — a
/// string parsed straight into the parser, as the tests do — adds nothing, so an error
/// from one reads exactly as it did before locations existed.
fn locate_error(program: &Program, chunk: ChunkId, ip: usize, mut error: EvalError) -> EvalError {
    // `ip` has already advanced past the instruction that failed.
    let Some(node) = program
        .chunks
        .get(chunk as usize)
        .and_then(|c| c.spans.get(ip.saturating_sub(1)))
    else {
        return error;
    };
    if let Some(at) = crate::ast::locate(*node) {
        error.message = format!("{} at {}", error.message, at);
    }
    error
}

/// Make room for a callee's frame. The register file only ever grows to the deepest
/// call the program actually makes.
fn grow(regs: &mut Vec<Value>, need: usize) {
    if regs.len() < need {
        regs.resize(need, Value::Unit);
    }
}

/// The tree-walker's wording, so a wrong-arity call reads the same on both engines.
fn check_arity(expected: u16, got: u16) -> Result<(), EvalError> {
    if expected == got {
        return Ok(());
    }
    Err(EvalError {
        message: format!("Lambda expects {} argument(s), got {}", expected, got),
    })
}

fn too_deep(name: &str) -> EvalError {
    EvalError {
        message: format!("Recursion went deeper than {} calls in `{}`", MAX_FRAMES, name),
    }
}

/// A value being called has to be a function, and the message matches the tree-walker's.
fn as_closure(value: &Value) -> Result<Rc<Closure>, EvalError> {
    match value {
        Value::Closure(c) => Ok(c.clone()),
        other => Err(EvalError {
            message: format!("Attempted to call a non-function value: {}", other),
        }),
    }
}
