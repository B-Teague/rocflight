//! Type checking and inference
//!
//! Bidirectional type checking: synthesis (infer) + checking (verify)
//! Phase 3: Lambdas, calls, let bindings

use crate::ast::{Expr, BinOp, Pattern};
use crate::error::TypeError;
use super::{Type, Substitution};

/// Type checker with Hindley-Milner inference
pub struct TypeChecker {
    subst: Substitution,
    next_var: u32,
    /// Methods a `where` clause promised, which may be dispatched on a type variable
    /// inference has not resolved. Set from the parser before checking.
    where_methods: Vec<String>,
    /// How deep synthesis is inside an UNANNOTATED lambda body.
    ///
    /// Such a body is checked before any call site is seen, so its parameters are still
    /// bare type variables and a dispatch on one cannot be resolved yet. roc infers
    /// these from the call sites; this checker synthesises once, so inside a lambda it
    /// falls back to the builtin table rather than refusing.
    // ponytail: a real fix propagates argument types back into the body at each call
    // site. Deferring costs the dispatch check inside lambdas only.
    lambda_depth: u32,
    /// Types of names in scope, innermost scope last, each with the type-variable ids
    /// that are universally quantified for it (empty for a monomorphic binding).
    ///
    /// A polymorphic binding is INSTANTIATED at every use: `identity : a -> a` used at
    /// Str and then at I64 needs a fresh variable each time, or the first use pins `a`
    /// and the second fails.
    ///
    /// Before this existed every identifier synthesised to a fresh variable, which
    /// meant the checker could not know a value's declared type — and so could not
    /// reject a tag outside a closed union, check a `match` for exhaustiveness, or
    /// give `x.field` a real type. Annotations reaching the AST are what fill it.
    env: Vec<Vec<(String, Type, Vec<u32>)>>,
}

impl TypeChecker {
    /// Create new type checker
    /// Record the methods a `where` clause promised, before checking begins.
    pub fn allow_dispatch(&mut self, methods: Vec<String>) {
        self.where_methods = methods;
    }

    pub fn new() -> Self {
        TypeChecker {
            where_methods: Vec::new(),
            lambda_depth: 0,
            subst: Substitution::new(),
            next_var: 0,
            env: vec![Vec::new()],
        }
    }

    /// Enter a new scope (a lambda body, or a match arm).
    fn push_scope(&mut self) {
        self.env.push(Vec::new());
    }

    /// Leave the innermost scope. The outermost is never popped.
    fn pop_scope(&mut self) {
        if self.env.len() > 1 {
            self.env.pop();
        }
    }

    /// Record a monomorphic name's type in the innermost scope.
    fn bind(&mut self, name: &str, ty: Type) {
        if let Some(scope) = self.env.last_mut() {
            scope.push((name.to_string(), ty, Vec::new()));
        }
    }

    /// Record a name whose type variables are universally quantified.
    fn bind_poly(&mut self, name: &str, ty: Type, generics: Vec<u32>) {
        if let Some(scope) = self.env.last_mut() {
            scope.push((name.to_string(), ty, generics));
        }
    }

    /// Type variables still free in the environment.
    ///
    /// These may NOT be generalised at a let-binding: they belong to something still
    /// being inferred — an enclosing lambda's parameter, say — so quantifying one here
    /// would let two uses disagree about a type that is in fact fixed.
    fn env_type_vars(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for scope in &self.env {
            for (_, ty, generics) in scope {
                let mut vars = Vec::new();
                Self::type_vars_in(&self.subst.apply(ty), &mut vars);
                out.extend(vars.into_iter().filter(|v| !generics.contains(v)));
            }
        }
        out
    }

    /// Look a name up, innermost scope first so shadowing works.
    ///
    /// A polymorphic binding comes back INSTANTIATED, with fresh variables standing in
    /// for its quantified ones, so separate uses cannot constrain each other.
    fn lookup(&mut self, name: &str) -> Option<Type> {
        let found = self.env.iter().rev().find_map(|scope| {
            scope
                .iter()
                .rev()
                .find(|(n, _, _)| n == name)
                .map(|(_, t, g)| (t.clone(), g.clone()))
        })?;
        let (ty, generics) = found;
        Some(if generics.is_empty() { ty } else { self.instantiate(&ty, &generics) })
    }

    /// Replace each quantified variable with a fresh one, consistently.
    ///
    /// `a -> a` instantiates to `$7 -> $7`, not `$7 -> $8`: the two positions are the
    /// same variable, they are just a *new* same variable at this use site.
    fn instantiate(&mut self, ty: &Type, generics: &[u32]) -> Type {
        let mapping: Vec<(u32, Type)> =
            generics.iter().map(|id| (*id, self.fresh_var())).collect();
        Self::substitute_vars(ty, &mapping)
    }

    /// Structural substitution of type variables by id.
    fn substitute_vars(ty: &Type, mapping: &[(u32, Type)]) -> Type {
        match ty {
            Type::TypeVar(id) => mapping
                .iter()
                .find(|(from, _)| from == id)
                .map(|(_, to)| to.clone())
                .unwrap_or_else(|| ty.clone()),
            Type::List(inner) => {
                Type::List(Box::new(Self::substitute_vars(inner, mapping)))
            }
            Type::Function(param, result) => Type::Function(
                Box::new(Self::substitute_vars(param, mapping)),
                Box::new(Self::substitute_vars(result, mapping)),
            ),
            Type::Tuple(items) => Type::Tuple(
                items.iter().map(|t| Self::substitute_vars(t, mapping)).collect(),
            ),
            Type::Record { fields: fields, .. } => Type::closed_record(
                fields
                    .iter()
                    .map(|(n, t)| (n.clone(), Self::substitute_vars(t, mapping)))
                    .collect(),
            ),
            Type::TagUnion { tags, open } => Type::TagUnion {
                tags: tags
                    .iter()
                    .map(|(n, payload)| {
                        (
                            n.clone(),
                            payload.iter().map(|t| Self::substitute_vars(t, mapping)).collect(),
                        )
                    })
                    .collect(),
                open: *open,
            },
            Type::Nominal { name, backing } => Type::Nominal {
                name: name.clone(),
                backing: Box::new(Self::substitute_vars(backing, mapping)),
            },
            other => other.clone(),
        }
    }

    /// Collect every type-variable id appearing in a type.
    ///
    /// Applied to an ANNOTATION, these are the universally quantified variables: a
    /// lowercase name in a signature means "any type", so each use may pick its own.
    /// It also keeps the parser's ids out of unification entirely — the two allocate
    /// from the same number space, so an annotation's `$1` would otherwise collide
    /// with the checker's first fresh variable.
    fn type_vars_in(ty: &Type, out: &mut Vec<u32>) {
        match ty {
            Type::TypeVar(id) => {
                if !out.contains(id) {
                    out.push(*id);
                }
            }
            Type::List(inner) | Type::Nominal { backing: inner, .. } => {
                Self::type_vars_in(inner, out)
            }
            Type::Function(param, result) => {
                Self::type_vars_in(param, out);
                Self::type_vars_in(result, out);
            }
            Type::Tuple(items) => items.iter().for_each(|t| Self::type_vars_in(t, out)),
            Type::Record { fields: fields, .. } => {
                fields.iter().for_each(|(_, t)| Self::type_vars_in(t, out))
            }
            Type::TagUnion { tags, .. } => tags
                .iter()
                .for_each(|(_, payload)| payload.iter().for_each(|t| Self::type_vars_in(t, out))),
            _ => {}
        }
    }

    /// Check `expr` against an expected type, rather than inferring it.
    ///
    /// The two cases that need checking rather than synthesis:
    ///
    /// * a **lambda** — the expected type supplies its parameter types, which is how a
    ///   `match` inside the body learns the parameter's declared tag union and can be
    ///   checked for exhaustiveness;
    /// * a **tag** — the expected union decides whether the tag is a member at all.
    ///
    /// Everything else falls back to synthesising and unifying, which is equivalent.
    pub fn check(&mut self, expr: &Expr, expected: &Type) -> Result<(), TypeError> {
        let resolved = self.subst.apply(expected);

        match expr {
            // Numeric literals are POLYMORPHIC: `255` is a U8 in `x : U8`, an I64 in
            // `x : I64`. Synthesising them as I64 and unifying would reject every
            // annotation that is not I64.
            Expr::Int(_) if resolved.is_numeric() => Ok(()),
            Expr::Float(_) if resolved.is_fractional() => Ok(()),

            // Arithmetic inherits the expected type, so `x : U8 = 1 + 2` works for the
            // same reason a bare literal does.
            Expr::BinOp { left, op, right }
                if resolved.is_numeric()
                    && matches!(
                        op,
                        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div
                            | BinOp::IntDiv | BinOp::Rem
                    ) =>
            {
                self.check(left, &resolved)?;
                self.check(right, &resolved)
            }

            Expr::Lambda { params, body } => {
                match Self::peel_params(&resolved, params.len()) {
                    Some((param_types, result_type)) => {
                        self.push_scope();
                        for (param, ty) in params.iter().zip(param_types.iter()) {
                            self.bind(param, ty.clone());
                        }
                        // Deferred here as well as in the inferred case: a body may
                        // dispatch on the result of a builtin this interpreter does
                        // not model, which comes back as a bare variable through no
                        // fault of the program.
                        self.lambda_depth += 1;
                        let outcome = self.check(body, &result_type);
                        self.lambda_depth -= 1;
                        self.pop_scope();
                        outcome
                    }
                    // The annotation is not a function of this arity. Fall back so the
                    // mismatch is reported by unification rather than silently ignored.
                    None => {
                        let actual = self.synth(expr)?;
                        self.unify(&actual, &resolved)
                    }
                }
            }
            _ => {
                let actual = self.synth(expr)?;
                self.unify(&actual, &resolved)
            }
        }
    }

    /// Peel a curried function type into its parameter types and final result.
    ///
    /// `A -> (B -> C)` gives `([A, B], C)`. Used to push an annotation's parameter
    /// types into a lambda's scope, which is how a `match` on a parameter learns the
    /// parameter's declared union — and therefore whether the arms are exhaustive.
    fn peel_params(ty: &Type, count: usize) -> Option<(Vec<Type>, Type)> {
        let mut params = Vec::with_capacity(count);
        let mut current = ty.clone();
        for _ in 0..count {
            match current {
                Type::Function(param, result) => {
                    params.push(*param);
                    current = *result;
                }
                _ => return None,
            }
        }
        Some((params, current))
    }

    /// Generate a fresh type variable
    pub fn fresh_var(&mut self) -> Type {
        let var = Type::TypeVar(self.next_var);
        self.next_var += 1;
        var
    }

    /// Synthesize (infer) type of expression
    pub fn synth(&mut self, expr: &Expr) -> Result<Type, TypeError> {
        match expr {
            Expr::Str(_) => Ok(Type::Str),
            Expr::Unit => Ok(Type::Unit),
            Expr::Bool(_) => Ok(Type::Bool),
            Expr::Range { start, end, .. } => {
                for bound in [start, end] {
                    let bound_type = self.synth(bound)?;
                    self.unify(&bound_type, &Type::I64)?;
                }
                Ok(Type::Range)
            }
            Expr::Tuple(items) => {
                // Positional, so each element keeps its own type — no joining.
                let mut types = Vec::with_capacity(items.len());
                for item in items {
                    types.push(self.synth(item)?);
                }
                Ok(Type::Tuple(types))
            }
            Expr::TupleIndex { tuple, index } => {
                let tuple_type = self.synth(tuple)?;
                // Likewise for a nominal over a tuple.
                let resolved = match self.subst.apply(&tuple_type) {
                    Type::Nominal { backing, .. } => *backing,
                    other => other,
                };
                match resolved {
                    Type::Tuple(items) => items.get(*index).cloned().ok_or_else(|| TypeError {
                        message: format!(
                            "Tuple has {} element(s), so .{} is out of range",
                            items.len(),
                            index
                        ),
                        expected: format!("a tuple with at least {} element(s)", index + 1),
                        actual: Type::Tuple(items.clone()).to_string(),
                        line: 0,
                        col: 0,
                    }),
                    // Receiver type not resolved (no type environment for
                    // identifiers); eval will catch a genuine mistake.
                    _ => Ok(self.fresh_var()),
                }
            }
            Expr::RecordUpdate { base, fields } => {
                let base_type = self.synth(base)?;
                let mut updated = Vec::new();
                for (name, value) in fields {
                    updated.push((name.to_string(), self.synth(value)?));
                }

                match self.subst.apply(&base_type) {
                    Type::Record { fields: known, .. } => {
                        // The result keeps the base's shape, with named fields replaced.
                        // A field the base does not have is an error: an update cannot
                        // add one.
                        let mut result = known.clone();
                        for (name, ty) in updated {
                            match result.iter_mut().find(|(field, _)| *field == name) {
                                Some(slot) => slot.1 = ty,
                                None => {
                                    return Err(TypeError {
                                        message: format!(
                                            "Record has no field `{}` to update",
                                            name
                                        ),
                                        expected: Type::closed_record(known).to_string(),
                                        actual: name,
                                        line: 0,
                                        col: 0,
                                    })
                                }
                            }
                        }
                        Ok(Type::closed_record(result))
                    }
                    // Base type not resolved yet; eval catches a real mistake.
                    _ => Ok(self.fresh_var()),
                }
            }
            Expr::List(items) => {
                // Every element must share one type. An empty list's element type is
                // unconstrained, so it gets a fresh variable.
                let mut element = self.fresh_var();
                for item in items {
                    let item_type = self.synth(item)?;
                    element = self.join(&element, &item_type)?;
                }
                Ok(Type::List(Box::new(element)))
            }
            Expr::Match { scrutinee, arms } => {
                let scrutinee_type = self.synth(scrutinee)?;

                // Every pattern must be able to match the scrutinee, and every arm
                // body must agree on a type. Tag patterns join into a union the same
                // way tag expressions do, so `Red`/`Green` arms accept a [Green, Red]
                // scrutinee.
                let mut result: Option<Type> = None;
                for arm in arms {
                    for pattern in &arm.patterns {
                        let pattern_type = self.pattern_type(pattern)?;
                        self.unify(&scrutinee_type, &pattern_type)?;
                    }

                    // The arm's bindings are in scope for its guard and body, with the
                    // types their positions imply.
                    self.push_scope();
                    let resolved_scrutinee = self.subst.apply(&scrutinee_type);
                    for pattern in &arm.patterns {
                        self.bind_pattern(pattern, &resolved_scrutinee);
                    }

                    let outcome = (|checker: &mut Self| -> Result<Type, TypeError> {
                        // A guard is a Bool, checked after its pattern matches.
                        if let Some(guard) = &arm.guard {
                            let guard_type = checker.synth(guard)?;
                            checker.unify(&guard_type, &Type::Bool)?;
                        }
                        checker.synth(&arm.body)
                    })(self);
                    self.pop_scope();

                    let body_type = outcome?;
                    result = Some(match result {
                        None => body_type,
                        Some(previous) => self.join(&previous, &body_type)?,
                    });
                }

                // Exhaustiveness. Only possible now that a closed union can reach here
                // through an annotation: a scrutinee whose type is an inferred (open)
                // union carries no list of "all the cases", so nothing can be checked.
                let resolved = self.subst.apply(&scrutinee_type);
                // A nominal over a tag union is still exhaustively checkable — the
                // union is its backing type. roc checks these too.
                let resolved = match &resolved {
                    Type::Nominal { backing, .. } => (**backing).clone(),
                    other => other.clone(),
                };
                if let Type::TagUnion { tags, open: false } = &resolved {
                    let covers_everything = arms.iter().any(|arm| {
                        arm.guard.is_none()
                            && arm.patterns.iter().any(|p| {
                                matches!(p, Pattern::Wildcard | Pattern::Binding(_))
                            })
                    });

                    if !covers_everything {
                        // A guarded arm may not run, so it does not count as coverage.
                        let uncovered: Vec<&str> = tags
                            .iter()
                            .filter(|(tag, _)| {
                                !arms.iter().any(|arm| {
                                    arm.guard.is_none()
                                        && arm.patterns.iter().any(|p| {
                                            matches!(p, Pattern::Tag { name, .. } if name == tag)
                                        })
                                })
                            })
                            .map(|(tag, _)| tag.as_str())
                            .collect();

                        if !uncovered.is_empty() {
                            return Err(TypeError {
                                message: format!(
                                    "This match does not cover all cases; missing: {}",
                                    uncovered.join(", ")
                                ),
                                expected: resolved.to_string(),
                                actual: format!("{} arm(s)", arms.len()),
                                line: 0,
                                col: 0,
                            });
                        }
                    }
                }

                result.ok_or_else(|| TypeError {
                    message: "A match needs at least one arm".to_string(),
                    expected: "one or more arms".to_string(),
                    actual: "none".to_string(),
                    line: 0,
                    col: 0,
                })
            }
            Expr::If { condition, then_branch, otherwise } => {
                // The condition must be a Bool — roc is explicit that a number will
                // not do: "This if condition must evaluate to a Bool".
                let condition_type = self.synth(condition)?;
                self.unify(&condition_type, &Type::Bool)?;

                // Both branches must agree, since the `if` has a single type.
                let then_type = self.synth(then_branch)?;
                let else_type = self.synth(otherwise)?;
                // `join`, not `unify`: branches yielding different tags produce the
                // union of them, not just the first branch's type.
                self.join(&then_type, &else_type)
            }
            Expr::VarDecl { name, value, body } => {
                let bound = self.synth(value)?;
                self.bind(name, bound);
                self.synth(body)
            }
            Expr::Assign { name, value, body } => {
                // A reassignment must agree with what the `var` already holds.
                let assigned = self.synth(value)?;
                if let Some(existing) = self.lookup(name) {
                    self.unify(&existing, &assigned)?;
                }
                self.synth(body)
            }
            Expr::For { name, iterable, body } => {
                let iterable_type = self.synth(iterable)?;
                let element = match self.subst.apply(&iterable_type) {
                    // A range yields whole numbers without being a list.
                    Type::Range => Type::I64,
                    // Still unknown — a parameter whose call site has not been seen.
                    // Forcing it to a `List` here would reject a range passed in later.
                    Type::TypeVar(_) => self.fresh_var(),
                    _ => {
                        let element = self.fresh_var();
                        self.unify(&iterable_type, &Type::List(Box::new(element.clone())))?;
                        element
                    }
                };

                self.push_scope();
                self.bind(name, element);
                let outcome = self.synth(body);
                self.pop_scope();
                outcome?;
                Ok(Type::Unit)
            }
            Expr::While { condition, body } => {
                let condition_type = self.synth(condition)?;
                self.unify(&condition_type, &Type::Bool)?;
                self.synth(body)?;
                Ok(Type::Unit)
            }
            // `break` never produces a value; it leaves the loop.
            Expr::Break => Ok(self.fresh_var()),
            // `return` leaves the function, so it fits wherever it appears.
            Expr::Return(value) => {
                self.synth(value)?;
                Ok(self.fresh_var())
            }
            // `crash` never returns, so it too fits anywhere.
            Expr::Crash(message) => {
                self.synth(message)?;
                Ok(self.fresh_var())
            }
            Expr::Expect(condition) => {
                let condition_type = self.synth(condition)?;
                self.unify(&condition_type, &Type::Bool)?;
                Ok(Type::Unit)
            }
            Expr::Dbg(value) => {
                self.synth(value)?;
                Ok(Type::Unit)
            }
            Expr::Dispatch { receiver, method, args } => {
                let receiver_type = self.synth(receiver)?;
                let mut arg_types = Vec::with_capacity(args.len());
                for arg in args {
                    arg_types.push(self.synth(arg)?);
                }

                // Dispatch is STATIC: the receiver's type picks the module. If the type
                // is still an unresolved variable there is nothing to dispatch on, and
                // roc says so too — "trying to dispatch a method named to_str on an
                // unresolved type variable".
                let resolved = self.subst.apply(&receiver_type);
                // A `where` clause promised this method exists on whatever the caller
                // supplies, so an unresolved receiver is fine here — roc has already
                // checked the constraint is satisfied at each call site.
                let promised = self.where_methods.iter().any(|m| m == method);
                if matches!(resolved, Type::TypeVar(_)) && !promised && self.lambda_depth == 0 {
                    return Err(TypeError {
                        message: format!(
                            "Cannot dispatch `{}` on an unresolved type; annotate the receiver",
                            method
                        ),
                        expected: "a receiver with a known type".to_string(),
                        actual: resolved.to_string(),
                        line: 0,
                        col: 0,
                    });
                }

                // A result type is needed for CHAINING: `xs.len().to_str()` dispatches
                // on what `len` returned, so an unconstrained variable there makes the
                // second dispatch impossible.
                // A nominal receiver dispatches to its own method block, and here the
                // TYPE says which: `c : Counter` means `Counter.method`. This is real
                // static dispatch — the evaluator has to fall back to a name search,
                // because values carry no nominal tag.
                if let Type::Nominal { name, .. } = &resolved {
                    if let Some(signature) = self.lookup(&format!("{}.{}", name, method)) {
                        // Applying it consumes the receiver plus the written arguments.
                        let applied = Self::peel_params(&signature, arg_types.len() + 1);
                        if let Some((_, result)) = applied {
                            return Ok(result);
                        }
                    }
                }

                // `negate` keeps its receiver's type; everything else comes from the
                // shared builtin table.
                if *method == "negate" {
                    return Ok(resolved);
                }
                Ok(self.builtin_result(method, Some(&resolved), &arg_types))
            }
            Expr::OptionalField { record, field } => {
                let record_type = self.synth(record)?;
                let resolved = match self.subst.apply(&record_type) {
                    Type::Nominal { backing, .. } => *backing,
                    other => other,
                };
                // `Ok(value)` or `Err(MissingField)`. The union is CLOSED: those are
                // the only two outcomes.
                let value = match &resolved {
                    Type::Record { fields: known, .. } => known
                        .iter()
                        .find(|(name, _)| name == field)
                        // The Ok payload is the field's own type, not the Optional
                        // wrapper around it.
                        .map(|(_, t)| match t {
                            Type::Optional(inner) => (**inner).clone(),
                            other => other.clone(),
                        })
                        .unwrap_or_else(|| self.fresh_var()),
                    _ => self.fresh_var(),
                };
                Ok(Type::TagUnion {
                    tags: vec![
                        ("Err".to_string(), vec![Type::TagUnion {
                            tags: vec![("MissingField".to_string(), Vec::new())],
                            open: false,
                        }]),
                        ("Ok".to_string(), vec![value]),
                    ],
                    open: false,
                })
            }
            Expr::FieldAccess { record, field } => {
                let record_type = self.synth(record)?;
                // A nominal over a record supports field access on its backing — roc
                // allows `p.x` for `p : Point`, and a method body relies on it.
                let resolved = match self.subst.apply(&record_type) {
                    Type::Nominal { backing, .. } => *backing,
                    other => other,
                };
                match resolved {
                    Type::Record { fields: fields, .. } => fields
                        .iter()
                        .find(|(name, _)| name == field)
                        .map(|(_, ty)| ty.clone())
                        .ok_or_else(|| TypeError {
                            message: format!("Record has no field '{}'", field),
                            expected: format!("a record with field '{}'", field),
                            actual: Type::closed_record(fields.clone()).to_string(),
                            line: 0,
                            col: 0,
                        }),
                    // Receiver type not resolved yet (no type environment for
                    // identifiers). Accept it; eval will catch a real mistake.
                    _ => Ok(self.fresh_var()),
                }
            }
            Expr::Record(fields) => {
                // Sorted by field name, so `{ x: 1, y: 2 }` and `{ y: 2, x: 1 }`
                // produce the same type and therefore unify.
                let mut typed = Vec::with_capacity(fields.len());
                for (name, value) in fields {
                    typed.push((name.to_string(), self.synth(value)?));
                }
                typed.sort_by(|a, b| a.0.cmp(&b.0));
                Ok(Type::closed_record(typed))
            }
            // A tag literal is a one-tag union. Unifying it with another union
            // merges them, so `if b Red else Green` comes out as [Green, Red].
            Expr::Tag { name, args } => {
                let mut payload = Vec::with_capacity(args.len());
                for arg in args {
                    payload.push(self.synth(arg)?);
                }
                Ok(Type::TagUnion {
                    tags: vec![(name.to_string(), payload)],
                    open: true,
                })
            }
            // Interpolation always produces a Str, but its embedded expressions still
            // have to be checked — skipping them meant a whole class of error inside
            // `${...}` went unreported, including the one that hid broken chained
            // dispatch in this project's own test files.
            Expr::StrInterp(parts) => {
                for part in parts {
                    if let crate::ast::StrPart::Expr(inner) = part {
                        self.synth(inner)?;
                    }
                }
                Ok(Type::Str)
            }
            Expr::Int(_) => Ok(Type::I64),       // Integer literals default to I64
            Expr::Float(_) => Ok(Type::F64),     // Float literals default to F64
            Expr::Ident(name) => {
                // A name in scope has a known type now.
                if let Some(ty) = self.lookup(name) {
                    return Ok(self.subst.apply(&ty));
                }
                // Effects the default host provides (`echo!`) have known signatures.
                if let Some((params, ret)) = crate::platform::host::lookup(name) {
                    let ret_type = if ret == "{}" { Type::Unit } else { Type::Str };
                    let mut ty = ret_type;
                    for param in params.iter().rev() {
                        let param_type = if *param == "Str" { Type::Str } else { self.fresh_var() };
                        ty = Type::Function(Box::new(param_type), Box::new(ty));
                    }
                    return Ok(ty);
                }
                Ok(self.fresh_var())
            }
            // `Bool.True` / `Bool.False` are VALUES. Treating every qualified name as a
            // function meant the checker and the evaluator disagreed about these, which
            // stayed invisible until annotations were actually checked against.
            Expr::Qualified { module: "Bool", name: "True" | "False" } => Ok(Type::Bool),
            // A nominal's method block binds `Type.method` as an ordinary name, so a
            // qualified reference resolves from the environment before falling back to
            // "some function" — `Counter.start` is a Counter, not a function.
            Expr::Qualified { module, name }
                if self.lookup(&format!("{}.{}", module, name)).is_some() =>
            {
                Ok(self
                    .lookup(&format!("{}.{}", module, name))
                    .expect("checked by the guard"))
            }
            Expr::Qualified { module: _, name: _ } => {
                // Qualified names are typically functions from builtins
                // Create a function type that can accept arguments
                // Input type: fresh var, Output type: fresh var
                let input_type = self.fresh_var();
                let output_type = self.fresh_var();
                Ok(Type::Function(
                    Box::new(input_type),
                    Box::new(output_type),
                ))
            }
            Expr::BinOp { left, op, right } => {
                let left_type = self.synth(left)?;
                let right_type = self.synth(right)?;

                match op {
                    // Arithmetic returns the type of its operands, not always I64.
                    // Returning I64 unconditionally made `quot : F64` reject
                    // `quot = 7.0 / 2.0`, which only showed up once annotations were
                    // actually checked.
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                        self.unify(&left_type, &right_type)?;
                        Ok(self.subst.apply(&left_type))
                    }
                    // `//` and `%` are integer-only in Roc.
                    BinOp::IntDiv | BinOp::Rem => {
                        self.unify(&left_type, &right_type)?;
                        Ok(Type::I64)
                    }
                    // Comparison yields Bool, not an integer.
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        Ok(Type::Bool)
                    }
                    // `and` / `or` take and return Bool.
                    BinOp::And | BinOp::Or => Ok(Type::Bool),
                }
            }
            Expr::Lambda { params, body } => {
                // For each parameter, allocate a fresh type variable
                let mut param_types = vec![];
                for _ in params {
                    param_types.push(self.fresh_var());
                }

                // The parameters have to be IN SCOPE while the body is synthesised, or
                // every mention of one is a brand new variable and nothing the body
                // learns about a parameter reaches the function's type.
                self.push_scope();
                for (param, ty) in params.iter().zip(param_types.iter()) {
                    self.bind(param, ty.clone());
                }
                self.lambda_depth += 1;
                let body_type = self.synth(body);
                self.lambda_depth -= 1;
                self.pop_scope();
                let body_type = body_type?;

                // Build function type: (T1 -> T2 -> ... -> Tn)
                let mut result_type = body_type;
                for param_type in param_types.into_iter().rev() {
                    result_type = Type::Function(Box::new(param_type), Box::new(result_type));
                }

                Ok(result_type)
            }
            // A qualified builtin call — `List.len(xs)` — resolves its result the same
            // way `xs.len()` does, so the two spellings behave alike when chained.
            Expr::Call { func, args } if matches!(**func, Expr::Qualified { .. }) => {
                let mut arg_types = Vec::with_capacity(args.len());
                for arg in args {
                    arg_types.push(self.synth(arg)?);
                }
                if let Expr::Qualified { module, name } = &**func {
                    // A nominal's method is an ordinary binding, so its own signature
                    // decides the result — not the builtin table, which would hand back
                    // an unconstrained variable and break any chaining off it.
                    if let Some(signature) = self.lookup(&format!("{}.{}", module, name)) {
                        if let Some((_, result)) =
                            Self::peel_params(&signature, arg_types.len())
                        {
                            return Ok(result);
                        }
                    }
                    // Drop the receiver: `List.fold(xs, 0, f)` has its accumulator
                    // second, matching `xs.fold(0, f)`.
                    let rest = if arg_types.is_empty() { &arg_types[..] } else { &arg_types[1..] };
                    let rest = rest.to_vec();
                    // `List.concat(xs, ys)` passes its subject first, exactly as
                    // `xs.concat(ys)` does, so the receiver is the first argument.
                    let receiver = arg_types.first().map(|t| self.subst.apply(t));
                    return Ok(self.builtin_result(name, receiver.as_ref(), &rest));
                }
                unreachable!("guarded by the match arm")
            }
            Expr::Call { func, args } => {
                // Unify the callee with `arg -> fresh` for each argument, rather than
                // requiring it to already BE a `Type::Function`.
                //
                // Identifiers still synth to a fresh type variable (there is no type
                // environment yet), so insisting on a concrete function type here
                // rejected every call to a user-defined function — `add5(37)` failed
                // with "Cannot call non-function type: $5". Unification handles both
                // cases: a variable gets bound to the function type, and a concrete
                // function type is checked as before.
                //
                // Ceiling: a non-function callee is caught only when its type is
                // already concrete — `42(1)` and `"hi"(1)` fail in `unify`, but
                // `x = 42` then `x(1)` does not, because `x` synths to a fresh var.
                // That needs a type environment for identifiers; until then this
                // errs toward accepting, which is the right trade — the previous
                // behaviour rejected every call to a user-defined function.
                // ponytail: fix by giving `synth` an environment, alongside phase 18.
                let mut current_type = self.synth(func)?;

                for arg in args {
                    let arg_type = self.synth(arg)?;
                    let return_type = self.fresh_var();
                    let expected = Type::Function(
                        Box::new(arg_type),
                        Box::new(return_type.clone()),
                    );
                    self.unify(&current_type, &expected)?;
                    current_type = return_type;
                }

                Ok(self.subst.apply(&current_type))
            }
            Expr::Let { name, annotation, value, body } => {
                match annotation {
                    // Declared: the annotation is authoritative, and the value is
                    // CHECKED against it rather than merely inferred. That is what
                    // rejects `c : [Red, Green]` with `c = Blue`.
                    Some(declared) => {
                        let mut generics = Vec::new();
                        Self::type_vars_in(declared, &mut generics);

                        // The definition is checked against its OWN instance, so
                        // checking the body cannot pin the quantified variables for
                        // everyone else.
                        let instance = if generics.is_empty() {
                            declared.clone()
                        } else {
                            self.instantiate(declared, &generics)
                        };
                        // Bound BEFORE the value is checked, so a recursive call
                        // inside the body resolves through the annotation. Without
                        // this, `hanoi` calling itself produced an unresolved type and
                        // anything dispatched on the result failed.
                        self.bind_poly(name, declared.clone(), generics.clone());
                        self.check(value, &instance)?;
                        self.bind_poly(name, declared.clone(), generics);
                    }
                    // Inferred: generalise, exactly as an annotated binding is. Without
                    // this, `describe = |c| ...` is monomorphic — the first call site
                    // pins its parameter and the second is a type error, even though
                    // roc accepts both. A variable is quantified only if it is free in
                    // the inferred type and NOT free in the enclosing environment,
                    // which is what keeps an enclosing lambda's parameter fixed.
                    None => {
                        let inferred = self.synth(value)?;
                        let inferred = self.subst.apply(&inferred);

                        let mut generics = Vec::new();
                        Self::type_vars_in(&inferred, &mut generics);
                        let outer = self.env_type_vars();
                        generics.retain(|v| !outer.contains(v));
                        generics.dedup();

                        self.bind_poly(name, inferred, generics);
                    }
                }
                self.synth(body)
            }
        }
    }

    /// Result type of a builtin, for both `xs.len()` and `List.len(xs)`.
    ///
    /// Needed for CHAINING: the result is the next receiver, so an unconstrained
    /// variable makes a following dispatch impossible. `args` are the types written
    /// AFTER the receiver, which is what `fold` needs.
    ///
    /// `len` is I64 rather than roc's U64 because integer widths are not
    /// distinguished here — see the note on numeric unification.
    ///
    /// ponytail: a partial table, covering the builtins that exist. Anything else is
    /// unconstrained, which costs only a chain off it.
    fn builtin_result(
        &mut self,
        method: &str,
        receiver: Option<&Type>,
        args: &[Type],
    ) -> Type {
        match method {
            "to_str" | "inspect" => Type::Str,
            // `concat` keeps the RECEIVER's type: `Str.concat` gives a Str, and
            // `List.concat` a List. Naming only the method cannot tell them apart.
            "concat" => match receiver {
                Some(Type::List(inner)) => Type::List(inner.clone()),
                Some(Type::Str) | None => Type::Str,
                Some(other) => other.clone(),
            },
            "is_empty" | "not" => Type::Bool,
            "len" => Type::I64,
            // `fold` returns its accumulator — the first argument after the receiver,
            // which a name-only table cannot express.
            "fold" => args.first().cloned().unwrap_or_else(|| self.fresh_var()),
            // `map` keeps the receiver's shape with a new element type.
            "map" => Type::List(Box::new(self.fresh_var())),
            // These give back what they were handed.
            "reverse" | "sort_with" | "drop_first" | "drop_last" | "append" | "prepend" => {
                receiver.cloned().unwrap_or_else(|| self.fresh_var())
            }
            // `haystack.split_first(needle)` gives `Ok({ before, after })` on a hit and
            // `Err(NotFound)` on a miss. The field types matter: destructuring the Ok
            // is how a caller gets two more `Str`s to go on splitting.
            "split_first" | "split_last" => Type::TagUnion {
                tags: vec![
                    (
                        "Ok".to_string(),
                        vec![Type::closed_record(vec![
                            ("after".to_string(), Type::Str),
                            ("before".to_string(), Type::Str),
                        ])],
                    ),
                    ("Err".to_string(), vec![self.fresh_var()]),
                ],
                open: true,
            },
            "join_with" | "with_ascii_uppercased" | "with_ascii_lowercased" | "trim" => Type::Str,
            _ => self.fresh_var(),
        }
    }

    /// Bind the names a pattern introduces, with the types their positions imply.
    ///
    /// Walks the pattern against the scrutinee's type, so `Foo(n, s)` against
    /// `[Foo(I64, Str)]` gives `n: I64` and `s: Str`. Where the type is not concrete
    /// enough to say, a fresh variable is used rather than nothing — the name is still
    /// in scope, just unconstrained.
    fn bind_pattern(&mut self, pattern: &Pattern, scrutinee: &Type) {
        match pattern {
            Pattern::Wildcard => {}
            Pattern::Binding(name) => self.bind(name, scrutinee.clone()),
            Pattern::Int(_) | Pattern::Float(_) | Pattern::Str(_) => {}
            Pattern::Tag { name, args } => {
                let payload = match scrutinee {
                    Type::TagUnion { tags, .. } => tags
                        .iter()
                        .find(|(tag, _)| tag == name)
                        .map(|(_, payload)| payload.clone()),
                    _ => None,
                };
                for (index, arg) in args.iter().enumerate() {
                    let ty = payload
                        .as_ref()
                        .and_then(|p| p.get(index).cloned())
                        .unwrap_or_else(|| self.fresh_var());
                    self.bind_pattern(arg, &ty);
                }
            }
            Pattern::Record { fields, rest } => {
                // A nominal's backing record. Only the named fields are constrained,
                // so the record's other fields are free.
                let record = match scrutinee {
                    Type::Nominal { backing, .. } => (**backing).clone(),
                    other => other.clone(),
                };
                for (name, pattern) in fields {
                    let ty = match &record {
                        Type::Record { fields: known, .. } => known
                            .iter()
                            .find(|(field, _)| field == name)
                            .map(|(_, t)| t.clone()),
                        _ => None,
                    }
                    .unwrap_or_else(|| self.fresh_var());
                    self.bind_pattern(pattern, &ty);
                }

                // `..rest` is a record of the fields NOT named, so its type has fewer
                // fields than the scrutinee — that is what makes it remove one.
                if let Some(rest_name) = rest {
                    let remaining = match &record {
                        Type::Record { fields: known, .. } => Type::closed_record(
                            known
                                .iter()
                                .filter(|(field, _)| {
                                    !fields.iter().any(|(named, _)| named == field)
                                })
                                .cloned()
                                .collect(),
                        ),
                        _ => self.fresh_var(),
                    };
                    self.bind(rest_name, remaining);
                }
            }
            Pattern::Tuple(items) => {
                for (index, item) in items.iter().enumerate() {
                    let ty = match scrutinee {
                        Type::Tuple(types) => types.get(index).cloned(),
                        _ => None,
                    }
                    .unwrap_or_else(|| self.fresh_var());
                    self.bind_pattern(item, &ty);
                }
            }
            Pattern::List { before, rest, after } => {
                let element = match scrutinee {
                    Type::List(inner) => (**inner).clone(),
                    _ => self.fresh_var(),
                };
                for item in before.iter().chain(after.iter()) {
                    self.bind_pattern(item, &element);
                }
                // `.. as name` binds the skipped middle, which is a list of the same
                // element type.
                if let Some(Some(name)) = rest {
                    self.bind(name, Type::List(Box::new(element)));
                }
            }
        }
    }

    /// The type a pattern can match.
    ///
    /// A wildcard or binding matches anything, so it gets a fresh variable. A literal
    /// matches its own type. A tag pattern is a one-tag union, exactly like a tag
    /// expression, so `unify` merges arms into the scrutinee's union.
    ///
    /// Bindings a pattern introduces are recorded by the `Match` arm, which pushes a
    /// scope and binds each one to the type its position implies.
    fn pattern_type(&mut self, pattern: &Pattern) -> Result<Type, TypeError> {
        Ok(match pattern {
            Pattern::Wildcard | Pattern::Binding(_) => self.fresh_var(),
            Pattern::Int(_) => Type::I64,
            Pattern::Float(_) => Type::F64,
            Pattern::Str(_) => Type::Str,
            Pattern::Tag { name, args } => {
                let mut payload = Vec::with_capacity(args.len());
                for arg in args {
                    payload.push(self.pattern_type(arg)?);
                }
                Type::TagUnion { tags: vec![(name.to_string(), payload)], open: true }
            }
            Pattern::Tuple(items) => {
                let mut types = Vec::with_capacity(items.len());
                for item in items {
                    types.push(self.pattern_type(item)?);
                }
                Type::Tuple(types)
            }
            Pattern::Record { fields, rest } => {
                let mut types = Vec::with_capacity(fields.len());
                for (name, pattern) in fields {
                    types.push((name.to_string(), self.pattern_type(pattern)?));
                }
                types.sort_by(|a, b| a.0.cmp(&b.0));
                // With `..rest` the pattern matches a record with MORE fields than it
                // names, so a closed record type would be wrong. A fresh variable lets
                // it accept any wider record; `bind_pattern` still gives `rest` the
                // precise remainder when the scrutinee's type is known.
                if rest.is_some() {
                    self.fresh_var()
                } else {
                    Type::closed_record(types)
                }
            }
            Pattern::List { before, rest, after } => {
                // Every element pattern constrains the same element type. A `.. as
                // name` binding is a List of that element type, but bindings do not
                // reach a type environment yet, so nothing records it.
                let mut element = self.fresh_var();
                for pattern in before.iter().chain(after.iter()) {
                    let pattern_type = self.pattern_type(pattern)?;
                    element = self.join(&element, &pattern_type)?;
                }
                let _ = rest;
                Type::List(Box::new(element))
            }
        })
    }

    /// Unify two types and return the type that covers both.
    ///
    /// For most types that is just the unified type, but two tag unions join to the
    /// UNION of their tags: the branches of `if b Red else Green` have types [Red]
    /// and [Green], and the `if` as a whole is [Green, Red]. `unify` alone cannot
    /// express that, because it reports success or failure rather than a type.
    pub fn join(&mut self, t1: &Type, t2: &Type) -> Result<Type, TypeError> {
        self.unify(t1, t2)?;
        let (a, b) = (self.subst.apply(t1), self.subst.apply(t2));

        if let (
            Type::TagUnion { tags: a_tags, open: a_open },
            Type::TagUnion { tags: b_tags, open: b_open },
        ) = (&a, &b)
        {
            let mut merged = a_tags.clone();
            for (name, payload) in b_tags {
                if !merged.iter().any(|(n, _)| n == name) {
                    merged.push((name.clone(), payload.clone()));
                }
            }
            merged.sort_by(|x, y| x.0.cmp(&y.0));
            // The join is closed only if both sides were: a closed union joined with
            // an open one can still grow.
            return Ok(Type::TagUnion { tags: merged, open: *a_open || *b_open });
        }
        Ok(a)
    }

    pub fn unify(&mut self, t1: &Type, t2: &Type) -> Result<(), TypeError> {
        let t1 = self.subst.apply(t1);
        let t2 = self.subst.apply(t2);

        if t1 == t2 {
            return Ok(());
        }

        match (&t1, &t2) {
            // TypeVar cases
            (Type::TypeVar(v1), Type::TypeVar(v2)) if v1 == v2 => Ok(()),
            (Type::TypeVar(v), t) | (t, Type::TypeVar(v)) => {
                if self.occurs_check(*v, t) {
                    Err(TypeError {
                        message: format!("Infinite type: ${} = {}", v, t),
                        expected: t1.to_string(),
                        actual: t2.to_string(),
                        line: 0,
                        col: 0,
                    })
                } else {
                    self.subst.insert(*v, t.clone());
                    Ok(())
                }
            }
            // Two nominals interchange only if they are the SAME nominal.
            (Type::Nominal { name: a, backing: a_backing },
             Type::Nominal { name: b, backing: b_backing }) => {
                if a != b {
                    return Err(TypeError {
                        message: format!("{} and {} are different nominal types", a, b),
                        expected: t1.to_string(),
                        actual: t2.to_string(),
                        line: 0,
                        col: 0,
                    });
                }
                self.unify(a_backing, b_backing)
            }
            // Nominal against anything else: compare the backing type. roc accepts a
            // plain record where a `:=` nominal is expected, so this is deliberate
            // rather than lax — verified against the compiler.
            (Type::Nominal { backing, .. }, other) | (other, Type::Nominal { backing, .. }) => {
                let backing = (**backing).clone();
                let other = other.clone();
                self.unify(&backing, &other)
            }
            // Numeric widths are not distinguished: the evaluator has ONE integer
            // representation (i64) and one fractional one (f64), so enforcing widths
            // in the checker would be theatre — it would reject programs the
            // interpreter runs correctly. `roc check` is the authority on widths, and
            // every golden pair goes through it.
            //
            // ponytail: proper numeric literals need a constrained type variable
            // ("some integer"), which is real Hindley-Milner work. Revisit if the
            // evaluator ever grows per-width arithmetic.
            (a, b) if a.is_integer() && b.is_integer() => Ok(()),
            (a, b) if a.is_fractional() && b.is_fractional() => Ok(()),
            // A number LITERAL is polymorphic in roc: `46` is whatever the context
            // wants, so `[46, 69] |> safe_variance` with `safe_variance : List(Dec)`
            // is fine. Widths already unify freely for the reason above; the same
            // reasoning applies across the integer/fractional line, since the
            // evaluator converts between its two representations on demand.
            (a, b) if a.is_numeric() && b.is_numeric() => Ok(()),
            // List unification
            (Type::List(a), Type::List(b)) => self.unify(a, b),
            // Tag-union unification is permissive: the shared tags must agree on
            // payload arity and types, but neither side has to list the other's
            // extra tags. `if b Red else Green` unifies [Red] with [Green] and the
            // branch types are then merged by `join`.
            //
            // Closed-union membership (rejecting `c : [Red, Green]` with `c = Blue`)
            // is NOT checked here and cannot be: the parser skips type annotations,
            // so the declared union never reaches the AST. `roc check` enforces it,
            // which is why every golden pair is checked by the real compiler.
            // ponytail: needs annotations in the AST — see the type-variables phase.
            (
                Type::TagUnion { tags: a, open: a_open },
                Type::TagUnion { tags: b, open: b_open },
            ) => {
                // Shared tags must agree on payload arity and types.
                for (name, a_payload) in a.iter() {
                    if let Some((_, b_payload)) = b.iter().find(|(n, _)| n == name) {
                        if a_payload.len() != b_payload.len() {
                            return Err(TypeError {
                                message: format!(
                                    "Tag {} used with {} payload(s) and {} payload(s)",
                                    name,
                                    a_payload.len(),
                                    b_payload.len()
                                ),
                                expected: t1.to_string(),
                                actual: t2.to_string(),
                                line: 0,
                                col: 0,
                            });
                        }
                        for (x, y) in a_payload.iter().zip(b_payload.iter()) {
                            self.unify(x, y)?;
                        }
                    }
                }

                // A CLOSED union may not gain tags. This is what makes
                // `c : [Red, Green]` reject `c = Blue` — the check that was impossible
                // while annotations never reached the AST.
                let missing = |closed: &[(String, Vec<Type>)],
                               other: &[(String, Vec<Type>)]|
                 -> Option<String> {
                    other
                        .iter()
                        .find(|(n, _)| !closed.iter().any(|(c, _)| c == n))
                        .map(|(n, _)| n.clone())
                };

                if !a_open {
                    if let Some(extra) = missing(a, b) {
                        return Err(TypeError {
                            message: format!(
                                "Tag {} is not a member of the tag union {}",
                                extra, t1
                            ),
                            expected: t1.to_string(),
                            actual: t2.to_string(),
                            line: 0,
                            col: 0,
                        });
                    }
                }
                if !b_open {
                    if let Some(extra) = missing(b, a) {
                        return Err(TypeError {
                            message: format!(
                                "Tag {} is not a member of the tag union {}",
                                extra, t2
                            ),
                            expected: t2.to_string(),
                            actual: t1.to_string(),
                            line: 0,
                            col: 0,
                        });
                    }
                }
                Ok(())
            }
            // Tuples unify positionally and only at the same arity.
            (Type::Tuple(a), Type::Tuple(b)) if a.len() == b.len() => {
                for (x, y) in a.iter().zip(b.iter()) {
                    self.unify(x, y)?;
                }
                Ok(())
            }
            // An optional field unifies with the value's type whether it is present
            // or not — that is what `?:` means.
            (Type::Optional(inner), other) | (other, Type::Optional(inner)) => {
                let inner = (**inner).clone();
                let other = other.clone();
                self.unify(&inner, &other)
            }
            // Record unification, walked by NAME rather than position, because an
            // optional field may be absent on one side and so the two lists can differ
            // in length. A field missing from one side is only acceptable when the
            // other declares it optional.
            (
                Type::Record { fields: a, open: a_open },
                Type::Record { fields: b, open: b_open },
            ) => {
                let missing_ok = |ty: &Type| matches!(ty, Type::Optional(_));

                for (name, ta) in a.iter() {
                    match b.iter().find(|(n, _)| n == name) {
                        Some((_, tb)) => self.unify(ta, tb)?,
                        // An OPEN record promises only the fields it lists, so the
                        // other side may lack the rest.
                        None if *b_open || missing_ok(ta) => {}
                        None => {
                            return Err(TypeError {
                                message: format!("Record is missing field `{}`", name),
                                expected: t1.to_string(),
                                actual: t2.to_string(),
                                line: 0,
                                col: 0,
                            })
                        }
                    }
                }
                for (name, tb) in b.iter() {
                    if a.iter().any(|(n, _)| n == name) {
                        continue;
                    }
                    if !*a_open && !missing_ok(tb) {
                        return Err(TypeError {
                            message: format!("Record has an unexpected field `{}`", name),
                            expected: t1.to_string(),
                            actual: t2.to_string(),
                            line: 0,
                            col: 0,
                        });
                    }
                }
                Ok(())
            }
            // Function unification
            (Type::Function(a1, b1), Type::Function(a2, b2)) => {
                self.unify(a1, a2)?;
                self.unify(b1, b2)
            }
            // Type mismatch
            _ => Err(TypeError {
                message: format!("Cannot unify {} with {}", t1, t2),
                expected: t1.to_string(),
                actual: t2.to_string(),
                line: 0,
                col: 0,
            }),
        }
    }

    /// Occurs check: prevent infinite types
    fn occurs_check(&self, var: u32, ty: &Type) -> bool {
        let ty = self.subst.apply(ty);
        match ty {
            Type::TypeVar(v) => v == var,
            Type::List(inner) => self.occurs_check(var, &inner),
            Type::Function(a, b) => self.occurs_check(var, &a) || self.occurs_check(var, &b),
            _ => false,
        }
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

