//! AST → bytecode.
//!
//! Everything this pass does is work the tree-walker does again on every execution:
//! deciding which register a name lives in, which chunk a call goes to, which of an
//! enclosing function's values a closure needs, where a branch lands. Doing it once is
//! the entire point.
//!
//! **Refusals are explicit.** Anything this cannot lower is an `Err` naming it. There
//! is no fallback interpreter to quietly take over, which is the point of there being
//! one engine: a program either compiles or says why. The `expr` match is exhaustive
//! over `Expr` on purpose, so a variant added to the AST is a compile error here.

use super::{Chunk, ChunkId, CondKind, Op, Program, Reg};
use crate::ast::{Expr, MatchArm, Pattern, StrPart};
use crate::eval::Value;
use std::rc::Rc;

/// The top level, resolved: what is a function, what is a value, and where each lives.
struct Tops {
    /// `(name, chunk, arity)` for every top-level binding whose value is a lambda.
    fns: Vec<(&'static str, ChunkId, u16)>,
    /// Every other top-level binding, in declaration order. The index is the slot.
    globals: Vec<&'static str>,
    /// `exposing [hello]` — a bare name standing for a module's qualified one.
    aliases: Vec<(&'static str, &'static str)>,
    /// Does this program define an operator method (`plus`, `is_eq`, …) on a nominal?
    ///
    /// Almost none do, so the ordinary `Bin` opcode skips the check entirely and only
    /// a program that overloads an operator pays for the lookup.
    operator_methods: bool,
}

/// The method names roc maps its operators onto. `a + b` IS `a.plus(b)`.
const OPERATOR_METHODS: &[&str] = &[
    "plus", "minus", "times", "div_by", "div_trunc_by", "rem_by", "is_lt", "is_gt",
    "is_lte", "is_gte", "is_eq",
];

impl Tops {
    /// Every top-level function whose name ends in `.method` — a nominal's method
    /// block compiles to exactly that, so this is `methods_named` done at compile time.
    fn methods(&self, method: &str) -> Vec<(&'static str, ChunkId, u16)> {
        let suffix = format!(".{}", method);
        self.fns
            .iter()
            .filter(|(name, ..)| name.ends_with(&suffix))
            .copied()
            .collect()
    }

    fn func(&self, name: &str) -> Option<(ChunkId, u16)> {
        self.fns.iter().find(|(n, ..)| *n == name).map(|(_, c, a)| (*c, *a))
    }

    fn global(&self, name: &str) -> Option<u32> {
        self.globals.iter().position(|n| *n == name).map(|i| i as u32)
    }

    /// The qualified name a bare one was exposed as, if any.
    fn alias(&self, name: &str) -> Option<&'static str> {
        self.aliases.iter().find(|(bare, _)| *bare == name).map(|(_, full)| *full)
    }
}

/// Where a capture's value comes from, as seen by the ENCLOSING function.
///
/// A closure copies its captures out of the frame that creates it, so each one is
/// either one of that frame's registers, one of *its* captures (the grandparent's
/// value, threaded down), or the enclosing function itself.
#[derive(Debug, Clone, Copy)]
enum CapSource {
    Local(Reg),
    Capture(u16),
    /// The enclosing function's own closure — a nested lambda that calls the named
    /// function it is written inside.
    Enclosing,
}

/// How a name resolved.
enum Found {
    /// A register in this frame: a parameter or a `let`.
    Local(Reg),
    /// The function this name belongs to is the one running.
    SelfRef,
    /// An enclosing function's value, copied in when the closure was made.
    Capture(u16),
    /// A top-level binding that is not a function.
    Global(u32),
    /// Resolved, but the VM will not compile this use of it.
    Refused(String),
    /// A top-level function, which is a chunk rather than a value.
    Func(ChunkId, u16),
}

/// One local module of a multi-file program: its AST, the type its names hang on, and
/// the names it exposes bare.
pub struct Module<'a> {
    pub ast: &'a Expr,
    /// `Dir/Hello` exposes its names as `Hello.name`.
    pub type_name: &'static str,
    pub exposed: Vec<&'static str>,
}

/// Everything a program is made of.
pub struct Unit<'a> {
    /// Compiled into the SAME program as the app, ahead of it, so a module's top level
    /// is part of this one's. The tree-walker gets the same effect by evaluating each
    /// module into the shared global scope first.
    pub modules: Vec<Module<'a>>,
    pub app: &'a Expr,
    /// The name the app header declared, if it is an app rather than a module.
    pub entry: Option<&'a str>,
    /// `import "data.txt" as text : Str` — file contents, read before compiling.
    pub ingested: Vec<(&'static str, String)>,
    /// The `BinOp` nodes the checker proved have integer operands, from
    /// `TypeChecker::integer_binops`. Empty is always safe: it just means every
    /// operator goes through the generic opcode.
    pub integer_binops: std::collections::HashSet<crate::ast::NodeId>,
    /// Which module each `Dispatch` node's receiver belongs to, from
    /// `TypeChecker::dispatch_modules`. Empty is always safe: it just means every
    /// dispatch resolves the way it did before the checker was consulted.
    pub dispatch_modules: std::collections::HashMap<crate::ast::NodeId, &'static str>,
    /// Which module each `BinOp` node's operands belong to, from
    /// `TypeChecker::binop_modules`. Empty means no operator is dispatched.
    pub binop_modules: std::collections::HashMap<crate::ast::NodeId, &'static str>,
    /// The nominals in scope, as `(name, backing)`, from `Parser::nominals`. What the
    /// VM makes of them is `Program::nominal_shapes`.
    pub nominals: Vec<(&'static str, crate::types::Type)>,
    /// The names among `nominals` that were declared with `::`, the opaque form.
    pub opaque_nominals: Vec<&'static str>,
    /// Literal nodes the checker typed as `Dec`, from `TypeChecker::dec_literals`.
    /// They are lowered as fixed-point values rather than integers or floats.
    pub dec_literals: std::collections::HashSet<crate::ast::NodeId>,
    /// Literal nodes nothing pinned down, from `TypeChecker::fractional_literals`.
    /// roc defaults an unconstrained numeral to a fractional type.
    pub fractional_literals: std::collections::HashSet<crate::ast::NodeId>,
    /// `Json.parse` call sites and the type each must produce, from
    /// `TypeChecker::parse_targets`. Passed to the builtin as an extra argument.
    pub parse_targets: std::collections::HashMap<crate::ast::NodeId, crate::types::Type>,
    /// Bare names the builtin module DECLARES but does not define — its low-level ops.
    ///
    /// They are calls into Rust, so a missing one is a runtime message naming the op
    /// rather than a compile error on a name that is, after all, declared. That is how
    /// a qualified builtin like `Str.repeat` already behaves.
    pub intrinsics: std::collections::HashSet<&'static str>,
    /// `roc test` semantics: run the top-level `expect`s and tally them. A normal run
    /// SKIPS them — roc only treats a top-level `expect` as a test — while an `expect`
    /// inside a function body runs either way.
    pub test_mode: bool,
}

/// Compile a single-file program.
pub fn compile(ast: &Expr, entry: Option<&str>) -> Result<Program, String> {
    compile_unit(&Unit {
        modules: Vec::new(),
        app: ast,
        entry,
        ingested: Vec::new(),
        integer_binops: std::collections::HashSet::new(),
        dispatch_modules: std::collections::HashMap::new(),
        binop_modules: std::collections::HashMap::new(),
        nominals: Vec::new(),
        opaque_nominals: Vec::new(),
        dec_literals: std::collections::HashSet::new(),
        fractional_literals: std::collections::HashSet::new(),
        parse_targets: std::collections::HashMap::new(),
        intrinsics: std::collections::HashSet::new(),
        // The bare helper is what the unit tests and `vm::eval` use: run everything.
        test_mode: true,
    })
}

/// Compile a whole program, local modules and ingested files included.
pub fn compile_unit(unit: &Unit) -> Result<Program, String> {
    let (ast, entry) = (unit.app, unit.entry);
    // The top level is a chain of `let`s ending in an expression.
    //
    // A binding to `_` whose value is itself a chain is flattened INTO the top level:
    // the parser produces that shape for a file whose declarations are followed by
    // top-level `expect`s, and its `main!` and helpers are the program's top level
    // however the tree came out. Without this they are block-locals, and a helper
    // declared below `main!` cannot be referenced from inside it.
    let mut bindings: Vec<(&'static str, &Expr)> = Vec::new();
    let mut statements: Vec<&Expr> = Vec::new();

    // Each module's top level, ahead of the app's. Its trailing expression keeps its
    // place as a statement — that is where a module's `expect`s live.
    let mut aliases: Vec<(&'static str, &'static str)> = Vec::new();
    for module in &unit.modules {
        let mut cursor = module.ast;
        while let Expr::Let { name, value, body, .. } = cursor {
            bindings.push((name, value.as_ref()));
            cursor = body;
        }
        statements.push(cursor);
        // `exposing [hello]` makes `Hello.hello` reachable as plain `hello`.
        for name in &module.exposed {
            aliases.push((name, qualify(module.type_name, name)));
        }
    }

    let mut cursor = ast;
    let tail = loop {
        match cursor {
            Expr::Let { name: "_", value, body, .. } if matches!(**value, Expr::Let { .. }) => {
                let mut inner = value.as_ref();
                while let Expr::Let { name, value, body, .. } = inner {
                    bindings.push((name, value.as_ref()));
                    inner = body;
                }
                // Whatever the inner chain ended in was bound to `_` and discarded, so
                // it stays a statement: run for its effects, value thrown away.
                statements.push(inner);
                cursor = body;
            }
            // `_ = <expr>` binds nothing: it is a statement run for its effect, and
            // that is the shape a top-level `expect` arrives in. As a binding it
            // would take a global slot under the name `_` and, worse, be compiled
            // as an ordinary in-function `expect` rather than as a test.
            Expr::Let { name: "_", value, body, .. } => {
                statements.push(value.as_ref());
                cursor = body;
            }
            Expr::Let { name, value, body, .. } => {
                bindings.push((name, value.as_ref()));
                cursor = body;
            }
            other => break other,
        }
    };

    // Chunk 0 is the top level itself, so top-level functions start at 1. Ids are
    // handed out before any body is compiled — that is what lets two functions call
    // each other, and a function call one declared further down the file.
    let mut tops = Tops {
        fns: Vec::new(),
        globals: Vec::new(),
        aliases,
        operator_methods: false,
    };
    // An ingested file is a top-level Str, known before the program starts.
    for (name, _) in &unit.ingested {
        tops.globals.push(name);
    }
    for (name, value) in &bindings {
        match value {
            Expr::Lambda { params, .. } => {
                let id = tops.fns.len() as ChunkId + 1;
                tops.fns.push((name, id, params.len() as u16));
            }
            _ => tops.globals.push(name),
        }
    }
    tops.operator_methods = tops.fns.iter().any(|(name, ..)| {
        OPERATOR_METHODS.iter().any(|m| name.ends_with(&format!(".{}", m)))
    });

    let mut c = Compiler {
        tops,
        // One reserved slot per chunk whose id is already known. Nested lambdas
        // reserve theirs as they are found.
        chunks: (0..=bindings.iter().filter(|(_, v)| matches!(v, Expr::Lambda { .. })).count())
            .map(|_| None)
            .collect(),
        states: Vec::new(),
        node: ast.id(),
        integer_binops: unit.integer_binops.clone(),
        dispatch_modules: unit.dispatch_modules.clone(),
        binop_modules: unit.binop_modules.clone(),
        dec_literals: unit.dec_literals.clone(),
        fractional_literals: unit.fractional_literals.clone(),
        parse_targets: unit.parse_targets.clone(),
        intrinsics: unit.intrinsics.clone(),
    };

    for (name, value) in &bindings {
        if let Expr::Lambda { params, body, .. } = value {
            let (chunk, _) = c.tops.func(name).expect("collected above");
            // A top-level function is at the outermost level, so it has nothing to
            // capture: every free name in it is a global or another top-level function.
            let captures = c.function(chunk, name, params, body, None)?;
            debug_assert!(captures.is_empty(), "a top-level function captured something");
        }
    }

    // The top level: assign each global in order, then evaluate the trailing
    // expression. Order matters — a global that reads one declared below it gets
    // "Used before it was defined", which is what the tree-walker does too.
    c.states.push(FnState::new("top level", Rc::new(Vec::new()), None, false));
    for (name, text) in &unit.ingested {
        let reg = c.alloc()?;
        c.constant(reg, crate::eval::str_value(text.clone()))?;
        let idx = c.tops.global(name).expect("reserved above");
        c.emit(Op::StoreGlob { idx, src: reg });
        c.st().next_reg = reg;
    }
    for (name, value) in &bindings {
        if matches!(value, Expr::Lambda { .. }) {
            continue;
        }
        let save = c.st().next_reg;
        let src = c.expr(value)?;
        let idx = c.tops.global(name).expect("collected above");
        c.emit(Op::StoreGlob { idx, src });
        c.st().next_reg = save;
    }
    // Statements the flattening lifted out of a `_` binding, run for their effects.
    //
    // ponytail: after the globals rather than interleaved with them in source order.
    // The only thing this shape holds today is top-level `expect`s, which run after
    // the declarations anyway; interleaving matters once those are compiled (V5).
    for statement in &statements {
        let save = c.st().next_reg;
        c.top_statement(statement, unit.test_mode)?;
        c.st().next_reg = save;
    }
    // A file whose last declaration is a top-level `expect` has it as the trailing
    // expression rather than a statement. It is still a test, so it gets the same
    // treatment and the top level's own value is `{}` either way.
    if matches!(tail, Expr::Expect(..)) {
        c.top_statement(tail, unit.test_mode)?;
        let src = c.literal(Value::Unit)?;
        c.emit(Op::Ret { src });
    } else {
        c.tail(tail)?;
    }
    let top = c.states.pop().expect("pushed above");
    c.chunks[0] = Some(top.finish(0, 0));

    let entry = match entry {
        None => None,
        Some(name) => Some(c.tops.func(name).ok_or_else(|| {
            format!("vm: the entry point `{}` is not a top-level function", name)
        })?),
    };

    let chunks = c
        .chunks
        .into_iter()
        .map(|slot| slot.expect("every reserved chunk id was compiled"))
        .collect();
    // The dispatch tables. Only the compiler knows which top-level functions are
    // methods — they are the ones whose name is `Type.method` — and only the running
    // value can resolve a dispatch the checker could not type.
    let mut methods = std::collections::HashMap::new();
    let mut methods_by_name: std::collections::HashMap<&'static str, Vec<(&'static str, ChunkId)>> =
        std::collections::HashMap::new();
    for (qualified, chunk, _) in &c.tops.fns {
        if let Some((module, method)) = qualified.rsplit_once('.') {
            let module: &'static str = Box::leak(module.to_string().into_boxed_str());
            let method: &'static str = Box::leak(method.to_string().into_boxed_str());
            methods.insert((module, method), *chunk);
            methods_by_name.entry(method).or_default().push((qualified, *chunk));
        }
    }

    Ok(Program {
        chunks,
        n_globals: c.tops.globals.len(),
        top: 0,
        entry,
        methods,
        methods_by_name,
        nominal_shapes: unit
            .nominals
            .iter()
            .map(|(name, backing)| (*name, crate::vm::shape_of(backing)))
            .collect(),
        opaque_shapes: unit
            .nominals
            .iter()
            .filter(|(name, _)| unit.opaque_nominals.contains(name))
            .map(|(_, backing)| crate::vm::shape_of(backing))
            .collect(),
    })
}

/// One function being compiled. `Compiler::states` is a stack of these, so a nested
/// lambda can see the frames it is written inside.
struct FnState {
    code: Vec<Op>,
    spans: Vec<crate::ast::NodeId>,
    consts: Vec<Value>,
    /// Name → register, innermost last. A linear scan, but at COMPILE time and over a
    /// single function's names — the run-time scan this replaces walked every scope of
    /// every enclosing call, comparing strings, on every identifier.
    locals: Vec<Local>,
    /// One entry per enclosing loop, holding the `break` jumps still to be pointed at
    /// that loop's exit. Per FUNCTION, so a `break` inside a lambda inside a loop is
    /// refused rather than jumping out of a frame that is no longer running.
    loops: Vec<Vec<u32>>,
    /// Where each capture comes from, in declaration order. The index is what
    /// `LoadCap` uses at run time.
    captures: Vec<CapSource>,
    capture_names: Vec<&'static str>,
    /// Field and tag names, by index. A record literal's names are appended as a
    /// consecutive run, which is what `MakeRecord` reads.
    names: Vec<&'static str>,
    /// Literal patterns, by index — see `Op::TestLit`.
    pats: Vec<Pattern>,
    /// The next free register. Temporaries are freed by restoring this, so a register
    /// is reused by the next expression instead of the frame growing with the AST.
    next_reg: Reg,
    /// The high-water mark, which is the frame size.
    max_reg: Reg,
    /// The name this function was bound to, if it was bound by a `let`. Resolving it
    /// inside the body yields the running closure, which is how a block-local function
    /// calls itself without capturing a binding that does not exist yet.
    self_name: Option<&'static str>,
    params: Rc<Vec<&'static str>>,
    name: &'static str,
    /// The top level is not a function, so `return` there is an error rather than a
    /// silent disagreement with the tree-walker about what it means.
    in_function: bool,
}

impl FnState {
    fn new(
        name: &'static str,
        params: Rc<Vec<&'static str>>,
        self_name: Option<&'static str>,
        in_function: bool,
    ) -> Self {
        FnState {
            code: Vec::new(),
            spans: Vec::new(),
            consts: Vec::new(),
            locals: Vec::new(),
            loops: Vec::new(),
            captures: Vec::new(),
            capture_names: Vec::new(),
            names: Vec::new(),
            pats: Vec::new(),
            next_reg: 0,
            max_reg: 0,
            self_name,
            params,
            name,
            in_function,
        }
    }

    fn finish(self, chunk: ChunkId, arity: u16) -> Chunk {
        // One span per instruction, or an error's location is somebody else's. A
        // `code.push` that skipped `emit` is exactly how that goes wrong, and it did.
        debug_assert_eq!(
            self.code.len(),
            self.spans.len(),
            "chunk `{}` has {} instructions and {} spans",
            self.name,
            self.code.len(),
            self.spans.len()
        );
        Chunk {
            code: self.code,
            spans: self.spans,
            consts: self.consts,
            n_regs: self.max_reg,
            arity,
            names: self.names,
            pats: self.pats,
            bare: Rc::new(super::Closure {
                chunk,
                params: self.params.clone(),
                captures: Vec::new(),
            }),
            params: self.params,
            name: self.name,
        }
    }

    fn local(&self, name: &str) -> Option<&Local> {
        self.locals.iter().rev().find(|l| l.name == name)
    }

    fn local_mut(&mut self, name: &str) -> Option<&mut Local> {
        self.locals.iter_mut().rev().find(|l| l.name == name)
    }
}

/// A name bound in a function's own frame.
struct Local {
    name: &'static str,
    reg: Reg,
    /// Declared with `var`, so it may be assigned.
    is_var: bool,
    /// A closure has copied this value. Assigning it afterwards would leave that copy
    /// stale, so the assignment is refused — see `Compiler::upvalue`.
    captured: bool,
}

struct Compiler {
    tops: Tops,
    chunks: Vec<Option<Chunk>>,
    states: Vec<FnState>,
    /// The node being compiled, stamped onto every instruction it emits.
    node: crate::ast::NodeId,
    /// Which `BinOp` nodes may use the integer-only opcode.
    integer_binops: std::collections::HashSet<crate::ast::NodeId>,
    /// See `Unit::dispatch_modules`.
    dispatch_modules: std::collections::HashMap<crate::ast::NodeId, &'static str>,
    /// See `Unit::binop_modules`.
    binop_modules: std::collections::HashMap<crate::ast::NodeId, &'static str>,
    /// See `Unit::dec_literals`.
    dec_literals: std::collections::HashSet<crate::ast::NodeId>,
    /// See `Unit::fractional_literals`.
    fractional_literals: std::collections::HashSet<crate::ast::NodeId>,
    /// See `Unit::parse_targets`.
    parse_targets: std::collections::HashMap<crate::ast::NodeId, crate::types::Type>,
    /// Bare low-level names the builtin module declares; see `Unit::intrinsics`.
    intrinsics: std::collections::HashSet<&'static str>,
}

impl Compiler {
    /// The function being compiled.
    fn st(&mut self) -> &mut FnState {
        self.states.last_mut().expect("a function is always being compiled")
    }

    fn emit(&mut self, op: Op) {
        let node = self.node;
        let st = self.st();
        st.code.push(op);
        // Parallel to `code`: whichever node the compiler is working on owns the
        // instructions it emits, which is how a runtime error finds its line.
        st.spans.push(node);
    }

    fn alloc(&mut self) -> Result<Reg, String> {
        let st = self.st();
        let reg = st.next_reg;
        st.next_reg = reg
            .checked_add(1)
            .ok_or_else(|| "vm: a function needs more than 65535 registers".to_string())?;
        st.max_reg = st.max_reg.max(st.next_reg);
        Ok(reg)
    }

    /// Reserve registers up to and including `reg`, so later temporaries land above it.
    fn reserve(&mut self, reg: Reg) -> Result<(), String> {
        while self.st().next_reg <= reg {
            self.alloc()?;
        }
        Ok(())
    }

    fn constant(&mut self, dst: Reg, value: Value) -> Result<(), String> {
        let st = self.st();
        let k = u32::try_from(st.consts.len())
            .map_err(|_| "vm: too many constants".to_string())?;
        st.consts.push(value);
        // Through `emit`, so this instruction gets a span like every other. Pushing
        // straight onto `code` left the span table one short, and every error after
        // the first constant in a chunk lost its location.
        self.emit(Op::LoadK { dst, k });
        Ok(())
    }

    /// The index of the next instruction, for patching a jump once its target is known.
    fn here(&mut self) -> u32 {
        self.st().code.len() as u32
    }

    fn patch_to_here(&mut self, at: u32) {
        let target = self.here();
        match &mut self.st().code[at as usize] {
            Op::Jump { to }
            | Op::JumpFalse { to, .. }
            | Op::TestLit { to, .. }
            | Op::TestTag { to, .. }
            | Op::TestTuple { to, .. }
            | Op::TestRecord { to, .. }
            | Op::TestList { to, .. }
            | Op::GetFieldOr { to, .. }
            | Op::IterNext { to, .. } => *to = target,
            other => unreachable!("patched a {:?}, which is not a jump", other),
        }
    }

    /// The index of `name` in this chunk's name table, adding it if it is new.
    fn name_idx(&mut self, name: &'static str) -> Result<u16, String> {
        let st = self.st();
        if let Some(i) = st.names.iter().position(|n| *n == name) {
            return Ok(i as u16);
        }
        let idx = u16::try_from(st.names.len())
            .map_err(|_| "vm: more than 65535 names in one function".to_string())?;
        st.names.push(name);
        Ok(idx)
    }

    /// Append names as a CONSECUTIVE run and answer where it starts. A record's fields
    /// are read as one span, so they cannot be deduplicated individually.
    fn names_run(&mut self, names: &[&'static str]) -> Result<u16, String> {
        let st = self.st();
        let start = u16::try_from(st.names.len())
            .map_err(|_| "vm: more than 65535 names in one function".to_string())?;
        st.names.extend_from_slice(names);
        u16::try_from(st.names.len())
            .map_err(|_| "vm: more than 65535 names in one function".to_string())?;
        Ok(start)
    }

    fn pat_idx(&mut self, pattern: Pattern) -> Result<u16, String> {
        let st = self.st();
        let idx = u16::try_from(st.pats.len())
            .map_err(|_| "vm: more than 65535 literal patterns in one function".to_string())?;
        st.pats.push(pattern);
        Ok(idx)
    }

    /// Resolve a name the way the tree-walker's `lookup` would: innermost first.
    /// The nominal whose method block is being compiled, from the function's own name.
    ///
    /// Inside `Graph :: … .{ … }` a sibling method is in scope UNQUALIFIED — roc lets
    /// `from_list` call `from_dict(…)` — but it is bound here as `Graph.from_dict`.
    fn enclosing_type(&self) -> Option<&'static str> {
        self.states
            .iter()
            .rev()
            .find_map(|state| state.name.rsplit_once('.').map(|(owner, _)| owner))
    }

    fn resolve(&mut self, name: &'static str) -> Option<Found> {
        let level = self.states.len() - 1;
        if let Some(l) = self.states[level].local(name) {
            return Some(Found::Local(l.reg));
        }
        if self.states[level].self_name == Some(name) {
            return Some(Found::SelfRef);
        }
        match self.upvalue(level, name) {
            Err(e) => return Some(Found::Refused(e)),
            Ok(Some(idx)) => return Some(Found::Capture(idx)),
            Ok(None) => {}
        }
        if let Some(idx) = self.tops.global(name) {
            return Some(Found::Global(idx));
        }
        if let Some((c, a)) = self.tops.func(name) {
            return Some(Found::Func(c, a));
        }
        // Last: a name another module exposed. Checked after everything else so a
        // local binding of the same name wins, as it does in the tree-walker.
        let full = self.tops.alias(name)?;
        match self.resolve(full) {
            None => Some(Found::Refused(format!(
                "module does not expose `{}`",
                name
            ))),
            found => found,
        }
    }

    /// Find `name` in a function enclosing `level`, threading a capture down each
    /// level in between, and answer its capture index in `level`.
    ///
    /// This is the whole of closure conversion: after it, a captured variable is an
    /// index into a `Vec`, and nothing at run time knows it ever had a name.
    fn upvalue(&mut self, level: usize, name: &'static str) -> Result<Option<u16>, String> {
        if let Some(i) = self.states[level].capture_names.iter().position(|n| *n == name) {
            return Ok(Some(i as u16));
        }
        // Level 0 is a top-level function or the top level itself: there is no
        // enclosing frame, so an unresolved name there is a global or an error.
        let Some(parent) = level.checked_sub(1) else { return Ok(None) };

        if let Some(l) = self.states[parent].local_mut(name) {
            if l.is_var {
                // ponytail: a `var` a closure captures needs a shared cell
                // (`Rc<RefCell<Value>>`) so both see the assignments; captured by value
                // it would silently go stale, and the tree-walker's shared frames make
                // it work. Nothing in the language's own suite does this, so it is
                // refused rather than built for. Add the cell if a real program wants it.
                return Err(format!(
                    "vm: `{}` is a `var` captured by a closure, which needs a shared cell (V3 refuses it rather than copying it and going stale)",
                    name
                ));
            }
            l.captured = true;
            let reg = l.reg;
            return Ok(self.add_capture(level, name, CapSource::Local(reg)));
        }
        if self.states[parent].self_name == Some(name) {
            return Ok(self.add_capture(level, name, CapSource::Enclosing));
        }
        match self.upvalue(parent, name)? {
            None => Ok(None),
            Some(in_parent) => Ok(self.add_capture(level, name, CapSource::Capture(in_parent))),
        }
    }

    fn add_capture(&mut self, level: usize, name: &'static str, src: CapSource) -> Option<u16> {
        let st = &mut self.states[level];
        let idx = u16::try_from(st.captures.len()).ok()?;
        st.captures.push(src);
        st.capture_names.push(name);
        Some(idx)
    }

    /// Compile a lambda into `chunk`, and answer what it captured.
    fn function(
        &mut self,
        chunk: ChunkId,
        name: &'static str,
        params: &Rc<Vec<&'static str>>,
        body: &Expr,
        self_name: Option<&'static str>,
    ) -> Result<Vec<CapSource>, String> {
        self.states.push(FnState::new(name, params.clone(), self_name, true));
        for param in params.iter() {
            let reg = self.alloc()?;
            self.st().locals.push(Local { name: param, reg, is_var: false, captured: false });
        }
        // The body is in tail position by definition, which is what turns a
        // tail-recursive function into a loop.
        let result = self.tail(body);
        let st = self.states.pop().expect("pushed above");
        result?;
        let captures = st.captures.clone();
        let arity = u16::try_from(params.len()).map_err(|_| "vm: too many parameters".to_string())?;
        self.chunks[chunk as usize] = Some(st.finish(chunk, arity));
        Ok(captures)
    }

    /// Compile a lambda in an expression position: a new chunk, plus the code that
    /// gathers its captures and builds the closure.
    fn closure(
        &mut self,
        name: &'static str,
        params: &Rc<Vec<&'static str>>,
        body: &Expr,
        self_name: Option<&'static str>,
    ) -> Result<Reg, String> {
        self.chunks.push(None);
        let chunk = (self.chunks.len() - 1) as ChunkId;
        let captures = self.function(chunk, name, params, body, self_name)?;

        // The captured values go in consecutive registers, which is where
        // `MakeClosure` reads them from.
        let cap_base = self.st().next_reg;
        for (i, src) in captures.iter().enumerate() {
            let target = cap_base + i as Reg;
            self.reserve(target)?;
            match *src {
                CapSource::Local(reg) => self.emit(Op::Move { dst: target, src: reg }),
                CapSource::Capture(idx) => self.emit(Op::LoadCap { dst: target, idx }),
                CapSource::Enclosing => self.emit(Op::LoadSelf { dst: target }),
            }
        }
        let n = u16::try_from(captures.len()).map_err(|_| "vm: too many captures".to_string())?;
        self.st().next_reg = cap_base;
        let dst = self.alloc()?;
        self.emit(Op::MakeClosure { dst, chunk, base: cap_base, n });
        Ok(dst)
    }

    /// Put a call's arguments in consecutive registers and answer where they start.
    fn arguments(&mut self, args: &[Expr]) -> Result<(Reg, u16), String> {
        let refs: Vec<&Expr> = args.iter().collect();
        self.values(&refs)
    }

    /// Evaluate expressions into consecutive registers: a call's arguments, a list's
    /// elements, a record's field values. Answers where the run starts and how long it
    /// is, which is the shape every aggregate and call opcode expects.
    fn values(&mut self, args: &[&Expr]) -> Result<(Reg, u16), String> {
        let arg_base = self.st().next_reg;
        for (i, arg) in args.iter().enumerate() {
            let target = arg_base + i as Reg;
            self.reserve(target)?;
            let save = self.st().next_reg;
            let reg = self.expr(arg)?;
            if reg != target {
                self.emit(Op::Move { dst: target, src: reg });
            }
            // Free the argument's own temporaries but keep the argument itself.
            self.st().next_reg = save;
        }
        let argc = u16::try_from(args.len()).map_err(|_| "vm: too many arguments".to_string())?;
        Ok((arg_base, argc))
    }

    /// A call whose callee is a top-level function, if this one is.
    fn direct_callee(&mut self, func: &Expr) -> Option<(ChunkId, u16)> {
        let name = match func {
            Expr::Ident(name, _) => *name,
            _ => return None,
        };
        // A local, a capture or a self-reference shadows a top-level name, so those
        // have to be checked first — the callee is then a value, not a chunk.
        match self.resolve(name) {
            Some(Found::Func(chunk, arity)) => Some((chunk, arity)),
            _ => None,
        }
    }

    /// A statement at the top level, where `expect` means something different.
    ///
    /// A top-level `expect` is a TEST: `roc test` runs it and tallies it, and a normal
    /// `roc run` skips it entirely — the condition is never evaluated, so a call it
    /// makes has no effects either. Every other statement runs both ways, and so does
    /// an `expect` inside a function body, which is a runtime assertion rather than a
    /// test and is never tallied.
    fn top_statement(&mut self, statement: &Expr, test_mode: bool) -> Result<(), String> {
        let Expr::Expect(condition, _) = statement else {
            self.expr(statement)?;
            return Ok(());
        };
        if !test_mode {
            return Ok(());
        }
        let cond = self.expr(condition)?;
        self.emit(Op::TestExpect { cond });
        Ok(())
    }

    /// Compile `e` in TAIL position: the code emitted ends the function, either by
    /// returning or by handing the frame to a tail call.
    fn tail(&mut self, e: &Expr) -> Result<(), String> {
        let enclosing = std::mem::replace(&mut self.node, e.id());
        let result = self.tail_inner(e);
        self.node = enclosing;
        result
    }

    fn tail_inner(&mut self, e: &Expr) -> Result<(), String> {
        match e {
            Expr::If { condition, then_branch, otherwise, .. } => {
                let save = self.st().next_reg;
                let cond = self.expr(condition)?;
                self.st().next_reg = save;
                let jump_to_else = self.here();
                self.emit(Op::JumpFalse { cond, to: u32::MAX, kind: CondKind::If });
                // Each branch ends the function itself, so there is no jump over the
                // else branch and no destination register to agree on.
                self.tail(then_branch)?;
                self.st().next_reg = save;
                self.patch_to_here(jump_to_else);
                self.tail(otherwise)?;
                self.st().next_reg = save;
                Ok(())
            }

            Expr::Let { name, value, body, .. } => {
                self.bind(name, value, false)?;
                let result = self.tail(body);
                self.st().locals.pop();
                result
            }

            Expr::VarDecl { name, value, body, .. } => {
                self.bind(name, value, true)?;
                let result = self.tail(body);
                self.st().locals.pop();
                result
            }

            Expr::Assign { name, value, body, .. } => {
                self.assign(name, value)?;
                self.tail(body)
            }

            // Every arm's body is in tail position too, so a function that is one
            // `match` returns straight out of the arm that matched.
            Expr::Match { scrutinee, arms, .. } => {
                self.compile_match(scrutinee, arms, true)?;
                Ok(())
            }

            // `return f(x)` is still a tail call.
            Expr::Return(inner, _) if self.st().in_function => self.tail(inner),

            Expr::Call { func, args, .. } => {
                if let Some((chunk, arity)) = self.direct_callee(func) {
                    let (arg_base, argc) = self.arguments(args)?;
                    check_arity(func_name(func), arity, argc)?;
                    self.emit(Op::TailCall { func: 0, chunk: Some(chunk), base: arg_base, argc });
                    return Ok(());
                }
                // A builtin is not a tail call: it returns a value, which this
                // function then returns. Missing this arm made every builtin call in
                // tail position compile as a call to a `Value::Builtin` instead —
                // "Attempted to call a non-function value", on 24 golden pairs.
                if let Some(src) = self.builtin_call(func, args)? {
                    self.emit(Op::Ret { src });
                    return Ok(());
                }
                // The callee is a value. It is loaded BEFORE the arguments, so its
                // register is below them and survives the arguments moving down.
                let save = self.st().next_reg;
                let callee = self.expr(func)?;
                let (arg_base, argc) = self.arguments(args)?;
                self.st().next_reg = save;
                self.emit(Op::TailCall { func: callee, chunk: None, base: arg_base, argc });
                Ok(())
            }

            other => {
                let src = self.expr(other)?;
                self.emit(Op::Ret { src });
                Ok(())
            }
        }
    }

    /// Bind `name` to `value` in a fresh register, and push it as a local.
    ///
    /// The caller pops the local when the binding goes out of scope.
    fn bind(&mut self, name: &'static str, value: &Expr, is_var: bool) -> Result<(), String> {
        let save = self.st().next_reg;
        // A lambda bound by a `let` may call itself by name: record the name so the
        // body resolves it to the running closure rather than to a binding that does
        // not exist yet.
        let src = match value {
            Expr::Lambda { params, body, .. } => self.closure(name, params, body, Some(name))?,
            other => self.expr(other)?,
        };
        self.st().next_reg = save;
        // The binding gets a register of its own. Aliasing `src` would break as soon
        // as `src` was a temporary the next expression reuses.
        let slot = self.alloc()?;
        if slot != src {
            self.emit(Op::Move { dst: slot, src });
        }
        self.st().locals.push(Local { name, reg: slot, is_var, captured: false });
        Ok(())
    }

    /// Compile `e`, and answer which register holds its value.
    ///
    /// The register may be a local's, so nothing may write to a returned register
    /// except through an op that reads its inputs first (`Bin` does) or a fresh
    /// destination (everything else).
    fn expr(&mut self, e: &Expr) -> Result<Reg, String> {
        // Whatever this node emits is attributed to it. Restored afterwards so a
        // parent's own instructions are not blamed on its last child.
        let enclosing = std::mem::replace(&mut self.node, e.id());
        let result = self.expr_inner(e);
        self.node = enclosing;
        result
    }

    fn expr_inner(&mut self, e: &Expr) -> Result<Reg, String> {
        match e {
            // A literal the checker typed as `Dec` is a FIXED-POINT value: `Dec` keeps
            // eighteen decimal places exactly, which an f64 cannot.
            Expr::Int(n, id) if self.dec_literals.contains(id) => {
                self.literal(Value::Dec(n * crate::eval::DEC_SCALE))
            }
            // Nothing ever said what this numeral is, so it defaults — and roc's
            // default is `Dec`, not a float. Checked against the compiler: a whole
            // `F64` prints `1500`, a whole `Dec` prints `1500.0`, and `x = 1500` with
            // no annotation prints `1500.0`.
            Expr::Int(n, id) if self.fractional_literals.contains(id) => {
                self.literal(Value::Dec(n.saturating_mul(crate::eval::DEC_SCALE)))
            }
            Expr::Int(n, _) => self.literal(Value::Int(*n)),
            // The literal as it was WRITTEN, not as the nearest double to it.
            Expr::Float(_, exact, id) if self.dec_literals.contains(id) => {
                self.literal(Value::Dec(*exact))
            }
            Expr::Float(f, ..) => self.literal(Value::Float(*f)),
            Expr::Bool(b, _) => self.literal(Value::Bool(*b)),
            Expr::Str(s, _) => self.literal(Value::Str(Rc::from(*s))),
            Expr::Unit(_) => self.literal(Value::Unit),

            Expr::Ident(name, _) => self.use_name(name),

            Expr::Lambda { params, body, .. } => self.closure("<lambda>", params, body, None),

            Expr::BinOp { left, op, right, id } => {
                let save = self.st().next_reg;
                // Both sides are evaluated, `&&` and `||` included, because that is
                // what the tree-walker does — see `apply_binop`. Short-circuiting is a
                // change to the LANGUAGE's behaviour, not to the VM's, and it does not
                // belong in a phase whose gate is "identical to the tree-walker".
                let a = self.expr(left)?;
                let b = self.expr(right)?;
                self.st().next_reg = save;
                let dst = self.alloc()?;
                // An operator is dispatched only when the CHECKER says its operands are
                // a nominal that defines the matching method. `operator_methods` alone
                // is a program-wide switch: it sent every `==` through a method search,
                // so one `Try.is_eq` in scope answered for tuples and tags too.
                if self.tops.operator_methods && self.operator_dispatches(id, *op) {
                    self.emit(Op::BinDispatch { dst, a, b, op: *op });
                } else if self.integer_binops.contains(id)
                    && !matches!(op, crate::ast::BinOp::And | crate::ast::BinOp::Or)
                {
                    // The checker says both sides are integers, so the shapes need not
                    // be examined again at run time.
                    self.emit(Op::BinInt { dst, a, b, op: *op });
                } else {
                    self.emit(Op::Bin { dst, a, b, op: *op });
                }
                Ok(dst)
            }

            Expr::If { condition, then_branch, otherwise, .. } => {
                let save = self.st().next_reg;
                let cond = self.expr(condition)?;
                self.st().next_reg = save;
                // The destination is allocated before either branch, so both can land
                // their result in the same place whichever way the jump goes.
                let dst = self.alloc()?;
                let jump_to_else = self.here();
                self.emit(Op::JumpFalse { cond, to: u32::MAX, kind: CondKind::If });

                let after_cond = self.st().next_reg;
                let then_reg = self.expr(then_branch)?;
                if then_reg != dst {
                    self.emit(Op::Move { dst, src: then_reg });
                }
                self.st().next_reg = after_cond;
                let jump_to_end = self.here();
                self.emit(Op::Jump { to: u32::MAX });

                self.patch_to_here(jump_to_else);
                let else_reg = self.expr(otherwise)?;
                if else_reg != dst {
                    self.emit(Op::Move { dst, src: else_reg });
                }
                self.st().next_reg = after_cond;
                self.patch_to_here(jump_to_end);
                Ok(dst)
            }

            Expr::Let { name, value, body, .. } => {
                self.bind(name, value, false)?;
                let result = self.expr(body);
                self.st().locals.pop();
                result
            }

            // `return` in the middle of a block: the code after it is unreachable,
            // which is exactly what a jump to the function's exit means.
            Expr::Return(inner, _) if self.st().in_function => {
                let src = self.expr(inner)?;
                self.emit(Op::Ret { src });
                Ok(src)
            }
            Expr::Return(_, _) => Err("vm: `return` outside a function".to_string()),

            // `Json.parse(text)` is given the TYPE it must produce as a second
            // argument: nothing at run time can recover it, and reading `[1,2,3]` back
            // into a `List(ItemKind)` is only possible knowing it.
            Expr::Call { func, args, id }
                if matches!(&**func, Expr::Qualified { module: "Json", name: "parse", .. })
                    && self.parse_targets.contains_key(id) =>
            {
                let target = type_descriptor(&self.parse_targets[id]);
                let name = self.names_run(&["Json", "parse"])?;
                let arg_base = self.st().next_reg;
                let (_, argc) = self.arguments(args)?;
                self.st().next_reg = arg_base + argc as u16;
                let slot = self.alloc()?;
                self.constant(slot, target)?;
                self.st().next_reg = arg_base;
                let dst = self.alloc()?;
                self.emit(Op::CallBuiltin { dst, name, base: arg_base, argc: argc + 1 });
                Ok(dst)
            }

            Expr::Call { func, args, .. } => {
                if let Some((chunk, arity)) = self.direct_callee(func) {
                    let (arg_base, argc) = self.arguments(args)?;
                    check_arity(func_name(func), arity, argc)?;
                    self.st().next_reg = arg_base;
                    let dst = self.alloc()?;
                    self.emit(Op::CallFn { dst, chunk, base: arg_base, argc });
                    return Ok(dst);
                }
                if let Some(dst) = self.builtin_call(func, args)? {
                    return Ok(dst);
                }
                // The callee is a value in a register: a parameter, a capture, a
                // `let`-bound closure, or the result of another call.
                let save = self.st().next_reg;
                let callee = self.expr(func)?;
                let (arg_base, argc) = self.arguments(args)?;
                self.st().next_reg = save;
                let dst = self.alloc()?;
                self.emit(Op::Call { dst, func: callee, base: arg_base, argc });
                Ok(dst)
            }

            Expr::List(items, _) => {
                let (base, n) = self.arguments(items)?;
                self.st().next_reg = base;
                let dst = self.alloc()?;
                self.emit(Op::MakeList { dst, base, n });
                Ok(dst)
            }

            Expr::Tuple(items, _) => {
                let (base, n) = self.arguments(items)?;
                self.st().next_reg = base;
                let dst = self.alloc()?;
                self.emit(Op::MakeTuple { dst, base, n });
                Ok(dst)
            }

            Expr::Tag { name, args, .. } => {
                let name = self.name_idx(name)?;
                let (base, n) = self.arguments(args)?;
                self.st().next_reg = base;
                let dst = self.alloc()?;
                self.emit(Op::MakeTag { dst, name, base, n });
                Ok(dst)
            }

            Expr::Record(fields, _) => {
                let field_names: Vec<&'static str> = fields.iter().map(|(n, _)| *n).collect();
                let name = self.names_run(&field_names)?;
                let values: Vec<&Expr> = fields.iter().map(|(_, v)| v).collect();
                let (base, n) = self.values(&values)?;
                self.st().next_reg = base;
                let dst = self.alloc()?;
                self.emit(Op::MakeRecord { dst, name, base, n });
                Ok(dst)
            }

            Expr::RecordUpdate { base: record, fields, .. } => {
                let field_names: Vec<&'static str> = fields.iter().map(|(n, _)| *n).collect();
                let name = self.names_run(&field_names)?;
                let save = self.st().next_reg;
                let obj = self.expr(record)?;
                self.reserve(obj)?;
                let values: Vec<&Expr> = fields.iter().map(|(_, v)| v).collect();
                let (base, n) = self.values(&values)?;
                self.st().next_reg = save;
                let dst = self.alloc()?;
                self.emit(Op::UpdateRecord { dst, obj, name, base, n });
                Ok(dst)
            }

            Expr::FieldAccess { record, field, .. } => {
                let name = self.name_idx(field)?;
                let save = self.st().next_reg;
                let obj = self.expr(record)?;
                self.st().next_reg = save;
                let dst = self.alloc()?;
                self.emit(Op::GetField { dst, obj, name });
                Ok(dst)
            }

            Expr::OptionalField { record, field, .. } => {
                let name = self.name_idx(field)?;
                let save = self.st().next_reg;
                let obj = self.expr(record)?;
                self.st().next_reg = save;
                let dst = self.alloc()?;
                self.emit(Op::GetOptField { dst, obj, name });
                Ok(dst)
            }

            Expr::TupleIndex { tuple, index, .. } => {
                let i = u16::try_from(*index)
                    .map_err(|_| "vm: tuple index out of range".to_string())?;
                let save = self.st().next_reg;
                let obj = self.expr(tuple)?;
                self.st().next_reg = save;
                let dst = self.alloc()?;
                self.emit(Op::GetIndex { dst, obj, i });
                Ok(dst)
            }

            Expr::Match { scrutinee, arms, .. } => {
                let dst = self.compile_match(scrutinee, arms, false)?;
                Ok(dst.expect("a non-tail match has a destination"))
            }

            // `Module.name` as a VALUE — `xs.map(Str.inspect)`.
            Expr::Qualified { module, name, .. } => {
                // A nominal's method block binds `Type.method` as an ordinary
                // top-level name, so it resolves exactly like any other name — and it
                // need not be a function: `Counter.start = { n: 0 }` is a global.
                let qualified = qualify(module, name);
                if self.resolve(qualified).is_some() {
                    return self.use_name(qualified);
                }
                // `Bool.True` and `Bool.False` are VALUES, not nullary builtins.
                if *module == "Bool" {
                    match *name {
                        "True" => return self.literal(Value::Bool(true)),
                        "False" => return self.literal(Value::Bool(false)),
                        _ => {}
                    }
                }
                // `U64.highest` is a value too, so it is CALLED here rather than left
                // as a function to be called later — there is no later, a constant is
                // used where it stands.
                if crate::eval::is_numeric_constant(module, name) {
                    let idx = self.names_run(&[module, name])?;
                    let base = self.st().next_reg;
                    self.reserve(base)?;
                    self.st().next_reg = base;
                    let dst = self.alloc()?;
                    self.emit(Op::CallBuiltin { dst, name: idx, base, argc: 0 });
                    return Ok(dst);
                }
                let name = self.name_idx(qualified)?;
                let dst = self.alloc()?;
                self.emit(Op::MakeBuiltin { dst, name });
                Ok(dst)
            }

            Expr::StrInterp(parts, _) => self.interpolation(parts),

            Expr::Dispatch { receiver, method, args, id } => {
                self.dispatch(receiver, method, args, *id)
            }

            // The three statement forms. Each yields `{}`, as in the tree-walker.
            Expr::Expect(condition, _) => {
                let save = self.st().next_reg;
                let cond = self.expr(condition)?;
                self.st().next_reg = save;
                self.emit(Op::Expect { cond });
                self.literal(Value::Unit)
            }

            Expr::Dbg(value, _) => {
                let save = self.st().next_reg;
                let src = self.expr(value)?;
                self.st().next_reg = save;
                self.emit(Op::Dbg { src });
                self.literal(Value::Unit)
            }

            Expr::Crash(message, _) => {
                let save = self.st().next_reg;
                let src = self.expr(message)?;
                self.st().next_reg = save;
                self.emit(Op::Crash { src });
                // Never reached: `Crash` leaves the program. Every expression still has
                // to answer with a register.
                self.literal(Value::Unit)
            }

            Expr::Range { start, end, inclusive, .. } => {
                let save = self.st().next_reg;
                let a = self.expr(start)?;
                let b = self.expr(end)?;
                self.st().next_reg = save;
                let dst = self.alloc()?;
                self.emit(Op::MakeRange { dst, start: a, end: b, inclusive: *inclusive });
                Ok(dst)
            }

            // `var x = value` then the rest of the block. Like a `let`, except the
            // binding may be assigned — which, compiled, is a write to its register.
            Expr::VarDecl { name, value, body, .. } => {
                self.bind(name, value, true)?;
                let result = self.expr(body);
                self.st().locals.pop();
                result
            }

            Expr::Assign { name, value, body, .. } => {
                self.assign(name, value)?;
                self.expr(body)
            }

            Expr::For { name, iterable, body, .. } => {
                self.for_loop(name, iterable, body)?;
                // `for` is a statement: its value is `{}`, as in the tree-walker.
                self.literal(Value::Unit)
            }

            Expr::While { condition, body, .. } => {
                self.while_loop(condition, body)?;
                self.literal(Value::Unit)
            }

            Expr::Break(_) => {
                if self.st().loops.is_empty() {
                    // The tree-walker raises a break signal that nothing catches, so
                    // what a bare `break` does there is not worth copying.
                    return Err("vm: `break` outside a loop".to_string());
                }
                let at = self.here();
                self.emit(Op::Jump { to: u32::MAX });
                self.st().loops.last_mut().expect("checked above").push(at);
                // `break` leaves the loop, so nothing reads this register — but every
                // expression has to answer with one.
                self.literal(Value::Unit)
            }

            // No catch-all. Every `Expr` variant is compiled, and an exhaustive match
            // is what keeps it that way: a new one added to the AST is a compile error
            // here rather than a program the VM quietly refuses at run time.
        }
    }

    /// A `match`, as a chain of compare-and-branch.
    ///
    /// Every arm's tests jump to the next alternative on failure, so a match costs the
    /// tests it actually runs and nothing else. The tree-walker instead allocates a
    /// `Vec` of bindings per pattern attempt, pushes a scope, and binds each name into
    /// it — for every arm it tries, not just the one that wins.
    ///
    /// In tail position each arm's body ends the function itself, so there is no result
    /// register and no jump to a common exit: `area = |s| match s { ... }` returns
    /// straight out of the arm that matched.
    fn compile_match(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        tail: bool,
    ) -> Result<Option<Reg>, String> {
        let v = self.expr(scrutinee)?;
        // The scrutinee is read by every arm, so its register stays allocated for the
        // whole match rather than being freed as an ordinary temporary.
        self.reserve(v)?;
        let dst = if tail { None } else { Some(self.alloc()?) };
        let arm_base = self.st().next_reg;

        let mut ends: Vec<u32> = Vec::new();
        for arm in arms {
            // `A | B => body` compiles the body once per alternative. Sharing it would
            // mean the alternatives had to agree on which register each binding lives
            // in, which they do not.
            for pattern in &arm.patterns {
                let locals_before = self.st().locals.len();
                let mut fails: Vec<u32> = Vec::new();
                self.pattern(pattern, v, &mut fails)?;

                if let Some(guard) = &arm.guard {
                    // The guard sees the pattern's bindings, and a false guard skips
                    // the arm rather than failing the match.
                    let save = self.st().next_reg;
                    let cond = self.expr(guard)?;
                    self.st().next_reg = save;
                    fails.push(self.here());
                    self.emit(Op::JumpFalse { cond, to: u32::MAX, kind: CondKind::Guard });
                }

                match dst {
                    None => self.tail(&arm.body)?,
                    Some(dst) => {
                        let save = self.st().next_reg;
                        let body = self.expr(&arm.body)?;
                        if body != dst {
                            self.emit(Op::Move { dst, src: body });
                        }
                        self.st().next_reg = save;
                        ends.push(self.here());
                        self.emit(Op::Jump { to: u32::MAX });
                    }
                }

                for at in fails {
                    self.patch_to_here(at);
                }
                // The bindings and registers of a failed arm are dead: the next arm
                // reuses the registers, and no name it did not bind is in scope.
                self.st().locals.truncate(locals_before);
                self.st().next_reg = arm_base;
            }
        }

        // roc requires the arms to be exhaustive and this interpreter cannot verify
        // that, so falling off the end reports it rather than inventing a value.
        self.emit(Op::NoMatch { obj: v });
        for at in ends {
            self.patch_to_here(at);
        }
        Ok(dst)
    }

    /// Emit the tests that `pattern` implies, and the destructuring its sub-patterns
    /// need. Every test's jump is collected in `fails`, for the caller to point at the
    /// next alternative.
    ///
    /// Bindings become locals. A binding written before a LATER test fails is simply
    /// dead — the register is reused and the local is popped — which is the compiled
    /// equivalent of the tree-walker throwing away a half-filled bindings vector.
    fn pattern(&mut self, pattern: &Pattern, v: Reg, fails: &mut Vec<u32>) -> Result<(), String> {
        match pattern {
            Pattern::Wildcard => Ok(()),

            Pattern::Binding(name) => {
                let slot = self.alloc()?;
                self.emit(Op::Move { dst: slot, src: v });
                self.st().locals.push(Local { name, reg: slot, is_var: false, captured: false });
                Ok(())
            }

            Pattern::Int(_) | Pattern::Float(_) | Pattern::Str(_) => {
                let pat = self.pat_idx(pattern.clone())?;
                fails.push(self.here());
                self.emit(Op::TestLit { obj: v, pat, to: u32::MAX });
                Ok(())
            }

            Pattern::Tag { name, args } => {
                let name_idx = self.name_idx(name)?;
                let n = u16::try_from(args.len())
                    .map_err(|_| "vm: too many tag arguments".to_string())?;
                fails.push(self.here());
                self.emit(Op::TestTag { obj: v, name: name_idx, n, to: u32::MAX });
                for (i, arg) in args.iter().enumerate() {
                    if matches!(arg, Pattern::Wildcard) {
                        continue;
                    }
                    let elem = self.alloc()?;
                    self.emit(Op::GetPayload { dst: elem, obj: v, i: i as u16 });
                    self.pattern(arg, elem, fails)?;
                }
                Ok(())
            }

            Pattern::Tuple(items) => {
                let n = u16::try_from(items.len())
                    .map_err(|_| "vm: too many tuple elements".to_string())?;
                fails.push(self.here());
                self.emit(Op::TestTuple { obj: v, n, to: u32::MAX });
                for (i, item) in items.iter().enumerate() {
                    if matches!(item, Pattern::Wildcard) {
                        continue;
                    }
                    let elem = self.alloc()?;
                    self.emit(Op::GetIndex { dst: elem, obj: v, i: i as u16 });
                    self.pattern(item, elem, fails)?;
                }
                Ok(())
            }

            Pattern::Record { fields, rest } => {
                fails.push(self.here());
                self.emit(Op::TestRecord { obj: v, to: u32::MAX });
                for (field, sub) in fields {
                    let name = self.name_idx(field)?;
                    let slot = self.alloc()?;
                    // A record that lacks a named field is not a match, not an error,
                    // so this read is itself one of the tests.
                    fails.push(self.here());
                    self.emit(Op::GetFieldOr { dst: slot, obj: v, name, to: u32::MAX });
                    self.pattern(sub, slot, fails)?;
                }
                if let Some(rest_name) = rest {
                    // `..rest` binds every field the pattern did NOT name.
                    let named: Vec<&'static str> = fields.iter().map(|(f, _)| *f).collect();
                    let name = self.names_run(&named)?;
                    let n = u16::try_from(named.len())
                        .map_err(|_| "vm: too many record fields".to_string())?;
                    let slot = self.alloc()?;
                    self.emit(Op::GetRest { dst: slot, obj: v, name, n });
                    self.st()
                        .locals
                        .push(Local { name: rest_name, reg: slot, is_var: false, captured: false });
                }
                Ok(())
            }

            Pattern::List { before, rest, after } => {
                let fixed = before.len() + after.len();
                let n = u16::try_from(fixed).map_err(|_| "vm: list pattern too long".to_string())?;
                fails.push(self.here());
                // Without a `..` the length has to be exact; with one the list only has
                // to be long enough to cover the fixed patterns.
                self.emit(Op::TestList { obj: v, n, exact: rest.is_none(), to: u32::MAX });

                for (i, item) in before.iter().enumerate() {
                    if matches!(item, Pattern::Wildcard) {
                        continue;
                    }
                    let elem = self.alloc()?;
                    self.emit(Op::GetElem { dst: elem, obj: v, i: i as u16, from_end: false });
                    self.pattern(item, elem, fails)?;
                }
                // The trailing patterns are positioned from the END, since what `..`
                // absorbed is only known at run time.
                for (j, item) in after.iter().enumerate() {
                    if matches!(item, Pattern::Wildcard) {
                        continue;
                    }
                    let from_end = (after.len() - 1 - j) as u16;
                    let elem = self.alloc()?;
                    self.emit(Op::GetElem { dst: elem, obj: v, i: from_end, from_end: true });
                    self.pattern(item, elem, fails)?;
                }
                if let Some(Some(rest_name)) = rest {
                    let slot = self.alloc()?;
                    self.emit(Op::GetSlice {
                        dst: slot,
                        obj: v,
                        front: before.len() as u16,
                        back: after.len() as u16,
                    });
                    self.st()
                        .locals
                        .push(Local { name: rest_name, reg: slot, is_var: false, captured: false });
                }
                Ok(())
            }
        }
    }

    /// Emit the code that reads `name`, wherever the compiler decided it lives.
    fn use_name(&mut self, name: &'static str) -> Result<Reg, String> {
        match self.resolve(name) {
            Some(Found::Local(reg)) => Ok(reg),
            Some(Found::SelfRef) => {
                let dst = self.alloc()?;
                self.emit(Op::LoadSelf { dst });
                Ok(dst)
            }
            Some(Found::Capture(idx)) => {
                let dst = self.alloc()?;
                self.emit(Op::LoadCap { dst, idx });
                Ok(dst)
            }
            Some(Found::Global(idx)) => {
                let dst = self.alloc()?;
                self.emit(Op::LoadGlob { dst, idx });
                Ok(dst)
            }
            // A top-level function used as a value: a closure over nothing.
            Some(Found::Func(chunk, _)) => {
                let dst = self.alloc()?;
                self.emit(Op::MakeClosure { dst, chunk, base: dst, n: 0 });
                Ok(dst)
            }
            Some(Found::Refused(why)) => Err(why),
            None => {
                // The same sibling rule, for a method used as a VALUE rather than
                // called: `map(xs, helper)` inside the block `helper` belongs to.
                if let Some(owner) = self.enclosing_type() {
                    let qualified = qualify(owner, name);
                    if self.resolve(qualified).is_some() {
                        return self.use_name(qualified);
                    }
                }
                Err(format!("Undefined variable: {}", name))
            }
        }
    }

    /// A call to a builtin, a host effect, or a nominal's method by qualified name.
    ///
    /// `None` when the callee is none of those, and the caller falls back to calling a
    /// value in a register.
    fn builtin_call(&mut self, func: &Expr, args: &[Expr]) -> Result<Option<Reg>, String> {
        match func {
            // `Str.concat(a, b)`, or `Point.show(p)` for a nominal's own method.
            Expr::Qualified { module, name, .. } => {
                let qualified = qualify(module, name);
                // A nominal name that is not a function — `Counter.start = { n: 0 }` —
                // is a value being called, which the caller compiles as such.
                if self.tops.global(qualified).is_some() {
                    return Ok(None);
                }
                if let Some((chunk, arity)) = self.tops.func(qualified) {
                    let (arg_base, argc) = self.arguments(args)?;
                    check_arity(qualified, arity, argc)?;
                    self.st().next_reg = arg_base;
                    let dst = self.alloc()?;
                    self.emit(Op::CallFn { dst, chunk, base: arg_base, argc });
                    return Ok(Some(dst));
                }
                let name = self.names_run(&[module, name])?;
                let (arg_base, argc) = self.arguments(args)?;
                self.st().next_reg = arg_base;
                let dst = self.alloc()?;
                self.emit(Op::CallBuiltin { dst, name, base: arg_base, argc });
                Ok(Some(dst))
            }

            Expr::Ident(bare, _) => {
                // A local, a capture or a global shadows all of this: a name bound in
                // the program is called as a value, not as a builtin.
                if self.resolve(bare).is_some() {
                    return Ok(None);
                }
                // A SIBLING method, called by its bare name from inside the same
                // method block: `from_list = |l| from_dict(…)` inside `Graph`.
                if let Some(owner) = self.enclosing_type() {
                    if let Some((chunk, arity)) = self.tops.func(qualify(owner, bare)) {
                        let (arg_base, argc) = self.arguments(args)?;
                        check_arity(bare, arity, argc)?;
                        self.st().next_reg = arg_base;
                        let dst = self.alloc()?;
                        self.emit(Op::CallFn { dst, chunk, base: arg_base, argc });
                        return Ok(Some(dst));
                    }
                }
                // An effect of the default host. `!` is part of the name.
                if crate::platform::host::lookup(bare).is_some() {
                    let name = self.name_idx(bare)?;
                    let (arg_base, argc) = self.arguments(args)?;
                    self.st().next_reg = arg_base;
                    let dst = self.alloc()?;
                    self.emit(Op::CallHost { dst, name, base: arg_base, argc });
                    return Ok(Some(dst));
                }
                // A LOW-LEVEL op: a bare name `Builtin.roc` calls but never defines,
                // which the real compiler injects and rocflight answers from Rust.
                // Sent under a module of its own, because Roc has no module for these.
                if crate::eval::low_level_arity(bare).is_some() || self.intrinsics.contains(bare) {
                    if let Some(arity) = crate::eval::low_level_arity(bare) {
                        check_arity(bare, arity as u16, args.len() as u16)?;
                    }
                    let name = self.names_run(&["LowLevel", bare])?;
                    let (arg_base, argc) = self.arguments(args)?;
                    self.st().next_reg = arg_base;
                    let dst = self.alloc()?;
                    self.emit(Op::CallBuiltin { dst, name, base: arg_base, argc });
                    return Ok(Some(dst));
                }
                // Bare `to_str(x)`. The tree-walker stringifies any value here, which
                // is what `Num.to_str` does, so it goes to the same place.
                if *bare == "to_str" {
                    let name = self.names_run(&["Num", "to_str"])?;
                    let (arg_base, argc) = self.arguments(args)?;
                    self.st().next_reg = arg_base;
                    let dst = self.alloc()?;
                    self.emit(Op::CallBuiltin { dst, name, base: arg_base, argc });
                    return Ok(Some(dst));
                }
                Ok(None)
            }

            _ => Ok(None),
        }
    }

    /// Does this operator go through a method?
    ///
    /// Yes when the checker named a module that defines one. Yes ALSO when it named no
    /// module at all: roc erases nominals, so an unannotated `Money.{ cents: 5 }` is
    /// just a record here and its `plus` has to stay reachable. No when the module is
    /// named and has no such method — which is what keeps `1 + 2`, `"a" == "b"` and a
    /// tuple comparison away from whatever `is_eq` happens to be in scope.
    fn operator_dispatches(&self, node: &crate::ast::NodeId, op: crate::ast::BinOp) -> bool {
        let Some(method) = operator_method_name(op) else { return false };
        match self.binop_modules.get(node) {
            Some(module) => self.tops.func(qualify(module, method)).is_some(),
            None => true,
        }
    }

    /// `receiver.method(args)`.
    ///
    /// A nominal's own method block wins, and the compiler can resolve it: it is a
    /// top-level function whose name ends in `.method`. Everything else depends on the
    /// receiver's type at run time and becomes `DispatchMethod`.
    fn dispatch(
        &mut self,
        receiver: &Expr,
        method: &'static str,
        args: &[Expr],
        node: crate::ast::NodeId,
    ) -> Result<Reg, String> {
        let all = self.tops.methods(method);

        // What the CHECKER says the receiver is. A method name alone cannot pick a
        // definition once more than one type defines it, and `Builtin.roc` has every
        // type defining `map`, `len`, `is_eq` and `to_hash`. With the receiver's module
        // in hand the choice is exact: `Type.method` if that type defines one, and
        // otherwise the builtin for that module, whatever else is in scope.
        let candidates: Vec<(&'static str, ChunkId, u16)> =
            match self.dispatch_modules.get(&node) {
                Some(module) => {
                    let owner = qualify(module, method);
                    all.iter().copied().filter(|(name, ..)| *name == owner).collect()
                }
                // No module: a bare record or tag, or a variable a `where` clause
                // covers. Unchanged from before — the single candidate by name, or a
                // runtime dispatch on the value.
                None => all,
            };

        // With no module named, the choice belongs to the running value: `DispatchMethod`
        // looks the method up by what the receiver turns out to be, and falls back to a
        // uniquely-named one for a nominal, which is a bare record at run time. Picking
        // the only candidate HERE is what made a loaded `Stream` answer `xs.map(f)`.
        let candidates: Vec<(&'static str, ChunkId, u16)> =
            if self.dispatch_modules.contains_key(&node) { candidates } else { Vec::new() };

        // The receiver is the first argument either way — which is why roc's builtins
        // take their subject first: `xs.map(f)` is `List.map(xs, f)`.
        let arg_base = self.st().next_reg;
        self.reserve(arg_base)?;
        let save = self.st().next_reg;
        let got = self.expr(receiver)?;
        if got != arg_base {
            self.emit(Op::Move { dst: arg_base, src: got });
        }
        self.st().next_reg = save;
        let (_, rest) = self.arguments(args)?;
        let argc = rest + 1;
        self.st().next_reg = arg_base;
        let dst = self.alloc()?;

        match candidates.first() {
            Some(&(name, chunk, arity)) => {
                check_arity(name, arity, argc)?;
                self.emit(Op::CallFn { dst, chunk, base: arg_base, argc });
            }
            None => {
                let name = self.name_idx(method)?;
                self.emit(Op::DispatchMethod { dst, name, base: arg_base, argc });
            }
        }
        Ok(dst)
    }

    /// `"a${x}b"` — the literal segments and the values between them.
    ///
    /// Normalised to strict alternation at compile time: n values and n+1 literals,
    /// with empty literals inserted where the source has two expressions in a row. The
    /// opcode then needs no structure of its own.
    fn interpolation(&mut self, parts: &[StrPart]) -> Result<Reg, String> {
        let mut literals: Vec<&'static str> = vec![""];
        let mut exprs: Vec<&Expr> = Vec::new();
        for part in parts {
            match part {
                StrPart::Literal(text) => {
                    // Two literals in a row would need joining, which the parser does
                    // not produce; a literal after a value starts a new segment.
                    let last = literals.last_mut().expect("seeded with one");
                    if last.is_empty() {
                        *last = text;
                    } else {
                        return Err("vm: two string literals in a row".to_string());
                    }
                }
                StrPart::Expr(e) => {
                    exprs.push(e);
                    literals.push("");
                }
            }
        }
        let name = self.names_run(&literals)?;
        let (base, n) = self.values(&exprs)?;
        self.st().next_reg = base;
        let dst = self.alloc()?;
        self.emit(Op::Interp { dst, name, base, n });
        Ok(dst)
    }

    /// `x = value` where `x` already exists: a write to wherever it lives.
    ///
    /// The tree-walker's `assign` walks the scopes and updates the binding in place;
    /// this resolves it once, at compile time, to a register or a global slot.
    fn assign(&mut self, name: &'static str, value: &Expr) -> Result<(), String> {
        let save = self.st().next_reg;
        let src = self.expr(value)?;
        self.st().next_reg = save;

        if let Some(l) = self.st().local(name) {
            let (reg, captured) = (l.reg, l.captured);
            if captured {
                // A closure has already copied this value, so assigning it now would
                // leave that copy stale. The same shared cell that a captured `var`
                // would need fixes this too.
                return Err(format!(
                    "vm: `{}` is assigned after a closure captured it, which needs a shared cell",
                    name
                ));
            }
            if reg != src {
                self.emit(Op::Move { dst: reg, src });
            }
            return Ok(());
        }
        if let Some(idx) = self.tops.global(name) {
            self.emit(Op::StoreGlob { idx, src });
            return Ok(());
        }
        // The tree-walker's wording: an assignment to a name that does not exist is
        // almost always a missing `var`.
        Err(format!("Cannot assign to `{}`: it is not declared with `var`", name))
    }

    /// `for x in iterable { body }` — one `IterNext` per iteration, and for a range
    /// no list is ever built.
    fn for_loop(&mut self, name: &'static str, iterable: &Expr, body: &Expr) -> Result<(), String> {
        let save = self.st().next_reg;
        let iter = self.expr(iterable)?;
        self.reserve(iter)?;
        // The position reached so far. A register, so the loop needs no state outside
        // the frame and nothing to allocate.
        let idx = self.alloc()?;
        self.constant(idx, Value::Int(0))?;
        let item = self.alloc()?;

        // The `IterNext` is both the top of the loop and the test that leaves it:
        // the body jumps back to it, and its own `to` points past the loop.
        let top = self.here();
        self.emit(Op::IterNext { dst: item, iter, idx, to: u32::MAX });

        self.st().locals.push(Local { name, reg: item, is_var: false, captured: false });
        self.st().loops.push(Vec::new());
        let body_base = self.st().next_reg;
        let result = self.expr(body);
        self.st().next_reg = body_base;
        let breaks = self.st().loops.pop().expect("pushed above");
        self.st().locals.pop();
        result?;

        self.emit(Op::Jump { to: top });
        self.patch_to_here(top);
        for at in breaks {
            self.patch_to_here(at);
        }
        self.st().next_reg = save;
        Ok(())
    }

    fn while_loop(&mut self, condition: &Expr, body: &Expr) -> Result<(), String> {
        let save = self.st().next_reg;
        let top = self.here();
        let cond = self.expr(condition)?;
        self.st().next_reg = save;
        let failed = self.here();
        self.emit(Op::JumpFalse { cond, to: u32::MAX, kind: CondKind::While });

        self.st().loops.push(Vec::new());
        let result = self.expr(body);
        self.st().next_reg = save;
        let breaks = self.st().loops.pop().expect("pushed above");
        result?;

        self.emit(Op::Jump { to: top });
        self.patch_to_here(failed);
        for at in breaks {
            self.patch_to_here(at);
        }
        Ok(())
    }

    fn literal(&mut self, value: Value) -> Result<Reg, String> {
        let dst = self.alloc()?;
        self.constant(dst, value)?;
        Ok(dst)
    }
}

/// A direct call's arity is known at compile time, so a wrong one need not wait for
/// run time. The name makes it a better message than the tree-walker's.
fn check_arity(name: &str, arity: u16, argc: u16) -> Result<(), String> {
    if arity == argc {
        return Ok(());
    }
    Err(format!("`{}` expects {} argument(s), got {}", name, arity, argc))
}

/// `Module.name`, as one `&'static str` the name tables and lookups can share.
///
/// Leaked, like every other identifier the parser produces: these live as long as the
/// program does.
fn qualify(module: &str, name: &str) -> &'static str {
    Box::leak(format!("{}.{}", module, name).into_boxed_str())
}

fn func_name(func: &Expr) -> &'static str {
    match func {
        Expr::Ident(name, _) => name,
        _ => "a function",
    }
}


/// The method an operator is sugar for. Agrees with `eval::dispatch_operator`, which
/// makes the same mapping when the call actually runs.
fn operator_method_name(op: crate::ast::BinOp) -> Option<&'static str> {
    use crate::ast::BinOp;
    Some(match op {
        BinOp::Add => "plus",
        BinOp::Sub => "minus",
        BinOp::Mul => "times",
        BinOp::Div => "div_by",
        BinOp::IntDiv => "div_trunc_by",
        BinOp::Rem => "rem_by",
        BinOp::Lt => "is_lt",
        BinOp::Gt => "is_gt",
        BinOp::Le => "is_lte",
        BinOp::Ge => "is_gte",
        BinOp::Eq | BinOp::Ne => "is_eq",
        _ => return None,
    })
}

/// A type as a runtime value, for the builtins that must read one.
///
/// Only the shape a JSON reader needs: a list of what, a nominal by name, and a
/// stopping point for everything else — where the reader falls back to reading the
/// document as it stands.
fn type_descriptor(ty: &crate::types::Type) -> Value {
    use crate::types::Type;
    match ty {
        Type::List(inner) => Value::tag("List", vec![type_descriptor(inner)]),
        Type::Nominal { name, .. } => Value::tag(
            "Nominal",
            vec![crate::eval::str_value(name.clone())],
        ),
        // `Try(a, e)` is how a parse result is written, and the `a` is what to read.
        Type::TagUnion { tags, .. } => match tags.iter().find(|(tag, _)| tag == "Ok") {
            Some((_, payload)) if payload.len() == 1 => type_descriptor(&payload[0]),
            _ => Value::Unit,
        },
        _ => Value::Unit,
    }
}
