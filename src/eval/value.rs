//! Runtime values for the evaluator

use std::fmt;
use crate::ast::Expr;
use crate::eval::Environment;

/// Runtime value
#[derive(Clone)]
pub enum Value {
    /// String value
    /// A string.
    ///
    /// `Rc<str>` rather than `&'static str`: the only way to get a `&'static str` at
    /// runtime is to leak, and every concatenation, interpolation and `to_str` made one.
    /// Building a 40,000-character string that way leaked 710 MB and never gave any of
    /// it back. Cloning stays cheap — a refcount bump instead of a pointer copy.
    Str(std::rc::Rc<str>),
    /// Integer value (64-bit signed)
    Int(i64),
    /// Float value (64-bit)
    Float(f64),
    /// Builtin function marker: name + arity
    Builtin(String, usize),
    /// Lambda closure: params + body + captured environment
    Lambda {
        /// `Rc` for the same reason as `body`: the closure is rebuilt on every call,
        /// and a plain `Vec` meant a heap allocation per call to copy a handful of
        /// names that never change.
        params: std::rc::Rc<Vec<&'static str>>,
        /// `Rc` rather than `Box`: `apply` rebuilds the closure on every call to tie
        /// the recursive knot, and a `Box` made that a DEEP clone of the whole function
        /// body — every AST node, on every call.
        body: std::rc::Rc<Expr<'static>>,
        env: Environment,
        /// The name this closure was bound to, when it was bound by a `let`.
        ///
        /// A closure captures its environment as it was BEFORE its own binding
        /// existed, so a recursive call cannot find itself there. `apply` rebinds the
        /// closure under this name in the call frame, which ties the knot without
        /// making the environment shared and mutable.
        self_name: Option<&'static str>,
    },
    /// Empty record `{}` — Roc's unit value.
    Unit,
    /// Record value. Fields keep insertion order; `Str.inspect` sorts a copy.
    Record(Vec<(&'static str, Value)>),
    /// Boolean.
    Bool(bool),
    /// List value.
    List(Vec<Value>),
    /// Tuple value. Fixed length, elements may differ in type.
    Tuple(Vec<Value>),
    /// A range of integers, `start` to `end`, `end` included only if `inclusive`.
    ///
    /// Deliberately NOT a list: roc keeps ranges opaque, so building one as a list
    /// would show `[0, 1, 2]` where roc shows `<opaque>` and would wrongly satisfy a
    /// `List` parameter.
    Range { start: i64, end: i64, inclusive: bool },
    /// Tag value: `Ok(x)`, `Err(e)`, `Red`.
    Tag(&'static str, Vec<Value>),
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Str(s) => write!(f, "Str({})", s),
            Value::Int(n) => write!(f, "Int({})", n),
            Value::Float(n) => write!(f, "Float({})", n),
            Value::Builtin(name, arity) => write!(f, "Builtin({}, {})", name, arity),
            Value::Lambda { params, .. } => {
                write!(f, "Lambda(|{}| ...)", params.join(", "))
            }
            Value::Unit => write!(f, "Unit"),
            Value::Bool(b) => write!(f, "Bool({})", b),
            Value::List(items) => write!(f, "List({:?})", items),
            Value::Tuple(items) => write!(f, "Tuple({:?})", items),
            Value::Range { start, end, inclusive } => {
                write!(f, "Range({}..{}{})", start, if *inclusive { "=" } else { "<" }, end)
            }
            Value::Record(fields) => write!(f, "Record({:?})", fields),
            Value::Tag(name, args) => write!(f, "Tag({}, {:?})", name, args),
        }
    }
}

/// Build a `Value::Str` from anything string-shaped.
pub fn str_value(text: impl Into<std::rc::Rc<str>>) -> Value {
    Value::Str(text.into())
}

/// A string as roc shows it inside a container or an inspect: quoted, with `\\` and
/// `"` escaped. A tab or newline is left as itself — roc does not escape those.
pub fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Str(s) => write!(f, "{}", quoted(s)),
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(n) => {
                // roc prints a whole float WITHOUT a trailing `.0` — `1500.0` shows as
                // `1500` and `0.0` as `0`. Rust's own `{}` already does that.
                //
                // (An unconstrained integer literal still differs: roc defaults it to a
                // fractional type and shows `42.0`, while this interpreter keeps it an
                // integer. That is the documented numeric-default divergence, not this.)
                write!(f, "{}", n)
            }
            Value::Builtin(name, arity) => write!(f, "<builtin {}/{}>", name, arity),
            Value::Lambda { params, .. } => write!(f, "<lambda |{}|>", params.join(", ")),
            Value::Unit => write!(f, "{{}}"),
            Value::Bool(b) => write!(f, "{}", if *b { "True" } else { "False" }),
            Value::List(items) => {
                let rendered: Vec<String> = items.iter().map(|i| i.to_string()).collect();
                write!(f, "[{}]", rendered.join(", "))
            }
            Value::Tuple(items) => {
                let rendered: Vec<String> = items.iter().map(|i| i.to_string()).collect();
                write!(f, "({})", rendered.join(", "))
            }
            // roc renders a range as `<opaque>`.
            Value::Range { .. } => write!(f, "<opaque>"),
            Value::Record(fields) => {
                if fields.is_empty() {
                    return write!(f, "{{}}");
                }
                let rendered: Vec<String> =
                    fields.iter().map(|(k, v)| format!("{}: {}", k, v)).collect();
                write!(f, "{{ {} }}", rendered.join(", "))
            }
            Value::Tag(name, args) => {
                if args.is_empty() {
                    write!(f, "{}", name)
                } else {
                    let rendered: Vec<String> = args.iter().map(|a| a.to_string()).collect();
                    write!(f, "{}({})", name, rendered.join(", "))
                }
            }
        }
    }
}
