//! Roc's builtins, operators and runtime helpers.
//!
//! What is left of what used to be a tree-walking interpreter. The walker itself is
//! gone — the register VM in `crate::vm` runs every program now — and these are the
//! parts a compiled program still calls into: the forty-odd builtins, the operator
//! table, `Str.inspect`, and the handful of statement forms that do something to the
//! world (`expect`, `dbg`, `crash`, the host's `echo!`).
//!
//! They take and return `Value`s and hold no interpreter state, which is why they
//! survived the switch unchanged.

use crate::ast::{BinOp, Pattern};
use crate::error::EvalError;

pub mod value;

pub use value::{str_value, Value};


/// A nominal's own `to_inspect`, if it defines one.
fn custom_inspect(value: &Value) -> Option<Value> {
    for (_, func) in crate::vm::methods_named("to_inspect") {
        if let Ok(shown @ Value::Str(_)) = call_function(func, vec![value.clone()]) {
            return Some(shown);
        }
    }
    None
}


/// The methods every `Try` answers: `Ok(v)` and `Err(e)` are just tags, so these
/// cannot come from the builtin table, which is keyed by module.
///
/// `None` means the method is not one of these, and the caller carries on.
/// The `Try` methods — `Ok`/`Err` answer these whatever module they came from.
///
/// Takes VALUES: it always evaluated every argument up front anyway, and the VM's
/// dispatch opcode has nothing else to give it.
pub fn try_method(
    tag: &str,
    payload: &[Value],
    method: &str,
    evaluated: Vec<Value>,
) -> Result<Option<Value>, EvalError> {
    let is_ok = tag == "Ok";
    let inner = payload.first().cloned().unwrap_or(Value::Unit);
    let first = || evaluated.first().cloned().unwrap_or(Value::Unit);

    Ok(Some(match method {
        "is_ok" => Value::Bool(is_ok),
        "is_err" => Value::Bool(!is_ok),
        // Rebuild the same tag around the mapped payload; the other side passes
        // through untouched.
        "map_ok" if is_ok => Value::tag("Ok", vec![call_function(first(), vec![inner])?]),
        "map_err" if !is_ok => Value::tag("Err", vec![call_function(first(), vec![inner])?]),
        "map_ok" | "map_err" => Value::tag(
            if is_ok { "Ok" } else { "Err" },
            payload.to_vec(),
        ),
        "with_default" => {
            if is_ok {
                inner
            } else {
                first()
            }
        }
        "on_err" if !is_ok => call_function(first(), vec![inner])?,
        "on_err" => Value::tag("Ok", payload.to_vec()),
        _ => return Ok(None),
    }))
}

/// `List.*` builtins.
///
/// Argument order follows roc: the list comes first, and `fold` takes
/// `(list, initial, fn)` with the accumulator as the callback's first parameter.
/// `List.len` returns a U64 in roc — worth remembering, since mixing it with I64
/// arms in a match is a type error.
/// The elements of a List, or of a Range without ever building one.
///
/// `(1..=2_000_000).iter()` used to materialize two million `Value`s — 190 MB — before
/// the first element was looked at. A range knows its elements from three integers, so
/// the ones that only need to WALK the elements walk them instead.
enum Elements {
    List(std::vec::IntoIter<Value>),
    Range(std::ops::RangeInclusive<i64>),
}

impl Iterator for Elements {
    type Item = Value;

    fn next(&mut self) -> Option<Value> {
        match self {
            Elements::List(items) => items.next(),
            Elements::Range(range) => range.next().map(Value::Int),
        }
    }
}

impl Elements {
    /// How many elements there are, without walking them.
    fn count_of(value: &Value) -> Option<usize> {
        match value {
            Value::List(items) => Some(items.len()),
            Value::Range { start, end, inclusive } => {
                let last = if *inclusive { *end } else { *end - 1 };
                Some((last - start + 1).max(0) as usize)
            }
            _ => None,
        }
    }
}

/// A List or a Range, as something to iterate.
fn elements(value: Value, name: &str) -> Result<Elements, EvalError> {
    match value {
        Value::List(items) => Ok(Elements::List(items.into_iter())),
        Value::Range { start, end, inclusive } => {
            let last = if inclusive { end } else { end - 1 };
            Ok(Elements::Range(start..=last))
        }
        other => Err(EvalError {
            message: format!("List.{} needs a List, got {}", name, other),
        }),
    }
}

fn call_list_builtin(name: &str, args: Vec<Value>) -> Result<Value, EvalError> {
    let expect = |wanted: usize, got: usize| -> Result<(), EvalError> {
        if wanted == got {
            Ok(())
        } else {
            Err(EvalError {
                message: format!("List.{} expects {} argument(s), got {}", name, wanted, got),
            })
        }
    };

    let as_list = |v: Value| -> Result<Vec<Value>, EvalError> {
        match v {
            Value::List(items) => Ok(items),
            other => Err(EvalError {
                message: format!("List.{} needs a List, got {}", name, other),
            }),
        }
    };

    match name {
        // A range knows its length from its bounds, so neither of these walks anything.
        "len" => {
            expect(1, args.len())?;
            let count = Elements::count_of(&args[0]).ok_or_else(|| EvalError {
                message: format!("List.{} needs a List, got {}", name, args[0]),
            })?;
            Ok(Value::Int(count as i64))
        }
        "is_empty" => {
            expect(1, args.len())?;
            let count = Elements::count_of(&args[0]).ok_or_else(|| EvalError {
                message: format!("List.{} needs a List, got {}", name, args[0]),
            })?;
            Ok(Value::Bool(count == 0))
        }
        "map" => {
            expect(2, args.len())?;
            let capacity = Elements::count_of(&args[0]).unwrap_or(0);
            let items = elements(args[0].clone(), name)?;
            let func = args[1].clone();
            // The OUTPUT is a list either way, but the input need not become one first.
            let mut out = Vec::with_capacity(capacity);
            for item in items {
                out.push(call_function(func.clone(), vec![item])?);
            }
            Ok(Value::List(out))
        }
        "fold" => {
            expect(3, args.len())?;
            let items = elements(args[0].clone(), name)?;
            let mut acc = args[1].clone();
            let func = args[2].clone();
            for item in items {
                acc = call_function(func.clone(), vec![acc, item])?;
            }
            Ok(acc)
        }
        "concat" => {
            expect(2, args.len())?;
            let mut items = as_list(args[0].clone())?;
            items.extend(as_list(args[1].clone())?);
            Ok(Value::List(items))
        }
        "append" => {
            expect(2, args.len())?;
            let mut items = as_list(args[0].clone())?;
            items.push(args[1].clone());
            Ok(Value::List(items))
        }
        "prepend" => {
            expect(2, args.len())?;
            let mut items = vec![args[1].clone()];
            items.extend(as_list(args[0].clone())?);
            Ok(Value::List(items))
        }
        "reverse" => {
            expect(1, args.len())?;
            let mut items = as_list(args[0].clone())?;
            items.reverse();
            Ok(Value::List(items))
        }
        // `Ok(first)` or `Err(ListWasEmpty)`, matching roc.
        "first" | "last" => {
            expect(1, args.len())?;
            let items = as_list(args[0].clone())?;
            let picked = if name == "first" { items.first() } else { items.last() };
            Ok(match picked {
                Some(v) => Value::tag("Ok", vec![v.clone()]),
                None => Value::tag("Err", vec![Value::tag("ListWasEmpty", vec![])]),
            })
        }
        "get" => {
            expect(2, args.len())?;
            let items = as_list(args[0].clone())?;
            let index = match args[1] {
                Value::Int(n) if n >= 0 => n as usize,
                _ => usize::MAX,
            };
            Ok(match items.get(index) {
                Some(v) => Value::tag("Ok", vec![v.clone()]),
                None => Value::tag("Err", vec![Value::tag("OutOfBounds", vec![])]),
            })
        }
        "keep_if" | "drop_if" => {
            expect(2, args.len())?;
            let items = elements(args[0].clone(), name)?;
            let func = args[1].clone();
            let keep = name == "keep_if";
            let mut out = Vec::new();
            for item in items {
                if matches!(call_function(func.clone(), vec![item.clone()])?, Value::Bool(b) if b == keep)
                {
                    out.push(item);
                }
            }
            Ok(Value::List(out))
        }
        "take_first" | "take_last" | "drop_first" | "drop_last" => {
            expect(2, args.len())?;
            let items = as_list(args[0].clone())?;
            let n = match args[1] {
                Value::Int(n) if n >= 0 => (n as usize).min(items.len()),
                _ => 0,
            };
            let taken = match name {
                "take_first" => items[..n].to_vec(),
                "take_last" => items[items.len() - n..].to_vec(),
                "drop_first" => items[n..].to_vec(),
                _ => items[..items.len() - n].to_vec(),
            };
            Ok(Value::List(taken))
        }
        // `fold_try` stops at the first `Err`, which is the whole point: the
        // accumulator is a Try and the fold short-circuits.
        "fold_try" => {
            expect(3, args.len())?;
            let items = elements(args[0].clone(), name)?;
            let mut acc = args[1].clone();
            let func = args[2].clone();
            for item in items {
                match call_function(func.clone(), vec![acc.clone(), item])? {
                    Value::Tag("Ok", payload) => {
                        acc = payload.first().cloned().unwrap_or(Value::Unit)
                    }
                    stop @ Value::Tag("Err", _) => return Ok(stop),
                    other => acc = other,
                }
            }
            Ok(Value::tag("Ok", vec![acc]))
        }
        // Collecting an iterator into a list. A range reaches here still a range —
        // `.iter()` no longer materializes one — so this is where it becomes a list.
        "from_iter" => {
            expect(1, args.len())?;
            Ok(Value::List(elements(args[0].clone(), name)?.collect()))
        }
        "contains" => {
            expect(2, args.len())?;
            let mut items = elements(args[0].clone(), name)?;
            Ok(Value::Bool(items.any(|v| values_equal(&v, &args[1]))))
        }
        _ => Err(EvalError {
            message: format!("Unknown function List.{}", name),
        }),
    }
}

/// Call an effect provided by the default (platformless) host.
///
/// `echo!` writes its argument to stdout with no trailing newline — matching
/// `roc run`, which is why the test .roc files spell newlines explicitly.
pub fn call_builtin_values(
    module: &str,
    name: &str,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    // `to_str` is dispatched on the numeric type, so it is spelled `I64.to_str`,
    // `F64.to_str`, `U8.to_str`, ... — not only `Num.to_str`. All of them
    // stringify the same way here; the interpreter does not yet track which
    // numeric type a value has (see IMPLEMENTATION_PHASES.md, numeric types).
    if name == "to_str" && is_numeric_module(module) {
        if args.len() != 1 {
            return Err(EvalError {
                message: format!("{}.to_str expects 1 argument, got {}", module, args.len()),
            });
        }
        let val = args[0].clone();
        return Ok(str_value(val.to_string()));
    }

    // `from_str` is dispatched on the numeric type too: `I64.from_str`, and so on.
    // It returns a Try, which is the tag union [Ok(a), Err(b)] — so the result is
    // an ordinary tag value. roc names the failure `BadNumStr`.
    if name == "from_str" && is_numeric_module(module) {
        if args.len() != 1 {
            return Err(EvalError {
                message: format!("{}.from_str expects 1 argument, got {}", module, args.len()),
            });
        }
        let text = match args[0].clone() {
            Value::Str(s) => s,
            other => {
                return Err(EvalError {
                    message: format!("{}.from_str needs a Str, got {}", module, other),
                })
            }
        };
        return Ok(match text.trim().parse::<i64>() {
            Ok(n) => Value::tag("Ok", vec![Value::Int(n)]),
            Err(_) => Value::tag("Err", vec![Value::tag("BadNumStr", vec![])]),
        });
    }

    // Width conversions: `I64.to_f64(n)`, `n.to_dec()`, `x.to_i64()`. Integer
    // widths are not modelled separately here, so every integer target is the same
    // conversion — only int/float actually changes the representation.
    if is_numeric_module(module) {
        if let Some(target) = name.strip_prefix("to_") {
            let value = args.first().cloned().unwrap_or(Value::Unit);
            let as_float = matches!(target, "f32" | "f64" | "dec" | "frac");
            let known = as_float
                || matches!(
                    target,
                    "u8" | "u16" | "u32" | "u64" | "u128"
                        | "i8" | "i16" | "i32" | "i64" | "i128"
                );
            if known {
                return match (value, as_float) {
                    (Value::Int(n), true) => Ok(Value::Float(n as f64)),
                    (Value::Int(n), false) => Ok(Value::Int(n)),
                    (Value::Float(f), true) => Ok(Value::Float(f)),
                    (Value::Float(f), false) => Ok(Value::Int(f as i64)),
                    (other, _) => Err(EvalError {
                        message: format!("Cannot convert {} to {}", other, target),
                    }),
                };
            }
        }
    }

    // Checked arithmetic. This interpreter uses f64/i64 throughout and does not
    // model saturation, so `*_try` always succeeds and `*_saturated` is the plain
    // operation. SafeMath's own Overflow branch is therefore never taken — which
    // matches roc for the inputs it uses.
    // ponytail: real saturation needs per-width arithmetic in the evaluator.
    if is_numeric_module(module) {
        let checked = name
            .strip_suffix("_try")
            .map(|op| (op, true))
            .or_else(|| name.strip_suffix("_saturated").map(|op| (op, false)));
        if let Some((op, wraps)) = checked {
            let numeric = |v: &Value| match v {
                Value::Int(n) => Some(*n as f64),
                Value::Float(f) => Some(*f),
                _ => None,
            };
            if let (Some(a), Some(b)) =
                (args.first().and_then(numeric), args.get(1).and_then(numeric))
            {
                let ints = matches!(
                    (args.first(), args.get(1)),
                    (Some(Value::Int(_)), Some(Value::Int(_)))
                );
                let result = match op {
                    // The subject comes first, so `a.minus_try(b)` is `a - b`.
                    "plus" => Some(a + b),
                    "minus" => Some(a - b),
                    "times" => Some(a * b),
                    "div" => (b != 0.0).then(|| a / b),
                    _ => None,
                };
                if let Some(value) = result {
                    let value = if ints && op != "div" {
                        Value::Int(value as i64)
                    } else {
                        Value::Float(value)
                    };
                    return Ok(if wraps { Value::tag("Ok", vec![value]) } else { value });
                }
            }
        }
    }

    // `negate` is what unary minus lowers to, dispatched on the numeric type.
    if name == "negate" && is_numeric_module(module) {
        if args.len() != 1 {
            return Err(EvalError {
                message: format!("{}.negate expects 1 argument, got {}", module, args.len()),
            });
        }
        return match &args[0] {
            Value::Int(n) => Ok(Value::Int(-n)),
            Value::Float(f) => Ok(Value::Float(-f)),
            other => Err(EvalError {
                message: format!("Cannot negate {}", other),
            }),
        };
    }

    if module == "List" {
        return call_list_builtin(name, args);
    }

    match (module, name) {
        ("Bool", "not") => {
            if args.len() != 1 {
                return Err(EvalError {
                    message: format!("Bool.not expects 1 argument, got {}", args.len()),
                });
            }
            match args[0].clone() {
                Value::Bool(b) => Ok(Value::Bool(!b)),
                other => Err(EvalError {
                    message: format!("Bool.not needs a Bool, got {}", other),
                }),
            }
        }
        // Effects from a real platform. `Stdout.line!` and friends are declared by
        // the platform but implemented in its compiled host, so the ones this
        // interpreter can run are implemented here and the rest are reported as a
        // gap rather than as an unknown name.
        ("Stdout", "line!") | ("Stderr", "line!") => {
            if args.len() != 1 {
                return Err(EvalError {
                    message: format!("{}.{} expects 1 argument, got {}", module, name, args.len()),
                });
            }
            let text = match &args[0] {
                Value::Str(s) => s.to_string(),
                other => other.to_string(),
            };
            if module == "Stderr" {
                eprintln!("{}", text);
            } else {
                println!("{}", text);
            }
            // `line! : Str => Try({}, [StdoutErr(IOErr), ..])`
            Ok(Value::tag("Ok", vec![Value::Unit]))
        }
        ("Stdout", "write!") | ("Stderr", "write!") => {
            if args.len() != 1 {
                return Err(EvalError {
                    message: format!("{}.{} expects 1 argument, got {}", module, name, args.len()),
                });
            }
            let text = match &args[0] {
                Value::Str(s) => s.to_string(),
                other => other.to_string(),
            };
            use std::io::Write;
            if module == "Stderr" {
                eprint!("{}", text);
                let _ = std::io::stderr().flush();
            } else {
                print!("{}", text);
                let _ = std::io::stdout().flush();
            }
            Ok(Value::tag("Ok", vec![Value::Unit]))
        }
        ("Str", "is_empty") => {
            if args.len() != 1 {
                return Err(EvalError {
                    message: format!("Str.is_empty expects 1 argument, got {}", args.len()),
                });
            }
            match &args[0] {
                Value::Str(text) => Ok(Value::Bool(text.is_empty())),
                other => Err(EvalError {
                    message: format!("Str.is_empty needs a Str, got {}", other),
                }),
            }
        }
        ("Str", "inspect") => {
            if args.len() != 1 {
                return Err(EvalError {
                    message: format!("Str.inspect expects 1 argument, got {}", args.len()),
                });
            }
            let val = args[0].clone();
            // A nominal may define `to_inspect` to control how it is shown.
            if let Some(custom) = custom_inspect(&val) {
                return Ok(custom);
            }
            Ok(str_value(inspect(&val)))
        }
        // `haystack.split_first(needle)` → `Ok({ before, after })`, or
        // `Err(NotFound)`. Splits at the FIRST occurrence only.
        ("Str", "split_first") => {
            let (text, needle) = match (args.first(), args.get(1)) {
                (Some(Value::Str(t)), Some(Value::Str(n))) => (&**t, &**n),
                _ => {
                    return Err(EvalError {
                        message: "Str.split_first needs two Str arguments".to_string(),
                    })
                }
            };
            Ok(match text.find(needle) {
                Some(i) => Value::tag(
                    "Ok",
                    vec![Value::Record(vec![
                        ("before", str_value(text[..i].to_string())),
                        (
                            "after",
                            str_value(text[i + needle.len()..].to_string()),
                        ),
                    ])],
                ),
                None => Value::tag("Err", vec![Value::tag("NotFound", vec![])]),
            })
        }
        // `Str.join_with(list, separator)`, the subject first as always.
        ("Str", "join_with") => {
            let (items, sep) = match (args.first(), args.get(1)) {
                (Some(Value::List(items)), Some(Value::Str(sep))) => (items, &**sep),
                _ => {
                    return Err(EvalError {
                        message: "Str.join_with needs a List and a Str".to_string(),
                    })
                }
            };
            let rendered: Vec<String> = items
                .iter()
                .map(|v| match v {
                    Value::Str(text) => text.to_string(),
                    other => other.to_string(),
                })
                .collect();
            Ok(str_value(rendered.join(sep)))
        }
        ("Str", "trim") | ("Str", "trim_start") | ("Str", "trim_end")
        | ("Str", "with_ascii_uppercased") | ("Str", "with_ascii_lowercased") => {
            let text = match args.first() {
                Some(Value::Str(t)) => &**t,
                _ => {
                    return Err(EvalError {
                        message: format!("Str.{} needs a Str", name),
                    })
                }
            };
            let out = match name {
                "trim" => text.trim().to_string(),
                "trim_start" => text.trim_start().to_string(),
                "trim_end" => text.trim_end().to_string(),
                "with_ascii_uppercased" => text.to_ascii_uppercase(),
                _ => text.to_ascii_lowercase(),
            };
            Ok(str_value(out))
        }
        ("Str", "starts_with") | ("Str", "ends_with") | ("Str", "contains") => {
            let (text, needle) = match (args.first(), args.get(1)) {
                (Some(Value::Str(t)), Some(Value::Str(n))) => (&**t, &**n),
                _ => {
                    return Err(EvalError {
                        message: format!("Str.{} needs two Str arguments", name),
                    })
                }
            };
            Ok(Value::Bool(match name {
                "starts_with" => text.starts_with(needle),
                "ends_with" => text.ends_with(needle),
                _ => text.contains(needle),
            }))
        }
        ("Str", "len") | ("Str", "count_utf8_bytes") => {
            let text = match args.first() {
                Some(Value::Str(t)) => &**t,
                _ => {
                    return Err(EvalError {
                        message: format!("Str.{} needs a Str", name),
                    })
                }
            };
            Ok(Value::Int(text.len() as i64))
        }
        ("Str", "split_on") => {
            let (text, sep) = match (args.first(), args.get(1)) {
                (Some(Value::Str(t)), Some(Value::Str(sep))) => (&**t, &**sep),
                _ => {
                    return Err(EvalError {
                        message: "Str.split_on needs two Str arguments".to_string(),
                    })
                }
            };
            Ok(Value::List(
                text.split(sep)
                    .map(|part| str_value(part.to_string()))
                    .collect(),
            ))
        }
        ("Str", "from_utf8") => {
            let bytes = match args.first() {
                Some(Value::List(items)) => items,
                _ => {
                    return Err(EvalError {
                        message: "Str.from_utf8 needs a List of bytes".to_string(),
                    })
                }
            };
            let raw: Vec<u8> = bytes
                .iter()
                .filter_map(|v| match v {
                    Value::Int(n) if (0..=255).contains(n) => Some(*n as u8),
                    _ => None,
                })
                .collect();
            Ok(match String::from_utf8(raw) {
                Ok(text) => Value::tag("Ok", vec![str_value(text)]),
                Err(_) => Value::tag("Err", vec![Value::tag("BadUtf8", vec![])]),
            })
        }
        ("Str", "to_utf8") => {
            let text = match args.first() {
                Some(Value::Str(t)) => &**t,
                _ => {
                    return Err(EvalError {
                        message: "Str.to_utf8 needs a Str".to_string(),
                    })
                }
            };
            Ok(Value::List(
                text.as_bytes().iter().map(|b| Value::Int(*b as i64)).collect(),
            ))
        }
        ("Str", "concat") => {
            let mut result = String::new();
            for val in &args {
                match val {
                    Value::Str(text) => result.push_str(text),
                    other => result.push_str(&other.to_string()),
                }
            }
            Ok(str_value(result))
        }
        _ => Err(EvalError {
            // A platform that declares this really does provide it; the gap is
            // this interpreter's, and the message should say which.
            message: if crate::platform::real::is_declared(module, name) {
                crate::platform::real::describe_gap(module, name)
            } else {
                format!("Unknown function {}.{}", module, name)
            },
        }),
    }
}

/// Apply binary operation
/// Route an operator to a user-defined method, if the operand type has one.
///
/// roc spells every operator as a method — `a + b` IS `a.plus(b)` — so a nominal
/// that defines `plus` gets `+`. Only tried for compound values: a type defining
/// `plus` must not hijack `1 + 2`, and the builtin path handles the primitives.
///
/// Dispatch is by method NAME, not by type, because values carry no nominal tag at
/// runtime — the same search `Dispatch` already does.
pub fn dispatch_operator(
    op: BinOp,
    left: &Value,
    right: &Value,
) -> Result<Option<Value>, EvalError> {
    if !matches!(left, Value::Record(_) | Value::Tag(..) | Value::Tuple(_)) {
        return Ok(None);
    }
    let method = match op {
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
        // `!=` is `is_eq` negated: roc asks for `is_eq` either way.
        BinOp::Eq | BinOp::Ne => "is_eq",
        _ => return Ok(None),
    };

    let mut candidates = crate::vm::methods_named(method);
    if candidates.len() != 1 {
        return Ok(None);
    }
    let (_, func) = candidates.pop().expect("checked len");
    let result = call_function(func, vec![left.clone(), right.clone()])?;
    Ok(Some(match (op, result) {
        (BinOp::Ne, Value::Bool(b)) => Value::Bool(!b),
        (_, other) => other,
    }))
}

/// Apply a binary operator to two already-evaluated operands.
///
/// An associated function rather than a method: it needs no evaluator state, and
/// the VM backend calls it too. Operator SEMANTICS live in exactly one place, so
/// the two engines cannot drift on what `//` does to a negative number.
///
/// The operands are BORROWED. No arm here needs to own them — every one either
/// copies a scalar out or builds a new value — and cloning them cost the VM 40% of
/// `fib`: two 32-byte `Value` clones per arithmetic instruction, for two integers.
/// The alternative was a fast path for `Int` inside the VM's `Bin` opcode, which
/// would have been a second implementation of Roc's arithmetic; this is the same
/// win with one.
pub fn apply_binop(op: BinOp, left: &Value, right: &Value) -> Result<Value, EvalError> {
    // Two integers, which is most arithmetic in most programs, and the case the VM's
    // `BinInt` opcode skips this dispatch for entirely.
    if let (Value::Int(a), Value::Int(b)) = (left, right) {
        if let Some(result) = int_binop(op, *a, *b) {
            return result;
        }
    }
    match (op, left, right) {
        // Arithmetic on integers
        (BinOp::Add, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
        (BinOp::Sub, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
        (BinOp::Mul, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
        (BinOp::Div, Value::Int(a), Value::Int(b)) => {
            if *b == 0 {
                Err(EvalError { message: "Division by zero".to_string() })
            } else {
                Ok(Value::Int(a / b))
            }
        }
        // `//` truncating division and `%` remainder — integers only in Roc.
        (BinOp::IntDiv, Value::Int(a), Value::Int(b)) => {
            if *b == 0 {
                Err(EvalError { message: "Division by zero".to_string() })
            } else {
                // Roc's `//` truncates toward zero, which is Rust's `/` for i64.
                Ok(Value::Int(a / b))
            }
        }
        (BinOp::Rem, Value::Int(a), Value::Int(b)) => {
            if *b == 0 {
                Err(EvalError { message: "Division by zero".to_string() })
            } else {
                Ok(Value::Int(a % b))
            }
        }
        // Arithmetic on floats
        (BinOp::Add, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
        (BinOp::Sub, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
        (BinOp::Mul, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
        (BinOp::Div, Value::Float(a), Value::Float(b)) => {
            if *b == 0.0 {
                Err(EvalError { message: "Division by zero".to_string() })
            } else {
                Ok(Value::Float(a / b))
            }
        }
        // Mixed int/float arithmetic
        (BinOp::Add, Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
        (BinOp::Add, Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + *b as f64)),
        (BinOp::Sub, Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 - b)),
        (BinOp::Sub, Value::Float(a), Value::Int(b)) => Ok(Value::Float(a - *b as f64)),
        (BinOp::Mul, Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 * b)),
        (BinOp::Mul, Value::Float(a), Value::Int(b)) => Ok(Value::Float(a * *b as f64)),
        (BinOp::Div, Value::Int(a), Value::Float(b)) => {
            if *b == 0.0 {
                Err(EvalError { message: "Division by zero".to_string() })
            } else {
                Ok(Value::Float(*a as f64 / b))
            }
        }
        (BinOp::Div, Value::Float(a), Value::Int(b)) => {
            if *b == 0 {
                Err(EvalError { message: "Division by zero".to_string() })
            } else {
                Ok(Value::Float(a / *b as f64))
            }
        }
        // String concatenation
        (BinOp::Add, Value::Str(a), Value::Str(b)) => {
            let concatenated = format!("{}{}", a, b);
            Ok(str_value(concatenated))
        }
        // Comparison operators — these yield Bool in Roc, not 0/1.
        (BinOp::Eq, a, b) => Ok(Value::Bool(values_equal(a, b))),
        (BinOp::Ne, a, b) => Ok(Value::Bool(!values_equal(a, b))),
        (BinOp::Lt, a, b) => compare(a, b, |o| o == std::cmp::Ordering::Less),
        (BinOp::Le, a, b) => compare(a, b, |o| o != std::cmp::Ordering::Greater),
        (BinOp::Gt, a, b) => compare(a, b, |o| o == std::cmp::Ordering::Greater),
        (BinOp::Ge, a, b) => compare(a, b, |o| o != std::cmp::Ordering::Less),
        // `and` / `or` are Bool-only in Roc. Both operands are already
        // evaluated, so these do not short-circuit — see the ponytail note.
        (BinOp::And, Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a && *b)),
        (BinOp::Or, Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a || *b)),
        // Numeric operands to `and`/`or`, kept for the `1 && 0` spelling used
        // before Bool existed. ponytail: drop once nothing relies on it.
        (BinOp::And, a, b) => Ok(Value::Bool(is_truthy(a) && is_truthy(b))),
        (BinOp::Or, a, b) => Ok(Value::Bool(is_truthy(a) || is_truthy(b))),
        (_op, _left, _right) => {
            Err(EvalError {
                message: "Invalid operands for operator".to_string(),
            })
        }
    }
}

/// Compare two numbers and turn the ordering into a Bool.
///
/// Replaces four near-identical arms per operator (int/int, float/float and both
/// mixed pairings); only the predicate differs between `<`, `<=`, `>` and `>=`.
fn compare(
    a: &Value,
    b: &Value,
    keep: impl Fn(std::cmp::Ordering) -> bool,
) -> Result<Value, EvalError> {
    let ordering = match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Float(x), Value::Float(y)) => cmp_f64(*x, *y)?,
        (Value::Int(x), Value::Float(y)) => cmp_f64(*x as f64, *y)?,
        (Value::Float(x), Value::Int(y)) => cmp_f64(*x, *y as f64)?,
        _ => {
            return Err(EvalError {
                message: "Comparison operators need numbers".to_string(),
            })
        }
    };
    Ok(Value::Bool(keep(ordering)))
}

/// Total-order two f64s, rejecting NaN rather than silently reporting `false`.
fn cmp_f64(x: f64, y: f64) -> Result<std::cmp::Ordering, EvalError> {
    x.partial_cmp(&y).ok_or_else(|| EvalError {
        message: "Cannot compare NaN".to_string(),
    })
}

/// Check if two values are equal
fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Str(s1), Value::Str(s2)) => s1 == s2,
        (Value::Int(n1), Value::Int(n2)) => n1 == n2,
        (Value::Bool(b1), Value::Bool(b2)) => b1 == b2,
        (Value::Unit, Value::Unit) => true,
        (Value::Float(f1), Value::Float(f2)) => (f1 - f2).abs() < 1e-10,
        (Value::Int(n), Value::Float(f)) => ((*n as f64) - f).abs() < 1e-10,
        (Value::Float(f), Value::Int(n)) => (f - (*n as f64)).abs() < 1e-10,
        // Bare tags compare by name; payload tags compare payloads too.
        (Value::Tag(n1, p1), Value::Tag(n2, p2)) => {
            n1 == n2
                && p1.len() == p2.len()
                && p1.iter().zip(p2.iter()).all(|(a, b)| values_equal(a, b))
        }
        // Tuples are equal element-wise at the same arity.
        (Value::Tuple(x), Value::Tuple(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| values_equal(a, b))
        }
        // Lists are equal element-wise, and only at the same length.
        (Value::List(x), Value::List(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| values_equal(a, b))
        }
        (Value::Record(x), Value::Record(y)) => {
            x.len() == y.len()
                && x.iter().zip(y.iter()).all(|((n1, v1), (n2, v2))| {
                    n1 == n2 && values_equal(v1, v2)
                })
        }
        _ => false,
    }
}

/// Check if a value is truthy (non-zero for numbers, non-empty for strings)
fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Int(n) => *n != 0,
        Value::Float(f) => *f != 0.0,
        Value::Str(s) => !s.is_empty(),
        _ => true, // lambdas and builtins are truthy
    }
}


/// Is `module` one of Roc's numeric types (or the generic `Num`)?
fn is_numeric_module(module: &str) -> bool {
    matches!(
        module,
        "Num"
            | "U8" | "U16" | "U32" | "U64" | "U128"
            | "I8" | "I16" | "I32" | "I64" | "I128"
            | "F32" | "F64"
            | "Dec"
    )
}

/// Render a value the way Roc's `Str.inspect` does.
///
/// Verified against `roc` nightly-2026-09-03:
///   * record fields are sorted **alphabetically**, not left in source order —
///     `{ zebra: 1, apple: 2 }` inspects as `{ apple: ..., zebra: ... }`
///   * sorting is recursive, so nested records are sorted too
///   * strings are quoted: `"hi"`
///   * booleans are `True` / `False`
///   * the empty record is `{}`
///
/// ponytail: integers render without a fractional part (`42`). Roc prints `42.0`
/// for an integer literal that nothing constrains, because it defaults to a
/// fractional type — but `42` once annotated `I64`. Matching that needs the type
/// checker to track numeric types (numeric-types phase); until then, annotate
/// numbers in any test that inspects them, which the golden pairs already do.
fn inspect(value: &Value) -> String {
    match value {
        Value::Str(s) => crate::eval::value::quoted(s),
        Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        Value::Unit => "{}".to_string(),
        Value::List(items) => {
            let rendered: Vec<String> = items.iter().map(inspect).collect();
            format!("[{}]", rendered.join(", "))
        }
        Value::Tuple(items) => {
            let rendered: Vec<String> = items.iter().map(inspect).collect();
            format!("({})", rendered.join(", "))
        }
        Value::Range { .. } => "<opaque>".to_string(),
        Value::Record(fields) => {
            if fields.is_empty() {
                return "{}".to_string();
            }
            let mut sorted: Vec<&(&str, Value)> = fields.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(b.0));
            let rendered: Vec<String> = sorted
                .iter()
                .map(|(name, v)| format!("{}: {}", name, inspect(v)))
                .collect();
            format!("{{ {} }}", rendered.join(", "))
        }
        Value::Tag(name, args) if args.is_empty() => name.to_string(),
        Value::Tag(name, args) => {
            let rendered: Vec<String> = args.iter().map(inspect).collect();
            format!("{}({})", name, rendered.join(", "))
        }
        other => other.to_string(),
    }
}

/// Does `pattern` match `value`? Collects any bindings it introduces.
///
/// Bindings are pushed onto `bindings` rather than bound directly so a partial match
/// leaves no trace: a nested pattern can bind several names and then fail on the last
/// element, and those names must not leak into the next arm.
/// Run one `expect` against an already-evaluated condition.
///
/// A failure is reported and execution CONTINUES — roc prints to stderr and carries on
/// rather than aborting — and the tally is process-wide, which is what `roc test`
/// reports. Shared with the VM's `Expect` opcode.
pub fn run_expect(value: &Value) -> Result<(), EvalError> {
    expect_ran();
    match value {
        Value::Bool(true) => Ok(()),
        Value::Bool(false) => {
            expect_failed();
            eprintln!("Expect failed: expect failed");
            Ok(())
        }
        other => Err(EvalError {
            message: format!("`expect` needs a Bool, got {}", other),
        }),
    }
}

/// `dbg value` — to stderr, so it never mixes into a program's output.
pub fn run_dbg(value: &Value) {
    eprintln!("[dbg] {}", inspect(value));
}

/// `crash "message"`. A Str crashes with its text; anything else with its rendering.
pub fn crash_error(value: &Value) -> EvalError {
    let text = match value {
        Value::Str(s) => s.to_string(),
        other => other.to_string(),
    };
    EvalError { message: format!("crash: {}", text) }
}

/// Integer arithmetic and comparison — the one implementation.
///
/// `None` for `and`/`or`, which are Bool-only in Roc and reach integers only through a
/// spelling kept for compatibility; those stay in `apply_binop`.
///
/// Its own function so the VM's `BinInt` opcode can compute a result without going
/// through `apply_binop`'s dispatch on operand shapes, which is worth about 24% of
/// `fib` — while integer semantics still exist in exactly one place.
pub fn int_binop(op: BinOp, a: i64, b: i64) -> Option<Result<Value, EvalError>> {
    let divide_by_zero = || {
        Some(Err(EvalError { message: "Division by zero".to_string() }))
    };
    Some(Ok(match op {
        BinOp::Add => Value::Int(a + b),
        BinOp::Sub => Value::Int(a - b),
        BinOp::Mul => Value::Int(a * b),
        // Roc's `//` truncates toward zero, which is Rust's `/` for i64.
        BinOp::Div | BinOp::IntDiv => {
            if b == 0 {
                return divide_by_zero();
            }
            Value::Int(a / b)
        }
        BinOp::Rem => {
            if b == 0 {
                return divide_by_zero();
            }
            Value::Int(a % b)
        }
        BinOp::Eq => Value::Bool(a == b),
        BinOp::Ne => Value::Bool(a != b),
        BinOp::Lt => Value::Bool(a < b),
        BinOp::Le => Value::Bool(a <= b),
        BinOp::Gt => Value::Bool(a > b),
        BinOp::Ge => Value::Bool(a >= b),
        BinOp::And | BinOp::Or => return None,
    }))
}

/// How a value renders INSIDE a string interpolation.
///
/// Not the same as `Display`: a `Str` interpolates without its quotes. Shared with the
/// VM's `Interp` opcode so `"x=${s}"` cannot come out differently on the two engines.
pub fn interpolated(value: &Value) -> String {
    match value {
        Value::Str(s) => s.to_string(),
        Value::Int(n) => n.to_string(),
        // Matches `Value`'s own Display: no trailing `.0` on a whole float.
        Value::Float(f) => f.to_string(),
        Value::Builtin(name, arity) => format!("<{}/{}>", name, arity),
        other => other.to_string(),
    }
}

/// Dispatch `method` on an evaluated receiver, the BUILTIN way.
///
/// A nominal's own method block is not consulted here: the compiler resolves that to a
/// chunk and calls it directly. Everything after that point is this function — the
/// `iter` shorthand, the `Try` methods, then the module's builtin table.
pub fn dispatch_builtin(
    receiver: Value,
    method: &str,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    // `.iter()` on something already iterable is the identity. A range STAYS a range:
    // building the list of its elements cost 190 MB on a two-million-element range, and
    // every builtin that only walks the elements can walk a range instead. It also
    // matches roc, which inspects a range as `<opaque>` rather than as a list.
    //
    // ponytail: still eager in the sense that `map` over a range builds its output
    // list. Fusing `map` into the consumer needs a real lazy iterator; this removes the
    // ceiling without one.
    if method == "iter" && args.is_empty() {
        if matches!(receiver, Value::List(_) | Value::Range { .. }) {
            return Ok(receiver);
        }
    }

    // `Ok`/`Err` carry no module, but they answer the Try methods. Done here rather
    // than in `module_for` because the payload has to be rebuilt around the result.
    if let Value::Tag(tag, payload) = &receiver {
        if matches!(*tag, "Ok" | "Err") {
            if let Some(result) = try_method(tag, payload, method, args.clone())? {
                return Ok(result);
            }
        }
    }

    let module = module_for(&receiver).ok_or_else(|| EvalError {
        message: format!("Cannot dispatch `{}` on {}", method, receiver),
    })?;

    // The receiver becomes the FIRST argument, which is why roc's builtins take their
    // subject first: `xs.map(f)` is `List.map(xs, f)`.
    let mut values = Vec::with_capacity(args.len() + 1);
    values.push(receiver);
    values.extend(args);
    call_builtin_values(module, method, values)
}

/// Run one of the default host's effects on already-evaluated arguments.
///
/// Shared with the VM's `CallHost` opcode. The host is the only reason a pure language
/// prints anything, so both engines have to reach the same one.
pub fn host_effect(name: &str, args: Vec<Value>) -> Result<Value, EvalError> {
    let (params, _) = crate::platform::host::lookup(name).ok_or_else(|| EvalError {
        message: format!("Unknown host effect '{}'", name),
    })?;
    if args.len() != params.len() {
        return Err(EvalError {
            message: format!("{} expects {} argument(s), got {}", name, params.len(), args.len()),
        });
    }
    match name {
        "echo!" => {
            match &args[0] {
                Value::Str(s) => print!("{}", s),
                other => print!("{}", other),
            }
            use std::io::Write;
            let _ = std::io::stdout().flush();
            Ok(Value::Unit)
        }
        _ => Err(EvalError {
            message: format!("Host effect '{}' is declared but not implemented", name),
        }),
    }
}

/// Does a LITERAL pattern — `1`, `3.5`, `"hello"` — match `value`?
///
/// Its own function because the VM's `TestLit` opcode calls it too, and because these
/// rules are NOT `values_equal`'s: a pattern matches a value of the same kind only, so
/// `1` does not match `1.0`, where `1 == 1.0` is `True`. Two engines disagreeing about
/// that would be a wrong answer in a `match`, not an error.
pub fn literal_pattern_matches(pattern: &Pattern, value: &Value) -> bool {
    match (pattern, value) {
        (Pattern::Int(expected), Value::Int(n)) => n == expected,
        // The same tolerance `values_equal` uses for two floats.
        (Pattern::Float(expected), Value::Float(n)) => (n - expected).abs() < 1e-10,
        (Pattern::Str(expected), Value::Str(s)) => &**s == *expected,
        _ => false,
    }
}

/// Call a function value with already-evaluated arguments.
///
/// The builtins' callback hook: `xs.map(f)` reaches a function through here, whether
/// `f` is a closure the compiler produced or a builtin passed as a value.
pub fn call_function(func: Value, args: Vec<Value>) -> Result<Value, EvalError> {
    match func {
        Value::Closure(closure) => crate::vm::call_closure(&closure, args),
        // A builtin passed as a value — `xs.map(Str.inspect)`.
        Value::Builtin(qualified, _) => {
            let (module, name) = qualified.split_once('.').ok_or_else(|| EvalError {
                message: format!("`{}` is not a qualified builtin", qualified),
            })?;
            call_builtin_values(module, name, args)
        }
        other => Err(EvalError {
            message: format!("Attempted to call a non-function value: {}", other),
        }),
    }
}

/// How many `expect`s ran, and how many failed.
///
/// Process-wide rather than a field, because every call builds its own `Evaluator` —
/// an `expect` inside a function would otherwise be counted by a tally that is thrown
/// away when the call returns. `roc test` reports these totals.
static EXPECTS_RUN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static EXPECTS_FAILED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn expect_ran() {
    EXPECTS_RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

fn expect_failed() {
    EXPECTS_FAILED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// `(ran, failed)` for every `expect` evaluated so far.
pub fn expect_tally() -> (usize, usize) {
    (
        EXPECTS_RUN.load(std::sync::atomic::Ordering::Relaxed),
        EXPECTS_FAILED.load(std::sync::atomic::Ordering::Relaxed),
    )
}

/// Which builtin module a value dispatches to.
///
/// The checker resolves dispatch from the receiver's TYPE; this resolves the same
/// thing from its runtime kind, and the two agree because a value's kind follows its
/// type. Numbers report `I64`/`F64` because the interpreter keeps one representation
/// of each — `is_numeric_module` accepts every numeric name, so `small.to_str()` on a
/// U8 lands in the same place.
///
/// `None` for records and tags: a nominal's methods would be found through its
/// declared type, but values carry no nominal wrapper (roc erases it), so there is
/// nothing at runtime to dispatch on. See the phase-20 notes.
pub fn module_for(value: &Value) -> Option<&'static str> {
    Some(match value {
        Value::Int(_) => "I64",
        Value::Float(_) => "F64",
        Value::Str(_) => "Str",
        Value::Bool(_) => "Bool",
        Value::List(_) => "List",
        // A range answers the List methods: `(1..=n).iter().fold(..)` is the idiom,
        // and the ones that only walk the elements never build a list.
        Value::Range { .. } => "List",
        Value::Record(_) | Value::Tag(..) | Value::Tuple(_) | Value::Unit => return None,
        Value::Closure(..) | Value::Builtin(..) => return None,
    })
}
