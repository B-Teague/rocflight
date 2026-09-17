//! Type checking and inference
//!
//! Bidirectional type checking: synthesis (infer) + checking (verify)
//! Phase 3: Lambdas, calls, let bindings

use crate::ast::{Expr, BinOp, Pattern};
use crate::error::TypeError;
use super::{Type, Substitution};

/// Type checker with Hindley-Milner inference
pub struct TypeChecker {
    /// Every type the program declares, by name — `Position : { x : I64, y : I64 }`,
    /// `IOErr := […]` — so a name used BEFORE its declaration (or declared in another
    /// module) can be resolved when it is met. The parser leaves such a use as
    /// `Nominal { name, backing: TypeVar(u32::MAX) }`, a placeholder, and unifying
    /// through that shared sentinel tied unrelated types to each other: every numeral
    /// in a record whose type was declared two lines further down defaulted to `Dec`.
    declared_types: std::collections::HashMap<String, Type>,
    /// `Module.member` types a platform's modules declare — `Host.stdin_bytes!` has an
    /// annotation and no body, exactly like a `Builtin.roc` intrinsic, and this is
    /// where its type comes from. Consulted after the program's own bindings.
    declared_signatures: std::collections::HashMap<String, Type>,
    subst: Substitution,
    /// Each `Dispatch` node and its RECEIVER's type, before the substitution is
    /// finished. Resolved afterwards into the module whose method block owns the call.
    dispatches: Vec<(crate::ast::NodeId, Type)>,
    /// Numeric LITERAL nodes and the type they were checked against.
    ///
    /// `Dec` is a 128-bit fixed-point value, not a float, so `0.0` in a `List(Dec)` has
    /// to reach the evaluator as one. Only the checker knows which; resolved through
    /// the finished substitution, like `integer_binops`.
    literals: Vec<(crate::ast::NodeId, Type)>,
    /// Type variables that came from a numeric LITERAL.
    ///
    /// A numeral is polymorphic — `15` is an I64 in one place and a Dec in another — so
    /// it synthesises to a variable rather than to I64. These are the variables that
    /// stand for one, which is what lets a dispatch on an unresolved numeral still
    /// resolve, and what `fractional_literals` defaults at the end.
    numeral_vars: std::collections::HashSet<u32>,
    /// Record literals written as `Name.{ … }`, from `Parser::nominal_literals`.
    /// The nominal is erased in the AST, so this is how the fields keep their types.
    nominal_literals: std::collections::HashMap<crate::ast::NodeId, Type>,
    /// Literal nodes with an explicit type suffix, from `Parser::suffixed_literals`.
    suffixed: std::collections::HashMap<crate::ast::NodeId, Type>,
    /// The result type of each lambda currently being checked, innermost last.
    ///
    /// A `return` leaves the FUNCTION, so what it hands back is the function's result
    /// — `|arg| { if …  { return 99 } else { … }; Str.count_utf8_bytes(first) }` is
    /// what tells `99` it is a byte count.
    returns: Vec<Type>,
    /// `Json.parse` call sites and the type each is expected to produce.
    ///
    /// Reading JSON back into a type needs to KNOW that type — `[1,2,3]` is a
    /// `List(ItemKind)` only because the annotation says so, and `ItemKind`'s own
    /// `parser_for` is what turns each number into a tag. Nothing at run time can
    /// recover that, so the checker has to say.
    parse_targets: std::collections::HashMap<crate::ast::NodeId, Type>,
    /// The nominal whose method block is being checked, if any.
    ///
    /// Inside `Graph :: … .{ … }` a sibling method is in scope UNQUALIFIED — roc lets
    /// `from_list` call `from_dict(…)` — but rocflight binds it as `Graph.from_dict`.
    /// Without this the bare name is unknown, its result is a fresh variable, and every
    /// use of the method it belongs to loses its type.
    enclosing_type: Vec<String>,
    /// Each `BinOp` node and the type its operands unified to, before the
    /// substitution is finished.
    ///
    /// The compiler asks for this so it can emit an integer-only opcode where both
    /// operands are known to be integers. Recorded rather than answered on the spot
    /// because inference is not finished yet: the type here may still be a variable
    /// that later unifies with `I64`.
    binops: Vec<(crate::ast::NodeId, Type)>,
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
    /// Bind every annotated top-level name BEFORE any body is checked.
    ///
    /// Declarations come in any order: `update_game` may call `prepend`, declared
    /// forty lines further down with its type written out. Checking bodies in file
    /// order gave such a call an unknown result, and every numeral that met it —
    /// `full.rest.drop_last(1)` — defaulted to a fraction. `check_let` binds the same
    /// annotation again when it reaches the definition, which changes nothing.
    pub fn predeclare(&mut self, ast: &Expr) {
        let mut cursor = ast;
        while let Expr::Let { name, annotation, value, body, .. } = cursor {
            // A `_ = <chain>` is the parser's shape for declarations followed by
            // top-level `expect`s; its chain is the program's top level too.
            if *name == "_" && matches!(**value, Expr::Let { .. }) {
                self.predeclare(value);
            }
            if let Some(declared) = annotation {
                let mut generics = Vec::new();
                Self::type_vars_in(declared, &mut generics);
                generics.sort_unstable();
                generics.dedup();
                self.bind_poly(name, declared.clone(), generics);
            }
            cursor = body;
        }
    }

    /// Make the program's type declarations known by name; see `declared_types`.
    pub fn declare_types<'a>(&mut self, declared: impl IntoIterator<Item = (&'a str, Type)>) {
        for (name, ty) in declared {
            self.declared_types.entry(name.to_string()).or_insert(ty);
        }
    }

    /// The substitution applied, and every placeholder for a declared name replaced
    /// by its declaration, however deep. A placeholder is a `Nominal` whose backing is
    /// a bare variable: the parser's sentinel, or — once an annotation has been
    /// instantiated — a fresh variable standing in for it, which is why the name and
    /// not the sentinel is what identifies one. A name already being expanded stays
    /// as it is, which keeps a recursive declaration finite.
    fn apply(&self, ty: &Type) -> Type {
        let applied = self.subst.apply(ty);
        if self.declared_types.is_empty() {
            return applied;
        }
        self.expand(&applied, &mut Vec::new())
    }

    /// What a placeholder named `name` stands for, if a declaration says.
    ///
    /// A `:=` nominal whose own backing is a bare variable — `Wrapper(a) := a` — is
    /// not a placeholder, and is left alone.
    fn placeholder_target(&self, name: &str) -> Option<&Type> {
        // `OsStr.OsStr` is `OsStr`'s own declaration seen from outside.
        let bare = name.rsplit('.').next().unwrap_or(name);
        let declared = self.declared_types.get(name).or_else(|| self.declared_types.get(bare))?;
        match declared {
            Type::Nominal { backing, .. } if matches!(**backing, Type::TypeVar(_)) => None,
            _ => Some(declared),
        }
    }

    fn expand(&self, ty: &Type, seen: &mut Vec<String>) -> Type {
        match ty {
            Type::Nominal { name, backing } => {
                if matches!(**backing, Type::TypeVar(_)) && !seen.iter().any(|s| s == name) {
                    if let Some(target) = self.placeholder_target(name) {
                        seen.push(name.clone());
                        let expanded = self.expand(target, seen);
                        seen.pop();
                        return expanded;
                    }
                }
                seen.push(name.clone());
                let backing = self.expand(backing, seen);
                seen.pop();
                Type::Nominal { name: name.clone(), backing: Box::new(backing) }
            }
            Type::List(inner) => Type::List(Box::new(self.expand(inner, seen))),
            Type::Optional(inner) => Type::Optional(Box::new(self.expand(inner, seen))),
            Type::Function(a, b) => Type::Function(Box::new(self.expand(a, seen)), Box::new(self.expand(b, seen))),
            Type::Tuple(items) => Type::Tuple(items.iter().map(|t| self.expand(t, seen)).collect()),
            Type::Record { fields, open } => Type::Record {
                fields: fields.iter().map(|(n, t)| (n.clone(), self.expand(t, seen))).collect(),
                open: *open,
            },
            Type::TagUnion { tags, open } => Type::TagUnion {
                tags: tags
                    .iter()
                    .map(|(n, args)| (n.clone(), args.iter().map(|t| self.expand(t, seen)).collect()))
                    .collect(),
                open: *open,
            },
            _ => ty.clone(),
        }
    }

    /// Make `Module.member : Type` annotations from a platform's modules known.
    pub fn declare_signatures(&mut self, signatures: impl IntoIterator<Item = (String, Type)>) {
        self.declared_signatures.extend(signatures);
    }

    pub fn allow_dispatch(&mut self, methods: Vec<String>) {
        self.where_methods = methods;
    }

    pub fn new() -> Self {
        TypeChecker {
            declared_types: std::collections::HashMap::new(),
            declared_signatures: std::collections::HashMap::new(),
            where_methods: Vec::new(),
            lambda_depth: 0,
            subst: Substitution::new(),
            binops: Vec::new(),
            dispatches: Vec::new(),
            enclosing_type: Vec::new(),
            literals: Vec::new(),
            numeral_vars: std::collections::HashSet::new(),
            nominal_literals: std::collections::HashMap::new(),
            suffixed: std::collections::HashMap::new(),
            returns: Vec::new(),
            parse_targets: std::collections::HashMap::new(),
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
                Self::type_vars_in(&self.apply(ty), &mut vars);
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
            Type::Record { fields, .. } => Type::closed_record(
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
            Type::Record { fields, .. } => {
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
        let resolved = self.apply(expected);

        match expr {
            // Numeric literals are POLYMORPHIC: `255` is a U8 in `x : U8`, an I64 in
            // `x : I64`. Synthesising them as I64 and unifying would reject every
            // annotation that is not I64.
            Expr::Int(_, id) if resolved.is_numeric() => {
                self.literals.push((*id, resolved.clone()));
                Ok(())
            }
            Expr::Float(_, _, id) if resolved.is_fractional() => {
                self.literals.push((*id, resolved.clone()));
                Ok(())
            }

            // Arithmetic inherits the expected type, so `x : U8 = 1 + 2` works for the
            // same reason a bare literal does.
            Expr::BinOp { left, op, right, id }
                if resolved.is_numeric()
                    && matches!(
                        op,
                        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div
                            | BinOp::IntDiv | BinOp::Rem
                    ) =>
            {
                self.binops.push((*id, resolved.clone()));
                self.check(left, &resolved)?;
                self.check(right, &resolved)
            }

            // `Json.parse(text)` checked against a type is the only place that type is
            // ever stated; remember it for the compiler.
            Expr::Call { func, id, .. }
                if matches!(&**func, Expr::Qualified { module: "Json", name: "parse", .. }) =>
            {
                self.parse_targets.insert(*id, resolved.clone());
                let actual = self.synth(expr)?;
                self.unify(&actual, &resolved)
            }

            // A nominal construction knows its own field types better than whatever it
            // is being checked against — an open record grown from field reads names
            // only the fields that were read. Synthesising routes it through the
            // declaration; the expectation is then checked against the result.
            _ if expr_id_has_nominal(self, expr) => {
                let actual = self.synth(expr)?;
                self.unify(&actual, &resolved)
            }

            // An OPTIONAL field holds an ordinary value of its inner type; the option
            // is about whether the field is there, not about what it holds.
            _ if matches!(resolved, Type::Optional(_)) => {
                let Type::Optional(inner) = &resolved else { unreachable!("matched") };
                let inner = (**inner).clone();
                self.check(expr, &inner)
            }

            // Every element against the element type, so a list of numeric literals
            // becomes what its annotation says: `[46, 69]` in a `List(Dec)` is a list
            // of fixed-point values, not of integers.
            Expr::List(items, _) if matches!(resolved, Type::List(_)) => {
                let Type::List(element) = &resolved else { unreachable!("matched") };
                for item in items {
                    self.check(item, element)?;
                }
                Ok(())
            }

            // Every field against what the record declares for it, so a literal in one
            // takes its type from the annotation rather than defaulting.
            Expr::Record(written, _) if matches!(resolved, Type::Record { .. }) => {
                let (declared, open) = match &resolved {
                    Type::Record { fields, open } => (fields.clone(), *open),
                    _ => unreachable!("matched"),
                };
                for (name, value) in written {
                    match declared.iter().find(|(field, _)| field == name) {
                        Some((_, ty)) => {
                            let ty = ty.clone();
                            self.check(value, &ty)?;
                        }
                        // Checking the fields it HAS is not checking the record:
                        // an unexpected field is still a mismatch, and a required one
                        // left out is too.
                        None if open => {
                            self.synth(value)?;
                        }
                        None => {
                            self.synth(value)?;
                            return Err(TypeError {
                                message: format!("Record has an unexpected field `{}`", name),
                                expected: resolved.to_string(),
                                actual: format!("a record with `{}`", name),
                                line: 0,
                                col: 0,
                            });
                        }
                    }
                }
                for (name, ty) in &declared {
                    if written.iter().any(|(field, _)| field == name) {
                        continue;
                    }
                    if !matches!(ty, Type::Optional(_)) {
                        return Err(TypeError {
                            message: format!("Record is missing field `{}`", name),
                            expected: resolved.to_string(),
                            actual: "a record without it".to_string(),
                            line: 0,
                            col: 0,
                        });
                    }
                }
                Ok(())
            }

            // A nominal wraps its backing, so checking against one checks against that.
            Expr::Record(..) | Expr::Tag { .. } | Expr::List(..)
                if matches!(resolved, Type::Nominal { .. }) =>
            {
                let Type::Nominal { backing, .. } = &resolved else { unreachable!("matched") };
                let backing = (**backing).clone();
                self.check(expr, &backing)
            }

            // A tuple against a tuple, element by element.
            Expr::Tuple(items, _) if matches!(&resolved, Type::Tuple(t) if t.len() == items.len()) =>
            {
                let Type::Tuple(declared) = &resolved else { unreachable!("matched") };
                let declared = declared.clone();
                for (item, ty) in items.iter().zip(declared.iter()) {
                    self.check(item, ty)?;
                }
                Ok(())
            }

            // A tag's payload against what the union declares for that tag, so a
            // literal inside one takes its type from the union: `Ok(147.66…)` against
            // `Try(Dec, …)` is a fixed-point value, not a float.
            Expr::Tag { name, args, .. } if matches!(resolved, Type::TagUnion { .. }) => {
                let Type::TagUnion { tags, .. } = &resolved else { unreachable!("matched") };
                match tags.iter().find(|(tag, _)| tag == name) {
                    Some((_, declared)) if declared.len() == args.len() => {
                        for (arg, ty) in args.iter().zip(declared.iter()) {
                            self.check(arg, ty)?;
                        }
                        Ok(())
                    }
                    // Not a tag this union declares, or a different arity: let
                    // unification report it rather than passing silently.
                    _ => {
                        let actual = self.synth(expr)?;
                        self.unify(&actual, &resolved)
                    }
                }
            }

            Expr::Lambda { params, body, .. } => {
                // `f! : () => {}` with `f! = || …`: the annotation's `()` is one unit
                // parameter and the lambda declares none, so peel that one arrow —
                // as a call `f!()` does — or the body is checked against the whole
                // function type.
                let arity = if params.is_empty()
                    && matches!(&resolved, Type::Function(param, _) if matches!(**param, Type::Unit))
                {
                    1
                } else {
                    params.len()
                };
                match Self::peel_params(&resolved, arity) {
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
                        self.returns.push(result_type.clone());
                        let outcome = self.check(body, &result_type);
                        self.returns.pop();
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
    /// `module_of`, but a numeral variable answers as the fractional type it will
    /// default to — an unresolved numeral still has a method block.
    fn module_named(&self, ty: &Type) -> Option<&'static str> {
        if let Type::TypeVar(v) = ty {
            // A numeral nothing pinned down defaults to `Dec`.
            return self.numeral_vars.contains(v).then_some("Dec");
        }
        module_of(ty)
    }

    /// The type of `name` read as a sibling of the method being checked.
    fn sibling(&mut self, name: &str) -> Option<Type> {
        let owner = self.enclosing_type.last()?.clone();
        self.lookup(&format!("{}.{}", owner, name))
    }

    /// A type as the program will actually see it: the substitution applied, and any
    /// numeral left unpinned resolved to the `Dec` it defaults to.
    pub fn defaulted(&self, ty: &Type) -> Type {
        self.default_numerals(&self.apply(ty), &self.numeral_vars)
    }

    fn default_numerals(&self, ty: &Type, numerals: &std::collections::HashSet<u32>) -> Type {
        match ty {
            Type::TypeVar(v) if numerals.contains(v) => Type::Dec,
            Type::List(inner) => Type::List(Box::new(self.default_numerals(inner, numerals))),
            Type::Optional(inner) => {
                Type::Optional(Box::new(self.default_numerals(inner, numerals)))
            }
            Type::Tuple(items) => {
                Type::Tuple(items.iter().map(|t| self.default_numerals(t, numerals)).collect())
            }
            Type::Function(a, b) => Type::Function(
                Box::new(self.default_numerals(a, numerals)),
                Box::new(self.default_numerals(b, numerals)),
            ),
            Type::Nominal { name, backing } => Type::Nominal {
                name: name.clone(),
                backing: Box::new(self.default_numerals(backing, numerals)),
            },
            Type::Record { fields, open } => Type::Record {
                fields: fields
                    .iter()
                    .map(|(n, t)| (n.clone(), self.default_numerals(t, numerals)))
                    .collect(),
                open: *open,
            },
            Type::TagUnion { tags, open } => Type::TagUnion {
                tags: tags
                    .iter()
                    .map(|(n, args)| {
                        (n.clone(), args.iter().map(|t| self.default_numerals(t, numerals)).collect())
                    })
                    .collect(),
                open: *open,
            },
            other => other.clone(),
        }
    }

    /// Pin the literals that carried an explicit type suffix: `255.U8` is a U8, not a
    /// numeral waiting to be defaulted.
    pub fn declare_suffixed_literals(&mut self, literals: &[(crate::ast::NodeId, Type)]) {
        self.suffixed.extend(literals.iter().cloned());
    }

    /// Tell the checker which record literals were written as a nominal construction.
    pub fn declare_nominal_literals(&mut self, literals: &[(crate::ast::NodeId, Type)]) {
        self.nominal_literals.extend(literals.iter().cloned());
    }

    /// The declared type of `Module.method`, from the program or from `Builtin.roc`.
    ///
    /// A name the program binds wins: a user's own `Counter.show` is theirs. Otherwise
    /// the answer comes from the source roc itself compiles — `Dict.get : Dict(k, v),
    /// k -> Try(v, [KeyNotFound])` is written down there, so there is no reason to
    /// guess a result type from a method's name.
    ///
    /// Instantiated fresh at every use, so two calls to `Dict.empty()` do not unify
    /// with each other.
    fn declared(&mut self, module: &str, method: &str) -> Option<Type> {
        let qualified = format!("{}.{}", module, method);
        if let Some(ty) = self.lookup(&qualified) {
            return Some(ty);
        }
        let ty = match self.declared_signatures.get(&qualified) {
            Some(ty) => ty.clone(),
            None => crate::builtin::signatures_for(module)
                .iter()
                .find(|(name, _)| *name == qualified)
                .map(|(_, ty)| ty.clone())?,
        };
        let mut generics = Vec::new();
        Self::type_vars_in(&ty, &mut generics);
        generics.sort_unstable();
        generics.dedup();
        Some(if generics.is_empty() { ty } else { self.instantiate(&ty, &generics) })
    }

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
    /// The `BinOp` nodes whose operands are both integers, once inference is done.
    ///
    /// Resolved through the finished substitution, so a node whose operands were only
    /// a type variable while it was being checked is answered correctly here. What the
    /// compiler does with it is emit `BinInt`.
    /// Which MODULE each `Dispatch` node's receiver belongs to, once inference is done.
    ///
    /// This is what makes `xs.map(f)` reach `List.map` rather than whichever loaded type
    /// happens to define a `map`. Resolved through the finished substitution, like
    /// `integer_binops`, so a receiver that was still a variable when it was checked is
    /// answered correctly here.
    ///
    /// A receiver with no module — a bare record, a tag union, a variable a `where`
    /// clause covers — is simply absent, and the compiler falls back to what it did
    /// before: the single candidate by name, or a runtime dispatch on the value.
    pub fn dispatch_modules(
        &self,
    ) -> std::collections::HashMap<crate::ast::NodeId, &'static str> {
        self.dispatches
            .iter()
            .filter_map(|(id, ty)| Some((*id, module_of(&self.apply(ty))?)))
            .collect()
    }

    /// Which module each `BinOp` node's operands belong to, once inference is done.
    ///
    /// An operator is a method — `a == b` is `a.is_eq(b)` — so it needs the same
    /// answer as `dispatch_modules`, and for the same reason: the operand's TYPE says
    /// whose `is_eq` runs. Without it a lone user `is_eq` anywhere in the program
    /// captures every comparison, tuples and tags included.
    pub fn binop_modules(&self) -> std::collections::HashMap<crate::ast::NodeId, &'static str> {
        self.binops
            .iter()
            .filter_map(|(id, ty)| Some((*id, operator_module(&self.apply(ty))?)))
            .collect()
    }

    /// The literal nodes whose type is `Dec`, once inference is done.
    ///
    /// What the compiler does with it is emit a fixed-point value rather than an
    /// integer or a float: `Dec` carries eighteen decimal places exactly, which is why
    /// roc prints `147.666666666666666666` where an f64 gives `147.66666666666666`.
    /// What each `Json.parse` call was expected to produce, resolved.
    pub fn json_parse_targets(
        &self,
    ) -> std::collections::HashMap<crate::ast::NodeId, Type> {
        self.parse_targets.iter().map(|(id, ty)| (*id, self.apply(ty))).collect()
    }

    pub fn dec_literals(&self) -> std::collections::HashSet<crate::ast::NodeId> {
        self.literals
            .iter()
            .filter(|(_, ty)| matches!(self.apply(ty), Type::Dec))
            .map(|(id, _)| *id)
            .collect()
    }

    /// The literal nodes nothing ever pinned down.
    ///
    /// roc DEFAULTS an unconstrained numeral to `Dec` — `x = 15` prints `15.0`, and
    /// `[1, 2, 3]` prints `[1.0, 2.0, 3.0]`. A literal that any annotation, parameter
    /// or operation reached is concrete by now and is not here.
    pub fn fractional_literals(&self) -> std::collections::HashSet<crate::ast::NodeId> {
        let mut pinned = std::collections::HashSet::new();
        let mut floating = std::collections::HashSet::new();
        for (id, ty) in &self.literals {
            match self.apply(ty) {
                Type::TypeVar(_) => {
                    floating.insert(*id);
                }
                _ => {
                    pinned.insert(*id);
                }
            }
        }
        floating.retain(|id| !pinned.contains(id));
        floating
    }

    pub fn integer_binops(&self) -> std::collections::HashSet<crate::ast::NodeId> {
        self.binops
            .iter()
            .filter(|(_, ty)| matches!(self.apply(ty), Type::I64))
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn synth(&mut self, expr: &Expr) -> Result<Type, TypeError> {
        match expr {
            _ if expr_id_has_nominal(self, expr) => {
                // Taken OUT while it is checked: `check` falls back to `synth` for a
                // shape it has no rule for, and that would arrive back here.
                let declared = self
                    .nominal_literals
                    .remove(&expr.id())
                    .expect("checked by the guard");
                // FRESH variables per construction: `Wrapper(a)` is one declaration and
                // `Wrapper.{ item: "roc" }` and `Wrapper.{ item: 42 }` are two uses of
                // it, so they must not share the `a`.
                let declared = {
                    let mut generics = Vec::new();
                    Self::type_vars_in(&declared, &mut generics);
                    generics.sort_unstable();
                    generics.dedup();
                    if generics.is_empty() {
                        declared
                    } else {
                        self.instantiate(&declared, &generics)
                    }
                };
                let backing = match &declared {
                    Type::Nominal { backing, .. } => (**backing).clone(),
                    other => other.clone(),
                };
                let outcome = self.check(expr, &backing);
                self.nominal_literals.insert(expr.id(), declared.clone());
                outcome?;
                Ok(declared)
            }
            Expr::Str(_, _) => Ok(Type::Str),
            Expr::Unit(_) => Ok(Type::Unit),
            Expr::Bool(_, _) => Ok(Type::Bool),
            Expr::Range { start, end, .. } => {
                for bound in [start, end] {
                    let bound_type = self.synth(bound)?;
                    self.unify(&bound_type, &Type::I64)?;
                }
                Ok(Type::Range)
            }
            Expr::Tuple(items, _) => {
                // Positional, so each element keeps its own type — no joining.
                let mut types = Vec::with_capacity(items.len());
                for item in items {
                    types.push(self.synth(item)?);
                }
                Ok(Type::Tuple(types))
            }
            Expr::TupleIndex { tuple, index, .. } => {
                let tuple_type = self.synth(tuple)?;
                // Likewise for a nominal over a tuple.
                let resolved = match self.apply(&tuple_type) {
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
            Expr::RecordUpdate { base, fields, .. } => {
                let base_type = self.synth(base)?;
                let known = match self.apply(&base_type) {
                    Type::Record { fields, .. } => fields,
                    Type::Nominal { backing, .. } => match *backing {
                        Type::Record { fields, .. } => fields,
                        _ => Vec::new(),
                    },
                    _ => Vec::new(),
                };
                let mut updated = Vec::new();
                for (name, value) in fields {
                    // CHECKED against the field it replaces, where the base knows it.
                    // `{ ..p, age: 31 }` on a `{ age: I64 }` makes `31` an I64 rather
                    // than leaving it to default.
                    match known.iter().find(|(field, _)| field == name) {
                        Some((_, declared)) => {
                            let declared = declared.clone();
                            self.check(value, &declared)?;
                            updated.push((name.to_string(), declared));
                        }
                        None => updated.push((name.to_string(), self.synth(value)?)),
                    }
                }

                // The base is still unknown — an unannotated parameter. It is at least
                // a record WITH these fields, and saying so links their types to
                // whatever the caller passes: `birthday = |p| { ..p, age: 31 }` then
                // `birthday(start)` is what tells `31` it is an I64.
                if known.is_empty() && matches!(self.apply(&base_type), Type::TypeVar(_)) {
                    let shape = Type::Record {
                        fields: updated.iter().map(|(n, t)| (n.clone(), t.clone())).collect(),
                        open: true,
                    };
                    let _ = self.unify(&base_type, &shape);
                }

                match self.apply(&base_type) {
                    Type::Record { fields: known, open } => {
                        // The result keeps the base's shape, with named fields replaced.
                        // A field the base does not have is an error: an update cannot
                        // add one — unless the base is OPEN, where "the fields seen so
                        // far" is all that is known and an update names another of them.
                        let mut result = known.clone();
                        for (name, ty) in updated {
                            match result.iter_mut().find(|(field, _)| *field == name) {
                                Some(slot) => slot.1 = ty,
                                None if open => result.push((name, ty)),
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
            Expr::List(items, _) => {
                // Every element must share one type. An empty list's element type is
                // unconstrained, so it gets a fresh variable.
                let mut element = self.fresh_var();
                for item in items {
                    let item_type = self.synth(item)?;
                    element = self.join(&element, &item_type)?;
                }
                Ok(Type::List(Box::new(element)))
            }
            Expr::Match { scrutinee, arms, .. } => {
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
                    let resolved_scrutinee = self.apply(&scrutinee_type);
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
                let resolved = self.apply(&scrutinee_type);
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
            Expr::If { condition, then_branch, otherwise, .. } => {
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
            Expr::For { name, iterable, body, .. } => {
                let iterable_type = self.synth(iterable)?;
                let element = match self.apply(&iterable_type) {
                    // A range yields whole numbers without being a list.
                    Type::Range => Type::I64,
                    // Still unknown — a parameter whose call site has not been seen.
                    // It is still a LIST of the element, which is what carries a type
                    // from the loop body out to the caller's argument: without the
                    // link, `total([1, 2, 3])` could never tell its literals that
                    // `$sum` is an I64. A range satisfies `List` in `unify`, so this
                    // does not shut one out.
                    Type::TypeVar(_) => {
                        let element = self.fresh_var();
                        self.unify(&iterable_type, &Type::List(Box::new(element.clone())))?;
                        element
                    }
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
            Expr::While { condition, body, .. } => {
                let condition_type = self.synth(condition)?;
                self.unify(&condition_type, &Type::Bool)?;
                self.synth(body)?;
                Ok(Type::Unit)
            }
            // `break` never produces a value; it leaves the loop.
            Expr::Break(_) => Ok(self.fresh_var()),
            // `return` leaves the function, so it fits wherever it appears.
            Expr::Return(value, _) => {
                let returned = self.synth(value)?;
                if let Some(expected) = self.returns.last().cloned() {
                    self.unify(&returned, &expected)?;
                }
                // A `return` never falls through, so it fits wherever it is written.
                Ok(self.fresh_var())
            }
            // `crash` never returns, so it too fits anywhere.
            Expr::Crash(message, _) => {
                self.synth(message)?;
                Ok(self.fresh_var())
            }
            Expr::Expect(condition, _) => {
                let condition_type = self.synth(condition)?;
                self.unify(&condition_type, &Type::Bool)?;
                Ok(Type::Unit)
            }
            Expr::Dbg(value, _) => {
                self.synth(value)?;
                Ok(Type::Unit)
            }
            Expr::Dispatch { receiver, method, args, id } => {
                let receiver_type = self.synth(receiver)?;
                self.dispatches.push((*id, receiver_type.clone()));

                // Dispatch is STATIC: the receiver's type picks the module. If the type
                // is still an unresolved variable there is nothing to dispatch on, and
                // roc says so too — "trying to dispatch a method named to_str on an
                // unresolved type variable".
                let resolved = self.apply(&receiver_type);
                // A `where` clause promised this method exists on whatever the caller
                // supplies, so an unresolved receiver is fine here — roc has already
                // checked the constraint is satisfied at each call site.
                let promised = self.where_methods.iter().any(|m| m == method);
                // A numeral variable is not "unresolved" in the sense that matters: it
                // is a number whose width is not yet fixed, and every numeric width
                // answers the same method block here.
                let numeral =
                    matches!(&resolved, Type::TypeVar(v) if self.numeral_vars.contains(v));
                if matches!(resolved, Type::TypeVar(_)) && !numeral && !promised && self.lambda_depth == 0 {
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

                // A TAG UNION has no method block of its own, so only the handful this
                // interpreter implements for a `Try` will run. roc reports the rest as
                // "This <name> method is being called on a value whose type doesn't have
                // that method", and so should this — otherwise `x.first().to_str()`
                // quietly answers `Str` for a `Try` that has no `to_str`.
                if matches!(resolved, Type::TagUnion { .. })
                    && !promised
                    && !matches!(
                        *method,
                        "is_ok" | "is_err" | "map_ok" | "map_err" | "with_default" | "on_err"
                    )
                    && self.declared("Tags", method).is_none()
                {
                    return Err(TypeError {
                        message: format!(
                            "This `{}` method is being called on a value whose type does not have it",
                            method
                        ),
                        expected: format!("a type with a `{}` method", method),
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
                // The receiver's type names its module, and the module's method block
                // declares the signature — for a user nominal and for `Builtin.roc`'s
                // own types alike, once `declare_builtins` has seeded them. Applying it
                // consumes the receiver plus the written arguments.
                if let Some(module) = self.module_named(&resolved) {
                    if let Some(signature) = self.declared(module, method) {
                        if let Some((params, result)) =
                            Self::peel_params(&signature, args.len() + 1)
                        {
                            // CHECKED against the declared parameters, before they are
                            // synthesised: a lambda argument needs its parameter types
                            // BEFORE its body is looked at, or the body infers them from
                            // nothing. `xs.fold(0, |b, x| b + x)` nested inside another
                            // fold is where that shows: without it `x` is a free
                            // variable and the elements never learn what they are.
                            // The RECEIVER FIRST, which is what carries a type into the
                            // arguments: `List.fold : List(a), state, (state, a ->
                            // state) -> state` only tells the lambda what `a` is once
                            // the list has said so.
                            //
                            // Except a range, which answers the List methods without
                            // being a List — `(1..=100).iter()` reaches `List.iter`.
                            if let Some(first) = params.first() {
                                if !matches!(resolved, Type::Range) {
                                    self.unify(first, &resolved)?;
                                }
                            }
                            for (arg, declared) in args.iter().zip(params.iter().skip(1)) {
                                let declared = self.apply(declared);
                                self.check(arg, &declared)?;
                            }
                            return Ok(self.apply(&result));
                        }
                    }
                }

                // `negate` keeps its receiver's type; everything else comes from the
                // shared builtin table.
                if *method == "negate" {
                    return Ok(resolved);
                }
                // No signature to go by: synthesise the arguments and fall back to the
                // checker's own table.
                let mut arg_types = Vec::with_capacity(args.len());
                for arg in args {
                    arg_types.push(self.synth(arg)?);
                }
                Ok(self.builtin_result(method, Some(&resolved), &arg_types))
            }
            Expr::OptionalField { record, field, .. } => {
                let record_type = self.synth(record)?;
                let resolved = match self.apply(&record_type) {
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
            Expr::FieldAccess { record, field, .. } => {
                let record_type = self.synth(record)?;
                // A nominal over a record supports field access on its backing — roc
                // allows `p.x` for `p : Point`, and a method body relies on it.
                let resolved = match self.apply(&record_type) {
                    Type::Nominal { backing, .. } => *backing,
                    other => other,
                };
                match resolved {
                    // An OPEN record promises only the fields it lists, so reading
                    // another one GROWS it. `|p| p.x + p.y` reaches here twice, and
                    // without this the second read failed against the record the first
                    // one had just invented.
                    Type::Record { ref fields, open: true }
                        if !fields.iter().any(|(name, _)| name == field) =>
                    {
                        let field_type = self.fresh_var();
                        let mut grown = fields.clone();
                        grown.push((field.to_string(), field_type.clone()));
                        let grown = Type::Record { fields: grown, open: true };
                        match record_type {
                            Type::TypeVar(v) => self.subst.insert(v, grown),
                            ref already => self.subst.rebind(already, grown),
                        }
                        Ok(field_type)
                    }
                    Type::Record { fields, .. } => fields
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
                    // Not resolved yet — an unannotated parameter. It is at least a
                    // record WITH this field, and saying so links the field's type to
                    // whatever the caller passes: `|a| 0 - a.cents` learns that `cents`
                    // is an I64 from the `Money` it is called with, so the `0` beside
                    // it does not default.
                    Type::TypeVar(_) => {
                        let field_type = self.fresh_var();
                        let shape = Type::Record {
                            fields: vec![(field.to_string(), field_type.clone())],
                            open: true,
                        };
                        let _ = self.unify(&record_type, &shape);
                        Ok(field_type)
                    }
                    _ => Ok(self.fresh_var()),
                }
            }
            // `Point.{ x: 3 }` is a nominal construction: its fields have the types the
            // declaration gave them, even though the AST no longer says which nominal
            // it was.

            Expr::Record(fields, _) => {
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
            Expr::Tag { name, args, .. } => {
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
            Expr::StrInterp(parts, _) => {
                for part in parts {
                    if let crate::ast::StrPart::Expr(inner) = part {
                        self.synth(inner)?;
                    }
                }
                Ok(Type::Str)
            }
            // A numeral is POLYMORPHIC. `15` is an I64 in `x : I64`, a `Dec` in a
            // `List(Dec)`, and — where nothing says — a FRACTIONAL value: roc prints
            // `15.0` for a bare `x = 15`. Synthesising I64 here made the interpreter
            // disagree with the compiler about its own examples.
            // A SUFFIXED literal already said what it is.
            Expr::Int(_, id) | Expr::Float(_, _, id) if self.suffixed.contains_key(id) => {
                Ok(self.suffixed[id].clone())
            }
            Expr::Int(_, id) => {
                let var = self.fresh_var();
                if let Type::TypeVar(v) = var {
                    self.numeral_vars.insert(v);
                }
                self.literals.push((*id, var.clone()));
                Ok(var)
            }
            Expr::Float(..) => Ok(Type::F64),     // Float literals default to F64
            Expr::Ident(name, _) => {
                // A name in scope has a known type now.
                if let Some(ty) = self.lookup(name) {
                    return Ok(self.apply(&ty));
                }
                // A SIBLING method, called by its bare name from inside the same
                // method block.
                if let Some(ty) = self.sibling(name) {
                    return Ok(self.apply(&ty));
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
            Expr::Qualified { module: "Bool", name: "True" | "False", .. } => Ok(Type::Bool),
            // A nominal's method block binds `Type.method` as an ordinary name, so a
            // qualified reference resolves from the environment before falling back to
            // "some function" — `Counter.start` is a Counter, not a function.
            Expr::Qualified { module, name, .. } => {
                if let Some(declared) = self.declared(module, name) {
                    return Ok(declared);
                }
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
            Expr::BinOp { left, op, right, id } => {
                let left_type = self.synth(left)?;
                let right_type = self.synth(right)?;
                self.binops.push((*id, left_type.clone()));

                match op {
                    // Arithmetic returns the type of its operands, not always I64.
                    // Returning I64 unconditionally made `quot : F64` reject
                    // `quot = 7.0 / 2.0`, which only showed up once annotations were
                    // actually checked.
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                        self.unify(&left_type, &right_type)?;
                        Ok(self.apply(&left_type))
                    }
                    // `//` and `%` are integer-only in Roc, which is a constraint on the
                    // OPERANDS as much as a promise about the result: `7 // 2` makes
                    // both literals integers rather than leaving them to default.
                    BinOp::IntDiv | BinOp::Rem => {
                        self.unify(&left_type, &right_type)?;
                        let _ = self.unify(&left_type, &Type::I64);
                        Ok(Type::I64)
                    }
                    // Comparison yields Bool, not an integer — but the two sides are
                    // still the same type, and a numeric literal on the right has no
                    // type of its own until something says so. Without this
                    // `variance == Ok(147.666666666666666666)` compares a fixed-point
                    // value against the nearest double to it, and they differ.
                    //
                    // The outcome is DISCARDED: roc rejects a mismatched comparison and
                    // this does not yet, and turning that on here would be a separate
                    // change with its own fallout.
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        let left_type = self.apply(&left_type);
                        let _ = self.check(right, &left_type);
                        Ok(Type::Bool)
                    }
                    // `and` / `or` take and return Bool.
                    BinOp::And | BinOp::Or => Ok(Type::Bool),
                }
            }
            Expr::Lambda { params, body, .. } => {
                // For each parameter, allocate a fresh type variable
                let mut param_types = vec![];
                for _ in params.iter() {
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
                let result = self.fresh_var();
                self.returns.push(result.clone());
                let body_type = self.synth(body);
                self.returns.pop();
                self.lambda_depth -= 1;
                self.pop_scope();
                let body_type = body_type?;
                // The body falls through to the same place a `return` jumps to.
                self.unify(&body_type, &result)?;
                let body_type = self.apply(&result);

                // Build function type: (T1 -> T2 -> ... -> Tn)
                //
                // `|| body` takes no parameters and is still a function: Roc spells its
                // empty parameter list `()`, the unit type, so it is `{} -> body` — the
                // same shape `enable_raw_mode! : () => {}` declares. Typing it as its
                // body made every zero-argument effect a value instead of a call.
                let mut result_type = body_type;
                if param_types.is_empty() {
                    result_type = Type::Function(Box::new(Type::Unit), Box::new(result_type));
                }
                for param_type in param_types.into_iter().rev() {
                    result_type = Type::Function(Box::new(param_type), Box::new(result_type));
                }

                Ok(result_type)
            }
            // A qualified builtin call — `List.len(xs)` — resolves its result the same
            // way `xs.len()` does, so the two spellings behave alike when chained.
            Expr::Call { func, args, .. } if matches!(**func, Expr::Qualified { .. }) => {
                let mut arg_types = Vec::with_capacity(args.len());
                for arg in args {
                    arg_types.push(self.synth(arg)?);
                }
                if let Expr::Qualified { module, name, .. } = &**func {
                    // A nominal's method is an ordinary binding, so its own signature
                    // decides the result — not the builtin table, which would hand back
                    // an unconstrained variable and break any chaining off it.
                    if let Some(signature) = self.declared(module, name) {
                        // `Dict.empty()` takes no arguments and has type `{} -> Dict`:
                        // Roc spells an empty parameter list `()`, which IS the unit
                        // type, so applying it still peels one arrow. Peeling none
                        // handed back the function itself, and `Dict.empty().insert(k, v)`
                        // then had nothing to dispatch on.
                        let takes_unit = arg_types.is_empty()
                            && matches!(&signature, Type::Function(param, _) if matches!(**param, Type::Unit));
                        let arity = if takes_unit { 1 } else { arg_types.len() };
                        if let Some((params, result)) = Self::peel_params(&signature, arity) {
                            // Check the ARGUMENTS against what was declared, not just
                            // read the result off the end.
                            if !takes_unit {
                                for (declared, given) in params.iter().zip(arg_types.iter()) {
                                    self.unify(declared, given)?;
                                }
                            }
                            return Ok(self.apply(&result));
                        }
                    }
                    // A qualified call NAMES its receiver's type: `I64.to_str(birds)`
                    // says `birds` is an I64, and `Str.concat(a, b)` says `a` is a Str.
                    // Without this an unannotated numeral stayed a numeral and
                    // defaulted to `Dec`, so `I64.to_str(3)` printed `3.0`.
                    //
                    // A CONSTRUCTOR is the exception: `I64.from_str(s)` takes a Str and
                    // returns an I64, so its first argument is not the receiver. They
                    // are spelled `from_…` throughout `Builtin.roc`.
                    if let Some(receiver) = arg_types.first() {
                        if !name.starts_with("from_") {
                            if let Some(declared) = type_named(module) {
                                let _ = self.unify(&declared, receiver);
                            }
                        }
                    }
                    // And a constructor RETURNS the module's type: `I64.from_str(s)` is
                    // a `Try(I64, …)`, which is what tells everything downstream of it
                    // that it is working with integers.
                    if let Some(declared) = type_named(module) {
                        match *name {
                            "from_str" => {
                                return Ok(Type::TagUnion {
                                    tags: vec![
                                        ("Err".to_string(), vec![self.fresh_var()]),
                                        ("Ok".to_string(), vec![declared]),
                                    ],
                                    open: true,
                                })
                            }
                            // `I64.to_str` and the `to_…` conversions say what they
                            // give back in their names.
                            "to_str" => return Ok(Type::Str),
                            _ => {}
                        }
                    }

                    // Drop the receiver: `List.fold(xs, 0, f)` has its accumulator
                    // second, matching `xs.fold(0, f)`.
                    let rest = if arg_types.is_empty() { &arg_types[..] } else { &arg_types[1..] };
                    let rest = rest.to_vec();
                    // `List.concat(xs, ys)` passes its subject first, exactly as
                    // `xs.concat(ys)` does, so the receiver is the first argument.
                    let receiver = arg_types.first().map(|t| self.apply(t));
                    return Ok(self.builtin_result(name, receiver.as_ref(), &rest));
                }
                unreachable!("guarded by the match arm")
            }
            Expr::Call { func, args, .. } => {
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
                    // When the callee's type is already known, CHECK the argument
                    // against the parameter rather than synthesising it. A numeric
                    // literal is polymorphic and only the parameter says which type it
                    // is — that is how `safe_variance([46, 69])` makes `Dec`s.
                    let known = self.apply(&current_type);
                    if let Type::Function(param, result) = known {
                        self.check(arg, &param)?;
                        current_type = *result;
                        continue;
                    }
                    let arg_type = self.synth(arg)?;
                    let return_type = self.fresh_var();
                    let expected = Type::Function(
                        Box::new(arg_type),
                        Box::new(return_type.clone()),
                    );
                    self.unify(&current_type, &expected)?;
                    current_type = return_type;
                }

                Ok(self.apply(&current_type))
            }
            // The statement spine — a block's `let`s, `var`s and assignments — is
            // walked in a loop rather than by recursing once per statement, so a
            // block's depth is bounded by memory rather than by the Rust stack, which
            // 6,000 statements overflowed.
            Expr::Let { .. } | Expr::VarDecl { .. } | Expr::Assign { .. } => {
                let mut cursor = expr;
                loop {
                    match cursor {
                        Expr::Let { name, annotation, value, body, .. } => {
                            // `Graph.from_dict = …` is a METHOD, and its siblings are
                            // in scope unqualified while its value is checked.
                            let owns = name.rsplit_once('.').map(|(owner, _)| owner.to_string());
                            if let Some(owner) = owns.clone() {
                                self.enclosing_type.push(owner);
                            }
                            let checked = self.check_let(name, annotation, value);
                            if owns.is_some() {
                                self.enclosing_type.pop();
                            }
                            checked?;
                            cursor = body;
                        }
                        Expr::VarDecl { name, value, body, .. } => {
                            let bound = self.synth(value)?;
                            self.bind(name, bound);
                            cursor = body;
                        }
                        Expr::Assign { name, value, body, .. } => {
                            // A reassignment must agree with what the `var` already holds.
                            let assigned = self.synth(value)?;
                            if let Some(existing) = self.lookup(name) {
                                self.unify(&existing, &assigned)?;
                            }
                            cursor = body;
                        }
                        other => return self.synth(other),
                    }
                }
            }
        }
    }

    /// The binding half of a `Let`: everything but its body.
    fn check_let(
        &mut self,
        name: &&'static str,
        annotation: &Option<Type>,
        value: &Expr,
    ) -> Result<(), TypeError> {
        {
            {
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
                        let inferred = self.apply(&inferred);

                        let mut generics = Vec::new();
                        Self::type_vars_in(&inferred, &mut generics);
                        // A numeral's type must not be quantified — `birds = 3` has
                        // ONE type, and `I64.to_str(birds)` is what fixes it.
                        // Generalising would give every use a fresh copy, so nothing
                        // could reach the literal and it would default to `Dec`.
                        // `unify` marks every variable a numeral's is bound to, either
                        // way round, so membership here is the whole test.
                        generics.retain(|v| !self.numeral_vars.contains(v));
                        // Walking the environment costs every binding in scope, so
                        // only a binding that still has something to quantify pays it.
                        // Most do not: a monomorphic `let` is the common case, and a
                        // block of thousands of them was quadratic.
                        if !generics.is_empty() {
                            let outer = self.env_type_vars();
                            generics.retain(|v| !outer.contains(v));
                        }
                        generics.dedup();

                        self.bind_poly(name, inferred, generics);
                    }
                }
                Ok(())
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
            // An iterator is walked with the List methods here, so it is typed as the
            // List it behaves like. Without this `xs.iter().map(f)` dispatches `map` on
            // an unconstrained variable, and the module it belongs to is unknowable.
            "iter" | "iter_rev" => Type::List(Box::new(self.fresh_var())),
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
                        // The scrutinee is not known yet — an unannotated parameter. It
                        // is still the SAME record apart from the named fields, so
                        // sharing its type keeps the remaining fields connected to
                        // whatever the caller passes: `drop_email(p)` then
                        // `trimmed.age` is what tells `age` it is an I64.
                        //
                        // ponytail: this says "the whole record" where the truth is
                        // "the record minus `email`", which `Type` cannot spell — it
                        // has no row variable. The field IS removed at run time; the
                        // cost is that reading it back type-checks when roc would
                        // refuse. A row-polymorphic record closes the gap.
                        other => other.clone(),
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
                } else if types.is_empty() {
                    // `Ok({}) =>` matches the unit value, and `{}` the expression IS
                    // `Type::Unit`; an empty closed record here failed to unify with
                    // it, and `main_for_host!` matches exactly that.
                    Type::Unit
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
        let (a, b) = (self.apply(t1), self.apply(t2));

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
        let t1 = self.apply(t1);
        let t2 = self.apply(t2);

        if t1 == t2 {
            return Ok(());
        }

        match (&t1, &t2) {
            // TypeVar cases
            (Type::TypeVar(v1), Type::TypeVar(v2)) if v1 == v2 => Ok(()),
            // `{}` is the unit type however it was built.
            (Type::Unit, Type::Record { fields, open: false })
            | (Type::Record { fields, open: false }, Type::Unit)
                if fields.is_empty() =>
            {
                Ok(())
            }
            (Type::TypeVar(v), t) | (t, Type::TypeVar(v)) => {
                // A NUMERAL variable stands for a number whose width is not yet fixed,
                // not for anything at all. Letting it become a `Bool` or a function is
                // what made `x = 42` then `x(1)` type-check, and `r : { x: Bool }` accept
                // `{ x: 1 }`.
                // The constraint TRAVELS: binding a numeral's variable to another
                // makes that one a numeral too, whichever way round the binding went.
                // Without this `pair : a, a -> a` accepted `pair(1, "s")` — the `1`
                // reached `a`, and `a` was then free to become a Str.
                if let Type::TypeVar(w) = t {
                    if self.numeral_vars.contains(v) {
                        self.numeral_vars.insert(*w);
                    } else if self.numeral_vars.contains(w) {
                        self.numeral_vars.insert(*v);
                    }
                }
                if self.numeral_vars.contains(v)
                    && !matches!(t, Type::TypeVar(_))
                    && !t.is_numeric()
                {
                    return Err(TypeError {
                        message: format!("A number cannot be used as {}", t),
                        expected: t.to_string(),
                        actual: "a number".to_string(),
                        line: 0,
                        col: 0,
                    });
                }
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
            // Two nominals interchange only if they are the SAME nominal — or if one
            // is BACKED BY the other. `Graph(a) :: Dict(a, List(a))` is a Dict wearing
            // a name, and within a module roc lets the backing type through, so a
            // `Graph` satisfies a `Dict` exactly as a plain record satisfies a nominal
            // over one. Refusing it made `GraphTraversal` fail with "Dict and Graph are
            // different nominal types".
            (Type::Nominal { name: a, backing: a_backing },
             Type::Nominal { name: b, backing: b_backing }) => {
                if a == b {
                    if a_backing == b_backing {
                        return Ok(());
                    }
                    return self.unify(a_backing, b_backing);
                }
                if matches!(&**a_backing, Type::Nominal { name, .. } if name == b) {
                    let other = t2.clone(); return self.unify(a_backing, &other);
                }
                if matches!(&**b_backing, Type::Nominal { name, .. } if name == a) {
                    let other = t1.clone(); return self.unify(&other, b_backing);
                }
                Err(TypeError {
                    message: format!("{} and {} are different nominal types", a, b),
                    expected: t1.to_string(),
                    actual: t2.to_string(),
                    line: 0,
                    col: 0,
                })
            }
            // A RANGE satisfies a `List`: it answers the List methods, `for` walks
            // either, and `eval::module_for` calls a range a List too. Keeping them
            // apart would make a function that loops over its argument reject a range.
            (Type::Range, Type::List(_)) | (Type::List(_), Type::Range) => Ok(()),

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

    /// Occurs check: prevent infinite types.
    ///
    /// Every compound type is looked inside, not just lists and functions. A variable
    /// bound to a record containing itself — `{ ..p, next: p }` — made `apply` recurse
    /// for ever, and the checker went down with a stack overflow instead of a message.
    fn occurs_check(&self, var: u32, ty: &Type) -> bool {
        let ty = self.apply(ty);
        let mut vars = Vec::new();
        Self::type_vars_in(&ty, &mut vars);
        vars.contains(&var)
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

/// `module_of`, widened for OPERATORS only.
///
/// A tuple and a bare tag union name no method block, and saying so is what keeps a
/// lone `is_eq` from answering `(1, "x") == (1, "x")` or `Green == Green`. They are not
/// answered for ordinary dispatch, where "no module" has to stay "unresolved" — a
/// `Try` really has no `to_str`, and reporting that is the point.
fn operator_module(ty: &Type) -> Option<&'static str> {
    Some(match ty {
        Type::Tuple(_) => "Tuple",
        // A nominal OVER a tag union arrives as `Type::Nominal` and is answered by
        // `module_of`; this is the case where the value is tags and nothing more.
        Type::TagUnion { .. } => "Tags",
        other => return module_of(other),
    })
}

/// The module whose method block owns a value of this type.
///
/// It agrees with `eval::module_for`, which answers the same question from a runtime
/// value: a method has to resolve the same way whether the compiler picks it or the VM
/// does. `None` means the type does not name a module — a bare record or tag has no
/// nominal to dispatch through, because roc erases nominals and the value carries no
/// tag to recover one from.
fn module_of(ty: &Type) -> Option<&'static str> {
    Some(match ty {
        Type::Str => "Str",
        Type::Bool => "Bool",
        Type::List(_) => "List",
        // A range answers the List methods — `(1..=n).iter().fold(..)` is the idiom —
        // and `eval::module_for` says the same about the runtime value.
        Type::Range => "List",
        // One integer representation and one float, so every width answers to the same
        // method block — `is_numeric_module` accepts them all on the runtime side.
        Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128 => "I64",
        Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 => "I64",
        Type::F32 | Type::F64 => "F64",
        Type::Dec => "Dec",
        // A tuple has no method block and cannot be a nominal's backing, so naming it
        // here is what says "no user method owns this operator". A RECORD is left
        // unnamed on purpose: roc erases nominals, so an unannotated `Money.{ cents: 5 }`
        // is only a record to the checker and may still own the method.

        // `c : Counter` dispatches into `Counter`'s own method block. This is the case
        // the runtime cannot reconstruct, which is why the compiler has to.
        // Interned because a method name is `&'static str` everywhere else in the
        // compiler, and this is asked once per dispatch node.
        Type::Nominal { name, .. } => crate::memory::string_pool::intern(name),
        _ => return None,
    })
}

/// The type a module name stands for, where it stands for one.
///
/// `I64` is a type; `List` needs an argument and `Dict` is not modelled with its own,
/// so only the ones a bare name fully determines are answered here.
fn type_named(module: &str) -> Option<Type> {
    Some(match module {
        "Str" => Type::Str,
        "Bool" => Type::Bool,
        "U8" => Type::U8, "U16" => Type::U16, "U32" => Type::U32,
        "U64" => Type::U64, "U128" => Type::U128,
        "I8" => Type::I8, "I16" => Type::I16, "I32" => Type::I32,
        "I64" => Type::I64, "I128" => Type::I128,
        "F32" => Type::F32, "F64" => Type::F64, "Dec" => Type::Dec,
        _ => return None,
    })
}

/// Was this expression written as a nominal construction — `Point.{ x: 3 }` or
/// `UserId.(7)`?
fn expr_id_has_nominal(checker: &TypeChecker, expr: &Expr) -> bool {
    checker.nominal_literals.contains_key(&expr.id())
}
