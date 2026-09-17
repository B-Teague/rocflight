//! Runtime values for the evaluator

use std::fmt;

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
    /// An integer. `i128`, because Roc has `U64`, `I128` and a `Dec` that is a
    /// 128-bit fixed-point value — an i64 cannot hold `U64.highest`, which
    /// `Builtin.roc` writes out in full.
    Int(i128),
    /// Float value (64-bit)
    Float(f64),
    /// A fixed-point decimal: the value times `Dec::SCALE`, exactly as roc stores it
    /// (`roc-compiler/src/builtins/dec.zig`, `decimal_places: u5 = 18`).
    ///
    /// NOT a float. `147.666666666666666666` is representable here and is not in an
    /// f64, which is the whole reason the type exists.
    Dec(i128),
    /// Builtin function marker: name + arity
    /// One of the interpreter's own functions, passed as a value — `xs.map(Str.inspect)`.
    ///
    /// `&'static str`, not `String`: the name is `Module.name` from the compiler's own
    /// tables, so it already lives as long as the program, and an owned `String` here
    /// made this the widest arm of the enum — which every other `Value` paid for.
    Builtin(&'static str, usize),
    /// A function value: a chunk to run, and the values it captured.
    ///
    /// Boxed, because this is otherwise the variant that would decide
    /// `size_of::<Value>()` — and `Value` is moved on every binding, argument, list
    /// element and return, so its width is a tax on the whole interpreter.
    Closure(std::rc::Rc<crate::vm::Closure>),
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
    Range { start: i128, end: i128, inclusive: bool },
    /// A tag value: `Ok(x)`, `Err(e)`, `Red`.
    ///
    /// The payload is behind an `Rc` for the same reason `Lambda` is: with a `Vec`
    /// inline this was the widest arm of the enum, so every `Value` in the program
    /// was as big as a tag. It also makes cloning a tag a refcount bump instead of a
    /// fresh allocation and a copy of every element, which is what passing one to a
    /// function does.
    ///
    /// Build one with `Value::tag`, which takes an ordinary `Vec`.
    Tag(&'static str, std::rc::Rc<Vec<Value>>),
}

impl Value {
    /// A tag value. Wraps the payload so call sites stay readable.
    pub fn tag(name: &'static str, payload: Vec<Value>) -> Value {
        Value::Tag(name, std::rc::Rc::new(payload))
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Str(s) => write!(f, "Str({})", s),
            Value::Int(n) => write!(f, "Int({})", n),
            Value::Float(n) => write!(f, "Float({})", n),
            Value::Dec(n) => write!(f, "Dec({})", crate::eval::dec_to_string(*n)),
            Value::Builtin(name, arity) => write!(f, "Builtin({}, {})", name, arity),
            Value::Closure(c) => write!(f, "Closure(|{}| ...)", c.params.join(", ")),
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
            Value::Dec(n) => write!(f, "{}", crate::eval::dec_to_string(*n)),
            Value::Builtin(name, arity) => write!(f, "<builtin {}/{}>", name, arity),
            Value::Closure(c) => write!(f, "<lambda |{}|>", c.params.join(", ")),
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

#[cfg(test)]
mod tests {
    use super::Value;

    /// `Value` is moved on every binding, argument, list element and return, so its
    /// width is a tax on the whole interpreter — and a register VM's main job is moving
    /// them. It was 64 bytes until `Lambda` and `Tag` were boxed. This is the guard
    /// against a new inline field quietly putting it back.
    ///
    /// 48 rather than 32 since `Int` became an `i128`: Roc has `U64`, `I128` and a
    /// fixed-point `Dec`, and `Builtin.roc` writes `U128.highest` out in full, so an
    /// i64 could not hold the language's own numbers. `i128` aligns to 16, which is
    /// what takes the enum from 32 to 48. Measured across the whole benchmark suite
    /// before it was accepted: nothing moved, and `records` got faster.
    #[test]
    fn value_stays_narrow() {
        assert_eq!(
            std::mem::size_of::<Value>(),
            48,
            "Value grew — box the new variant's payload instead"
        );
    }
}
