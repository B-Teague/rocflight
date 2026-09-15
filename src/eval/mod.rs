//! Expression evaluator (tree-walk interpreter)
//!
//! Phase 1: String evaluation
//! Phase 2: Numbers, identifiers (partial)
//! Phase 3: Lambdas, calls, let bindings

use crate::ast::{Expr, BinOp, Pattern};
use crate::error::EvalError;

pub mod value;
pub mod environment;

pub use value::{str_value, Value};
pub use environment::Environment;

/// Tree-walk interpreter
pub struct Evaluator {
    pub env: Environment,
    /// The value a `return` is carrying, if one is unwinding.
    ///
    /// The signal itself rides the error channel; an error carries only a message, so
    /// the value travels here and `apply` picks it up at the function boundary.
    returning: Option<Value>,
}

impl Evaluator {
    /// Create new evaluator
    pub fn new() -> Self {
        Evaluator {
            env: Environment::new(),
            returning: None,
        }
    }

    /// Evaluate expression to value
    pub fn eval(&mut self, expr: &Expr) -> Result<Value, EvalError> {
        match expr {
            Expr::Str(s) => Ok(str_value(*s)),
            Expr::Unit => Ok(Value::Unit),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Range { start, end, inclusive } => {
                let bound = |v: Value| -> Result<i64, EvalError> {
                    match v {
                        Value::Int(n) => Ok(n),
                        other => Err(EvalError {
                            message: format!("A range needs whole numbers, got {}", other),
                        }),
                    }
                };
                Ok(Value::Range {
                    start: bound(self.eval(start)?)?,
                    end: bound(self.eval(end)?)?,
                    inclusive: *inclusive,
                })
            }
            Expr::Tuple(items) => {
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    values.push(self.eval(item)?);
                }
                Ok(Value::Tuple(values))
            }
            Expr::TupleIndex { tuple, index } => {
                match self.eval(tuple)? {
                    Value::Tuple(items) => items.get(*index).cloned().ok_or_else(|| EvalError {
                        message: format!(
                            "Tuple has {} element(s), so .{} is out of range",
                            items.len(),
                            index
                        ),
                    }),
                    other => Err(EvalError {
                        message: format!("Cannot index .{} on {}", index, other),
                    }),
                }
            }
            Expr::RecordUpdate { base, fields } => {
                let mut record = match self.eval(base)? {
                    Value::Record(fields) => fields,
                    other => {
                        return Err(EvalError {
                            message: format!("Cannot update `{}`: it is not a record", other),
                        })
                    }
                };
                for (name, value) in fields {
                    let new_value = self.eval(value)?;
                    match record.iter_mut().find(|(field, _)| field == name) {
                        Some(slot) => slot.1 = new_value,
                        None => {
                            return Err(EvalError {
                                message: format!("Record has no field `{}` to update", name),
                            })
                        }
                    }
                }
                Ok(Value::Record(record))
            }
            Expr::List(items) => {
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    values.push(self.eval(item)?);
                }
                Ok(Value::List(values))
            }
            Expr::Match { scrutinee, arms } => {
                let value = self.eval(scrutinee)?;

                // Arms are tried in order; the first whose pattern matches AND whose
                // guard holds wins. Bindings live only for that arm.
                for arm in arms {
                    for pattern in &arm.patterns {
                        let mut bindings = Vec::new();
                        if !pattern_matches(pattern, &value, &mut bindings) {
                            continue;
                        }

                        self.env.push_scope();
                        for (name, bound) in bindings {
                            self.env.bind(name, bound);
                        }

                        // The guard sees the pattern's bindings, and a false guard
                        // means this arm is skipped rather than the match failing.
                        if let Some(guard) = &arm.guard {
                            let passed = match self.eval(guard) {
                                Ok(Value::Bool(b)) => b,
                                Ok(other) => {
                                    self.env.pop_scope();
                                    return Err(EvalError {
                                        message: format!(
                                            "A match guard must be a Bool, got {}",
                                            other
                                        ),
                                    });
                                }
                                Err(e) => {
                                    self.env.pop_scope();
                                    return Err(e);
                                }
                            };
                            if !passed {
                                self.env.pop_scope();
                                continue;
                            }
                        }

                        let result = self.eval(&arm.body);
                        self.env.pop_scope();
                        return result;
                    }
                }

                // Unreachable for a program roc accepted, since roc requires the arms
                // to be exhaustive. The interpreter cannot verify that itself (it
                // never sees the type annotations), so it reports it here instead of
                // silently returning something wrong.
                Err(EvalError {
                    message: format!("No match arm matched {}", value),
                })
            }
            Expr::If { condition, then_branch, otherwise } => {
                // Only the taken branch is evaluated.
                match self.eval(condition)? {
                    Value::Bool(true) => self.eval(then_branch),
                    Value::Bool(false) => self.eval(otherwise),
                    other => Err(EvalError {
                        message: format!(
                            "An if condition must be a Bool, got {}",
                            other
                        ),
                    }),
                }
            }
            Expr::VarDecl { name, value, body } => {
                let val = self.eval(value)?;
                self.env.bind(name, val);
                self.eval(body)
            }
            Expr::Assign { name, value, body } => {
                let val = self.eval(value)?;
                // Update in place. Binding instead would shadow, and a loop body's
                // scope is popped on the way out — the update would be lost.
                if !self.env.assign(name, val) {
                    return Err(EvalError {
                        message: format!(
                            "Cannot assign to `{}`: it is not declared with `var`",
                            name
                        ),
                    });
                }
                self.eval(body)
            }
            Expr::For { name, iterable, body } => {
                // A range is iterated WITHOUT being built: `for i in 0..<10_000_000`
                // would otherwise allocate ten million `Value`s before the first
                // iteration. Handled separately from a list rather than by collecting
                // into one.
                let iterable_value = self.eval(iterable)?;
                if let Value::Range { start, end, inclusive } = iterable_value {
                    let last = if inclusive { end } else { end - 1 };
                    let mut current = start;
                    while current <= last {
                        self.env.push_scope();
                        self.env.bind(name, Value::Int(current));
                        let outcome = self.eval(body);
                        self.env.pop_scope();
                        match outcome {
                            Ok(_) => {}
                            Err(e) if e.is_break() => return Ok(Value::Unit),
                            Err(e) => return Err(e),
                        }
                        current += 1;
                    }
                    return Ok(Value::Unit);
                }

                let items = match iterable_value {
                    Value::List(items) => items,
                    other => {
                        return Err(EvalError {
                            message: format!(
                                "`for` needs a List or a range to iterate, got {}",
                                other
                            ),
                        })
                    }
                };

                for item in items {
                    self.env.push_scope();
                    self.env.bind(name, item);
                    let outcome = self.eval(body);
                    self.env.pop_scope();
                    match outcome {
                        Ok(_) => {}
                        Err(e) if e.is_break() => break,
                        Err(e) => return Err(e),
                    }
                }
                // A loop is a statement: its value is unit.
                Ok(Value::Unit)
            }
            Expr::While { condition, body } => {
                loop {
                    match self.eval(condition)? {
                        Value::Bool(true) => {}
                        Value::Bool(false) => break,
                        other => {
                            return Err(EvalError {
                                message: format!(
                                    "A `while` condition must be a Bool, got {}",
                                    other
                                ),
                            })
                        }
                    }

                    self.env.push_scope();
                    let outcome = self.eval(body);
                    self.env.pop_scope();
                    match outcome {
                        Ok(_) => {}
                        Err(e) if e.is_break() => break,
                        Err(e) => return Err(e),
                    }
                }
                Ok(Value::Unit)
            }
            Expr::Return(value) => {
                // Evaluated first: the value must be ready before the unwind starts.
                self.returning = Some(self.eval(value)?);
                Err(EvalError::return_signal())
            }
            Expr::Crash(message) => {
                let text = match self.eval(message)? {
                    Value::Str(s) => s.to_string(),
                    other => other.to_string(),
                };
                Err(EvalError { message: format!("crash: {}", text) })
            }
            Expr::Expect(condition) => {
                // A failure is reported and execution continues — roc prints to stderr
                // and carries on rather than aborting.
                expect_ran();
                match self.eval(condition)? {
                    Value::Bool(true) => {}
                    Value::Bool(false) => {
                        expect_failed();
                        eprintln!("Expect failed: expect failed")
                    }
                    other => {
                        return Err(EvalError {
                            message: format!("`expect` needs a Bool, got {}", other),
                        })
                    }
                }
                Ok(Value::Unit)
            }
            Expr::Dbg(value) => {
                let shown = self.eval(value)?;
                eprintln!("[dbg] {}", inspect(&shown));
                Ok(Value::Unit)
            }
            Expr::Break => Err(EvalError::break_signal()),
            Expr::Dispatch { receiver, method, args } => {
                let receiver_value = self.eval(receiver)?;

                // A nominal's method block. Tried before the builtins so a type can
                // define its own `show` without colliding with one.
                let candidates = self.env.methods_named(method);
                if !candidates.is_empty() {
                    if candidates.len() > 1 {
                        return Err(EvalError {
                            message: format!(
                                "`{}` is ambiguous: {} all define it. Call it explicitly.",
                                method,
                                candidates
                                    .iter()
                                    .map(|(n, _)| *n)
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        });
                    }
                    let (_, func) = candidates.into_iter().next().expect("checked len");
                    let mut values = Vec::with_capacity(args.len() + 1);
                    values.push(receiver_value);
                    for arg in args {
                        values.push(self.eval(arg)?);
                    }
                    return apply(func, values);
                }

                // `.iter()` turns a range or list into an iterator. Here that is
                // simply the list of its elements.
                // ponytail: EAGER — a real iterator is lazy, so an infinite one would
                // hang and `Str.inspect` shows a list where roc shows `<opaque>`.
                // Nothing in the examples depends on either.
                if *method == "iter" && args.is_empty() {
                    match &receiver_value {
                        Value::List(items) => return Ok(Value::List(items.clone())),
                        Value::Range { start, end, inclusive } => {
                            let last = if *inclusive { *end } else { *end - 1 };
                            return Ok(Value::List((*start..=last).map(Value::Int).collect()));
                        }
                        _ => {}
                    }
                }

                // `Ok`/`Err` carry no module, but they answer the Try methods. Done
                // here rather than in `module_for` because the payload has to be
                // rebuilt around the result.
                if let Value::Tag(tag, payload) = &receiver_value {
                    if matches!(*tag, "Ok" | "Err") {
                        if let Some(result) =
                            self.try_method(tag, payload, method, args)?
                        {
                            return Ok(result);
                        }
                    }
                }

                let module = module_for(&receiver_value).ok_or_else(|| EvalError {
                    message: format!("Cannot dispatch `{}` on {}", method, receiver_value),
                })?;

                // The receiver becomes the FIRST argument, which is why roc's builtins
                // take their subject first: `xs.map(f)` is `List.map(xs, f)`.
                let mut values = Vec::with_capacity(args.len() + 1);
                values.push(receiver_value);
                for arg in args {
                    values.push(self.eval(arg)?);
                }
                self.call_builtin_values(module, method, values)
            }
            Expr::OptionalField { record, field } => {
                match self.eval(record)? {
                    Value::Record(fields) => Ok(match fields.iter().find(|(f, _)| f == field) {
                        Some((_, value)) => Value::Tag("Ok", vec![value.clone()]),
                        // Absent, which is the point of an optional field.
                        None => Value::Tag("Err", vec![Value::Tag("MissingField", vec![])]),
                    }),
                    other => Err(EvalError {
                        message: format!("Cannot read optional field `{}` on {}", field, other),
                    }),
                }
            }
            Expr::FieldAccess { record, field } => {
                let val = self.eval(record)?;
                match val {
                    Value::Record(fields) => fields
                        .iter()
                        .find(|(name, _)| name == field)
                        .map(|(_, v)| v.clone())
                        .ok_or_else(|| EvalError {
                            message: format!("Record has no field '{}'", field),
                        }),
                    other => Err(EvalError {
                        message: format!("Cannot access field '{}' on {}", field, other),
                    }),
                }
            }
            Expr::Record(fields) => {
                let mut vals = Vec::with_capacity(fields.len());
                for (name, value) in fields {
                    vals.push((*name, self.eval(value)?));
                }
                Ok(Value::Record(vals))
            }
            Expr::Tag { name, args } => {
                let vals = args
                    .iter()
                    .map(|a| self.eval(a))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Value::Tag(name, vals))
            }
            Expr::StrInterp(parts) => {
                let mut result = String::new();
                for part in parts {
                    match part {
                        crate::ast::StrPart::Literal(s) => result.push_str(s),
                        crate::ast::StrPart::Expr(e) => {
                            // Evaluate nested expression and convert to string
                            match self.eval(e) {
                                Ok(v) => {
                                    // Convert value to string without quotes
                                    let s = match &v {
                                        Value::Str(s) => s.to_string(),  // No extra quotes for strings
                                        Value::Int(n) => n.to_string(),
                                        // Matches `Value`'s own Display: no trailing
                                        // `.0` on a whole float.
                                        Value::Float(f) => f.to_string(),
                                        Value::Builtin(name, arity) => format!("<{}/{}>", name, arity),
                                        Value::Lambda { params, .. } => format!("<lambda |{}|>", params.join(", ")),
                                        Value::Unit => "{}".to_string(),
                                        Value::Tag(..) => v.to_string(),
                                        Value::Bool(..)
                                        | Value::Record(..)
                                        | Value::List(..)
                                        | Value::Tuple(..)
                                        | Value::Range { .. } => v.to_string(),
                                    };
                                    result.push_str(&s);
                                }
                                Err(e) => return Err(e),
                            }
                        }
                    }
                }
                Ok(str_value(result))
            }
            Expr::Int(n) => Ok(Value::Int(*n)),
            Expr::Float(f) => Ok(Value::Float(*f)),
            Expr::Ident(name) => {
                // Look up variable in environment
                match self.env.lookup(name) {
                    Some(val) => Ok(val),
                    None => Err(EvalError {
                        message: format!("Undefined variable: {}", name),
                    }),
                }
            }
            Expr::Qualified { module, name } => {
                // A nominal method block binds `Type.method` as an ordinary name, so a
                // bare reference like `Counter.start` resolves from the environment.
                if let Some(value) = self.env.lookup(&format!("{}.{}", module, name)) {
                    return Ok(value);
                }

                // `Bool.True` / `Bool.False` are values, not calls.
                if *module == "Bool" {
                    match *name {
                        "True" => return Ok(Value::Bool(true)),
                        "False" => return Ok(Value::Bool(false)),
                        _ => {}
                    }
                }

                // Handle builtin functions from modules
                match (*module, *name) {
                    ("Num", "to_str") => {
                        // Return a builtin function marker
                        // We'll handle it in Call evaluation
                        Err(EvalError {
                            message: "Num.to_str requires arguments".to_string(),
                        })
                    }
                    ("Stdout", "line!") => {
                        Err(EvalError {
                            message: "Stdout.line! requires arguments".to_string(),
                        })
                    }
                    // `xs.map(Str.inspect)` passes a builtin as a value. Arity is 1:
                    // every builtin used this way takes its subject and nothing else.
                    _ => Ok(Value::Builtin(format!("{}.{}", module, name), 1)),
                }
            }
            Expr::BinOp { left, op, right } => {
                let left_val = self.eval(left)?;
                let right_val = self.eval(right)?;
                if let Some(result) = self.dispatch_operator(*op, &left_val, &right_val)? {
                    return Ok(result);
                }
                self.apply_binop(*op, left_val, right_val)
            }
            Expr::Lambda { params, body } => {
                // Create a closure capturing the current environment
                // SAFETY: We transmute to 'static because the parsed AST remains
                // valid for the lifetime of the program. The body is part of the
                // parsed source, which we keep in memory.
                let static_body = unsafe {
                    std::mem::transmute::<std::rc::Rc<Expr<'_>>, std::rc::Rc<Expr<'static>>>(
                        std::rc::Rc::new(body.as_ref().clone())
                    )
                };
                Ok(Value::Lambda {
                    params: std::rc::Rc::new(params.clone()),
                    body: static_body,
                    env: self.env.clone(),
                    // Filled in by the `let` that binds it, if any.
                    self_name: None,
                })
            }
            Expr::Call { func, args } => {
                // Handle builtin functions and calls
                match &**func {
                    Expr::Qualified { module, name } => {
                        // A nominal's method block binds `Type.method` as an ordinary
                        // name, so check the environment before the builtins.
                        let qualified = format!("{}.{}", module, name);
                        if let Some(func @ Value::Lambda { .. }) = self.env.lookup(&qualified) {
                            let mut arg_vals = Vec::with_capacity(args.len());
                            for arg in args {
                                arg_vals.push(self.eval(arg)?);
                            }
                            return apply(func, arg_vals);
                        }
                        self.call_builtin(module, name, args)
                    }
                    Expr::Ident(name) => {
                        // First check if it's a variable bound to a lambda
                        if let Some(val) = self.env.lookup(name) {
                            match val {
                                // A builtin bound to a name — `my_concat = Str.concat`
                                // — is callable the same way a lambda is.
                                callable @ (Value::Lambda { .. } | Value::Builtin(..)) => {
                                    let mut arg_vals = Vec::with_capacity(args.len());
                                    for arg in args {
                                        arg_vals.push(self.eval(arg)?);
                                    }
                                    return apply(callable, arg_vals);
                                }
                                _ => {}  // Not callable, fall through
                            }
                        }

                        // Effects from the default host. `!` is part of the name.
                        if crate::platform::host::lookup(name).is_some() {
                            return self.call_host_effect(name, args);
                        }

                        // Try to call as builtin without module
                        match *name {
                            "to_str" => {
                                // Num.to_str(value)
                                if args.len() != 1 {
                                    return Err(EvalError {
                                        message: format!("to_str expects 1 argument, got {}", args.len()),
                                    });
                                }
                                let val = self.eval(&args[0])?;
                                Ok(str_value(val.to_string()))
                            }
                            _ => {
                                Err(EvalError {
                                    message: format!("Function '{}' not defined", name),
                                })
                            }
                        }
                    }
                    _ => {
                        // Evaluate the function expression (e.g., for chained calls like f()(x))
                        let func_val = self.eval(func)?;
                        match func_val {
                            lambda @ Value::Lambda { .. } => {
                                let mut arg_vals = Vec::with_capacity(args.len());
                                for arg in args {
                                    arg_vals.push(self.eval(arg)?);
                                }
                                apply(lambda, arg_vals)
                            }
                            _ => {
                                Err(EvalError {
                                    message: "Attempted to call a non-function value".to_string(),
                                })
                            }
                        }
                    }
                }
            }
            Expr::Let { name, value, body, .. } => {
                // Evaluate value
                let val = self.eval(value)?;

                // A lambda bound by a `let` may call itself. Record the name on the
                // closure so `apply` can put it back in scope for the body.
                let val = match val {
                    Value::Lambda { params, body, env, .. } => Value::Lambda {
                        params,
                        body,
                        env,
                        self_name: Some(name),
                    },
                    other => other,
                };

                // Bind in environment
                self.env.bind(name, val);

                // Evaluate body
                self.eval(body)
            }
        }
    }

    /// A nominal's own `to_inspect`, if one applies to this value.
    ///
    /// roc erases the nominal at runtime, so there is no tag saying which type a value
    /// belongs to and no way to pick the right method by type. Each candidate is tried
    /// in turn and the first that returns a Str without erroring wins.
    // ponytail: a heuristic. Two nominals whose `to_inspect` both accept the same
    // runtime shape cannot be told apart; resolving it needs the checker to record the
    // method at each call site, which it already knows.
    fn custom_inspect(&mut self, value: &Value) -> Option<Value> {
        for (_, func) in self.env.methods_named("to_inspect") {
            if let Ok(shown @ Value::Str(_)) = apply(func, vec![value.clone()]) {
                return Some(shown);
            }
        }
        None
    }

    /// The methods every `Try` answers: `Ok(v)` and `Err(e)` are just tags, so these
    /// cannot come from the builtin table, which is keyed by module.
    ///
    /// `None` means the method is not one of these, and the caller carries on.
    fn try_method(
        &mut self,
        tag: &str,
        payload: &[Value],
        method: &str,
        args: &[Expr],
    ) -> Result<Option<Value>, EvalError> {
        let is_ok = tag == "Ok";
        let inner = payload.first().cloned().unwrap_or(Value::Unit);

        let mut evaluated = Vec::with_capacity(args.len());
        for arg in args {
            evaluated.push(self.eval(arg)?);
        }
        let first = || evaluated.first().cloned().unwrap_or(Value::Unit);

        Ok(Some(match method {
            "is_ok" => Value::Bool(is_ok),
            "is_err" => Value::Bool(!is_ok),
            // Rebuild the same tag around the mapped payload; the other side passes
            // through untouched.
            "map_ok" if is_ok => Value::Tag("Ok", vec![apply(first(), vec![inner])?]),
            "map_err" if !is_ok => Value::Tag("Err", vec![apply(first(), vec![inner])?]),
            "map_ok" | "map_err" => Value::Tag(
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
            "on_err" if !is_ok => apply(first(), vec![inner])?,
            "on_err" => Value::Tag("Ok", payload.to_vec()),
            _ => return Ok(None),
        }))
    }

    /// `List.*` builtins.
    ///
    /// Argument order follows roc: the list comes first, and `fold` takes
    /// `(list, initial, fn)` with the accumulator as the callback's first parameter.
    /// `List.len` returns a U64 in roc — worth remembering, since mixing it with I64
    /// arms in a match is a type error.
    fn call_list_builtin(&mut self, name: &str, args: Vec<Value>) -> Result<Value, EvalError> {
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
            "len" => {
                expect(1, args.len())?;
                let items = as_list(args[0].clone())?;
                Ok(Value::Int(items.len() as i64))
            }
            "is_empty" => {
                expect(1, args.len())?;
                let items = as_list(args[0].clone())?;
                Ok(Value::Bool(items.is_empty()))
            }
            "map" => {
                expect(2, args.len())?;
                let items = as_list(args[0].clone())?;
                let func = args[1].clone();
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(apply(func.clone(), vec![item])?);
                }
                Ok(Value::List(out))
            }
            "fold" => {
                expect(3, args.len())?;
                let items = as_list(args[0].clone())?;
                let mut acc = args[1].clone();
                let func = args[2].clone();
                for item in items {
                    acc = apply(func.clone(), vec![acc, item])?;
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
                    Some(v) => Value::Tag("Ok", vec![v.clone()]),
                    None => Value::Tag("Err", vec![Value::Tag("ListWasEmpty", vec![])]),
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
                    Some(v) => Value::Tag("Ok", vec![v.clone()]),
                    None => Value::Tag("Err", vec![Value::Tag("OutOfBounds", vec![])]),
                })
            }
            "keep_if" | "drop_if" => {
                expect(2, args.len())?;
                let items = as_list(args[0].clone())?;
                let func = args[1].clone();
                let keep = name == "keep_if";
                let mut out = Vec::new();
                for item in items {
                    if matches!(apply(func.clone(), vec![item.clone()])?, Value::Bool(b) if b == keep)
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
                let items = as_list(args[0].clone())?;
                let mut acc = args[1].clone();
                let func = args[2].clone();
                for item in items {
                    match apply(func.clone(), vec![acc.clone(), item])? {
                        Value::Tag("Ok", payload) => {
                            acc = payload.into_iter().next().unwrap_or(Value::Unit)
                        }
                        stop @ Value::Tag("Err", _) => return Ok(stop),
                        other => acc = other,
                    }
                }
                Ok(Value::Tag("Ok", vec![acc]))
            }
            // An iterator is already a list here, so collecting one is a no-op.
            "from_iter" => {
                expect(1, args.len())?;
                Ok(Value::List(as_list(args[0].clone())?))
            }
            "contains" => {
                expect(2, args.len())?;
                let items = as_list(args[0].clone())?;
                Ok(Value::Bool(items.iter().any(|v| values_equal(v, &args[1]))))
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
    fn call_host_effect(&mut self, name: &str, args: &[Expr]) -> Result<Value, EvalError> {
        let (params, _) = crate::platform::host::lookup(name).ok_or_else(|| EvalError {
            message: format!("Unknown host effect '{}'", name),
        })?;

        if args.len() != params.len() {
            return Err(EvalError {
                message: format!(
                    "{} expects {} argument(s), got {}",
                    name,
                    params.len(),
                    args.len()
                ),
            });
        }

        match name {
            "echo!" => {
                let val = self.eval(&args[0])?;
                match val {
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

    /// Call builtin function from a module
    /// Call a builtin, evaluating its arguments first.
    fn call_builtin(&mut self, module: &str, name: &str, args: &[Expr]) -> Result<Value, EvalError> {
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.eval(arg)?);
        }
        self.call_builtin_values(module, name, values)
    }

    fn call_builtin_values(
        &mut self,
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
                Ok(n) => Value::Tag("Ok", vec![Value::Int(n)]),
                Err(_) => Value::Tag("Err", vec![Value::Tag("BadNumStr", vec![])]),
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
                        return Ok(if wraps { Value::Tag("Ok", vec![value]) } else { value });
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
            return self.call_list_builtin(name, args);
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
                Ok(Value::Tag("Ok", vec![Value::Unit]))
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
                Ok(Value::Tag("Ok", vec![Value::Unit]))
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
                if let Some(custom) = self.custom_inspect(&val) {
                    return Ok(custom);
                }
                Ok(str_value(inspect(&val)))
            }
            ("Stdout", "line!") => {
                if args.len() != 1 {
                    return Err(EvalError {
                        message: format!("Stdout.line! expects 1 argument, got {}", args.len()),
                    });
                }
                let val = args[0].clone();
                // Print to stdout, handling string formatting
                let output = match &val {
                    Value::Str(s) => s.to_string(),
                    _ => val.to_string(),
                };
                println!("{}", output);
                // Return empty value
                Ok(str_value(String::new()))
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
                    Some(i) => Value::Tag(
                        "Ok",
                        vec![Value::Record(vec![
                            ("before", str_value(text[..i].to_string())),
                            (
                                "after",
                                str_value(text[i + needle.len()..].to_string()),
                            ),
                        ])],
                    ),
                    None => Value::Tag("Err", vec![Value::Tag("NotFound", vec![])]),
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
                    Ok(text) => Value::Tag("Ok", vec![str_value(text)]),
                    Err(_) => Value::Tag("Err", vec![Value::Tag("BadUtf8", vec![])]),
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
    fn dispatch_operator(
        &mut self,
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

        let mut candidates = self.env.methods_named(method);
        if candidates.len() != 1 {
            return Ok(None);
        }
        let (_, func) = candidates.pop().expect("checked len");
        let result = apply(func, vec![left.clone(), right.clone()])?;
        Ok(Some(match (op, result) {
            (BinOp::Ne, Value::Bool(b)) => Value::Bool(!b),
            (_, other) => other,
        }))
    }

    fn apply_binop(&self, op: BinOp, left: Value, right: Value) -> Result<Value, EvalError> {
        match (op, left, right) {
            // Arithmetic on integers
            (BinOp::Add, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
            (BinOp::Sub, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
            (BinOp::Mul, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
            (BinOp::Div, Value::Int(a), Value::Int(b)) => {
                if b == 0 {
                    Err(EvalError { message: "Division by zero".to_string() })
                } else {
                    Ok(Value::Int(a / b))
                }
            }
            // `//` truncating division and `%` remainder — integers only in Roc.
            (BinOp::IntDiv, Value::Int(a), Value::Int(b)) => {
                if b == 0 {
                    Err(EvalError { message: "Division by zero".to_string() })
                } else {
                    // Roc's `//` truncates toward zero, which is Rust's `/` for i64.
                    Ok(Value::Int(a / b))
                }
            }
            (BinOp::Rem, Value::Int(a), Value::Int(b)) => {
                if b == 0 {
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
                if b == 0.0 {
                    Err(EvalError { message: "Division by zero".to_string() })
                } else {
                    Ok(Value::Float(a / b))
                }
            }
            // Mixed int/float arithmetic
            (BinOp::Add, Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 + b)),
            (BinOp::Add, Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + b as f64)),
            (BinOp::Sub, Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 - b)),
            (BinOp::Sub, Value::Float(a), Value::Int(b)) => Ok(Value::Float(a - b as f64)),
            (BinOp::Mul, Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 * b)),
            (BinOp::Mul, Value::Float(a), Value::Int(b)) => Ok(Value::Float(a * b as f64)),
            (BinOp::Div, Value::Int(a), Value::Float(b)) => {
                if b == 0.0 {
                    Err(EvalError { message: "Division by zero".to_string() })
                } else {
                    Ok(Value::Float(a as f64 / b))
                }
            }
            (BinOp::Div, Value::Float(a), Value::Int(b)) => {
                if b == 0 {
                    Err(EvalError { message: "Division by zero".to_string() })
                } else {
                    Ok(Value::Float(a / b as f64))
                }
            }
            // String concatenation
            (BinOp::Add, Value::Str(a), Value::Str(b)) => {
                let concatenated = format!("{}{}", a, b);
                Ok(str_value(concatenated))
            }
            // Comparison operators — these yield Bool in Roc, not 0/1.
            (BinOp::Eq, a, b) => Ok(Value::Bool(values_equal(&a, &b))),
            (BinOp::Ne, a, b) => Ok(Value::Bool(!values_equal(&a, &b))),
            (BinOp::Lt, a, b) => compare(&a, &b, |o| o == std::cmp::Ordering::Less),
            (BinOp::Le, a, b) => compare(&a, &b, |o| o != std::cmp::Ordering::Greater),
            (BinOp::Gt, a, b) => compare(&a, &b, |o| o == std::cmp::Ordering::Greater),
            (BinOp::Ge, a, b) => compare(&a, &b, |o| o != std::cmp::Ordering::Less),
            // `and` / `or` are Bool-only in Roc. Both operands are already
            // evaluated, so these do not short-circuit — see the ponytail note.
            (BinOp::And, Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(a && b)),
            (BinOp::Or, Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(a || b)),
            // Numeric operands to `and`/`or`, kept for the `1 && 0` spelling used
            // before Bool existed. ponytail: drop once nothing relies on it.
            (BinOp::And, a, b) => Ok(Value::Bool(is_truthy(&a) && is_truthy(&b))),
            (BinOp::Or, a, b) => Ok(Value::Bool(is_truthy(&a) || is_truthy(&b))),
            (_op, _left, _right) => {
                Err(EvalError {
                    message: "Invalid operands for operator".to_string(),
                })
            }
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

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
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
fn pattern_matches<'a>(
    pattern: &Pattern,
    value: &Value,
    bindings: &mut Vec<(&'static str, Value)>,
) -> bool {
    match pattern {
        Pattern::Wildcard => true,
        Pattern::Binding(name) => {
            bindings.push((name, value.clone()));
            true
        }
        Pattern::Int(expected) => matches!(value, Value::Int(n) if n == expected),
        Pattern::Float(expected) => {
            matches!(value, Value::Float(n) if (n - expected).abs() < 1e-10)
        }
        Pattern::Str(expected) => matches!(value, Value::Str(s) if &**s == *expected),
        // A nominal's backing record. Values carry no nominal wrapper — roc erases it,
        // as `Str.inspect` showing the bare record confirms — so this matches an
        // ordinary record.
        Pattern::Record { fields, rest } => match value {
            Value::Record(actual) => {
                let matched = fields.iter().all(|(name, pattern)| {
                    actual
                        .iter()
                        .find(|(field, _)| field == name)
                        .is_some_and(|(_, v)| pattern_matches(pattern, v, bindings))
                });
                if !matched {
                    return false;
                }
                // `..rest` collects the fields the pattern did not name.
                if let Some(rest_name) = rest {
                    let remaining: Vec<(&'static str, Value)> = actual
                        .iter()
                        .filter(|(field, _)| !fields.iter().any(|(named, _)| named == field))
                        .cloned()
                        .collect();
                    bindings.push((rest_name, Value::Record(remaining)));
                }
                true
            }
            _ => false,
        },
        Pattern::Tuple(items) => match value {
            Value::Tuple(values) if values.len() == items.len() => items
                .iter()
                .zip(values.iter())
                .all(|(p, v)| pattern_matches(p, v, bindings)),
            _ => false,
        },
        Pattern::List { before, rest, after } => {
            let items = match value {
                Value::List(items) => items,
                _ => return false,
            };

            match rest {
                // Exact length: one pattern per element.
                None => {
                    if items.len() != before.len() {
                        return false;
                    }
                    before
                        .iter()
                        .zip(items.iter())
                        .all(|(p, v)| pattern_matches(p, v, bindings))
                }
                // `..` absorbs whatever the fixed patterns do not cover, so the list
                // only has to be long enough.
                Some(binding) => {
                    if items.len() < before.len() + after.len() {
                        return false;
                    }
                    let split = items.len() - after.len();

                    for (p, v) in before.iter().zip(items[..before.len()].iter()) {
                        if !pattern_matches(p, v, bindings) {
                            return false;
                        }
                    }
                    for (p, v) in after.iter().zip(items[split..].iter()) {
                        if !pattern_matches(p, v, bindings) {
                            return false;
                        }
                    }
                    if let Some(name) = binding {
                        bindings.push((name, Value::List(items[before.len()..split].to_vec())));
                    }
                    true
                }
            }
        }
        Pattern::Tag { name, args } => match value {
            Value::Tag(tag_name, payload) => {
                if tag_name != name || payload.len() != args.len() {
                    return false;
                }
                args.iter()
                    .zip(payload.iter())
                    .all(|(arg, v)| pattern_matches(arg, v, bindings))
            }
            _ => false,
        },
    }
}

/// Call a lambda value with already-evaluated arguments.
///
/// Runs in the lambda's captured environment, not the caller's. Factored out because
/// the two call sites in `eval` had identical copies, and the List builtins need to
/// invoke a callback too.
pub fn apply(func: Value, args: Vec<Value>) -> Result<Value, EvalError> {
    match func {
        Value::Lambda { params, body, env, self_name } => {
            if args.len() != params.len() {
                return Err(EvalError {
                    message: format!(
                        "Lambda expects {} argument(s), got {}",
                        params.len(),
                        args.len()
                    ),
                });
            }
            // The environment AS CAPTURED, before the call frame is pushed. The
            // self-binding has to use this one: binding a closure whose environment
            // already contains the call frame would nest a copy of the environment at
            // every recursive step, and 500 levels of that exhausts the stack.
            let captured = self_name.map(|_| env.clone());

            let mut lambda_eval = Evaluator { env, returning: None };
            lambda_eval.env.push_scope();
            // Tie the recursive knot: the body may call the function by its own name,
            // which its captured environment predates.
            if let (Some(name), Some(captured)) = (self_name, captured) {
                lambda_eval.env.bind(
                    name,
                    Value::Lambda {
                        params: params.clone(),
                        body: body.clone(),
                        env: captured,
                        self_name: Some(name),
                    },
                );
            }
            for (param, arg) in params.iter().zip(args.into_iter()) {
                lambda_eval.env.bind(param, arg);
            }
            match lambda_eval.eval(&body) {
                // `return` unwinds to here — the function boundary — and its value is
                // the call's result.
                Err(e) if e.is_return() => Ok(lambda_eval
                    .returning
                    .take()
                    .unwrap_or(Value::Unit)),
                other => other,
            }
        }
        // A builtin passed as a value — `xs.map(Str.inspect)`.
        Value::Builtin(qualified, _) => {
            let (module, name) = qualified.split_once('.').ok_or_else(|| EvalError {
                message: format!("`{}` is not a qualified builtin", qualified),
            })?;
            Evaluator::new().call_builtin_values(module, name, args)
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
fn module_for(value: &Value) -> Option<&'static str> {
    Some(match value {
        Value::Int(_) => "I64",
        Value::Float(_) => "F64",
        Value::Str(_) => "Str",
        Value::Bool(_) => "Bool",
        Value::List(_) => "List",
        Value::Record(_)
        | Value::Tag(..)
        | Value::Tuple(_)
        | Value::Range { .. }
        | Value::Unit => return None,
        Value::Lambda { .. } | Value::Builtin(..) => return None,
    })
}
