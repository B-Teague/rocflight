//! Abstract Syntax Tree definitions for Roc
//!
//! Phase 1: String literals
//! Phase 2: Numbers, identifiers
//! Phase 3: Lambdas, calls, let bindings
//! Phase 4: Builtins, lambdas, qualified names

use std::fmt;

/// Binary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    /// Truncating integer division: `//`
    IntDiv,
    /// Remainder: `%`
    Rem,
    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    // Logical
    And,
    Or,
}

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BinOp::Add => write!(f, "+"),
            BinOp::Sub => write!(f, "-"),
            BinOp::Mul => write!(f, "*"),
            BinOp::Div => write!(f, "/"),
            BinOp::IntDiv => write!(f, "//"),
            BinOp::Rem => write!(f, "%"),
            BinOp::Eq => write!(f, "=="),
            BinOp::Ne => write!(f, "!="),
            BinOp::Lt => write!(f, "<"),
            BinOp::Le => write!(f, "<="),
            BinOp::Gt => write!(f, ">"),
            BinOp::Ge => write!(f, ">="),
            BinOp::And => write!(f, "&&"),
            BinOp::Or => write!(f, "||"),
        }
    }
}

/// Top-level expression
#[derive(Debug, Clone)]
pub enum Expr<'a> {
    /// String literal: "hello"
    Str(&'static str),
    /// String interpolation: "x=${expr}"
    StrInterp(Vec<StrPart<'a>>),
    /// Integer literal: 42, -3
    Int(i64),
    /// Float literal: 3.14, -2.5
    Float(f64),
    /// Identifier: x, main
    Ident(&'static str),
    /// Qualified name: Module.function
    Qualified {
        module: &'static str,
        name: &'static str,
    },
    /// Binary operation: left op right
    BinOp {
        left: Box<Expr<'a>>,
        op: BinOp,
        right: Box<Expr<'a>>,
    },
    /// Lambda function: |x| body or |x, y| x + y
    Lambda {
        params: Vec<&'static str>,
        body: Box<Expr<'a>>,
    },
    /// Function call: f(x) or add(1, 2)
    Call {
        func: Box<Expr<'a>>,
        args: Vec<Expr<'a>>,
    },
    /// Let binding: `x = value` followed by `body`.
    ///
    /// `annotation` is the declared type from a preceding `x : Type` line, when there
    /// was one. Carrying it here is what lets the checker know an identifier's type,
    /// reject a tag outside a closed union, and check a `match` for exhaustiveness.
    Let {
        name: &'static str,
        annotation: Option<crate::types::Type>,
        value: Box<Expr<'a>>,
        body: Box<Expr<'a>>,
    },
    /// Empty record `{}` — Roc's unit value.
    Unit,
    /// Record literal: `{ x: 1, y: 2 }`. Fields keep source order here; only
    /// `Str.inspect` sorts them.
    Record(Vec<(&'static str, Expr<'a>)>),
    /// Boolean literal, from `Bool.True` / `Bool.False`.
    Bool(bool),
    /// List literal: `[1, 2, 3]`, `[]`.
    List(Vec<Expr<'a>>),
    /// Record update: `{ ..base, field: value }`.
    ///
    /// Builds a NEW record from `base` with the named fields replaced; records are not
    /// mutated. Note the spelling — `{ base & field: value }` is rejected by roc.
    RecordUpdate {
        base: Box<Expr<'a>>,
        fields: Vec<(&'static str, Expr<'a>)>,
    },
    /// Range: `0..<3` (exclusive) or `1..=3` (inclusive).
    ///
    /// NOT a list — roc inspects a range as `<opaque>` and rejects passing one where a
    /// `List` is wanted. It is iterable by `for`.
    Range {
        start: Box<Expr<'a>>,
        end: Box<Expr<'a>>,
        inclusive: bool,
    },
    /// Tuple literal: `("Roc", 1)`.
    ///
    /// Heterogeneous and fixed-length, unlike a list. `(1)` is NOT a one-tuple — it
    /// is a parenthesised expression — so this always holds two or more elements.
    Tuple(Vec<Expr<'a>>),
    /// Positional tuple access: `pair.0`. Zero-based.
    TupleIndex {
        tuple: Box<Expr<'a>>,
        index: usize,
    },
    /// Pattern match: `match scrutinee { pattern => body ... }`.
    ///
    /// An expression, like `if`: every arm's body has the same type. Arms are tried
    /// in order and the first whose pattern matches (and whose guard holds) wins, so
    /// order is significant.
    Match {
        scrutinee: Box<Expr<'a>>,
        arms: Vec<MatchArm<'a>>,
    },
    /// Conditional expression: `if cond a else b`.
    ///
    /// An expression, not a statement, so `else` is mandatory — roc rejects a bare
    /// `if`. `else if` is not a separate form: it is an `If` whose `otherwise` is
    /// another `If`.
    If {
        condition: Box<Expr<'a>>,
        then_branch: Box<Expr<'a>>,
        otherwise: Box<Expr<'a>>,
    },
    /// `var x = value` then `body` — a REBINDABLE binding.
    ///
    /// Distinct from `Let`: a plain binding cannot be reassigned (roc reports it as a
    /// redeclaration), and only a `var` may appear on the left of an assignment.
    VarDecl {
        name: &'static str,
        value: Box<Expr<'a>>,
        body: Box<Expr<'a>>,
    },
    /// `x = value` where `x` is an existing `var` — updates it IN PLACE.
    ///
    /// Shadowing would be wrong: a loop body runs in its own scope, so a new binding
    /// there would be discarded and the value read after the loop unchanged.
    Assign {
        name: &'static str,
        value: Box<Expr<'a>>,
        body: Box<Expr<'a>>,
    },
    /// `for name in iterable { body }`. Evaluates to `{}`.
    For {
        name: &'static str,
        iterable: Box<Expr<'a>>,
        body: Box<Expr<'a>>,
    },
    /// `while condition { body }`. Evaluates to `{}`.
    While {
        condition: Box<Expr<'a>>,
        body: Box<Expr<'a>>,
    },
    /// `return value` — leaves the enclosing FUNCTION immediately.
    ///
    /// Unlike `break`, which leaves a loop, this unwinds to the lambda boundary.
    Return(Box<Expr<'a>>),
    /// `crash "message"` — aborts the program.
    Crash(Box<Expr<'a>>),
    /// `expect condition` — checks an assertion.
    ///
    /// A failure is REPORTED, not fatal: roc prints to stderr and carries on.
    Expect(Box<Expr<'a>>),
    /// `dbg value` — prints the value to stderr and carries on.
    Dbg(Box<Expr<'a>>),
    /// `break` — leaves the nearest enclosing loop.
    ///
    /// There is no `continue`: it crashes the roc compiler on nightly-2026-09-03, so
    /// no golden pair can be written against it.
    Break,
    /// Static dispatch: `receiver.method(args)`.
    ///
    /// Resolved through the RECEIVER'S TYPE — `n.to_str()` is `I64.to_str(n)` when
    /// `n : I64` — with the receiver becoming the first argument. That ordering is why
    /// roc's builtins take their subject first (`List.map(list, fn)`).
    ///
    /// Distinct from `FieldAccess`: `s.is_empty` reads a field, `s.is_empty()` calls a
    /// method, so the parens are what separate them.
    Dispatch {
        receiver: Box<Expr<'a>>,
        method: &'static str,
        args: Vec<Expr<'a>>,
    },
    /// Optional field access: `config.?timeout`.
    ///
    /// Yields `Ok(value)` when the field is present and `Err(MissingField)` when it is
    /// not. Only meaningful for a field declared `name ?: Type` on a nominal's backing
    /// record — using it on an ordinary field segfaults the roc compiler.
    OptionalField {
        record: Box<Expr<'a>>,
        field: &'static str,
    },
    /// Record field access: `point.x`.
    ///
    /// Distinct from `Expr::Qualified`: Roc capitalises modules and types, so a
    /// lowercase receiver (`point.x`) is a field access while an uppercase one
    /// (`Str.inspect`) is a module member.
    FieldAccess {
        record: Box<Expr<'a>>,
        field: &'static str,
    },
    /// Tag application: `Ok(x)`, `Err(e)`, or a bare tag like `Red` (no args).
    Tag {
        name: &'static str,
        args: Vec<Expr<'a>>,
    },
}

// A block `{ a = 1 \n f(a) \n expr }` is NOT its own variant: the parser lowers
// it to nested `Let`s, with non-binding statements bound to `_`.

/// One arm of a `match`: alternatives, an optional guard, and a body.
#[derive(Debug, Clone)]
pub struct MatchArm<'a> {
    /// `A | B => body` — the arm matches if ANY of these patterns match.
    pub patterns: Vec<Pattern>,
    /// `pattern if cond => body`. Checked only after the pattern matches, with the
    /// pattern's bindings in scope.
    pub guard: Option<Expr<'a>>,
    pub body: Expr<'a>,
}

/// A `match` pattern.
///
/// List patterns (`[]`, `[x, ..]`, `[1, .. as tail]`) are absent: they need lists,
/// which are a later phase. See IMPLEMENTATION_PHASES.md.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// `_` — matches anything, binds nothing.
    Wildcard,
    /// `x` — matches anything and binds it to `x`.
    Binding(&'static str),
    /// `1`, `3.5`, `"hello"` — matches an equal value.
    Int(i64),
    Float(f64),
    Str(&'static str),
    /// `Red`, `Foo(a, b)`, `Wrap(Inner(s))` — patterns nest to any depth.
    Tag {
        name: &'static str,
        args: Vec<Pattern>,
    },
    /// Tuple pattern: `(0, 0)`, `(x, 0)`. Fixed arity, matched element-wise.
    Tuple(Vec<Pattern>),
    /// Record pattern: `{ x, y }`, `Point.{ x }`, `{ email: _, ..rest }`.
    ///
    /// Each entry is a field name and the pattern matched against it; in a PATTERN a
    /// bare `x` is shorthand for `x: x` (in a record LITERAL it is not — `{ x }` there
    /// is a block).
    ///
    /// `rest` is `Some(name)` for `..name`, which binds every field NOT named into a
    /// new record — so naming a field `_` and capturing the rest removes it.
    Record {
        fields: Vec<(&'static str, Pattern)>,
        rest: Option<&'static str>,
    },
    /// List pattern: `[]`, `[a, b]`, `[1, 2, ..]`, `[2, .., 1]`, `[9, .. as tail]`.
    ///
    /// `rest` is `None` for an exact-length pattern. When present it holds the
    /// position of `..` within `before`+`after` and an optional name to bind the
    /// skipped elements to. At most one `..` per pattern.
    List {
        /// Elements matched from the front.
        before: Vec<Pattern>,
        /// `Some(name)` for `.. as name`, `Some(None)` shape handled by `rest`.
        rest: Option<Option<&'static str>>,
        /// Elements matched from the back, after the `..`.
        after: Vec<Pattern>,
    },
}

impl fmt::Display for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Pattern::Wildcard => write!(f, "_"),
            Pattern::Binding(name) => write!(f, "{}", name),
            Pattern::Int(n) => write!(f, "{}", n),
            Pattern::Float(n) => write!(f, "{}", n),
            Pattern::Str(s) => write!(f, "\"{}\"", s),
            Pattern::Tag { name, args } => {
                if args.is_empty() {
                    write!(f, "{}", name)
                } else {
                    let rendered: Vec<String> = args.iter().map(|a| a.to_string()).collect();
                    write!(f, "{}({})", name, rendered.join(", "))
                }
            }
            Pattern::Tuple(items) => {
                let rendered: Vec<String> = items.iter().map(|p| p.to_string()).collect();
                write!(f, "({})", rendered.join(", "))
            }
            Pattern::Record { fields, rest } => {
                let mut rendered: Vec<String> = fields
                    .iter()
                    .map(|(name, p)| {
                        // `{ x }` round-trips as itself when the pattern is just the
                        // field's own name.
                        if matches!(p, Pattern::Binding(b) if b == name) {
                            name.to_string()
                        } else {
                            format!("{}: {}", name, p)
                        }
                    })
                    .collect();
                if let Some(name) = rest {
                    rendered.push(format!("..{}", name));
                }
                write!(f, "{{ {} }}", rendered.join(", "))
            }
            Pattern::List { before, rest, after } => {
                let mut parts: Vec<String> = before.iter().map(|p| p.to_string()).collect();
                if let Some(binding) = rest {
                    parts.push(match binding {
                        Some(name) => format!(".. as {}", name),
                        None => "..".to_string(),
                    });
                }
                parts.extend(after.iter().map(|p| p.to_string()));
                write!(f, "[{}]", parts.join(", "))
            }
        }
    }
}

/// Part of a string interpolation
#[derive(Debug, Clone)]
pub enum StrPart<'a> {
    /// Literal part of string
    Literal(&'static str),
    /// Expression to interpolate: ${...}
    Expr(&'a Expr<'a>),
}

impl<'a> fmt::Display for Expr<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Str(s) => write!(f, "\"{}\"", s),
            Expr::StrInterp(parts) => {
                write!(f, "\"")?;
                for part in parts {
                    match part {
                        StrPart::Literal(s) => write!(f, "{}", s)?,
                        StrPart::Expr(e) => write!(f, "${{{}}}", e)?,
                    }
                }
                write!(f, "\"")
            }
            Expr::Int(n) => write!(f, "{}", n),
            Expr::Float(n) => write!(f, "{}", n),
            Expr::Ident(name) => write!(f, "{}", name),
            Expr::Unit => write!(f, "{{}}"),
            Expr::Bool(b) => write!(f, "Bool.{}", if *b { "True" } else { "False" }),
            Expr::List(items) => {
                let rendered: Vec<String> = items.iter().map(|i| i.to_string()).collect();
                write!(f, "[{}]", rendered.join(", "))
            }
            Expr::RecordUpdate { base, fields } => {
                let rendered: Vec<String> =
                    fields.iter().map(|(k, v)| format!("{}: {}", k, v)).collect();
                write!(f, "{{ ..{}, {} }}", base, rendered.join(", "))
            }
            Expr::Range { start, end, inclusive } => {
                write!(f, "{}..{}{}", start, if *inclusive { "=" } else { "<" }, end)
            }
            Expr::Tuple(items) => {
                let rendered: Vec<String> = items.iter().map(|i| i.to_string()).collect();
                write!(f, "({})", rendered.join(", "))
            }
            Expr::TupleIndex { tuple, index } => write!(f, "{}.{}", tuple, index),
            Expr::FieldAccess { record, field } => write!(f, "{}.{}", record, field),
            Expr::OptionalField { record, field } => write!(f, "{}.?{}", record, field),
            Expr::VarDecl { name, value, body } => {
                write!(f, "var {} = {} in {}", name, value, body)
            }
            Expr::Assign { name, value, body } => {
                write!(f, "{} := {} in {}", name, value, body)
            }
            Expr::For { name, iterable, body } => {
                write!(f, "for {} in {} {{ {} }}", name, iterable, body)
            }
            Expr::While { condition, body } => {
                write!(f, "while {} {{ {} }}", condition, body)
            }
            Expr::Return(value) => write!(f, "return {}", value),
            Expr::Crash(message) => write!(f, "crash {}", message),
            Expr::Expect(condition) => write!(f, "expect {}", condition),
            Expr::Dbg(value) => write!(f, "dbg {}", value),
            Expr::Break => write!(f, "break"),
            Expr::Dispatch { receiver, method, args } => {
                let rendered: Vec<String> = args.iter().map(|a| a.to_string()).collect();
                write!(f, "{}.{}({})", receiver, method, rendered.join(", "))
            }
            Expr::If { condition, then_branch, otherwise } => {
                write!(f, "if {} {} else {}", condition, then_branch, otherwise)
            }
            Expr::Match { scrutinee, arms } => {
                write!(f, "match {} {{", scrutinee)?;
                for arm in arms {
                    let pats: Vec<String> = arm.patterns.iter().map(|p| p.to_string()).collect();
                    write!(f, " {}", pats.join(" | "))?;
                    if let Some(guard) = &arm.guard {
                        write!(f, " if {}", guard)?;
                    }
                    write!(f, " => {}", arm.body)?;
                }
                write!(f, " }}")
            }
            Expr::Record(fields) => {
                let rendered: Vec<String> =
                    fields.iter().map(|(k, v)| format!("{}: {}", k, v)).collect();
                write!(f, "{{ {} }}", rendered.join(", "))
            }
            Expr::Tag { name, args } => {
                if args.is_empty() {
                    write!(f, "{}", name)
                } else {
                    let rendered: Vec<String> = args.iter().map(|a| a.to_string()).collect();
                    write!(f, "{}({})", name, rendered.join(", "))
                }
            }
            Expr::Qualified { module, name } => {
                write!(f, "{}.{}", module, name)
            }
            Expr::BinOp { left, op, right } => {
                write!(f, "({} {} {})", left, op, right)
            }
            Expr::Lambda { params, body } => {
                write!(f, "|{}| {}", params.join(", "), body)
            }
            Expr::Call { func, args } => {
                write!(f, "{}(", func)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ")")
            }
            Expr::Let { name, value, body, .. } => {
                write!(f, "let {} = {} in {}", name, value, body)
            }
        }
    }
}

