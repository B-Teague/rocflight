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


thread_local! {
    /// The `to_inspect` methods currently running, by name.
    ///
    /// A nominal's `to_inspect` almost always unwraps and inspects what is INSIDE —
    /// `|ItemKind.(k)| "ItemKind.(${Str.inspect(k)})"`. roc's unwrap changes the type,
    /// so the inner `Str.inspect` finds no custom method; rocflight erases nominals, so
    /// the inner value is the same value and the same method answers again, for ever.
    /// Refusing a method already on the stack is what ends it, and matches what roc
    /// does for the reason roc does it.
    static INSPECTING: std::cell::RefCell<Vec<&'static str>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// A nominal's own `to_inspect`, if it defines one.
fn custom_inspect(value: &Value) -> Option<Value> {
    for (name, func) in crate::vm::methods_named("to_inspect", value) {
        if INSPECTING.with(|running| running.borrow().contains(&name)) {
            continue;
        }
        INSPECTING.with(|running| running.borrow_mut().push(name));
        let shown = call_function(func, vec![value.clone()]);
        INSPECTING.with(|running| {
            running.borrow_mut().pop();
        });
        if let Ok(shown @ Value::Str(_)) = shown {
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
    /// Walked in place: the list is shared with whoever else holds it, and each
    /// element is cloned as it is reached rather than the whole list up front.
    List(std::rc::Rc<Vec<Value>>, usize),
    Range(std::ops::RangeInclusive<i128>),
}

impl Iterator for Elements {
    type Item = Value;

    fn next(&mut self) -> Option<Value> {
        match self {
            Elements::List(items, at) => {
                let item = items.get(*at).cloned();
                *at += 1;
                item
            }
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
        Value::List(items) => Ok(Elements::List(items, 0)),
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

    // Takes the argument OUT of `args`: when nothing else holds the list, the `Vec`
    // comes back without a copy and the operation below mutates it in place.
    let mut args = args;
    let as_list = |v: &mut Value| -> Result<Vec<Value>, EvalError> {
        match std::mem::replace(v, Value::Unit) {
            Value::List(items) => Ok(value::into_items(items)),
            other => Err(EvalError {
                message: format!("List.{} needs a List, got {}", name, other),
            }),
        }
    };
    // A read leaves the list where it is: taking it would copy a shared one, and
    // `xs.get(i)` in a loop over `xs` was quadratic for exactly that reason.
    fn peek<'a>(v: &'a Value, name: &str) -> Result<&'a [Value], EvalError> {
        match v {
            Value::List(items) => Ok(items),
            other => Err(EvalError {
                message: format!("List.{} needs a List, got {}", name, other),
            }),
        }
    }

    match name {
        // `List.repeat(item, n)` — n copies. `Dict` allocates its bucket table this way.
        "repeat" => {
            expect(2, args.len())?;
            let count = match args[1] {
                Value::Int(n) if n >= 0 => n as usize,
                ref other => {
                    return Err(EvalError {
                        message: format!("List.repeat needs a count, got {}", other),
                    })
                }
            };
            Ok(Value::list(vec![args[0].clone(); count]))
        }

        // Capacity is an allocation hint, and not observable through the API: roc's own
        // docs describe these as avoiding reallocation, never as changing a value.
        "reserve" | "release_excess_capacity" => {
            expect(if name == "reserve" { 2 } else { 1 }, args.len())?;
            as_list(&mut args[0]).map(Value::list)
        }
        "with_capacity" => {
            expect(1, args.len())?;
            Ok(Value::list(Vec::new()))
        }

        // A range knows its length from its bounds, so neither of these walks anything.
        "len" => {
            expect(1, args.len())?;
            let count = Elements::count_of(&args[0]).ok_or_else(|| EvalError {
                message: format!("List.{} needs a List, got {}", name, args[0]),
            })?;
            Ok(Value::Int(count as i128))
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
            Ok(Value::list(out))
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
            let mut items = as_list(&mut args[0])?;
            items.extend(as_list(&mut args[1])?);
            Ok(Value::list(items))
        }
        "append" => {
            expect(2, args.len())?;
            let mut items = as_list(&mut args[0])?;
            items.push(args[1].clone());
            Ok(Value::list(items))
        }
        "prepend" => {
            expect(2, args.len())?;
            let mut items = vec![args[1].clone()];
            items.extend(as_list(&mut args[0])?);
            Ok(Value::list(items))
        }
        // roc spells it `rev`; `reverse` is kept because this interpreter answered to
        // it before the name was checked against `Builtin.roc`.
        "rev" | "reverse" => {
            expect(1, args.len())?;
            let mut items = as_list(&mut args[0])?;
            items.reverse();
            Ok(Value::list(items))
        }
        // `Ok(first)` or `Err(ListWasEmpty)`, matching roc.
        "first" | "last" => {
            expect(1, args.len())?;
            let items = peek(&args[0], name)?;
            let picked = if name == "first" { items.first() } else { items.last() };
            Ok(match picked {
                Some(v) => Value::tag("Ok", vec![v.clone()]),
                None => Value::tag("Err", vec![Value::tag("ListWasEmpty", vec![])]),
            })
        }
        "get" => {
            expect(2, args.len())?;
            let items = peek(&args[0], name)?;
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
            Ok(Value::list(out))
        }
        "take_first" | "take_last" | "drop_first" | "drop_last" => {
            expect(2, args.len())?;
            let items = peek(&args[0], name)?;
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
            Ok(Value::list(taken))
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
            Ok(Value::list(elements(args[0].clone(), name)?.collect()))
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
/// One whole unit of a `Dec`: 10^18, roc's own scale.
///
/// `roc-compiler/src/builtins/dec.zig` sets `decimal_places: u5 = 18`, so every `Dec`
/// is an `i128` holding the value times this.
pub const DEC_SCALE: i128 = 1_000_000_000_000_000_000;

/// Render a `Dec` the way roc does: the whole part, a point, and the fraction with its
/// trailing zeros removed. A whole value keeps a single `.0`.
pub fn dec_to_string(raw: i128) -> String {
    let negative = raw < 0;
    let magnitude = raw.unsigned_abs();
    let whole = magnitude / DEC_SCALE as u128;
    let fraction = magnitude % DEC_SCALE as u128;
    let mut digits = format!("{:018}", fraction);
    while digits.ends_with('0') && digits.len() > 1 {
        digits.pop();
    }
    format!("{}{}.{}", if negative { "-" } else { "" }, whole, digits)
}

/// A `Dec` from the double a float literal parsed to.
///
/// Rounded at the eighteenth place, which is where a `Dec` stops.
pub fn dec_from_f64(value: f64) -> i128 {
    (value * DEC_SCALE as f64).round() as i128
}

/// Read a `Dec` out of a value, converting a whole number if that is what it is.
///
/// An integer literal in a `List(Dec)` reaches the evaluator as a `Dec` already — the
/// checker said so — but a length or a count arrives as an ordinary integer.
pub fn as_dec(value: &Value) -> Option<i128> {
    match value {
        Value::Dec(raw) => Some(*raw),
        Value::Int(n) => n.checked_mul(DEC_SCALE),
        // A float beside a `Dec` widens rather than dragging the `Dec` down to an f64:
        // the checker cannot always say that a fold's accumulator is fixed point, and
        // the alternative is losing the digits the type exists to keep.
        Value::Float(f) => Some(dec_from_f64(*f)),
        _ => None,
    }
}

/// `Dec` multiplication, without a 256-bit intermediate.
///
/// The naive `a * b / SCALE` overflows an i128 for anything past about 13: the operands
/// already carry 10^18 each, so their product carries 10^36 and `25.0 * 25.0` is
/// 6.25e38 against an i128 ceiling of 1.7e38. Splitting each operand into whole and
/// fractional parts keeps every term in range:
///
/// ```text
/// (qa·S + ra)(qb·S + rb) / S  =  qa·qb·S + qa·rb + ra·qb + ra·rb/S
/// ```
///
/// The last term is where the digits past the eighteenth go, which is exactly where a
/// `Dec` drops them anyway.
pub fn dec_mul(a: i128, b: i128) -> Option<i128> {
    let (qa, ra) = (a / DEC_SCALE, a % DEC_SCALE);
    let (qb, rb) = (b / DEC_SCALE, b % DEC_SCALE);
    qa.checked_mul(qb)?
        .checked_mul(DEC_SCALE)?
        .checked_add(qa.checked_mul(rb)?)?
        .checked_add(ra.checked_mul(qb)?)?
        .checked_add(ra.checked_mul(rb)? / DEC_SCALE)
}

/// `Dec` division, split the same way: `a/b·S` as a whole part and a remainder.
pub fn dec_div(a: i128, b: i128) -> Option<i128> {
    if b == 0 {
        return None;
    }
    let (whole, remainder) = (a / b, a % b);
    whole
        .checked_mul(DEC_SCALE)?
        .checked_add(remainder.checked_mul(DEC_SCALE)? / b)
}

/// `Dec` arithmetic. Fixed point, so a product and a quotient rescale.
///
/// Saturating rather than wrapping: roc's `times_saturated` is written against that,
/// and `SafeMath` detects an overflow by comparing against `Dec.highest`.
pub fn dec_binop(op: BinOp, a: i128, b: i128) -> Option<Result<Value, EvalError>> {
    let saturated = |v: Option<i128>| {
        Value::Dec(v.unwrap_or(if (a < 0) == (b < 0) { i128::MAX } else { i128::MIN }))
    };
    Some(Ok(match op {
        BinOp::Add => Value::Dec(a.saturating_add(b)),
        BinOp::Sub => Value::Dec(a.saturating_sub(b)),
        BinOp::Mul => saturated(dec_mul(a, b)),
        BinOp::Div => {
            if b == 0 {
                return Some(Err(EvalError { message: "Dec division by zero".to_string() }));
            }
            saturated(dec_div(a, b))
        }
        BinOp::Lt => Value::Bool(a < b),
        BinOp::Gt => Value::Bool(a > b),
        BinOp::Le => Value::Bool(a <= b),
        BinOp::Ge => Value::Bool(a >= b),
        BinOp::Eq => Value::Bool(a == b),
        BinOp::Ne => Value::Bool(a != b),
        _ => return None,
    }))
}

/// Mix bytes into a hasher's state. FNV-1a, 64-bit.
///
/// `Builtin.roc` declares every `Hasher.write_*` as an intrinsic and says only that a
/// hash must be consistent — the algorithm is the compiler's business, and `Dict`'s
/// fingerprints and bucket indices are computed from whatever it returns. FNV-1a is
/// small, has no state beyond the accumulator, and is what a `{ state : U64 }` hasher
/// can hold.
fn hash_mix(state: u64, bytes: &[u8]) -> u64 {
    let mut hash = state;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

/// Read a `Hasher`'s state, mix `bytes` in, and hand back the new one.
fn hasher_write(hasher: &Value, bytes: &[u8]) -> Result<Value, EvalError> {
    let state = match hasher {
        Value::Record(fields) => fields
            .iter()
            .find(|(name, _)| *name == "state")
            .and_then(|(_, value)| match value {
                Value::Int(n) => Some(*n as u64),
                _ => None,
            }),
        _ => None,
    };
    let state = state.ok_or_else(|| EvalError {
        message: format!("a Hasher is a record with a `state`, got {}", hasher),
    })?;
    Ok(Value::Record(vec![("state", Value::Int(hash_mix(state, bytes) as i128))]))
}

/// The bytes a value contributes to a hash.
///
/// Two values that are `==` must contribute the same bytes, which is the whole contract
/// a `Dict` relies on.
fn hash_bytes(value: &Value) -> Option<Vec<u8>> {
    Some(match value {
        Value::Int(n) => n.to_le_bytes().to_vec(),
        Value::Str(text) => text.as_bytes().to_vec(),
        Value::Bool(b) => vec![u8::from(*b)],
        Value::Float(f) => f.to_le_bytes().to_vec(),
        Value::Unit => Vec::new(),
        // A list or a tuple hashes as its elements in order; a tag as its name then its
        // payload. Records are not hashed — roc derives that from the shape, which
        // rocflight has no access to here.
        Value::List(_) | Value::Tuple(_) => {
            let mut bytes = Vec::new();
            for item in value.sequence().expect("matched a sequence") {
                bytes.extend(hash_bytes(item)?);
            }
            bytes
        }
        Value::Tag(name, payload) => {
            let mut bytes = name.as_bytes().to_vec();
            for item in payload.iter() {
                bytes.extend(hash_bytes(item)?);
            }
            bytes
        }
        _ => return None,
    })
}

/// `Hasher.write_u64(h, n)` and the fifteen siblings, plus `x.to_hash(h)`.
fn call_hasher(
    module: &str,
    method: &str,
    args: &[Value],
) -> Option<Result<Value, EvalError>> {
    // `Hasher.write_*`: the hasher first, then the value. Every width mixes the same
    // way, because rocflight keeps one integer representation.
    if module == "Hasher" && method.starts_with("write") {
        let (hasher, value) = (args.first()?, args.get(1)?);
        return Some(match hash_bytes(value) {
            Some(bytes) => hasher_write(hasher, &bytes),
            None => Err(EvalError {
                message: format!("{}.{} cannot hash {}", module, method, value),
            }),
        });
    }
    // `x.to_hash(hasher)`: the value first, then the hasher — it is a method ON the
    // value. Declared per type in `Builtin.roc` and an intrinsic for every type whose
    // representation the compiler owns.
    if method == "to_hash" {
        let (value, hasher) = (args.first()?, args.get(1)?);
        return Some(match hash_bytes(value) {
            Some(bytes) => hasher_write(hasher, &bytes),
            None => Err(EvalError {
                message: format!("{}.to_hash cannot hash {}", module, value),
            }),
        });
    }
    None
}

/// Is `Module.name` a CONSTANT rather than a function?
///
/// `U64.highest` is a value, so a bare reference to it has to be evaluated where it
/// stands. Left as a builtin it became the function value `<builtin U64.highest/1>`,
/// and `U64.highest - 1` then failed with "Invalid operands".
pub fn is_numeric_constant(module: &str, name: &str) -> bool {
    numeric_width(module).is_some() && matches!(name, "highest" | "lowest")
}

/// The bit width of a numeric module, and whether it is signed.
///
/// `Num` is not here: it is the shared namespace, not a width.
fn numeric_width(module: &str) -> Option<(u32, bool)> {
    Some(match module {
        "U8" => (8, false),
        "U16" => (16, false),
        "U32" => (32, false),
        "U64" => (64, false),
        "U128" => (128, false),
        "I8" => (8, true),
        "I16" => (16, true),
        "I32" => (32, true),
        "I64" => (64, true),
        "I128" => (128, true),
        _ => return None,
    })
}

/// Truncate a value to `bits`, interpreting the result as signed or unsigned.
///
/// This is what every `_wrap` operation in `Builtin.roc` means: keep the low bits and
/// let the rest go, which is how `U8.shl_wrap(200, 1)` is 144 rather than an overflow.
fn wrap_to(value: i128, bits: u32, signed: bool) -> i128 {
    if bits >= 128 {
        return value;
    }
    let truncated = value & ((1i128 << bits) - 1);
    if signed && (truncated >> (bits - 1)) & 1 == 1 {
        truncated - (1i128 << bits)
    } else {
        truncated
    }
}

/// `to_u32_wrap`, `to_i8_wrap`, `to_u64` — the target width is in the NAME.
///
/// The `_wrap` forms truncate; the plain ones widen and are only written where the
/// value is known to fit.
fn conversion_target(method: &str) -> Option<(u32, bool, bool)> {
    let (rest, wrapping) = match method.strip_suffix("_wrap") {
        Some(rest) => (rest, true),
        None => (method, false),
    };
    let rest = rest.strip_prefix("to_")?;
    let signed = match rest.as_bytes().first()? {
        b'u' => false,
        b'i' => true,
        _ => return None,
    };
    let bits: u32 = rest[1..].parse().ok()?;
    matches!(bits, 8 | 16 | 32 | 64 | 128).then_some((bits, signed, wrapping))
}

/// The checked and saturating arithmetic every numeric width declares, plus `Dec`.
///
/// `plus_try : a, a -> Try(a, [Overflow, ..])` and its siblings are how roc writes
/// arithmetic that must not wrap. `Dec` has no fixed width here — it is an i128 of
/// eighteen decimal places — so its bound is the i128's.
fn call_checked(
    module: &str,
    method: &str,
    args: &[Value],
) -> Option<Result<Value, EvalError>> {
    let is_dec = module == "Dec" || args.iter().any(|a| matches!(a, Value::Dec(_)));
    if !is_dec && numeric_width(module).is_none() {
        return None;
    }

    // `Dec.lowest` and `Dec.highest` are the bounds of the underlying i128.
    if is_dec {
        match method {
            "highest" => return Some(Ok(Value::Dec(i128::MAX))),
            "lowest" => return Some(Ok(Value::Dec(i128::MIN))),
            _ => {}
        }
    }
    // `n.to_dec()` widens a whole number into a fixed-point one.
    if method == "to_dec" {
        return Some(match as_dec(args.first()?) {
            Some(raw) => Ok(Value::Dec(raw)),
            None => Err(EvalError {
                message: format!("to_dec needs a number, got {}", args[0]),
            }),
        });
    }
    if !is_dec {
        return None;
    }

    let (a, b) = (as_dec(args.first()?)?, as_dec(args.get(1)?)?);
    let op = match method {
        "plus_try" | "plus_saturated" => BinOp::Add,
        "minus_try" | "minus_saturated" => BinOp::Sub,
        "times_try" | "times_saturated" => BinOp::Mul,
        "div_by" => BinOp::Div,
        _ => return None,
    };
    let exact = match op {
        BinOp::Add => a.checked_add(b),
        BinOp::Sub => a.checked_sub(b),
        // The product carries two factors of the scale; one comes back out.
        BinOp::Mul => dec_mul(a, b),
        _ => None,
    };
    Some(Ok(if method.ends_with("_try") {
        // `Try` on overflow, which is what lets `SafeMath` report one.
        match exact {
            Some(value) => Value::tag("Ok", vec![Value::Dec(value)]),
            None => Value::tag("Err", vec![Value::tag("Overflow", Vec::new())]),
        }
    } else {
        // Saturating: pinned at the bound, which is how `times_saturated` signals an
        // overflow to a caller that then compares against `Dec.highest`.
        match exact {
            Some(value) => Value::Dec(value),
            None => Value::Dec(if (a < 0) == (b < 0) { i128::MAX } else { i128::MIN }),
        }
    }))
}

/// The numeric operations `Builtin.roc` writes as methods on a width.
///
/// Bit manipulation, wrapping shifts and width conversions — the ops a `Dict`'s bucket
/// arithmetic is built from. They are dispatched on the module, which names the width,
/// so `U32.shl_wrap(1, 8)` truncates to 32 bits and `U8.shl_wrap(1, 8)` is 0.
///
/// `ponytail: one integer representation, i64. A U64 above i64::MAX is held as the same
/// BIT PATTERN, which every operation here is correct on, but a comparison or a
/// `to_str` on one reads it as negative. Closing that needs a wider Value integer.`
fn call_numeric(
    module: &str,
    method: &str,
    args: &[Value],
) -> Option<Result<Value, EvalError>> {
    let (bits, signed) = numeric_width(module)?;
    let whole = |v: &Value| match v {
        Value::Int(n) => Some(*n as i128),
        _ => None,
    };
    let out = |v: i128| Ok(Value::Int(wrap_to(v, bits, signed) as i128));

    // `U32.highest`, `I8.lowest` — the bounds of the width, as values rather than calls.
    //
    // SATURATED to what an i64 can hold, because that is the bound that is true here:
    // `U64.highest` is 18446744073709551615, which wraps to -1 in an i64, and
    // Builtin.roc's own capacity guard is `if b > U64.highest - a` — against -1 that
    // fires every time and a first insert crashes with "Dict capacity overflow".
    // Reporting the representable ceiling makes the guard mean what it says.
    // `ponytail: the ceiling IS the representation; a wider Value integer removes it.`
    match method {
        "highest" => {
            let width_max =
                if signed { (1i128 << (bits - 1)) - 1 } else { (1i128 << bits) - 1 };
            return Some(Ok(Value::Int(width_max.min(i64::MAX as i128) as i128)));
        }
        "lowest" => {
            let width_min = if signed { -(1i128 << (bits - 1)) } else { 0 };
            return Some(Ok(Value::Int(width_min.max(i64::MIN as i128) as i128)));
        }
        _ => {}
    }

    let a = whole(args.first()?)?;
    // A conversion reads its TARGET width off the method name, not the receiver's.
    if let Some((to_bits, to_signed, wrapping)) = conversion_target(method) {
        let converted = wrap_to(a, to_bits, to_signed);
        if !wrapping && converted != a {
            return Some(Err(EvalError {
                message: format!("{}.{} cannot hold {}", module, method, a),
            }));
        }
        return Some(Ok(Value::Int(converted as i128)));
    }

    let b = args.get(1).and_then(whole);
    Some(match (method, b) {
        ("bitwise_and", Some(b)) => out(a & b),
        ("bitwise_or", Some(b)) => out(a | b),
        ("bitwise_xor", Some(b)) => out(a ^ b),
        ("bitwise_not", _) => out(!a),
        ("shl_wrap", Some(n)) => out(a << (n as u32 % bits)),
        // Arithmetic: the sign bit is carried down, which is what makes
        // `I8.shr_wrap(x, 7)` the all-ones mask a seed check wants.
        ("shr_wrap", Some(n)) => out(a >> (n as u32 % bits)),
        // Zero-fill: the value is read as unsigned first, so nothing is carried down.
        ("shr_zf_wrap", Some(n)) => {
            let unsigned = wrap_to(a, bits, false) as u128;
            out((unsigned >> (n as u32 % bits)) as i128)
        }
        ("div_trunc_by", Some(0)) | ("rem_by", Some(0)) | ("div_floor_by", Some(0))
        | ("div_ceil_by", Some(0)) => Err(EvalError {
            message: format!("{}.{} divides by zero", module, method),
        }),
        ("div_trunc_by", Some(b)) => out(a / b),
        ("rem_by", Some(b)) => out(a % b),
        ("div_floor_by", Some(b)) => out(a.div_euclid(b)),
        ("div_ceil_by", Some(b)) => out(a / b + i128::from((a % b != 0) && ((a < 0) == (b < 0)))),
        _ => return None,
    })
}

/// The LOW-LEVEL ops `Builtin.roc` calls but never defines.
///
/// In the real compiler these are `LowLevel` variants that `canonicalize/
/// BuiltinLowLevel.zig` rewrites the annotation-only declarations into; the list of
/// them is `src/base/LowLevel.zig`, 502 long. rocflight keeps one integer
/// representation and one float, so the per-width families collapse and only these are
/// needed. The names are exactly as Builtin.roc spells them — `rocflight
/// --builtins=names` lists every one still missing.
///
/// This is also the registry: the compiler asks here whether a bare name is a
/// low-level op, so the list and the implementations cannot drift apart.
pub fn low_level_arity(name: &str) -> Option<usize> {
    Some(match name {
        "list_get_unsafe" => 2,
        "list_set_unsafe" => 3,
        "list_swap_unsafe" => 3,
        "list_append_unsafe" | "u8_list_append_unsafe" => 2,
        "list_replace_unsafe" => 3,
        "list_with_capacity" | "u8_list_with_capacity" => 1,
        "u8_list_len" => 1,
        "u8_list_get_unsafe" => 2,
        "hasher_finish" => 1,
        "dict_pseudo_seed" => 0,
        _ => return None,
    })
}

/// Run one low-level op. `low_level_arity` decides what reaches here.
fn call_low_level(name: &str, args: Vec<Value>) -> Result<Value, EvalError> {
    let wrong = |what: &str| EvalError { message: format!("{} needs {}", name, what) };
    // Moved out of `args`, so a list nothing else holds is mutated in place — which is
    // what `Dict`'s bucket writes count on being cheap.
    let mut args = args;
    let list = |v: &mut Value| match std::mem::replace(v, Value::Unit) {
        Value::List(items) => Ok(value::into_items(items)),
        other => Err(EvalError { message: format!("{} needs a List, got {}", name, other) }),
    };
    let index = |v: &Value| match v {
        Value::Int(n) if *n >= 0 => Ok(*n as usize),
        other => Err(EvalError { message: format!("{} needs an index, got {}", name, other) }),
    };
    match name {
        // "unsafe" means the CALLER has already proved the index is in bounds. Roc
        // elides the check; rocflight cannot elide a Rust bounds check, so an
        // out-of-range index is a message rather than a panic.
        // A read: the list stays where it is, no copy.
        "list_get_unsafe" | "u8_list_get_unsafe" => {
            let i = index(&args[1])?;
            let Value::List(items) = &args[0] else {
                return Err(EvalError { message: format!("{} needs a List, got {}", name, args[0]) });
            };
            items.get(i).cloned().ok_or_else(|| EvalError {
                message: format!("{}: index {} is past the end of a list of {}", name, i, items.len()),
            })
        }
        "list_set_unsafe" => {
            let i = index(&args[1])?;
            let mut items = list(&mut args[0])?;
            *items.get_mut(i).ok_or_else(|| wrong("an index within the list"))? = args[2].clone();
            Ok(Value::list(items))
        }
        "list_swap_unsafe" => {
            let (i, j) = (index(&args[1])?, index(&args[2])?);
            let mut items = list(&mut args[0])?;
            if i >= items.len() || j >= items.len() {
                return Err(wrong("two indices within the list"));
            }
            items.swap(i, j);
            Ok(Value::list(items))
        }
        "list_append_unsafe" | "u8_list_append_unsafe" => {
            let mut items = list(&mut args[0])?;
            items.push(args[1].clone());
            Ok(Value::list(items))
        }
        // Returns the new list paired with what was there, so a caller can reuse the
        // displaced value without a second lookup.
        "list_replace_unsafe" => {
            let i = index(&args[1])?;
            let mut items = list(&mut args[0])?;
            let slot = items.get_mut(i).ok_or_else(|| wrong("an index within the list"))?;
            let prev = std::mem::replace(slot, args[2].clone());
            Ok(Value::Record(vec![("list", Value::list(items)), ("prev", prev)]))
        }
        // Capacity is a hint about allocation, which is not observable through the API.
        "list_with_capacity" | "u8_list_with_capacity" => Ok(Value::list(Vec::new())),
        "u8_list_len" => match &args[0] {
            Value::List(items) => Ok(Value::Int(items.len() as i128)),
            other => Err(EvalError { message: format!("{} needs a List, got {}", name, other) }),
        },
        // `Hasher :: { state : U64 }`, and the digest IS the state: every `write_*`
        // has already mixed into it.
        "hasher_finish" => match &args[0] {
            Value::Record(fields) => fields
                .iter()
                .find(|(field, _)| *field == "state")
                .map(|(_, value)| value.clone())
                .ok_or_else(|| wrong("a Hasher with a `state` field")),
            other => Err(EvalError { message: format!("hasher_finish needs a Hasher, got {}", other) }),
        },
        // ponytail: a FIXED seed, where roc randomises per process. The seed exists to
        // make hash flooding impractical, which no test here can observe, and a
        // constant keeps a Dict's iteration order reproducible between runs.
        "dict_pseudo_seed" => Ok(Value::Int(0x243F_6A88_85A3_08D3u64 as i128)),
        _ => Err(EvalError { message: format!("low-level op `{}` is not implemented", name) }),
    }
}

pub fn call_builtin_values(
    module: &str,
    name: &str,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    // The low-level ops are spelled as bare names in Builtin.roc, so the compiler sends
    // them here under a module of its own rather than one of Roc's.
    if module == "LowLevel" {
        return call_low_level(name, args);
    }
    // `to_str` is dispatched on the numeric type, so it is spelled `I64.to_str`,
    // `F64.to_str`, `U8.to_str`, ... — not only `Num.to_str`. All of them
    // stringify the same way here; the interpreter does not yet track which
    // numeric type a value has (see IMPLEMENTATION_PHASES.md, numeric types).
    // The `Encoding` protocol's own operations, which a type's `encoder_for` calls to
    // add to the text: `Encoding.encode_u32(n, state)`.
    if module == "Encoding" {
        if let Some(name) = name.strip_prefix("encode_") {
            let (written, state) = (args.first(), args.get(1));
            if let (Some(written), Some(Value::Str(state))) = (written, state) {
                let rendered = match (name, written) {
                    ("str", Value::Str(text)) => crate::eval::value::quoted(text),
                    // The method NAME says which type is being written, and an
                    // integer one writes an integer: the `1` in
                    // `Encoding.encode_u32(1, state)` is an unconstrained numeral to
                    // this interpreter and would otherwise render as `1.0`.
                    (name, _) if name.starts_with('u') || name.starts_with('i') => {
                        match as_dec(written) {
                            Some(scaled) => (scaled / DEC_SCALE).to_string(),
                            None => json_write(written),
                        }
                    }
                    _ => json_write(written),
                };
                // `encode_… : value, state -> Try(state, [])`.
                return Ok(Value::tag(
                    "Ok",
                    vec![str_value(format!("{}{}", state, rendered))],
                ));
            }
        }
    }
    if module == "Json" {
        if let Some(result) = call_json(name, &args) {
            return result;
        }
    }
    if let Some(result) = call_hasher(module, name, &args) {
        return result;
    }
    if let Some(result) = call_checked(module, name, &args) {
        return result;
    }

    // The width-aware numeric operations, before the shared `Num` handling: they are
    // the ones whose ANSWER depends on which width the module names.
    if let Some(result) = call_numeric(module, name, &args) {
        return result;
    }

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
            Ok(n) => Value::tag("Ok", vec![Value::Int(n as i128)]),
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
                    (Value::Float(f), false) => Ok(Value::Int(f as i128)),
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
                        Value::Int(value as i128)
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
            Ok(Value::Int(text.len() as i128))
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
            Ok(Value::list(
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
            Ok(Value::list(
                text.as_bytes().iter().map(|b| Value::Int(*b as i128)).collect(),
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
        _ => {
            // A platform's hosted effect: the host's own code, reached through the
            // registry when rocflight is linked into that host.
            if let Some(answer) = crate::platform::hosted::call(module, name, &args) {
                return answer.map_err(|message| EvalError { message });
            }
            Err(EvalError {
                // A platform that declares this really does provide it: the gap is
                // that its signature could not be laid out for the host (a type the
                // loaded modules do not declare), and `--show-platforms` names which.
                message: if crate::platform::real::is_declared(module, name) {
                    format!(
                        "`{}.{}` is an effect the platform declares, but its signature could \
                         not be laid out for the host; run with --show-platforms to see why",
                        module, name
                    )
                } else {
                    format!("Unknown function {}.{}", module, name)
                },
            })
        }
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

    let mut candidates = crate::vm::methods_named(method, left);
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
    // A `Dec` on either side makes the whole operation fixed point. An ordinary
    // integer beside one is widened, which is how `total / n` works when `n` is a
    // length.
    if matches!(left, Value::Dec(_)) || matches!(right, Value::Dec(_)) {
        if let (Some(a), Some(b)) = (as_dec(left), as_dec(right)) {
            if let Some(result) = dec_binop(op, a, b) {
                return result;
            }
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
        (op, left, right) => {
            Err(EvalError {
                // Naming them: "Invalid operands for operator" left no way to tell
                // which operator, on what, without a bisect.
                message: format!("Invalid operands for {:?}: {} and {}", op, left, right),
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
        (Value::Dec(d1), Value::Dec(d2)) => d1 == d2,
        // `expect safe_variance([0]) == Ok(0)` compares a Dec against a whole number.
        (Value::Dec(d), Value::Int(n)) | (Value::Int(n), Value::Dec(d)) => {
            as_dec(&Value::Int(*n)).is_some_and(|scaled| scaled == *d)
        }
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
    // An OPAQUE nominal shows as `<opaque>`: roc will not print the inside of one, and
    // `AllSyntax`'s `Secret :: { key : Str }` relies on that to keep its key hidden.
    if matches!(value, Value::Record(_) | Value::Tag(..) | Value::Tuple(_))
        && crate::vm::is_opaque(value)
    {
        return "<opaque>".to_string();
    }
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
/// Run one `expect` inside a function body: a runtime assertion, not a test.
///
/// A failure is reported and execution CONTINUES — roc prints to stderr and carries on
/// rather than aborting — but the run ends up exiting non-zero. It is never tallied:
/// `roc test` counts top-level `expect`s only, even when one of them calls a function
/// whose own assertion fires. Shared with the VM's `Expect` opcode.
pub fn run_expect(value: &Value) -> Result<(), EvalError> {
    match value {
        Value::Bool(true) => Ok(()),
        Value::Bool(false) => {
            ASSERT_FAILED.store(true, std::sync::atomic::Ordering::Relaxed);
            report(Report::ExpectFailed, "expect failed");
            Ok(())
        }
        other => Err(EvalError {
            message: format!("`expect` needs a Bool, got {}", other),
        }),
    }
}

/// Run one TOP-LEVEL `expect`: a test, tallied for the `roc test` report.
pub fn run_test_expect(value: &Value) -> Result<(), EvalError> {
    expect_ran();
    match value {
        Value::Bool(false) => {
            expect_failed();
            report(Report::ExpectFailed, "expect failed");
            Ok(())
        }
        _ => run_expect(value),
    }
}

/// `dbg value` — to stderr, so it never mixes into a program's output.
pub fn run_dbg(value: &Value) {
    report(Report::Dbg, &inspect(value));
}

/// What a program has to say outside its output: a `dbg`, a failed inline `expect`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Report {
    Dbg,
    ExpectFailed,
}

/// Where reports go. By default to stderr, the way `roc` prints them. When rocflight
/// is linked into a platform's host the host owns the process and its `roc_dbg` /
/// `roc_expect_failed` are what should hear them, so the host installs itself here.
static REPORTER: std::sync::OnceLock<fn(Report, &str)> = std::sync::OnceLock::new();

/// Route reports to `to` for the rest of the process. Only the first caller wins.
pub fn set_reporter(to: fn(Report, &str)) {
    let _ = REPORTER.set(to);
}

fn report(kind: Report, message: &str) {
    match REPORTER.get() {
        Some(to) => to(kind, message),
        None => match kind {
            Report::Dbg => eprintln!("[dbg] {}", message),
            Report::ExpectFailed => eprintln!("Expect failed: {}", message),
        },
    }
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
pub fn int_binop(op: BinOp, a: i128, b: i128) -> Option<Result<Value, EvalError>> {
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

    // The `Encoding` protocol's reading half. A type's own `parser_for` dispatches
    // these ON the encoding it was handed — `encoding.parse_u32(state)` — and the
    // encoding this interpreter hands out is the marker below.
    if let Value::Tag(name, _) = &receiver {
        if &**name == JSON_ENCODING {
            if let Some(read) = method.strip_prefix("parse_") {
                return json_parse_piece(read, args.first());
            }
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

/// Did an `expect` inside a function body fail? A normal run exits 1 if so.
static ASSERT_FAILED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Whether any in-function `expect` failed during this run.
pub fn assert_failed() -> bool {
    ASSERT_FAILED.load(std::sync::atomic::Ordering::Relaxed)
}

/// `(ran, failed)` for every top-level `expect` evaluated so far.
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
        Value::Dec(_) => "Dec",
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

// --- JSON ----------------------------------------------------------------------------

/// The encoding value handed to a type's `parser_for` and `encoder_for`.
///
/// `Builtin.roc` has a real `JsonEncoding` with its own methods; this is a marker that
/// `dispatch_builtin` recognises, which is the same idea with the 108 members of the
/// protocol left out.
const JSON_ENCODING: &str = "#JsonEncoding";

/// One `encoding.parse_…(state)` step: read a piece off the front of the remaining
/// text and hand back what was read with the rest.
fn json_parse_piece(read: &str, state: Option<&Value>) -> Result<Value, EvalError> {
    let text = match state {
        Some(Value::Str(s)) => s.to_string(),
        other => {
            return Err(EvalError {
                message: format!("a parser's state is the text left to read, got {:?}", other),
            })
        }
    };
    let mut cursor = JsonCursor { bytes: text.as_bytes(), at: 0 };
    cursor.space();
    let value = match read {
        "str" => cursor.string().map(str_value),
        _ => cursor.value(),
    };
    Ok(match value {
        Some(value) => Value::tag(
            "Ok",
            vec![Value::Record(vec![
                ("rest", str_value(text[cursor.at..].to_string())),
                ("value", value),
            ])],
        ),
        None => Value::tag(
            "Err",
            vec![Value::tag("InvalidJson", vec![str_value(text)])],
        ),
    })
}

/// `Json.parse` and `Json.to_str`.
///
/// `Builtin.roc` derives these from the SHAPE being decoded, through the `Encoding`
/// protocol at `Builtin.roc:60` — 108 members of it. rocflight has no derivation, so it
/// reads JSON into its own values instead: an object is a record, an array a list, a
/// string a Str. Roc records are structural and rocflight erases nominals, so
/// `record.image.title` lands on a plain field and the shape never has to be consulted.
///
/// `ponytail: shape-blind. The annotation is not read, so a mismatch surfaces as a
/// missing field where it is used rather than as an `Err` at the parse, and a number
/// arrives as the interpreter's own integer or Dec rather than the annotated width.
/// Deriving from the shape is what closes that, and needs the Encoding protocol.`
fn call_json(method: &str, args: &[Value]) -> Option<Result<Value, EvalError>> {
    match method {
        "parse" | "from_str" => {
            let text = match args.first()? {
                Value::Str(s) => s.to_string(),
                other => {
                    return Some(Err(EvalError {
                        message: format!("Json.parse needs a Str, got {}", other),
                    }))
                }
            };
            // The TARGET type, when the compiler knew one. Without it the document is
            // read as it stands, which is enough whenever the shapes already line up —
            // roc records are structural, so an object becomes a record and
            // `record.image.title` lands on a field.
            let target = args.get(1).cloned().unwrap_or(Value::Unit);
            let invalid =
                || Value::tag("Err", vec![Value::tag("InvalidJson", vec![str_value(text.clone())])]);
            let mut cursor = JsonCursor { bytes: text.as_bytes(), at: 0 };
            Some(match json_read_as(&mut cursor, &target) {
                Err(e) => Err(e),
                Ok(Some(value)) if { cursor.space(); cursor.at >= cursor.bytes.len() } => {
                    Ok(Value::tag("Ok", vec![value]))
                }
                Ok(_) => Ok(invalid()),
            })
        }
        "to_str" | "encode" => Some(json_encode(args.first()?).map(str_value)),
        _ => None,
    }
}

/// Read a JSON document into the type `target` describes.
///
/// A `Nominal` hands the reading to that type's own `parser_for`, which is how
/// `EncodeDecode`'s `ItemKind` turns the number 1 back into `Text`. Anything else is
/// read as it stands.
fn json_read_as(
    cursor: &mut JsonCursor,
    target: &Value,
) -> Result<Option<Value>, EvalError> {
    cursor.space();
    match target {
        Value::Tag(tag, payload) if &**tag == "List" => {
            let element = payload.first().cloned().unwrap_or(Value::Unit);
            if cursor.bytes.get(cursor.at) != Some(&b'[') {
                return Ok(None);
            }
            cursor.at += 1;
            let mut items = Vec::new();
            cursor.space();
            if cursor.bytes.get(cursor.at) == Some(&b']') {
                cursor.at += 1;
                return Ok(Some(Value::list(items)));
            }
            loop {
                match json_read_as(cursor, &element)? {
                    Some(item) => items.push(item),
                    None => return Ok(None),
                }
                cursor.space();
                match cursor.bytes.get(cursor.at) {
                    Some(b',') => cursor.at += 1,
                    Some(b']') => {
                        cursor.at += 1;
                        return Ok(Some(Value::list(items)));
                    }
                    _ => return Ok(None),
                }
            }
        }
        Value::Tag(tag, payload) if &**tag == "Nominal" => {
            let Some(Value::Str(name)) = payload.first() else { return Ok(None) };
            custom_parser(name, cursor)
        }
        _ => Ok(cursor.value()),
    }
}

/// Run a type's own `parser_for` over the rest of the document.
fn custom_parser(
    nominal: &str,
    cursor: &mut JsonCursor,
) -> Result<Option<Value>, EvalError> {
    let Some(parser_for) = crate::vm::method_by_name(nominal, "parser_for") else {
        // No parser of its own: the nominal is erased anyway, so read the value plain.
        return Ok(cursor.value());
    };
    let encoding = Value::tag(JSON_ENCODING, Vec::new());
    let parser = call_function(parser_for, vec![encoding])?;
    let rest = std::str::from_utf8(&cursor.bytes[cursor.at..])
        .map_err(|_| EvalError { message: "JSON must be UTF-8".to_string() })?;
    let parsed = call_function(parser, vec![str_value(rest.to_string())])?;
    match parsed {
        Value::Tag(tag, payload) if &*tag == "Ok" => match payload.first() {
            Some(Value::Record(fields)) => {
                let field = |want: &str| fields.iter().find(|(n, _)| *n == want).map(|(_, v)| v);
                let (Some(value), Some(Value::Str(left))) = (field("value"), field("rest")) else {
                    return Ok(None);
                };
                // The parser reports what is LEFT, which is where the cursor now is.
                cursor.at = cursor.bytes.len() - left.len();
                Ok(Some(value.clone()))
            }
            _ => Ok(None),
        },
        _ => Ok(None),
    }
}

/// Render a value as JSON, letting a type's own `encoder_for` answer for it.
///
/// `Builtin.roc` derives an encoder from the shape through the `Encoding` protocol; a
/// type that wants a different rendering writes `encoder_for` itself, and
/// `EncodeDecode`'s `ItemKind` does — its ten tags encode as the numbers 1 to 10 rather
/// than as their names. Found by SHAPE, the same way a method is found for a value
/// whose nominal the runtime cannot name.
fn json_encode(value: &Value) -> Result<String, EvalError> {
    if let Some(custom) = custom_encoder(value)? {
        return Ok(custom);
    }
    Ok(match value {
        // A list drives each element's encoder and puts the separators in itself: the
        // element encoders know nothing about where they sit.
        Value::List(_) | Value::Tuple(_) => {
            let items = value.sequence().expect("matched a sequence");
            let mut parts = Vec::with_capacity(items.len());
            for item in items {
                parts.push(json_encode(item)?);
            }
            format!("[{}]", parts.join(","))
        }
        Value::Record(fields) => {
            let mut parts = Vec::with_capacity(fields.len());
            for (name, v) in fields {
                parts.push(format!("{}:{}", crate::eval::value::quoted(name), json_encode(v)?));
            }
            format!("{{{}}}", parts.join(","))
        }
        other => json_write(other),
    })
}

/// Run a type's own `encoder_for`, if it declares one.
fn custom_encoder(value: &Value) -> Result<Option<String>, EvalError> {
    if !matches!(value, Value::Record(_) | Value::Tag(..) | Value::Tuple(_)) {
        return Ok(None);
    }
    let mut candidates = crate::vm::methods_named("encoder_for", value);
    if candidates.len() != 1 {
        return Ok(None);
    }
    let (_, encoder_for) = candidates.pop().expect("checked len");
    // `encoder_for : encoding -> (value, state -> Try(state, []))`. The encoding is a
    // type witness the body never reads — it dispatches `Encoding.encode_*` on it —
    // so what is passed matters only in that something must be.
    let encoder = call_function(encoder_for, vec![Value::Unit])?;
    // The state is the text built so far. `Builtin.roc` threads a `List(U8)`; a Str is
    // the same thing said in the form this interpreter already has.
    let encoded = call_function(encoder, vec![value.clone(), str_value("")])?;
    match encoded {
        Value::Tag(tag, payload) if &*tag == "Ok" => match payload.first() {
            Some(Value::Str(text)) => Ok(Some(text.to_string())),
            other => Err(EvalError {
                message: format!("an encoder must give back the text so far, got {:?}", other),
            }),
        },
        other => Err(EvalError { message: format!("encoding failed: {}", other) }),
    }
}

/// Render a value as JSON.
fn json_write(value: &Value) -> String {
    match value {
        Value::Str(s) => crate::eval::value::quoted(s),
        Value::Int(n) => n.to_string(),
        Value::Dec(d) => dec_to_string(*d),
        Value::Float(f) => f.to_string(),
        Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        Value::Unit => "null".to_string(),
        Value::List(_) | Value::Tuple(_) => {
            let items = value.sequence().expect("matched a sequence");
            let parts: Vec<String> = items.iter().map(json_write).collect();
            format!("[{}]", parts.join(","))
        }
        Value::Record(fields) => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(name, v)| format!("{}:{}", crate::eval::value::quoted(name), json_write(v)))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        // A tag with no payload is its name, which is how roc's own JSON renders a
        // payload-less variant; one WITH a payload renders as the payload.
        Value::Tag(name, payload) => match payload.len() {
            0 => crate::eval::value::quoted(name),
            1 => json_write(&payload[0]),
            _ => {
                let parts: Vec<String> = payload.iter().map(json_write).collect();
                format!("[{}]", parts.join(","))
            }
        },
        other => crate::eval::value::quoted(&other.to_string()),
    }
}

/// A position in a JSON document.
struct JsonCursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl JsonCursor<'_> {
    fn space(&mut self) {
        while self.bytes.get(self.at).is_some_and(|b| b.is_ascii_whitespace()) {
            self.at += 1;
        }
    }

    fn eat(&mut self, word: &str) -> bool {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            return true;
        }
        false
    }

    fn value(&mut self) -> Option<Value> {
        self.space();
        match *self.bytes.get(self.at)? {
            b'"' => self.string().map(str_value),
            b'{' => self.object(),
            b'[' => self.array(),
            b't' => self.eat("true").then_some(Value::Bool(true)),
            b'f' => self.eat("false").then_some(Value::Bool(false)),
            b'n' => self.eat("null").then_some(Value::Unit),
            _ => self.number(),
        }
    }

    fn string(&mut self) -> Option<String> {
        if *self.bytes.get(self.at)? != b'"' {
            return None;
        }
        self.at += 1;
        let mut out = String::new();
        loop {
            match *self.bytes.get(self.at)? {
                b'"' => {
                    self.at += 1;
                    return Some(out);
                }
                b'\\' => {
                    self.at += 1;
                    let escaped = *self.bytes.get(self.at)?;
                    out.push(match escaped {
                        b'n' => '\n',
                        b't' => '\t',
                        b'r' => '\r',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        // `\uXXXX`, the only escape that is not one character.
                        b'u' => {
                            let hex = self.bytes.get(self.at + 1..self.at + 5)?;
                            let code = u32::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
                            self.at += 4;
                            char::from_u32(code)?
                        }
                        other => other as char,
                    });
                    self.at += 1;
                }
                _ => {
                    // A whole character, not a byte: slicing mid-UTF-8 would corrupt it.
                    let rest = std::str::from_utf8(&self.bytes[self.at..]).ok()?;
                    let ch = rest.chars().next()?;
                    out.push(ch);
                    self.at += ch.len_utf8();
                }
            }
        }
    }

    fn number(&mut self) -> Option<Value> {
        let start = self.at;
        if self.bytes.get(self.at) == Some(&b'-') {
            self.at += 1;
        }
        let mut fractional = false;
        while let Some(byte) = self.bytes.get(self.at) {
            match byte {
                b'0'..=b'9' => self.at += 1,
                b'.' | b'e' | b'E' | b'+' | b'-' => {
                    fractional = true;
                    self.at += 1;
                }
                _ => break,
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at]).ok()?;
        if text.is_empty() {
            return None;
        }
        if fractional {
            // A fractional JSON number is a `Dec`, the type roc defaults one to.
            Some(Value::Dec(dec_from_f64(text.parse::<f64>().ok()?)))
        } else {
            Some(Value::Int(text.parse::<i128>().ok()?))
        }
    }

    fn object(&mut self) -> Option<Value> {
        self.at += 1; // `{`
        let mut fields = Vec::new();
        self.space();
        if self.bytes.get(self.at) == Some(&b'}') {
            self.at += 1;
            return Some(Value::Record(fields));
        }
        loop {
            self.space();
            let name = self.string()?;
            // Field names outlive the program, as every other record's do — interned,
            // so a document with ten thousand objects leaks one copy of each name
            // rather than ten thousand.
            let name: &'static str = crate::memory::string_pool::intern(&name);
            self.space();
            if *self.bytes.get(self.at)? != b':' {
                return None;
            }
            self.at += 1;
            fields.push((name, self.value()?));
            self.space();
            match *self.bytes.get(self.at)? {
                b',' => self.at += 1,
                b'}' => {
                    self.at += 1;
                    // Sorted, as `Str.inspect` shows every record.
                    fields.sort_by(|a, b| a.0.cmp(b.0));
                    return Some(Value::Record(fields));
                }
                _ => return None,
            }
        }
    }

    fn array(&mut self) -> Option<Value> {
        self.at += 1; // `[`
        let mut items = Vec::new();
        self.space();
        if self.bytes.get(self.at) == Some(&b']') {
            self.at += 1;
            return Some(Value::list(items));
        }
        loop {
            items.push(self.value()?);
            self.space();
            match *self.bytes.get(self.at)? {
                b',' => self.at += 1,
                b']' => {
                    self.at += 1;
                    return Some(Value::list(items));
                }
                _ => return None,
            }
        }
    }
}
