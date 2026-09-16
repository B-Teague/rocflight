//! Pure Functional Parser for Roc
//!
//! Built with pure functional combinators (no external parser libraries)
//! Follows idiomatic Rust with Result-based error handling
//!
//! Pipeline:
//! 1. Desugar shorthand syntax
//! 2. Parse desugared code
//! 3. Build AST
//!
//! Parser Architecture:
//! - Recursive descent with explicit precedence levels
//! - Pure functions that return Result<(T, &str), ParseError>
//! - No side effects or external dependencies
//! - Full transparency and control over parsing behavior

use crate::ast::{Expr, StrPart, MatchArm, Pattern};
use crate::types::Type;
use crate::error::ParseError;
use crate::memory::string_pool;
use crate::desugaring::Desugarer;

/// Roc parser
pub struct Parser {
    input: String,
    pos: usize,
    /// App entry point (e.g., "main!") if app declaration found
    entry_point: Option<String>,
    /// The registered source this parser's nodes belong to, when it was given a name.
    source: Option<usize>,
    /// Counter for type variables introduced by annotations (`a -> a`, and any type
    /// name the interpreter does not model).
    next_type_var: u32,
    /// Annotations read but not yet claimed by a binding of the same name.
    pending_annotations: Vec<(&'static str, Type)>,
    /// Nominal types declared with `Name := backing`, so an annotation naming one
    /// resolves to it rather than to an anonymous fresh variable.
    nominals: Vec<(&'static str, Type)>,
    /// Dependencies from the app header: `(alias, spec, is_platform)`.
    ///
    /// `app [main!] { cli: platform "URL", pkg: "URL", roc: "nightly-..." }` — the
    /// `roc:` entry pins the compiler and is not a fetchable dependency.
    dependencies: Vec<(String, String, bool)>,
    /// Modules brought in by `import`, as `(dependency alias, module)`.
    imports: Vec<(String, String)>,
    /// Files ingested by `import "path" as name : Str`, as `(name, path)`. The file's
    /// CONTENTS become the binding, so the caller reads it relative to the source.
    ingests: Vec<(String, String)>,
    /// Top-level `expect`s, held back until every declaration is bound.
    ///
    /// roc loads a module completely and only then runs its tests, so an `expect` may
    /// use a function declared below it. Running them in source order called functions
    /// that did not exist yet.
    deferred_expects: Vec<Expr>,
    /// Local modules brought in by `import Hello exposing [hello]`, as
    /// `(module path, exposed names)`. The path is relative to the importing file and
    /// names a `.roc` beside it — `Dir/Hello` is `Dir/Hello.roc`.
    local_modules: Vec<(String, Vec<String>)>,
    /// How deep `parse_expr` is nested. Only the outermost call wraps the program in
    /// its nominal method bindings.
    expr_depth: u32,
    /// Defaults collected while parsing the record type of the current declaration,
    /// as `(field, default expression)`. Moved into `nominal_defaults` when the
    /// declaration completes.
    field_defaults: Vec<(String, Expr)>,
    /// Optional field names collected the same way.
    optional_fields: Vec<String>,
    /// Per-nominal field defaults, so `Name.{ ... }` can fill the omitted ones.
    nominal_defaults: Vec<(String, Vec<(String, Expr)>)>,
    /// Method names promised by a `where` clause, so the checker may dispatch them on
    /// a type variable that inference has not resolved.
    where_methods: Vec<String>,
    /// Type parameters of each parameterised nominal, as `(name, var ids in order)`.
    /// `Wrapper(a) := { item: a }` records the id that `a` was given, so `Wrapper(Str)`
    /// can substitute `Str` for it.
    nominal_params: Vec<(String, Vec<u32>)>,
    /// Names introduced by `var`, which are reassignable.
    ///
    /// Flat rather than scoped: a `var x` in one function also makes a later `x = e`
    /// in another read as a reassignment.
    // ponytail: flat set, make it scoped if a test ever shadows a sibling's `var` name.
    mutable_names: Vec<String>,
    /// Methods from nominal `.{ ... }` blocks, as `("Type.method", body)`.
    ///
    /// Wrapped around the program so they are ordinary bindings: `Secret.reveal` is
    /// then a plain lookup, and `s.reveal()` a dispatch that finds it.
    methods: Vec<(&'static str, Option<Type>, Expr)>,
    /// Type variables seen so far in the annotation being parsed.
    ///
    /// A repeated name must mean the SAME variable: `pair : a, a -> a` constrains both
    /// parameters to one type, and without this map each `a` became a separate fresh
    /// variable, so `pair(1, "s")` was wrongly accepted.
    annotation_vars: Vec<(String, u32)>,
}

impl Parser {
    /// Create new parser for input
    /// A parser whose nodes know which file they came from, so a runtime error can
    /// say where.
    ///
    /// `new` leaves that out, which is what the tests and the nested parse of a string
    /// interpolation want: without a registered source a node simply has no location,
    /// and an error reads exactly as it did before.
    pub fn named(file: &str, input: &str) -> Self {
        let mut parser = Parser::new(input);
        parser.source = Some(crate::ast::open_source(crate::ast::next_node_id(), file, input));
        parser
    }

    pub fn new(input: &str) -> Self {
        Parser {
            input: input.to_string(),
            pos: 0,
            next_type_var: 0,
            pending_annotations: Vec::new(),
            nominals: Vec::new(),
            annotation_vars: Vec::new(),
            dependencies: Vec::new(),
            imports: Vec::new(),
            ingests: Vec::new(),
            local_modules: Vec::new(),
            deferred_expects: Vec::new(),
            field_defaults: Vec::new(),
            optional_fields: Vec::new(),
            nominal_defaults: Vec::new(),
            where_methods: Vec::new(),
            nominal_params: Vec::new(),
            mutable_names: Vec::new(),
            expr_depth: 0,
            methods: Vec::new(),
            entry_point: None,
            source: None,
        }
    }

    /// Load and parse file (with desugaring)
    /// Returns: (AST, app_entry_point)
    pub fn from_file(path: &str) -> Result<(Expr, Option<String>), ParseError> {
        // Step 1: Load file
        let source = std::fs::read_to_string(path)
            .map_err(|e| ParseError {
                message: format!("Failed to read file: {}", e),
                position: 0,
            })?;

        // Step 2: Desugar shorthand syntax
        let desugarer = Desugarer::new(source.clone());
        let desugared = desugarer.desugar()?;

        // Step 3: Save desugared for debugging
        let _ = desugarer.save_debug(path, &desugared);

        // Step 4: Parse desugared code
        let mut parser = Parser::new(&desugared);
        let expr = parser.parse_expr()?;
        Ok((expr, parser.app_entry_point()))
    }

    /// Get the app entry point if one was found
    pub fn app_entry_point(&self) -> Option<String> {
        self.entry_point.clone()
    }

    /// Parse expression (entry point)
    /// Handles: let bindings, function calls, literals, top-level definitions
    /// A fresh node id, remembering where the parser currently is.
    ///
    /// Composite nodes should prefer `crate::ast::fresh_node_like(child)`, which takes
    /// the construct's START from its first child rather than its end from here.
    fn node(&self) -> crate::ast::NodeId {
        crate::ast::fresh_node(self.pos)
    }

    pub fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        let parsed = self.parse_expr_outer();
        // Closing the range bounds this file's nodes, so a node made LATER by an
        // enclosing parse does not resolve to this file.
        if let Some(handle) = self.source {
            crate::ast::close_source(handle);
        }
        parsed
    }

    fn parse_expr_outer(&mut self) -> Result<Expr, ParseError> {
        // Counted from the very start: a nominal's method block is parsed by
        // `skip_trivia` in the loop below, so a lambda inside it re-enters this
        // function before the main expression is reached.
        self.expr_depth += 1;
        let outcome = self.parse_expr_inner();
        self.expr_depth -= 1;
        let mut program = outcome?;

        // Only the outermost call brings nominal methods into scope; wrapping at every
        // level nested them inside the first method's own body.
        if self.expr_depth == 0 {
            for (name, annotation, value) in
                std::mem::take(&mut self.methods).into_iter().rev()
            {
                program = Expr::Let { id: self.node(),
                    name,
                    annotation,
                    value: Box::new(value),
                    body: Box::new(program),
                };
            }
            // The tests go last, after every declaration is in scope.
            let expects = std::mem::take(&mut self.deferred_expects);
            if !expects.is_empty() {
                Self::append_to_body(&mut program, expects);
            }
        }
        Ok(program)
    }

    /// Sequence `extra` at the innermost point of a `Let` spine, before its value.
    ///
    /// The file's value — usually `main!` — stays last, so what the program evaluates
    /// to is unchanged.
    fn append_to_body(program: &mut Expr, extra: Vec<Expr>) {
        let mut cursor = program;
        while let Expr::Let { body, .. } = cursor {
            cursor = body;
        }
        let tail = std::mem::replace(cursor, Expr::Unit(crate::ast::fresh_node_unlocated()));
        let mut rebuilt = tail;
        for value in extra.into_iter().rev() {
            rebuilt = Expr::Let { id: crate::ast::fresh_node_unlocated(),
                name: "_",
                annotation: None,
                value: Box::new(value),
                body: Box::new(rebuilt),
            };
        }
        *cursor = rebuilt;
    }

    /// The body of `parse_expr`, without the method-block wrapping.
    fn parse_expr_inner(&mut self) -> Result<Expr, ParseError> {
        self.skip_whitespace();

        // Skip app and import declarations at the top level
        loop {
            let rest = &self.input[self.pos..];

            if rest.starts_with("app ") {
                // Extract entry point from app declaration: app [entry!] { ... }
                if !self.extract_app_entry_point() {
                    self.skip_to_next_declaration();
                }
                self.skip_whitespace();
            } else if rest.starts_with("import ") {
                self.record_import();
                self.skip_to_line_end();
                self.skip_whitespace();
            } else {
                let before = self.pos;
                self.skip_trivia();
                if self.pos == before {
                    break;
                }
            }
        }

        // A platformless app may omit the header entirely: `main! = |_args| ...`
        // implies `app [main!] {}`. Verified against `roc run` on
        // nightly-2026-09-03, and against roc-compiler/test/echo/hello.roc.
        if self.entry_point.is_none() && self.has_top_level_binding("main!") {
            self.entry_point = Some("main!".to_string());
        }

        // Parse the main expression.
        self.parse_top_level()
    }

    /// Parse a type expression, e.g. `List(Str) => Try({}, [Exit(I8), ..])`.
    ///
    /// Grammar, in decreasing precedence:
    /// ```text
    /// type  := atom (',' atom)* ('->' | '=>') type    -- function
    ///        | atom
    /// atom  := Name ['(' type (',' type)* ')']        -- I64, List(T), Try(a, b)
    ///        | name                                   -- lowercase: a type variable
    ///        | '{' [field (',' field)*] '}'           -- record, {} is unit
    ///        | '(' type (',' type)* ')'               -- 1 elem groups, 2+ is a tuple
    ///        | '[' [tag (',' tag)*] [',' '..'] ']'    -- tag union, `..` means open
    /// ```
    ///
    /// `=>` (effectful) types the same as `->` here: the interpreter does not model
    /// effects, only their signatures.
    ///
    /// Parenthesisation is load-bearing in the argument list: `A, B -> C` takes two
    /// parameters, while `(A, B) -> C` takes one tuple.
    fn parse_type(&mut self) -> Result<Type, ParseError> {
        let mut params = vec![self.parse_type_atom()?];

        loop {
            self.skip_inline_whitespace();
            if !self.input[self.pos..].starts_with(',') {
                break;
            }
            // A comma here separates parameters, but it could also belong to an
            // enclosing list; only commit if another atom follows.
            let saved = self.pos;
            self.pos += 1;
            self.skip_inline_whitespace();
            match self.parse_type_atom() {
                Ok(atom) => params.push(atom),
                Err(_) => {
                    self.pos = saved;
                    break;
                }
            }
        }

        self.skip_inline_whitespace();
        let rest = &self.input[self.pos..];
        let is_arrow = rest.starts_with("->") || rest.starts_with("=>");
        if !is_arrow {
            if params.len() == 1 {
                return Ok(params.pop().expect("checked length"));
            }
            // A bare comma list with no arrow is not a type on its own.
            return Err(ParseError {
                message: "Expected '->' or '=>' after a type parameter list".to_string(),
                position: self.pos,
            });
        }
        // Past here an arrow follows, so the comma list really was parameters.
        self.pos += 2;
        self.skip_inline_whitespace();

        // Curried, matching how lambdas and calls are typed: `A, B -> C` becomes
        // `A -> (B -> C)`.
        let mut result = self.parse_type()?;
        for param in params.into_iter().rev() {
            result = Type::Function(Box::new(param), Box::new(result));
        }
        Ok(result)
    }

    /// Parse a type that does NOT treat a top-level comma as a parameter separator.
    ///
    /// Used everywhere a comma already means something else: record field values,
    /// tuple elements, tag payloads and applied-type arguments. Without this level,
    /// `{ x: Bool, y: Bool }` parses the first field's value as the parameter list
    /// `Bool, y` and then fails looking for an arrow.
    fn parse_type_operand(&mut self) -> Result<Type, ParseError> {
        let atom = self.parse_type_atom()?;
        self.skip_inline_whitespace();
        let rest = &self.input[self.pos..];
        if rest.starts_with("->") || rest.starts_with("=>") {
            self.pos += 2;
            let result = self.parse_type_operand()?;
            return Ok(Type::Function(Box::new(atom), Box::new(result)));
        }
        Ok(atom)
    }

    /// Parse one type atom. See `parse_type` for the grammar.
    fn parse_type_atom(&mut self) -> Result<Type, ParseError> {
        self.skip_inline_whitespace();
        let rest = &self.input[self.pos..];

        if rest.starts_with('{') {
            return self.parse_record_type();
        }
        if rest.starts_with('[') {
            return self.parse_tag_union_type();
        }
        if rest.starts_with('(') {
            self.pos += 1;
            let mut items = vec![self.parse_type_operand()?];
            loop {
                self.skip_inline_whitespace();
                if !self.input[self.pos..].starts_with(',') {
                    break;
                }
                self.pos += 1;
                items.push(self.parse_type_operand()?);
            }
            self.skip_inline_whitespace();
            if !self.input[self.pos..].starts_with(')') {
                return Err(ParseError {
                    message: "Expected ')' in type".to_string(),
                    position: self.pos,
                });
            }
            self.pos += 1;
            // One element is grouping, like `(I64 -> I64)`; more is a tuple.
            return Ok(if items.len() == 1 {
                items.pop().expect("checked length")
            } else {
                Type::Tuple(items)
            });
        }

        let (remaining, ident) = parse_identifier(rest).map_err(|_| ParseError {
            message: "Expected a type".to_string(),
            position: self.pos,
        })?;
        self.pos += rest.len() - remaining.len();
        let name = match ident {
            Expr::Ident(n, _) => n,
            other => {
                return Err(ParseError {
                    message: format!("Expected a type name, got {}", other),
                    position: self.pos,
                })
            }
        };

        // A lowercase name is a type variable (`a -> a`), universally quantified.
        // The same name within one annotation is the same variable.
        if !name.starts_with(|c: char| c.is_uppercase()) {
            return Ok(Type::TypeVar(self.annotation_var(name)));
        }

        // Applied type: `List(I64)`, `Try(a, b)`.
        let mut args = Vec::new();
        if self.input[self.pos..].starts_with('(') {
            self.pos += 1;
            loop {
                self.skip_inline_whitespace();
                if self.input[self.pos..].starts_with(')') {
                    self.pos += 1;
                    break;
                }
                args.push(self.parse_type_operand()?);
                self.skip_inline_whitespace();
                let rest = &self.input[self.pos..];
                if rest.starts_with(',') {
                    self.pos += 1;
                    // A trailing comma before `)` is legal here too.
                    self.skip_whitespace();
                    if self.input[self.pos..].starts_with(')') {
                        self.pos += 1;
                        break;
                    }
                } else if rest.starts_with(')') {
                    self.pos += 1;
                    break;
                } else {
                    return Err(ParseError {
                        message: format!("Expected ',' or ')' in {}(...)", name),
                        position: self.pos,
                    });
                }
            }
        }

        // A declared nominal wins over the fallback: `Point` is the nominal, not an
        // anonymous variable.
        if let Some(nominal) = self.nominal(name) {
            if args.is_empty() {
                return Ok(nominal);
            }
            // `Wrapper(Str)` — put the arguments in place of the declared parameters.
            if let Some((_, params)) = self.nominal_params.iter().find(|(n, _)| n == name) {
                let pairs: Vec<(u32, Type)> =
                    params.iter().copied().zip(args.iter().cloned()).collect();
                return Ok(substitute_type_vars(&nominal, &pairs));
            }
        }

        Ok(named_type(name, args, || Type::TypeVar(self.next_type_var_unchecked())))
    }

    /// Parse `{ x: I64, y: Str }`, or `{}` for unit.
    ///
    /// A record type may span LINES and carry comments between its fields, which is how
    /// roc writes anything wider than a couple of fields. The braces bound it, so
    /// crossing a newline here cannot run into the next declaration.
    fn parse_record_type(&mut self) -> Result<Type, ParseError> {
        self.pos += 1; // Skip '{'
        let mut fields: Vec<(String, Type)> = Vec::new();
        let mut open = false;

        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];
            if rest.starts_with('}') {
                self.pos += 1;
                break;
            }
            // `{ name: Str, .. }` or `{ name: Str, ..r }` — either spelling opens the
            // record. The name in `..r` ties two positions to the same leftovers,
            // which nothing here needs: an open record is open either way.
            if rest.starts_with("..") {
                open = true;
                self.pos += 2;
                let rest = &self.input[self.pos..];
                if let Ok((remaining, _)) = parse_identifier(rest) {
                    self.pos += rest.len() - remaining.len();
                }
                continue;
            }
            let (remaining, ident) = parse_identifier(rest)?;
            self.pos += rest.len() - remaining.len();
            let field = match ident {
                Expr::Ident(n, _) => n.to_string(),
                other => {
                    return Err(ParseError {
                        message: format!("Expected a field name in a record type, got {}", other),
                        position: self.pos,
                    })
                }
            };
            self.skip_whitespace();

            // `name ?: Type` declares an OPTIONAL field: it may be absent, and is read
            // with `.?name`, which yields a Try. Only allowed on a nominal's backing
            // record.
            let optional = self.input[self.pos..].starts_with("?:");
            if optional {
                self.pos += 2;
            } else if self.input[self.pos..].starts_with(':') {
                self.pos += 1;
            } else {
                return Err(ParseError {
                    message: format!("Expected ':' after record field '{}'", field),
                    position: self.pos,
                });
            }

            let field_type = self.parse_type_operand()?;
            self.skip_whitespace();

            // `name : Type ?? default` declares a DEFAULTED field: omitting it at
            // construction substitutes the default, so it is always present when read
            // and needs no unwrapping.
            if self.input[self.pos..].starts_with("??") {
                self.pos += 2;
                self.skip_whitespace();
                let default = self.parse_or_expr()?;
                self.field_defaults.push((field.clone(), default));
            }
            if optional {
                self.optional_fields.push(field.clone());
                fields.push((field, Type::Optional(Box::new(field_type))));
            } else {
                fields.push((field, field_type));
            }

            self.skip_whitespace();
            let rest = &self.input[self.pos..];
            if rest.starts_with(',') {
                self.pos += 1;
            } else if !rest.starts_with('}') {
                return Err(ParseError {
                    message: "Expected ',' or '}' in a record type".to_string(),
                    position: self.pos,
                });
            }
        }

        // `{}` is unit, not an empty record type. `{ .. }` is a record of anything.
        if fields.is_empty() && !open {
            return Ok(Type::Unit);
        }
        // Sorted, so field order in the annotation does not affect unification.
        fields.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(Type::Record { fields, open })
    }

    /// Parse `[Red, Green]`, `[Foo(I64, Str), Bar]`, `[Exit(I8), ..]`.
    ///
    /// May span LINES — roc writes a union of more than a few tags one per line. The
    /// brackets bound it, so crossing a newline cannot run into the next declaration.
    ///
    /// A trailing `..` marks the union OPEN: more tags may be added, and a `match` on
    /// it needs a wildcard. Without it the union is closed.
    fn parse_tag_union_type(&mut self) -> Result<Type, ParseError> {
        self.pos += 1; // Skip '['
        let mut tags: Vec<(String, Vec<Type>)> = Vec::new();
        let mut open = false;

        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];
            if rest.starts_with(']') {
                self.pos += 1;
                break;
            }
            if rest.starts_with("..") {
                self.pos += 2;
                open = true;
                self.skip_whitespace();
                if self.input[self.pos..].starts_with(']') {
                    self.pos += 1;
                    break;
                }
                continue;
            }

            let (remaining, ident) = parse_identifier(rest)?;
            self.pos += rest.len() - remaining.len();
            let tag = match ident {
                Expr::Ident(n, _) => n.to_string(),
                other => {
                    return Err(ParseError {
                        message: format!("Expected a tag name, got {}", other),
                        position: self.pos,
                    })
                }
            };

            let mut payload = Vec::new();
            if self.input[self.pos..].starts_with('(') {
                self.pos += 1;
                loop {
                    self.skip_whitespace();
                    if self.input[self.pos..].starts_with(')') {
                        self.pos += 1;
                        break;
                    }
                    payload.push(self.parse_type_operand()?);
                    self.skip_whitespace();
                    let rest = &self.input[self.pos..];
                    if rest.starts_with(',') {
                        self.pos += 1;
                    } else if rest.starts_with(')') {
                        self.pos += 1;
                        break;
                    } else {
                        return Err(ParseError {
                            message: format!("Expected ',' or ')' in tag {}", tag),
                            position: self.pos,
                        });
                    }
                }
            }
            tags.push((tag, payload));

            self.skip_whitespace();
            let rest = &self.input[self.pos..];
            if rest.starts_with(',') {
                self.pos += 1;
            } else if !rest.starts_with(']') {
                return Err(ParseError {
                    message: "Expected ',' or ']' in a tag union type".to_string(),
                    position: self.pos,
                });
            }
        }

        tags.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(Type::TagUnion { tags, open })
    }

    /// Skip spaces and tabs but NOT newlines.
    ///
    /// An annotation ends at its line break, so a type parser that skipped newlines
    /// would run on into the binding below it.
    fn skip_inline_whitespace(&mut self) {
        while let Some(c) = self.input[self.pos..].chars().next() {
            if c == ' ' || c == '\t' {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
    }

    /// The variable id for a named type variable in the annotation being parsed.
    ///
    /// Reused for a repeated name, so `a, a -> a` ties all three positions together.
    fn annotation_var(&mut self, name: &str) -> u32 {
        if let Some((_, id)) = self.annotation_vars.iter().find(|(n, _)| n == name) {
            return *id;
        }
        let id = self.fresh_type_var();
        self.annotation_vars.push((name.to_string(), id));
        id
    }

    /// Allocate a type variable id for an annotation's generic parameter.
    fn fresh_type_var(&mut self) -> u32 {
        self.next_type_var += 1;
        self.next_type_var
    }

    /// Same, for use where `&mut self` is already borrowed.
    fn next_type_var_unchecked(&mut self) -> u32 {
        self.fresh_type_var()
    }

    /// Skip whitespace, comments and type-annotation lines.
    ///
    /// Called wherever a declaration may appear: the top-level header loop and the
    /// top-level binding chain both need it, and a missed annotation there silently
    /// truncates the chain (the binding after it never gets parsed).
    fn skip_trivia(&mut self) {
        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];
            if rest.starts_with('#') {
                self.skip_to_line_end();
                continue;
            }
            if self.capture_nominal_declaration() {
                continue;
            }
            if self.skip_type_annotation() {
                continue;
            }
            break;
        }
    }

    /// Skip a standalone type annotation line such as `main! : List(Str) => Try(...)`.
    ///
    /// Annotations survive desugaring so the emitted .roc stays valid Roc, but the
    /// interpreter infers types itself, so they are not parsed. Returns whether one
    /// was consumed.
    fn skip_type_annotation(&mut self) -> bool {
        self.capture_type_annotation()
    }

    /// Add a nominal's defaulted fields to a construction that omitted them.
    ///
    /// `Cfg.{ host: "a" }` for `Cfg := { host: Str, port: U16 ?? 8080 }` becomes
    /// `{ host: "a", port: 8080 }`. Fields the construction supplied are left alone.
    fn fill_defaults(&self, type_name: &str, built: Expr) -> Expr {
        let Some((_, defaults)) = self.nominal_defaults.iter().find(|(n, _)| n == type_name)
        else {
            return built;
        };
        let Expr::Record(mut fields, _) = built else {
            // `Name.{}` parses as unit; a nominal with defaults still gets them.
            if matches!(built, Expr::Unit(_)) {
                return Expr::Record(
                    defaults
                        .iter()
                        .map(|(f, d)| (leak_field(f), d.clone()))
                        .collect(),
                    self.node(),
                );
            }
            return built;
        };

        for (field, default) in defaults {
            if !fields.iter().any(|(name, _)| name == field) {
                fields.push((leak_field(field), default.clone()));
            }
        }
        Expr::Record(fields, self.node())
    }

    /// Finish a tag expression whose name has been consumed, reading any payload.
    ///
    /// Shared by bare tags (`Ok(x)`) and nominal-qualified ones (`Animal.Dog(x)`),
    /// which build the same value.
    fn finish_tag(&mut self, name: &'static str) -> Result<Expr, ParseError> {
        let mut args = Vec::new();
        if self.input[self.pos..].starts_with('(') {
            self.pos += 1; // Skip '('
            self.skip_whitespace();
            if self.input[self.pos..].starts_with(')') {
                self.pos += 1;
            } else {
                loop {
                    args.push(self.parse_or_expr()?);
                    self.skip_whitespace();
                    let rest = &self.input[self.pos..];
                    if rest.starts_with(',') {
                        self.pos += 1;
                        self.skip_whitespace();
                    } else if rest.starts_with(')') {
                        self.pos += 1;
                        break;
                    } else {
                        return Err(ParseError {
                            message: "Expected ',' or ')' in tag arguments".to_string(),
                            position: self.pos,
                        });
                    }
                }
            }
        }
        self.skip_whitespace();
        Ok(Expr::Tag { id: self.node(), name, args })
    }

    /// Read a nominal type declaration: `Name := backing`.
    ///
    /// `Name :: backing` (opaque) is accepted as the same thing. Opacity only matters
    /// across module boundaries, which the interpreter does not have, so treating the
    /// two alike is honest rather than lazy — within one file roc does not distinguish
    /// them either (both allow field access and both accept the plain backing value).
    ///
    /// A trailing `.{ ... }` method block is skipped: methods need static dispatch,
    /// which is a later phase.
    fn capture_nominal_declaration(&mut self) -> bool {
        let rest = &self.input[self.pos..];
        let line_end = rest.find('\n').unwrap_or(rest.len());
        let line = &rest[..line_end];

        // `Name :=` / `Name ::` — the name must be capitalised, like every type.
        let assign = match line.find(":=").or_else(|| line.find("::")) {
            Some(i) => i,
            None => return false,
        };
        // `Wrapper(a) := ...` parameterises the nominal. The parameters need no
        // record of their own: the backing type's `a` becomes a fresh type variable
        // the same way a lowercase name in any annotation does.
        let declared = line[..assign].trim();
        let name = match declared.find('(') {
            Some(i) if declared.ends_with(')') => declared[..i].trim(),
            _ => declared,
        };
        if name.is_empty()
            || !name.starts_with(|c: char| c.is_uppercase())
            || !name.chars().all(|c| c.is_alphanumeric() || c == '_')
        {
            return false;
        }

        // Copied out before the mutable parse calls below end `rest`'s borrow.
        let name_owned: &'static str = Box::leak(name.to_string().into_boxed_str());
        let name = name.to_string();
        let line_start = self.pos;
        self.pos += assign + 2;

        // The backing type may span lines (a record written out field per line), so
        // parse it rather than assuming it ends at the newline.
        self.annotation_vars.clear();
        self.field_defaults.clear();
        self.optional_fields.clear();
        // Give each parameter its id BEFORE the backing type is parsed, so the `a` in
        // `{ item: a }` resolves to the same variable the parameter list declared.
        let param_names: Vec<String> = match declared.find('(') {
            Some(i) if declared.ends_with(')') => declared[i + 1..declared.len() - 1]
                .split(',')
                .map(|p| p.trim().to_string())
                .collect(),
            _ => Vec::new(),
        };
        let params: Vec<u32> =
            param_names.iter().map(|p| self.annotation_var(p)).collect();
        if !params.is_empty() {
            self.nominal_params.push((name.clone(), params));
        }
        match self.parse_type_operand() {
            Ok(backing) => {
                self.nominals.push((
                    name_owned,
                    Type::Nominal { name: name.clone(), backing: Box::new(backing) },
                ));
                // Defaults belong to THIS nominal; clear the scratch list so the next
                // declaration starts empty.
                let defaults = std::mem::take(&mut self.field_defaults);
                self.optional_fields.clear();
                if !defaults.is_empty() {
                    self.nominal_defaults.push((name.clone(), defaults));
                }
                self.parse_method_block(&name);
                true
            }
            Err(_) => {
                // Not something the type parser understands; leave it to be skipped.
                self.pos = line_start + line_end;
                true
            }
        }
    }

    /// Parse a `.{ ... }` method block after a nominal declaration.
    ///
    /// The block holds ordinary bindings — `reveal = |s| s.key` — which become
    /// functions namespaced under the type. They are recorded as `Type.method` and
    /// wrapped around the program by `parse_expr`, so `Secret.reveal` is an ordinary
    /// name lookup and `s.reveal()` is a dispatch that finds it.
    ///
    /// A method's annotation sits inside the block (`show : Counter -> Str`), and is
    /// claimed along with the binding — without it the method's parameter has no type,
    /// so `|c| c.n.to_str()` cannot dispatch on `c.n`.
    fn parse_method_block(&mut self, type_name: &str) {
        self.skip_inline_whitespace();
        if !self.input[self.pos..].starts_with(".{") {
            return;
        }
        self.pos += 2;

        loop {
            self.skip_trivia();
            let rest = &self.input[self.pos..];
            if rest.is_empty() {
                return;
            }
            if rest.starts_with('}') {
                self.pos += 1;
                return;
            }

            // `name = value`
            let Ok((remaining, ident)) = parse_identifier(rest) else {
                // Not a binding; step over it rather than spinning.
                self.pos += 1;
                continue;
            };
            let consumed = rest.len() - remaining.len();
            let after = rest[consumed..].trim_start();
            if !after.starts_with('=') || after.starts_with("==") || after.starts_with("=>") {
                self.pos += consumed.max(1);
                continue;
            }
            let Expr::Ident(method, _) = ident else {
                self.pos += consumed.max(1);
                continue;
            };

            self.pos += consumed;
            self.skip_whitespace();
            self.pos += 1; // Skip '='
            self.skip_whitespace();

            // Claimed before parsing the value: `skip_trivia` above already read the
            // annotation line into `pending_annotations`.
            let annotation = self.claim_annotation(method);

            match self.parse_or_expr() {
                Ok(value) => {
                    let qualified: &'static str =
                        Box::leak(format!("{}.{}", type_name, method).into_boxed_str());
                    self.methods.push((qualified, annotation, value));
                }
                Err(_) => return,
            }
        }
    }

    /// Method names any `where` clause in the file promised.
    pub fn where_methods(&self) -> Vec<String> {
        self.where_methods.clone()
    }

    /// Look up a nominal type by name.
    fn nominal(&self, name: &str) -> Option<Type> {
        self.nominals.iter().find(|(n, _)| *n == name).map(|(_, t)| t.clone())
    }

    /// Read a standalone annotation line, remembering its parsed type.
    ///
    /// Replaces the old skip-and-forget. The type is stashed in `pending_annotations`
    /// and claimed by the next binding of the same name; an annotation whose binding
    /// never appears is simply dropped, which is what roc allows too.
    ///
    /// A type the parser cannot make sense of is still skipped rather than raising —
    /// an annotation is documentation to the interpreter, and refusing the file over
    /// one would be worse than ignoring it. `roc check` is the authority on validity.
    fn capture_type_annotation(&mut self) -> bool {
        let rest = &self.input[self.pos..];
        let line_end = rest.find('\n').unwrap_or(rest.len());
        let line = rest[..line_end].trim();

        let colon = match line.find(':') {
            Some(c) => c,
            None => return false,
        };
        // `Module.name` and a trailing `!` are both legal in the name position.
        //
        // A PARAMETERISED alias — `Parser(a) : List(Str) -> Try(a, ...)` — puts its
        // type variables in parentheses after the name, exactly as the nominal form
        // does. The parameters need no record of their own: a lowercase name in the
        // aliased type already becomes a type variable.
        let declared = line[..colon].trim();
        let (name, alias_params): (String, Vec<String>) = match declared.find('(') {
            Some(i) if declared.ends_with(')') => (
                declared[..i].trim().to_string(),
                declared[i + 1..declared.len() - 1]
                    .split(',')
                    .map(|p| p.trim().to_string())
                    .collect(),
            ),
            _ => (declared.to_string(), Vec::new()),
        };
        let name = name.as_str();
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '!')
        {
            return false;
        }
        // `x :: y` is not an annotation.
        if line[colon + 1..].starts_with(':') {
            return false;
        }
        // A record field is `name: value` with no space before the colon; an
        // annotation is `name : Type`. Requiring the space keeps record literals
        // out of this path.
        if !line[..colon].ends_with(char::is_whitespace) {
            return false;
        }

        // `show : a -> Str where [a.to_str : a -> Str]` constrains the type variable.
        // The constraint is the compiler's to verify — all the interpreter takes from
        // it is permission to dispatch those methods on an unresolved variable.
        // The clause may sit on the signature's own line or on the next one.
        let clause_region: String = self.input[self.pos..].chars().take(400).collect();
        let promised: Vec<String> = match clause_region.find("where [").map(|i| {
            let tail = &clause_region[i..];
            tail.find(']').map(|end| &tail[..end]).unwrap_or(tail)
        }) {
            Some(clause) => clause
                .split(',')
                .filter_map(|c| {
                    let method = c.split(':').next()?.trim().trim_start_matches('[');
                    let (_, method) = method.rsplit_once('.')?;
                    let method = method.trim_end_matches("()");
                    (!method.is_empty()).then(|| method.to_string())
                })
                .collect(),
            None => Vec::new(),
        };

        // Consume `name :` then parse the type, stopping at the line break.
        let name_owned: &'static str = Box::leak(name.to_string().into_boxed_str());
        let line_start = self.pos;
        self.pos += colon + 1;
        // Each annotation has its own type variables: the `a` in one signature is
        // unrelated to the `a` in the next.
        self.annotation_vars.clear();
        // Give the parameters their ids first, so the aliased type's `a` is the same
        // variable the parameter list declared.
        let params: Vec<u32> =
            alias_params.iter().map(|p| self.annotation_var(p)).collect();
        self.where_methods.extend(promised);
        match self.parse_type() {
            Ok(ty) => {
                // `Bytes : List(U8)` is a TYPE ALIAS, not a value annotation — a
                // capitalised name has no binding to claim it. An alias is
                // transparent, so it is stored as the aliased type itself rather than
                // wrapped in `Nominal`: `Bytes` and `List(U8)` are the same type.
                if name_owned.starts_with(|c: char| c.is_uppercase()) {
                    self.nominals.push((name_owned, ty));
                    if !params.is_empty() {
                        self.nominal_params.push((name.to_string(), params));
                    }
                } else {
                    self.pending_annotations.push((name_owned, ty));
                }
                // The annotation ends at EOL — unless the type itself spanned lines,
                // as a record type written one field per line does. Whichever ran
                // further is the real end.
                self.pos = self.pos.max(line_start + line_end);
                // A `where` clause may sit on its own continuation line, after the
                // signature. Its constraints were already read off the text; what is
                // left is to step over them so they are not parsed as code.
                self.consume_where_clause();
            }
            Err(_) => self.pos = line_start + line_end,
        }
        true
    }

    /// Step over a `where [...]` clause that follows a signature, wherever it sits.
    ///
    /// The clause is the compiler's to verify; the interpreter has already taken the
    /// method names it promises. Written on its own line it would otherwise be parsed
    /// as an expression, and `where` is not one.
    fn consume_where_clause(&mut self) {
        let saved = self.pos;
        self.skip_whitespace();
        if !starts_with_keyword(&self.input[self.pos..], "where") {
            self.pos = saved;
            return;
        }
        self.pos += "where".len();
        self.skip_whitespace();
        if !self.input[self.pos..].starts_with('[') {
            self.pos = saved;
            return;
        }
        // Bracket-counted, since a constraint's own type may hold `[...]`.
        let mut depth = 0usize;
        while self.pos < self.input.len() {
            match self.input[self.pos..].chars().next() {
                Some('[') => depth += 1,
                Some(']') => {
                    depth -= 1;
                    if depth == 0 {
                        self.pos += 1;
                        return;
                    }
                }
                _ => {}
            }
            self.pos += self.input[self.pos..].chars().next().map_or(1, |c| c.len_utf8());
        }
    }

    /// Take the pending annotation for `name`, if one was declared.
    fn claim_annotation(&mut self, name: &str) -> Option<Type> {
        let index = self.pending_annotations.iter().position(|(n, _)| *n == name)?;
        Some(self.pending_annotations.remove(index).1)
    }

    /// Is `name` bound at the start of a line (column 0)? Used to detect the
    /// implicit entry point of a headerless platformless app.
    fn has_top_level_binding(&self, name: &str) -> bool {
        self.input.lines().any(|line| {
            line.strip_prefix(name)
                .map(|after| after.trim_start().starts_with('='))
                .unwrap_or(false)
        })
    }

    /// Extract app entry point from declaration like: app [main!] { ... }
    /// Returns whether the header was consumed in full (dependency map included).
    ///
    /// When it was, the caller must NOT also skip to the next line: the cursor already
    /// sits on the line after the header, and skipping again swallowed the first
    /// `import`.
    fn extract_app_entry_point(&mut self) -> bool {
        self.pos += 4; // Skip "app "
        self.skip_whitespace();

        let rest = &self.input[self.pos..];
        if rest.starts_with('[') {
            self.pos += 1; // Skip '['
            self.skip_whitespace();

            let rest = &self.input[self.pos..];
            // Find the identifier (entry point), including trailing '!'
            let mut end = 0;
            for (i, ch) in rest.chars().enumerate() {
                if ch == ']' || ch.is_whitespace() {
                    end = i;
                    break;
                }
                if i == rest.len() - 1 {
                    end = rest.len();
                }
            }

            if end > 0 {
                let entry_point = rest[..end].to_string();
                self.entry_point = Some(entry_point);
                self.pos += end;
            }
        }

        // `app [main!] { cli: platform "URL", pkg: "URL", roc: "version" }`
        self.skip_whitespace();
        if self.input[self.pos..].starts_with(']') {
            self.pos += 1;
            self.skip_whitespace();
        }
        if self.input[self.pos..].starts_with('{') {
            self.parse_dependencies();
            return true;
        }
        false
    }

    /// Parse the app header's dependency map.
    ///
    /// Entries look like `alias: platform "URL"` or `alias: "URL"`. The `roc:` entry
    /// pins the compiler version rather than naming something to fetch, so it is
    /// recorded like any other and filtered out by the loader (its spec is not an
    /// archive URL, so it resolves to nothing).
    fn parse_dependencies(&mut self) {
        self.pos += 1; // Skip '{'

        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];
            if rest.is_empty() || rest.starts_with('}') {
                if !rest.is_empty() {
                    self.pos += 1;
                }
                self.skip_whitespace();
                return;
            }
            if rest.starts_with(',') || rest.starts_with('#') {
                if rest.starts_with('#') {
                    self.skip_to_line_end();
                } else {
                    self.pos += 1;
                }
                continue;
            }

            // alias
            let Ok((remaining, ident)) = parse_identifier(rest) else {
                // Something unexpected; skip a character rather than spinning.
                self.pos += 1;
                continue;
            };
            self.pos += rest.len() - remaining.len();
            let alias = match ident {
                Expr::Ident(name, _) => name.to_string(),
                _ => continue,
            };

            self.skip_whitespace();
            if !self.input[self.pos..].starts_with(':') {
                continue;
            }
            self.pos += 1;
            self.skip_whitespace();

            // optional `platform` keyword
            let is_platform = starts_with_keyword(&self.input[self.pos..], "platform");
            if is_platform {
                self.pos += "platform".len();
                self.skip_whitespace();
            }

            // the quoted spec
            let rest = &self.input[self.pos..];
            if !rest.starts_with('"') {
                continue;
            }
            let Some(close) = rest[1..].find('"') else {
                return;
            };
            let spec = rest[1..1 + close].to_string();
            self.pos += close + 2;

            self.dependencies.push((alias, spec, is_platform));
        }
    }

    /// Dependencies declared in the app header.
    pub fn dependencies(&self) -> &[(String, String, bool)] {
        &self.dependencies
    }

    /// Modules brought in by `import`, as `(dependency alias, module)`.
    ///
    /// `import cli.Stdout` gives `("cli", "Stdout")`; a bare `import Module` gives
    /// `("", "Module")`, meaning a module beside the app rather than one from a
    /// dependency.
    pub fn imports(&self) -> &[(String, String)] {
        &self.imports
    }

    /// Local modules imported, as `(module path, names it exposes)`.
    pub fn local_modules(&self) -> &[(String, Vec<String>)] {
        &self.local_modules
    }

    /// Files ingested by `import "path" as name`, as `(binding name, path)`.
    pub fn ingests(&self) -> &[(String, String)] {
        &self.ingests
    }

    /// Note an `import` line. The cursor stays put; the caller skips the line.
    fn record_import(&mut self) {
        let rest = &self.input["import ".len() + self.pos..];
        let line = rest.lines().next().unwrap_or("");
        // `import cli.Stdout exposing [x]` — only the module path is needed here.
        let path = line.split_whitespace().next().unwrap_or("").trim();
        if path.is_empty() {
            return;
        }

        // `import "sample.txt" as sample : Str` INGESTS a file: the binding is the
        // file's contents, not a module. The quotes are what say so.
        if let Some(file) = path.strip_prefix('"').and_then(|p| p.strip_suffix('"')) {
            let name = line
                .split_whitespace()
                .skip_while(|w| *w != "as")
                .nth(1)
                .unwrap_or("")
                .trim_end_matches(':');
            if !name.is_empty() {
                self.ingests.push((name.to_string(), file.to_string()));
            }
            return;
        }
        match path.split_once('.') {
            Some((alias, module)) => {
                self.imports.push((alias.to_string(), module.to_string()))
            }
            None => {
                // A module beside the app. `exposing [a, b]` says which of its names
                // become usable unqualified; the rest stay behind `Module.name`.
                let exposed = line
                    .split_once('[')
                    .and_then(|(_, rest)| rest.split_once(']'))
                    .map(|(names, _)| {
                        names
                            .split(',')
                            .map(|n| n.trim().to_string())
                            .filter(|n| !n.is_empty())
                            .collect()
                    })
                    .unwrap_or_default();
                self.local_modules.push((path.to_string(), exposed));
                self.imports.push((String::new(), path.to_string()));
            }
        }
    }

    /// Skip until next declaration or expression
    fn skip_to_next_declaration(&mut self) {
        while self.pos < self.input.len() {
            let rest = &self.input[self.pos..];
            if rest.starts_with('\n') {
                self.pos += 1;
                self.skip_whitespace();
                return;
            }
            self.pos += 1;
        }
    }

    /// Skip to end of line
    fn skip_to_line_end(&mut self) {
        while self.pos < self.input.len() && self.input.as_bytes()[self.pos] != b'\n' {
            self.pos += 1;
        }
    }

    /// Parse let binding or regular expression
    fn parse_let_or_expr(&mut self) -> Result<Expr, ParseError> {
        self.skip_trivia();

        let rest = &self.input[self.pos..];
        if rest.is_empty() {
            return Err(ParseError {
                message: "Unexpected end of input".to_string(),
                position: self.pos,
            });
        }

        // Check for "let" keyword
        if rest.starts_with("let ") {
            self.pos += 4; // Skip "let "
            self.skip_whitespace();

            // Parse variable name
            let rest = &self.input[self.pos..];
            let (remaining, name_expr) = parse_identifier(rest)?;
            self.pos += rest.len() - remaining.len();

            let name = match name_expr {
                Expr::Ident(n, _) => n,
                _ => unreachable!(),
            };

            self.skip_whitespace();

            // Expect "="
            let rest = &self.input[self.pos..];
            if !rest.starts_with('=') {
                return Err(ParseError {
                    message: "Expected '=' after variable name in let binding".to_string(),
                    position: self.pos,
                });
            }
            self.pos += 1;
            self.skip_whitespace();

            // Parse value expression
            let value = Box::new(self.parse_primary_expr()?);

            self.skip_whitespace();

            // Expect "in"
            let rest = &self.input[self.pos..];
            if !rest.starts_with("in") {
                return Err(ParseError {
                    message: "Expected 'in' after value in let binding".to_string(),
                    position: self.pos,
                });
            }
            // Make sure "in" is followed by whitespace or end of input
            let after_in = &rest[2..];
            if !after_in.is_empty() && !after_in.starts_with(|c: char| c.is_whitespace()) {
                return Err(ParseError {
                    message: "Expected whitespace or end of input after 'in'".to_string(),
                    position: self.pos + 2,
                });
            }
            self.pos += 2; // Skip "in"
            self.skip_whitespace();

            // Parse body expression
            let body = Box::new(self.parse_let_or_expr()?);

            Ok(Expr::Let { id: self.node(), name, annotation: None, value, body })
        } else {
            // Top-level destructuring: `(a, b) = value`, then the rest of the file.
            // Same shape as inside a block, and likewise a one-arm match.
            if rest.starts_with('(') || rest.starts_with('{') {
                let saved = self.pos;
                let opens_paren = rest.starts_with('(');
                let parsed = if opens_paren {
                    self.parse_tuple_pattern()
                } else {
                    self.parse_record_pattern()
                };
                let destructured = match parsed {
                    Ok(pattern) => {
                        self.skip_whitespace();
                        let after = &self.input[self.pos..];
                        if after.starts_with('=')
                            && !after.starts_with("==")
                            && !after.starts_with("=>")
                        {
                            Some(pattern)
                        } else {
                            self.pos = saved;
                            None
                        }
                    }
                    Err(_) => {
                        self.pos = saved;
                        None
                    }
                };

                if let Some(pattern) = destructured {
                    self.pos += 1; // Skip '='
                    self.skip_whitespace();
                    let value = self.parse_or_expr()?;
                    self.skip_trivia();

                    if self.input[self.pos..].is_empty() {
                        return Err(ParseError {
                            message: "A destructuring binding needs something after it: \
                                      it binds names rather than producing a value"
                                .to_string(),
                            position: self.pos,
                        });
                    }
                    let body = self.parse_let_or_expr()?;
                    return self.destructure_at_top_level(pattern, value, body);
                }
            }

            // Check for top-level binding: name = expr
            // This is similar to let but at file level
            let lookahead_rest = &self.input[self.pos..];
            if let Ok((remaining, expr)) = parse_identifier(lookahead_rest) {
                let lookahead_pos = lookahead_rest.len() - remaining.len();
                let after_ident = &lookahead_rest[lookahead_pos..].trim_start();

                if after_ident.starts_with('=') && !after_ident.starts_with("==") {
                    // This is a binding!
                    if let Expr::Ident(name, _) = expr {
                        self.pos += lookahead_pos;
                        self.skip_whitespace();
                        self.pos += 1; // Skip '='
                        self.skip_whitespace();
                        let annotation = self.claim_annotation(name);

                        // Parse value. This must go through the full operator
                        // precedence chain, not just `parse_call_expr`: a top-level
                        // binding can be any expression (`a = 2 + (3 * 4)`), exactly
                        // like a binding inside a block.
                        let value = Box::new(self.parse_or_expr()?);

                        self.skip_whitespace();

                        // Check if there's more content
                        let rest3 = &self.input[self.pos..];
                        if rest3.is_empty() {
                            // Nothing follows, so the file's value is this binding.
                            // The body refers to the name rather than cloning the
                            // value: cloning doubled the AST and made the evaluator
                            // build the value twice, discarding the second copy.
                            return Ok(Expr::Let { id: self.node(),
                                name,
                                annotation,
                                value,
                                body: Box::new(Expr::Ident(name, self.node())),
                            });
                        } else {
                            // Continue parsing the rest of the chain.
                            self.skip_trivia();
                            if self.input[self.pos..].is_empty() {
                                // Trailing annotations/comments only: this binding is
                                // the last one, so its value is the file's value.
                                return Ok(Expr::Let { id: self.node(),
                                    name,
                                    annotation,
                                    value,
                                    body: Box::new(Expr::Ident(name, self.node())),
                                });
                            }
                            let body = Box::new(self.parse_let_or_expr()?);
                            return Ok(Expr::Let { id: self.node(), name, annotation, value, body });
                        }
                    }
                }
            }

            self.parse_or_expr()
        }
    }

    /// Parse the file's statements, in sequence.
    ///
    /// `parse_let_or_expr` stops at the first statement that is not a binding, because
    /// a binding carries the rest of the file as its body while a bare expression has
    /// nowhere to put it. At FILE scope there is always more that might follow — a
    /// module of `expect`s is nothing but bare expressions — so anything left over is
    /// parsed and sequenced after. Not done inside `parse_let_or_expr` itself: a
    /// lambda body without braces goes through it too, and would swallow the rest of
    /// the file.
    fn parse_top_level(&mut self) -> Result<Expr, ParseError> {
        let value = self.parse_let_or_expr()?;
        // A top-level test waits for the whole file, so it is set aside here and put
        // back at the end by `parse_expr`.
        if self.expr_depth == 1 && matches!(value, Expr::Expect(_, _)) {
            self.deferred_expects.push(value);
            self.skip_trivia();
            if self.input[self.pos..].is_empty() {
                return Ok(Expr::Unit(self.node()));
            }
            return self.parse_top_level();
        }
        // Only the OUTERMOST parse owns the rest of the file. A braceless lambda body
        // re-enters `parse_expr`, and sequencing there would make `|n| n + 1` swallow
        // every declaration after it.
        if self.expr_depth > 1 {
            return Ok(value);
        }
        self.skip_trivia();
        if self.input[self.pos..].is_empty() {
            return Ok(value);
        }
        Ok(Expr::Let { id: self.node(),
            name: "_",
            annotation: None,
            value: Box::new(value),
            body: Box::new(self.parse_top_level()?),
        })
    }

    /// Parse `expr ?? default`, the loosest operator.
    ///
    /// Desugars to `match expr { Ok(v) => v, Err(_) => default }`. Purely local:
    /// unlike `?`, nothing has to move into the arm, so the rewrite happens right
    /// here rather than in the block fold.
    ///
    /// `??` binds looser than arithmetic — `x ?? 1 + 2` is `x ?? (1 + 2)`, verified
    /// against roc — which is why the right side is parsed at the or-level below.
    fn parse_or_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_or_inner()?;

        loop {
            self.skip_whitespace();
            if !self.input[self.pos..].starts_with("??") {
                break;
            }
            self.pos += 2;
            self.skip_whitespace();
            let default = self.parse_or_inner()?;

            left = Expr::Match { id: self.node(),
                scrutinee: Box::new(left),
                arms: vec![
                    MatchArm {
                        patterns: vec![Pattern::Tag {
                            name: "Ok",
                            args: vec![Pattern::Binding("v")],
                        }],
                        guard: None,
                        body: Expr::Ident("v", self.node()),
                    },
                    MatchArm {
                        patterns: vec![Pattern::Tag {
                            name: "Err",
                            args: vec![Pattern::Wildcard],
                        }],
                        guard: None,
                        body: default,
                    },
                ],
            };
        }

        Ok(left)
    }

    /// Parse logical OR: a || b
    fn parse_or_inner(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and_expr()?;

        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];

            // Roc spells these `or` / `and`; `||` and `&&` are also accepted here.
            // Both spellings happen to be two characters wide.
            let matched = (rest.starts_with("||") && !rest.starts_with("|||"))
                || starts_with_keyword(rest, "or");
            if matched {
                self.pos += 2;
                self.skip_whitespace();
                let right = self.parse_and_expr()?;
                left = Expr::BinOp { id: crate::ast::fresh_node_like(&left),
                    left: Box::new(left),
                    op: crate::ast::BinOp::Or,
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }

        Ok(left)
    }

    /// Parse logical AND: a && b
    fn parse_and_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_comparison_expr()?;

        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];

            let matched = if rest.starts_with("&&") && !rest.starts_with("&&&") {
                self.pos += 2;
                true
            } else if starts_with_keyword(rest, "and") {
                self.pos += 3;
                true
            } else {
                false
            };
            if matched {
                self.skip_whitespace();
                let right = self.parse_comparison_expr()?;
                left = Expr::BinOp { id: crate::ast::fresh_node_like(&left),
                    left: Box::new(left),
                    op: crate::ast::BinOp::And,
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }

        Ok(left)
    }

    /// Parse comparison: a == b, a < b, etc.
    fn parse_comparison_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_range_expr()?;

        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];

            let op = if rest.starts_with("==") {
                self.pos += 2;
                crate::ast::BinOp::Eq
            } else if rest.starts_with("!=") {
                self.pos += 2;
                crate::ast::BinOp::Ne
            } else if rest.starts_with("<=") {
                self.pos += 2;
                crate::ast::BinOp::Le
            } else if rest.starts_with(">=") {
                self.pos += 2;
                crate::ast::BinOp::Ge
            } else if rest.starts_with('<') && !rest.starts_with("<<") {
                self.pos += 1;
                crate::ast::BinOp::Lt
            } else if rest.starts_with('>') && !rest.starts_with(">>") {
                self.pos += 1;
                crate::ast::BinOp::Gt
            } else {
                break;
            };

            self.skip_whitespace();
            let right = self.parse_range_expr()?;
            left = Expr::BinOp { id: crate::ast::fresh_node_like(&left),
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Parse a range: `0..<5` (end excluded) or `1..=5` (end included).
    ///
    /// Non-associative — `a..<b..<c` is not a range of ranges — so this reads at most
    /// one operator and does not loop.
    fn parse_range_expr(&mut self) -> Result<Expr, ParseError> {
        let left = self.parse_additive_expr()?;
        self.skip_whitespace();

        let rest = &self.input[self.pos..];
        let inclusive = if rest.starts_with("..<") {
            false
        } else if rest.starts_with("..=") {
            true
        } else {
            return Ok(left);
        };
        self.pos += 3;
        self.skip_whitespace();

        Ok(Expr::Range { id: self.node(),
            start: Box::new(left),
            end: Box::new(self.parse_additive_expr()?),
            inclusive,
        })
    }

    /// Parse addition/subtraction: a + b, a - b
    fn parse_additive_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_multiplicative_expr()?;

        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];

            let op = if rest.starts_with('+') {
                self.pos += 1;
                crate::ast::BinOp::Add
            } else if rest.starts_with('-') && !is_next_digit(rest) {
                // A `-` with space before it and NONE after starts a unary negation,
                // not a subtraction. roc rejects `m -n` outright for this reason.
                //
                // It is not a nicety: without it, a line ending in a value followed by
                // a line starting with `-x` reads as subtraction ACROSS the newline —
                // `n = 5` then `-n` became `5 - n`.
                //
                // The gap has to be found by looking BEHIND: `parse_primary_expr`
                // consumes trailing whitespace, so by the time this loop runs the
                // newline is already gone. Same reason the call parser needs it.
                let tight_right = !rest[1..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_whitespace());
                if tight_right && self.preceded_by_whitespace() {
                    break;
                }
                self.pos += 1;
                crate::ast::BinOp::Sub
            } else {
                break;
            };

            self.skip_whitespace();
            let right = self.parse_multiplicative_expr()?;
            left = Expr::BinOp { id: crate::ast::fresh_node_like(&left),
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Parse multiplication/division: a * b, a / b
    fn parse_multiplicative_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary_expr()?;

        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];

            // `//` must be tested before `/`, or it reads as two divisions.
            let op = if rest.starts_with("//") {
                self.pos += 2;
                crate::ast::BinOp::IntDiv
            } else if rest.starts_with('*') {
                self.pos += 1;
                crate::ast::BinOp::Mul
            } else if rest.starts_with('/') {
                self.pos += 1;
                crate::ast::BinOp::Div
            } else if rest.starts_with('%') {
                self.pos += 1;
                crate::ast::BinOp::Rem
            } else {
                break;
            };

            self.skip_whitespace();
            let right = self.parse_unary_expr()?;
            left = Expr::BinOp { id: crate::ast::fresh_node_like(&left),
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Parse unary minus: `-x`.
    ///
    /// `-x` IS `x.negate()` — roc lowers it to that method, which is why negating a
    /// Str fails with "This negate method is being called on a value whose type
    /// doesn't have that method" rather than a syntax error. Building a `Dispatch`
    /// here reuses phase 20 wholesale and gives the same diagnostic for free.
    ///
    /// Looser than `|>`, so `-n |> inc` is `-(inc(n))` = -6, not `inc(-n)` = -4.
    /// Verified against roc; the two readings differ, so the numbers matter.
    ///
    /// A negative literal (`-5`) is handled by the number lexer instead, so it stays
    /// one token rather than becoming a negate call on 5.
    fn parse_unary_expr(&mut self) -> Result<Expr, ParseError> {
        self.skip_whitespace();
        let rest = &self.input[self.pos..];

        // `-` followed by a digit is a negative literal, not a negation.
        if rest.starts_with('-') && !rest[1..].starts_with(|c: char| c.is_ascii_digit()) {
            self.pos += 1;
            self.skip_whitespace();
            let operand = self.parse_unary_expr()?;
            return Ok(Expr::Dispatch { id: self.node(),
                receiver: Box::new(operand),
                method: "negate",
                args: Vec::new(),
            });
        }

        self.parse_pipe_expr()
    }

    /// Parse `x |> f`, the pipeline operator.
    ///
    /// `x |> f` is `f(x)`, and `x |> f(a)` is `f(x, a)` — the piped value is
    /// PREPENDED to whatever arguments were written, the same convention static
    /// dispatch uses. Left-associative, so `x |> f |> g` is `g(f(x))`.
    ///
    /// It binds TIGHTER than every binary operator, which is the opposite of most
    /// languages. Verified against roc: `1 + 2 |> inc` is `1 + inc(2)` = 4, and
    /// `2 * 3 |> inc` is `2 * inc(3)` = 8. That is why this level sits between the
    /// multiplicative operators and the call level rather than at the top.
    fn parse_pipe_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_call_expr()?;

        loop {
            self.skip_whitespace();
            if !self.input[self.pos..].starts_with("|>") {
                break;
            }
            self.pos += 2;
            self.skip_whitespace();

            let target = self.parse_call_expr()?;
            left = match target {
                // Already a call: the piped value joins its arguments, in front.
                Expr::Call { func, args, .. } => {
                    let mut all = Vec::with_capacity(args.len() + 1);
                    all.push(left);
                    all.extend(args);
                    Expr::Call { id: self.node(), func, args: all }
                }
                // A bare function — a name, a lambda, `Module.fn` — is applied to it.
                func => Expr::Call { id: self.node(), func: Box::new(func), args: vec![left] },
            };
        }

        Ok(left)
    }

    /// Parse function call or primary expression
    fn parse_call_expr(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary_expr()?;

        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];

            // Postfix field access: `r.x`, and chains like `r.a.b`. Applies to any
            // receiver — `f(x).field` and `{ a: 1 }.a` included — which is why it
            // lives here rather than in the identifier branch of parse_primary_expr.
            // Optional field access: `.?name`. Yields a Try rather than the value,
            // because the field may be absent.
            if rest.starts_with(".?") && rest[2..].starts_with(is_ident_start) {
                self.pos += 2;
                let rest = &self.input[self.pos..];
                let (remaining, field_expr) = parse_identifier(rest)?;
                self.pos += rest.len() - remaining.len();
                if let Expr::Ident(field, _) = field_expr {
                    expr = Expr::OptionalField { id: self.node(), record: Box::new(expr), field };
                    self.skip_whitespace();
                    continue;
                }
                return Err(ParseError {
                    message: "Expected a field name after `.?`".to_string(),
                    position: self.pos,
                });
            }

            // Positional tuple access: `.0`, `.1`. Same postfix slot as `.field`,
            // distinguished by the index being digits rather than an identifier.
            if rest.starts_with('.') && rest[1..].starts_with(|c: char| c.is_ascii_digit()) {
                self.pos += 1; // Skip '.'
                let digits: String = self.input[self.pos..]
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                self.pos += digits.len();
                let index = digits.parse::<usize>().map_err(|_| ParseError {
                    message: format!("Invalid tuple index .{}", digits),
                    position: self.pos,
                })?;
                expr = Expr::TupleIndex { id: self.node(), tuple: Box::new(expr), index };
                self.skip_whitespace();
                continue;
            }

            if rest.starts_with('.') && rest[1..].starts_with(is_ident_start) {
                self.pos += 1; // Skip '.'
                let rest = &self.input[self.pos..];
                let (remaining, field_expr) = parse_identifier(rest)?;

                // `.name(` is a method call, `.name` a field read. The parens are the
                // only difference, so the check has to happen before committing to a
                // FieldAccess. No whitespace is allowed before the `(`, same as any
                // other call.
                let after_name = &remaining[..];
                if after_name.starts_with('(') {
                    if let Expr::Ident(method, _) = field_expr {
                        self.pos += rest.len() - remaining.len();
                        let args = self.parse_call_arguments()?;
                        expr = Expr::Dispatch { id: self.node(), receiver: Box::new(expr), method, args };
                        self.skip_whitespace();
                        continue;
                    }
                }
                self.pos += rest.len() - remaining.len();
                match field_expr {
                    Expr::Ident(field, _) => {
                        expr = Expr::FieldAccess { id: self.node(), record: Box::new(expr), field };
                        self.skip_whitespace();
                        continue;
                    }
                    _ => {
                        return Err(ParseError {
                            message: "Expected a field name after '.'".to_string(),
                            position: self.pos,
                        })
                    }
                }
            }

            // Function application requires NO whitespace before the `(`: roc rejects
            // `f (1)` ("this token cannot start a statement here"). That rule is what
            // makes `if n > 0 (if n > 10 "big" else "small") else "neg"` unambiguous —
            // without it, the condition `n > 0` swallows the parenthesised
            // then-branch as a call on `0`.
            //
            // Field access does NOT share this rule: `r .x` is accepted by roc.
            if rest.starts_with('(') && !self.preceded_by_whitespace() {
                self.pos += 1; // Skip '('
                self.skip_whitespace();

                let mut args = Vec::new();

                // Parse arguments. Each argument is a full expression, not a single
                // atom: `I64.to_str(inc(41))` nests a call, and `f(a + 1)` nests an
                // operator. `parse_primary_expr` handles neither, and stops after
                // `inc`, leaving the `(` to fail the closing-paren check.
                let rest = &self.input[self.pos..];
                if !rest.starts_with(')') {
                    loop {
                        args.push(self.parse_or_expr()?);
                        self.skip_whitespace();

                        let rest = &self.input[self.pos..];
                        if rest.starts_with(',') {
                            self.pos += 1;
                            self.skip_whitespace();
                            // A trailing comma before `)` is legal.
                            if self.input[self.pos..].starts_with(')') {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }

                self.skip_whitespace();
                let rest = &self.input[self.pos..];
                if !rest.starts_with(')') {
                    return Err(ParseError {
                        message: "Expected ')' after function arguments".to_string(),
                        position: self.pos,
                    });
                }
                self.pos += 1; // Skip ')'

                expr = Expr::Call { id: self.node(),
                    func: Box::new(expr),
                    args,
                };
            } else {
                break;
            }
        }

        Ok(expr)
    }

    /// Parse primary expression: number, string, identifier, lambda
    /// Parse a multiline string: consecutive lines each beginning with `\\`.
    ///
    /// The value is the text after each `\\`, joined with newlines and with NO trailing
    /// newline. Content is RAW — `\\t` inside one is a backslash and a `t`, not a tab —
    /// but `${...}` interpolation still works, which is why the parts are built rather
    /// than the text interned whole.
    fn parse_multiline_string(&mut self) -> Result<Expr, ParseError> {
        let mut content = String::new();
        let mut first = true;

        loop {
            if !self.input[self.pos..].starts_with("\\\\") {
                break;
            }
            self.pos += 2;

            let rest = &self.input[self.pos..];
            let end = rest.find('\n').unwrap_or(rest.len());
            if !first {
                content.push('\n');
            }
            first = false;
            content.push_str(&rest[..end]);
            self.pos += end;

            // Another `\\` after the newline continues the same string; anything else
            // ends it, and the cursor stays where the string stopped.
            let resume = self.pos;
            self.pos += self.input[self.pos..].len() - self.input[self.pos..].trim_start().len();
            if !self.input[self.pos..].starts_with("\\\\") {
                self.pos = resume;
                break;
            }
        }

        self.skip_whitespace();
        if content.contains("${") {
            let parts =
                parse_interpolation_parts(&content, &self.nominals, &self.nominal_defaults)?;
            return Ok(Expr::StrInterp(parts, self.node()));
        }
        Ok(Expr::Str(string_pool::intern(&content), self.node()))
    }

    /// Parse `'a'` into the code point it denotes.
    ///
    /// The result is an `Int` because that is what roc makes of it: `'a' + 1` is 98,
    /// and a grapheme literal unifies with any number type.
    fn parse_grapheme_literal(&mut self) -> Result<Expr, ParseError> {
        let open = self.pos;
        self.pos += 1; // opening quote

        let rest = &self.input[self.pos..];
        let (ch, width) = match rest.chars().next() {
            Some('\\') => {
                let escape = rest[1..].chars().next().ok_or_else(|| ParseError {
                    message: "Unclosed grapheme literal".to_string(),
                    position: open,
                })?;
                match escape {
                    'n' => ('\n', 2),
                    't' => ('\t', 2),
                    'r' => ('\r', 2),
                    '\\' => ('\\', 2),
                    '\'' => ('\'', 2),
                    '"' => ('"', 2),
                    // `'\u(e9)'`, the same escape strings use.
                    'u' if rest[2..].starts_with('(') => {
                        let close = rest.find(')').ok_or_else(|| ParseError {
                            message: "Unclosed \\u( escape".to_string(),
                            position: open,
                        })?;
                        let hex = &rest[3..close];
                        let c = u32::from_str_radix(hex, 16)
                            .ok()
                            .and_then(char::from_u32)
                            .ok_or_else(|| ParseError {
                                message: format!("Invalid unicode escape \\u({})", hex),
                                position: open,
                            })?;
                        (c, close + 1)
                    }
                    other => {
                        return Err(ParseError {
                            message: format!("Unknown escape \\{} in a grapheme literal", other),
                            position: open,
                        })
                    }
                }
            }
            Some(c) => (c, c.len_utf8()),
            None => {
                return Err(ParseError {
                    message: "Unclosed grapheme literal".to_string(),
                    position: open,
                })
            }
        };

        self.pos += width;
        if !self.input[self.pos..].starts_with('\'') {
            return Err(ParseError {
                message: "A grapheme literal holds exactly one character".to_string(),
                position: open,
            });
        }
        self.pos += 1;

        Ok(Expr::Int(ch as i64, self.node()))
    }

    fn parse_primary_expr(&mut self) -> Result<Expr, ParseError> {
        self.skip_whitespace();
        // Where this expression starts. Most nodes are built after their text has been
        // consumed, so `self.pos` by then points PAST them; a literal parsed by one of
        // the free functions does not know its position at all. Stamping the start here
        // fixes both, and every composite node inherits it from its first child.
        let start = self.pos;
        let parsed = self.parse_primary_inner();
        if let Ok(expr) = &parsed {
            crate::ast::relocate(expr.id(), start);
        }
        parsed
    }

    fn parse_primary_inner(&mut self) -> Result<Expr, ParseError> {
        self.skip_whitespace();

        let rest = &self.input[self.pos..];
        if rest.is_empty() {
            return Err(ParseError {
                message: "Unexpected end of input".to_string(),
                position: self.pos,
            });
        }

        // A multiline string: `\\line` continued over consecutive lines.
        if rest.starts_with("\\\\") {
            return self.parse_multiline_string();
        }

        // Grapheme literal: `'a'` is the number 97, not a one-character Str. roc has
        // no character type — the literal is a number literal spelled visually.
        if rest.starts_with('\'') {
            return self.parse_grapheme_literal();
        }

        // `match scrutinee { ... }`. Before the identifier branch, like `if`.
        if starts_with_keyword(rest, "match") {
            return self.parse_match();
        }

        // Statement keywords. Each takes one operand, so they parse like a prefix
        // operator rather than needing a statement position of their own.
        if starts_with_keyword(rest, "return") {
            self.pos += 6;
            self.skip_whitespace();
            return Ok(Expr::Return(Box::new(self.parse_or_expr()?), self.node()));
        }
        if starts_with_keyword(rest, "crash") {
            self.pos += 5;
            self.skip_whitespace();
            return Ok(Expr::Crash(Box::new(self.parse_or_expr()?), self.node()));
        }
        if starts_with_keyword(rest, "expect") {
            self.pos += 6;
            self.skip_whitespace();
            return Ok(Expr::Expect(Box::new(self.parse_or_expr()?), self.node()));
        }
        if starts_with_keyword(rest, "dbg") {
            self.pos += 3;
            self.skip_whitespace();
            return Ok(Expr::Dbg(Box::new(self.parse_or_expr()?), self.node()));
        }

        // Loops are EXPRESSIONS whose value is `{}`, so they may be bound
        // (`y = for n in xs { ... }`) as well as used as statements. roc allows both.
        if starts_with_keyword(rest, "for") {
            return self.parse_for();
        }
        if starts_with_keyword(rest, "while") {
            return self.parse_while();
        }
        if starts_with_keyword(rest, "break") {
            self.pos += 5;
            self.skip_whitespace();
            return Ok(Expr::Break(self.node()));
        }

        // `if cond then else otherwise`. Checked before the identifier branch, or
        // `if` would parse as a variable named "if".
        if starts_with_keyword(rest, "if") {
            return self.parse_if();
        }

        // Prefix `!` is logical not. Unrelated to the `!` that ends an effectful
        // name: that one is part of the identifier and never appears in front.
        // Canonicalises to `Bool.not(x)`, matching the upstream compiler.
        if rest.starts_with('!') && !rest.starts_with("!=") {
            self.pos += 1;
            self.skip_whitespace();
            let operand = self.parse_call_expr()?;
            return Ok(Expr::Call { id: self.node(),
                func: Box::new(Expr::Qualified { id: self.node(), module: "Bool", name: "not" }),
                args: vec![operand],
            });
        }

        // List literal: `[1, 2, 3]`, `[]`.
        if rest.starts_with('[') {
            return self.parse_list();
        }

        // `{` starts either a record literal or a block.
        if rest.starts_with('{') {
            return self.parse_braced();
        }

        // `(` opens either a grouped expression or a tuple. A comma decides:
        // `(1)` is grouping, `(1, 2)` is a tuple. roc has no one-tuple.
        if rest.starts_with('(') {
            self.pos += 1;
            self.skip_whitespace();
            let first = self.parse_or_expr()?;
            self.skip_whitespace();

            if self.input[self.pos..].starts_with(',') {
                let mut items = vec![first];
                while self.input[self.pos..].starts_with(',') {
                    self.pos += 1;
                    self.skip_whitespace();
                    // A trailing comma before `)` is allowed.
                    if self.input[self.pos..].starts_with(')') {
                        break;
                    }
                    items.push(self.parse_or_expr()?);
                    self.skip_whitespace();
                }
                if !self.input[self.pos..].starts_with(')') {
                    return Err(ParseError {
                        message: "Expected ')' to close tuple".to_string(),
                        position: self.pos,
                    });
                }
                self.pos += 1;
                self.skip_whitespace();
                return Ok(Expr::Tuple(items, self.node()));
            }

            if !self.input[self.pos..].starts_with(')') {
                return Err(ParseError {
                    message: "Expected ')' to close parenthesised expression".to_string(),
                    position: self.pos,
                });
            }
            self.pos += 1;
            self.skip_whitespace();
            return Ok(first);
        }

        // Try lambda first: |x| body or |x, y| body
        if rest.starts_with('|') {
            return self.parse_lambda();
        }

        // Try number
        if let Ok((remaining, expr)) = parse_number_literal(rest) {
            self.pos += rest.len() - remaining.len();
            self.skip_whitespace();
            return Ok(expr);
        }

        // Try string
        if rest.starts_with('"') {
            return self.parse_string();
        }

        // Try identifier or qualified name
        if let Some(first_char) = rest.chars().next() {
            if is_ident_start(first_char) {
                if let Ok((remaining, expr)) = parse_identifier(rest) {
                    self.pos += rest.len() - remaining.len();

                    // `x.y` is either a module member or a record field access.
                    // Roc capitalises modules and types, so the receiver's case
                    // decides: `Str.inspect` is a module member, `point.x` a field.
                    if let Expr::Ident(base, _) = expr {
                        if base.starts_with(|c: char| c.is_uppercase()) {
                            // `Name.{ ... }` builds a nominal from its backing record.
                            // The nominal name is erased in the value, matching roc:
                            // `Str.inspect` on one shows the bare record.
                            if self.input[self.pos..].starts_with(".{") {
                                self.pos += 1; // Skip '.', leaving the '{'
                                let built = self.parse_braced()?;
                                // Omitting a defaulted field substitutes its default,
                                // so the record always has it — which is why a
                                // defaulted field needs no unwrapping to read.
                                return Ok(self.fill_defaults(base, built));
                            }
                            // `Name.(payload)` builds a nominal over a non-record
                            // payload: `UserId.(7)` is 7, `Pair.(1, "two")` is the
                            // tuple. Like `.{ }`, the nominal name is erased — roc
                            // inspects both as the bare payload. Skipping the `.`
                            // leaves the `(`, which already parses as grouping or a
                            // tuple depending on the comma.
                            if self.input[self.pos..].starts_with(".(") {
                                self.pos += 1;
                                return self.parse_primary_expr();
                            }
                            if let Some(qualified) = self.try_parse_qualified(base) {
                                // `Animal.Dog(x)` is the tag `Dog(x)`: the
                                // qualification says which nominal it belongs to, and
                                // carries no runtime weight.
                                if let Expr::Qualified { module, name, .. } = qualified {
                                    if self.nominal(module).is_some()
                                        && name.starts_with(|c: char| c.is_uppercase())
                                    {
                                        return self.finish_tag(name);
                                    }
                                }
                                return Ok(qualified);
                            }
                        }
                    }
                    // Lowercase `x.y` is a field access, handled as a postfix
                    // operator in `parse_call_expr`.

                    // A capitalised identifier that is not `Module.name` is a tag:
                    // `Ok(x)`, `Err(e)`, or a bare tag like `Red`.
                    if let Expr::Ident(name, _) = expr {
                        if name.starts_with(|c: char| c.is_uppercase()) {
                            return self.finish_tag(name);
                        }
                    }

                    self.skip_whitespace();
                    return Ok(expr);
                }
            }
        }

        // Fallback to string parsing for error message
        self.parse_string()
    }

    /// Parse `Module.name` after an uppercase identifier, if a `.name` follows.
    fn try_parse_qualified(&mut self, module: &'static str) -> Option<Expr> {
        let rest = &self.input[self.pos..];
        if !rest.starts_with('.') || !rest[1..].starts_with(is_ident_start) {
            return None;
        }
        self.pos += 1; // Skip '.'
        let rest = &self.input[self.pos..];
        let (remaining, name_expr) = parse_identifier(rest).ok()?;
        self.pos += rest.len() - remaining.len();
        match name_expr {
            Expr::Ident(name, _) => {
                self.skip_whitespace();
                Some(Expr::Qualified { id: self.node(), module, name })
            }
            _ => None,
        }
    }

    /// Is the character immediately before the cursor whitespace?
    ///
    /// Used to tell function application (`f(1)`) from a grouped expression that
    /// merely follows something (`n > 0 (…)`). `parse_primary_expr` consumes
    /// trailing whitespace, so the distinction has to be recovered by looking back.
    ///
    /// ponytail: a look-behind, not a token stream. A real lexer would carry
    /// adjacency on each token and this would be a field check. Worth doing if more
    /// constructs come to depend on spacing.
    fn preceded_by_whitespace(&self) -> bool {
        self.input[..self.pos]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_whitespace())
    }

    /// Build the `match` that `name = value?` desugars to.
    ///
    /// ```text
    /// match value {
    ///     Ok(name) => continuation
    ///     Err(e) => Err(e)
    /// }
    /// ```
    ///
    /// ponytail: the error is bound to the fixed name `e`, so a `?` whose
    /// continuation refers to an outer `e` would see the error instead. A gensym
    /// would fix it; needs the parser to carry a counter, and no test has hit it.
    /// Build the `match` that `?` desugars to.
    ///
    /// `ok` is the pattern the unwrapped value is bound by — a plain name for
    /// `x = e?`, or a whole destructuring pattern for `{ a, b } = e?`.
    ///
    /// `mapper`, when given, is the lambda from `e ? |err| Replacement(err)`: the error
    /// is rewritten before it propagates, which is how a caller turns a builtin's
    /// error into one of its own.
    fn propagate_error_pattern(
        ok: Pattern,
        value: Expr,
        continuation: Expr,
        mapper: Option<Expr>,
    ) -> Expr {
        let propagated = match mapper {
            Some(map) => Expr::Tag { id: crate::ast::fresh_node_unlocated(),
                name: "Err",
                args: vec![Expr::Call { id: crate::ast::fresh_node_unlocated(),
                    func: Box::new(map),
                    args: vec![Expr::Ident("e", crate::ast::fresh_node_unlocated())],
                }],
            },
            None => Expr::Tag { id: crate::ast::fresh_node_unlocated(), name: "Err", args: vec![Expr::Ident("e", crate::ast::fresh_node_unlocated())] },
        };

        Expr::Match { id: crate::ast::fresh_node_unlocated(),
            scrutinee: Box::new(value),
            arms: vec![
                MatchArm { patterns: vec![ok], guard: None, body: continuation },
                MatchArm {
                    patterns: vec![Pattern::Tag {
                        name: "Err",
                        args: vec![Pattern::Binding("e")],
                    }],
                    guard: None,
                    body: propagated,
                },
            ],
        }
    }

    /// Parse `match scrutinee { pattern => body ... }`.
    ///
    /// Arms are separated by newlines; a comma between them is allowed but not
    /// required. Arms are kept in source order because matching stops at the first
    /// one that succeeds.
    fn parse_match(&mut self) -> Result<Expr, ParseError> {
        self.pos += 5; // Skip "match"
        self.skip_whitespace();

        // The scrutinee stops on its own at the `{`, since nothing in an expression
        // can continue into a brace.
        let scrutinee = Box::new(self.parse_or_expr()?);
        self.skip_whitespace();

        if !self.input[self.pos..].starts_with('{') {
            return Err(ParseError {
                message: "Expected '{' after the match scrutinee".to_string(),
                position: self.pos,
            });
        }
        self.pos += 1; // Skip '{'

        let mut arms = Vec::new();
        loop {
            self.skip_trivia();
            let rest = &self.input[self.pos..];
            if rest.is_empty() {
                return Err(ParseError {
                    message: "Unexpected end of input inside match; expected '}'".to_string(),
                    position: self.pos,
                });
            }
            if rest.starts_with('}') {
                self.pos += 1;
                self.skip_whitespace();
                break;
            }

            arms.push(self.parse_match_arm()?);

            // A comma between arms is optional.
            self.skip_whitespace();
            if self.input[self.pos..].starts_with(',') {
                self.pos += 1;
            }
        }

        if arms.is_empty() {
            return Err(ParseError {
                message: "A match needs at least one arm".to_string(),
                position: self.pos,
            });
        }

        Ok(Expr::Match { id: self.node(), scrutinee, arms })
    }

    /// Parse one arm: `A | B if guard => body`.
    fn parse_match_arm(&mut self) -> Result<MatchArm, ParseError> {
        let mut patterns = vec![self.parse_pattern()?];

        // Alternatives: `A | B | C`. Careful not to eat `||`.
        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];
            // `|>` is the pipeline operator, not an alternative separator.
            if rest.starts_with('|') && !rest.starts_with("||") && !rest.starts_with("|>") {
                self.pos += 1;
                self.skip_whitespace();
                patterns.push(self.parse_pattern()?);
            } else {
                break;
            }
        }

        // Optional guard, evaluated with the pattern's bindings in scope.
        self.skip_whitespace();
        let guard = if starts_with_keyword(&self.input[self.pos..], "if") {
            self.pos += 2;
            self.skip_whitespace();
            Some(self.parse_or_expr()?)
        } else {
            None
        };

        self.skip_whitespace();
        if !self.input[self.pos..].starts_with("=>") {
            return Err(ParseError {
                message: "Expected '=>' after a match pattern".to_string(),
                position: self.pos,
            });
        }
        self.pos += 2; // Skip "=>"
        self.skip_whitespace();

        let body = self.parse_or_expr()?;
        Ok(MatchArm { patterns, guard, body })
    }

    /// Parse a single pattern.
    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        self.skip_whitespace();
        let rest = &self.input[self.pos..];

        // `_` is the wildcard. `_name` is an ordinary binding that documents being
        // unused, so only a bare `_` counts.
        if rest.starts_with('_') && !rest[1..].starts_with(is_ident_char) {
            self.pos += 1;
            self.skip_whitespace();
            return Ok(Pattern::Wildcard);
        }

        if rest.starts_with('(') {
            return self.parse_tuple_pattern();
        }

        if rest.starts_with('[') {
            return self.parse_list_pattern();
        }

        // A bare record pattern — `|{ x, y }|`, or a `match` arm. Previously only the
        // nominal spelling `Name.{ ... }` reached the record-pattern parser, so
        // destructuring a plain record in a parameter was a parse error.
        if rest.starts_with('{') {
            return self.parse_record_pattern();
        }

        if rest.starts_with('"') {
            return match self.parse_string()? {
                Expr::Str(s, _) => Ok(Pattern::Str(s)),
                other => Err(ParseError {
                    message: format!("Only plain strings may be patterns, got {}", other),
                    position: self.pos,
                }),
            };
        }

        if let Ok((remaining, expr)) = parse_number_literal(rest) {
            self.pos += rest.len() - remaining.len();
            self.skip_whitespace();
            return match expr {
                Expr::Int(n, _) => Ok(Pattern::Int(n)),
                Expr::Float(n, _) => Ok(Pattern::Float(n)),
                other => Err(ParseError {
                    message: format!("Unsupported numeric pattern: {}", other),
                    position: self.pos,
                }),
            };
        }

        if let Ok((remaining, ident)) = parse_identifier(rest) {
            self.pos += rest.len() - remaining.len();
            let name = match ident {
                Expr::Ident(n, _) => n,
                other => {
                    return Err(ParseError {
                        message: format!("Unsupported pattern: {}", other),
                        position: self.pos,
                    })
                }
            };

            // `Name.{ x, y }` destructures a nominal's backing record, and
            // `Name.Tag(p)` matches a nominal union's tag.
            if name.starts_with(|c: char| c.is_uppercase()) {
                if self.input[self.pos..].starts_with(".{") {
                    self.pos += 1; // Skip '.', leaving the '{'
                    return self.parse_record_pattern();
                }
                // `Name.(payload)` unwraps a nominal over a NON-record backing, the
                // pattern counterpart of the `Name.(x)` constructor. The nominal is
                // erased at runtime, so the pattern is just its payload's.
                if self.input[self.pos..].starts_with(".(") {
                    self.pos += 2; // Skip '.('
                    let inner = self.parse_pattern()?;
                    self.skip_whitespace();
                    // `Pair.(a, b)` unwraps a tuple backing.
                    if self.input[self.pos..].starts_with(',') {
                        let mut items = vec![inner];
                        while self.input[self.pos..].starts_with(',') {
                            self.pos += 1;
                            self.skip_whitespace();
                            if self.input[self.pos..].starts_with(')') {
                                break;
                            }
                            items.push(self.parse_pattern()?);
                            self.skip_whitespace();
                        }
                        if self.input[self.pos..].starts_with(')') {
                            self.pos += 1;
                            self.skip_whitespace();
                        }
                        return Ok(Pattern::Tuple(items));
                    }
                    if !self.input[self.pos..].starts_with(')') {
                        return Err(ParseError {
                            message: format!("Expected ')' to close `{}.(`", name),
                            position: self.pos,
                        });
                    }
                    self.pos += 1;
                    self.skip_whitespace();
                    return Ok(inner);
                }
                if self.input[self.pos..].starts_with('.')
                    && self.input[self.pos + 1..].starts_with(|c: char| c.is_uppercase())
                {
                    self.pos += 1; // Skip '.'
                    let rest = &self.input[self.pos..];
                    let (leftover, tag_ident) = parse_identifier(rest)?;
                    self.pos += rest.len() - leftover.len();
                    if let Expr::Ident(tag, _) = tag_ident {
                        return self.finish_tag_pattern(tag);
                    }
                }
            }

            // Capitalised means a tag, lowercase means a binding — the same rule the
            // expression parser uses to tell `Red` from a variable.
            if name.starts_with(|c: char| c.is_uppercase()) {
                return self.finish_tag_pattern(name);
            }

            self.skip_whitespace();
            return Ok(Pattern::Binding(name));
        }

        Err(ParseError {
            message: "Expected a pattern".to_string(),
            position: self.pos,
        })
    }

    /// Parse `if cond a else b`.
    ///
    /// The condition needs no parentheses and the branches no braces. There is no
    /// ambiguity about where the condition ends because Roc applies functions with
    /// `f(x)`, never by juxtaposition — so the expression parser stops on its own
    /// once the then-branch begins.
    ///
    /// `else` is mandatory: `if` is an expression, and roc rejects a bare `if` with
    /// "The second branch of this if does not match the previous branch".
    ///
    /// `else if` is parsed by recursing, which builds an `If` whose `otherwise` is
    /// another `If` — there is no separate else-if node.
    fn parse_if(&mut self) -> Result<Expr, ParseError> {
        self.pos += 2; // Skip "if"
        self.skip_whitespace();

        let condition = Box::new(self.parse_or_expr()?);
        self.skip_whitespace();

        let then_branch = Box::new(self.parse_or_expr()?);
        self.skip_whitespace();

        if !starts_with_keyword(&self.input[self.pos..], "else") {
            return Err(ParseError {
                message: "Expected 'else': every if must have an else branch".to_string(),
                position: self.pos,
            });
        }
        self.pos += 4; // Skip "else"
        self.skip_whitespace();

        // `else if ...` recurses; anything else is an ordinary expression.
        let otherwise = if starts_with_keyword(&self.input[self.pos..], "if") {
            Box::new(self.parse_if()?)
        } else {
            Box::new(self.parse_or_expr()?)
        };

        Ok(Expr::If { id: self.node(), condition, then_branch, otherwise })
    }

    /// Parse a list literal: `[1, 2, 3]`, `[]`. A trailing comma is allowed.
    fn parse_list(&mut self) -> Result<Expr, ParseError> {
        self.pos += 1; // Skip '['
        let mut items = Vec::new();

        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];
            if rest.is_empty() {
                return Err(ParseError {
                    message: "Unexpected end of input in list literal".to_string(),
                    position: self.pos,
                });
            }
            if rest.starts_with(']') {
                self.pos += 1;
                self.skip_whitespace();
                break;
            }

            items.push(self.parse_or_expr()?);
            self.skip_whitespace();
            let rest = &self.input[self.pos..];
            if rest.starts_with(',') {
                self.pos += 1;
            } else if !rest.starts_with(']') {
                return Err(ParseError {
                    message: "Expected ',' or ']' in list literal".to_string(),
                    position: self.pos,
                });
            }
        }

        Ok(Expr::List(items, self.node()))
    }

    /// Build a top-level `(a, b) = value` binding.
    ///
    /// Uses indexing rather than a `match`, because a top-level binding must NOT be
    /// scoped: the continuation is the rest of the file, which defines things the
    /// runner looks up afterwards (`main!` among them), and a match arm pops its
    /// scope on the way out. Inside a block a match is correct and is what is used.
    ///
    /// ```text
    /// (a, b) = pair      ->   __destructure = pair
    /// <rest>                  a = __destructure.0
    ///                         b = __destructure.1
    ///                         <rest>
    /// ```
    ///
    /// Ceiling: only bindings and wildcards are accepted here. roc also allows a
    /// refutable element (`(1, b) = pair`); that needs a match, so it is rejected
    /// with a pointer to the block form rather than silently skipping the check.
    fn destructure_at_top_level(
        &mut self,
        pattern: Pattern,
        value: Expr,
        body: Expr,
    ) -> Result<Expr, ParseError> {
        // A record destructuring binds by FIELD NAME rather than position, but is
        // otherwise the same shape, so both lower to indexing into one temporary.
        let items: Vec<(Accessor, Pattern)> = match pattern {
            Pattern::Tuple(items) => items
                .into_iter()
                .enumerate()
                .map(|(index, p)| (Accessor::Index(index), p))
                .collect(),
            Pattern::Record { fields, rest } => {
                if rest.is_some() {
                    return Err(ParseError {
                        message: "`..rest` is not supported in a TOP-LEVEL destructuring: \
                                  building the remaining record needs a match, which a \
                                  top-level binding cannot use. Destructure inside a block."
                            .to_string(),
                        position: self.pos,
                    });
                }
                fields
                    .into_iter()
                    .map(|(name, p)| (Accessor::Field(name), p))
                    .collect()
            }
            other => {
                return Err(ParseError {
                    message: format!("Unsupported top-level destructuring pattern: {}", other),
                    position: self.pos,
                })
            }
        };

        // Bind the value once; the element bindings then index into it.
        const TEMP: &str = "__destructure";
        let mut chain = body;
        for (accessor, item) in items.iter().rev() {
            let name = match item {
                Pattern::Binding(name) => *name,
                // Nothing to bind, so the element can be skipped entirely.
                Pattern::Wildcard => continue,
                other => {
                    return Err(ParseError {
                        message: format!(
                            "Only names and `_` are supported in a top-level \
                             destructuring; `{}` needs a match inside a block",
                            other
                        ),
                        position: self.pos,
                    })
                }
            };
            let value = match accessor {
                Accessor::Index(index) => Expr::TupleIndex { id: crate::ast::fresh_node_unlocated(),
                    tuple: Box::new(Expr::Ident(TEMP, crate::ast::fresh_node_unlocated())),
                    index: *index,
                },
                Accessor::Field(field) => Expr::FieldAccess { id: crate::ast::fresh_node_unlocated(),
                    record: Box::new(Expr::Ident(TEMP, crate::ast::fresh_node_unlocated())),
                    field,
                },
            };
            chain = Expr::Let { id: crate::ast::fresh_node_unlocated(),
                name,
                annotation: None,
                value: Box::new(value),
                body: Box::new(chain),
            };
        }

        Ok(Expr::Let { id: crate::ast::fresh_node_unlocated(),
            name: TEMP,
            annotation: None,
            value: Box::new(value),
            body: Box::new(chain),
        })
    }

    /// Finish a tag pattern whose name has been consumed, reading any payload.
    ///
    /// Shared by bare tags (`Foo(a, b)`) and nominal-qualified ones
    /// (`Animal.Dog(name)`), which match the same value.
    fn finish_tag_pattern(&mut self, name: &'static str) -> Result<Pattern, ParseError> {
        let mut args = Vec::new();
        // Payload patterns nest: `Wrap(Inner(s))`.
        if self.input[self.pos..].starts_with('(') {
            self.pos += 1;
            loop {
                self.skip_whitespace();
                if self.input[self.pos..].starts_with(')') {
                    self.pos += 1;
                    break;
                }
                args.push(self.parse_pattern()?);
                self.skip_whitespace();
                let rest = &self.input[self.pos..];
                if rest.starts_with(',') {
                    self.pos += 1;
                } else if rest.starts_with(')') {
                    self.pos += 1;
                    break;
                } else {
                    return Err(ParseError {
                        message: "Expected ',' or ')' in a tag pattern".to_string(),
                        position: self.pos,
                    });
                }
            }
        }
        self.skip_whitespace();
        Ok(Pattern::Tag { name, args })
    }

    /// Parse a record pattern: `{ x, y }` or `{ x: 0, y }`.
    ///
    /// Reached through a nominal destructuring (`Point.{ x }`). A bare field name is
    /// shorthand for binding it to itself.
    fn parse_record_pattern(&mut self) -> Result<Pattern, ParseError> {
        self.pos += 1; // Skip '{'
        let mut fields = Vec::new();
        let mut rest_binding: Option<&'static str> = None;

        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];
            if rest.is_empty() {
                return Err(ParseError {
                    message: "Unexpected end of input in record pattern".to_string(),
                    position: self.pos,
                });
            }
            if rest.starts_with('}') {
                self.pos += 1;
                self.skip_whitespace();
                break;
            }

            // `..rest` binds every field the pattern does not name, which is how a
            // field gets removed: name it `_`, capture the rest.
            if rest.starts_with("..") {
                self.pos += 2;
                self.skip_whitespace();
                let rest = &self.input[self.pos..];
                let (leftover, ident) = parse_identifier(rest)?;
                self.pos += rest.len() - leftover.len();
                match ident {
                    Expr::Ident(name, _) => rest_binding = Some(name),
                    other => {
                        return Err(ParseError {
                            message: format!("Expected a name after `..`, got {}", other),
                            position: self.pos,
                        })
                    }
                }
                self.skip_whitespace();
                if self.input[self.pos..].starts_with(',') {
                    self.pos += 1;
                }
                continue;
            }

            let (leftover, ident) = parse_identifier(rest)?;
            self.pos += rest.len() - leftover.len();
            let field = match ident {
                Expr::Ident(n, _) => n,
                other => {
                    return Err(ParseError {
                        message: format!("Expected a field name in a record pattern, got {}", other),
                        position: self.pos,
                    })
                }
            };

            self.skip_whitespace();
            let pattern = if self.input[self.pos..].starts_with(':') {
                self.pos += 1;
                self.parse_pattern()?
            } else {
                // `{ x }` means `{ x: x }`.
                Pattern::Binding(field)
            };
            fields.push((field, pattern));

            self.skip_whitespace();
            let rest = &self.input[self.pos..];
            if rest.starts_with(',') {
                self.pos += 1;
            } else if !rest.starts_with('}') {
                return Err(ParseError {
                    message: "Expected ',' or '}' in a record pattern".to_string(),
                    position: self.pos,
                });
            }
        }

        Ok(Pattern::Record { fields, rest: rest_binding })
    }

    /// Parse a tuple pattern: `(0, 0)`, `(x, 0)`. Fixed arity, element-wise.
    fn parse_tuple_pattern(&mut self) -> Result<Pattern, ParseError> {
        self.pos += 1; // Skip '('
        let mut items = Vec::new();

        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];
            if rest.is_empty() {
                return Err(ParseError {
                    message: "Unexpected end of input in tuple pattern".to_string(),
                    position: self.pos,
                });
            }
            if rest.starts_with(')') {
                self.pos += 1;
                self.skip_whitespace();
                break;
            }

            items.push(self.parse_pattern()?);
            self.skip_whitespace();
            let rest = &self.input[self.pos..];
            if rest.starts_with(',') {
                self.pos += 1;
            } else if !rest.starts_with(')') {
                return Err(ParseError {
                    message: "Expected ',' or ')' in tuple pattern".to_string(),
                    position: self.pos,
                });
            }
        }

        Ok(Pattern::Tuple(items))
    }

    /// Parse a list pattern: `[]`, `[a, b]`, `[1, 2, ..]`, `[2, .., 1]`,
    /// `[9, .. as tail]`.
    ///
    /// At most one `..`. Patterns before it match from the front, patterns after it
    /// from the back, and `.. as name` binds the skipped middle as a list.
    fn parse_list_pattern(&mut self) -> Result<Pattern, ParseError> {
        self.pos += 1; // Skip '['
        let mut before = Vec::new();
        let mut after = Vec::new();
        let mut rest_binding: Option<Option<&'static str>> = None;

        loop {
            self.skip_whitespace();
            let remaining = &self.input[self.pos..];
            if remaining.is_empty() {
                return Err(ParseError {
                    message: "Unexpected end of input in list pattern".to_string(),
                    position: self.pos,
                });
            }
            if remaining.starts_with(']') {
                self.pos += 1;
                self.skip_whitespace();
                break;
            }

            // `..` or `.. as name`. Checked before the element parser, since `.` is
            // not the start of any pattern.
            if remaining.starts_with("..") {
                if rest_binding.is_some() {
                    return Err(ParseError {
                        message: "A list pattern may contain at most one `..`".to_string(),
                        position: self.pos,
                    });
                }
                self.pos += 2;
                self.skip_whitespace();

                let mut name = None;
                if starts_with_keyword(&self.input[self.pos..], "as") {
                    self.pos += 2;
                    self.skip_whitespace();
                    let rest = &self.input[self.pos..];
                    let (leftover, ident) = parse_identifier(rest)?;
                    self.pos += rest.len() - leftover.len();
                    match ident {
                        Expr::Ident(n, _) => name = Some(n),
                        other => {
                            return Err(ParseError {
                                message: format!("Expected a name after `.. as`, got {}", other),
                                position: self.pos,
                            })
                        }
                    }
                }
                rest_binding = Some(name);
            } else {
                let pattern = self.parse_pattern()?;
                if rest_binding.is_some() {
                    after.push(pattern);
                } else {
                    before.push(pattern);
                }
            }

            self.skip_whitespace();
            let remaining = &self.input[self.pos..];
            if remaining.starts_with(',') {
                self.pos += 1;
            } else if !remaining.starts_with(']') {
                return Err(ParseError {
                    message: "Expected ',' or ']' in list pattern".to_string(),
                    position: self.pos,
                });
            }
        }

        Ok(Pattern::List { before, rest: rest_binding, after })
    }

    /// Parse a parenthesised argument list, cursor on the `(`.
    ///
    /// Shared by ordinary calls and by static dispatch, which differ only in what they
    /// do with the result.
    fn parse_call_arguments(&mut self) -> Result<Vec<Expr>, ParseError> {
        self.pos += 1; // Skip '('
        self.skip_whitespace();
        let mut args = Vec::new();

        if self.input[self.pos..].starts_with(')') {
            self.pos += 1;
            return Ok(args);
        }

        loop {
            args.push(self.parse_or_expr()?);
            self.skip_whitespace();
            let rest = &self.input[self.pos..];
            if rest.starts_with(',') {
                self.pos += 1;
                self.skip_whitespace();
                // A trailing comma before `)` is legal, and is how roc writes a call
                // whose arguments are spread over several lines.
                if self.input[self.pos..].starts_with(')') {
                    self.pos += 1;
                    break;
                }
            } else if rest.starts_with(')') {
                self.pos += 1;
                break;
            } else {
                return Err(ParseError {
                    message: "Expected ',' or ')' after function arguments".to_string(),
                    position: self.pos,
                });
            }
        }
        Ok(args)
    }

    /// Parse `for name in iterable { body }`.
    ///
    /// The loop's value is `{}` — it is a statement, not something you bind.
    fn parse_for(&mut self) -> Result<Expr, ParseError> {
        self.pos += 3; // Skip "for"
        self.skip_whitespace();

        let rest = &self.input[self.pos..];
        let (remaining, ident) = parse_identifier(rest)?;
        self.pos += rest.len() - remaining.len();
        let name = match ident {
            Expr::Ident(n, _) => n,
            other => {
                return Err(ParseError {
                    message: format!("Expected a loop variable after `for`, got {}", other),
                    position: self.pos,
                })
            }
        };

        self.skip_whitespace();
        if !starts_with_keyword(&self.input[self.pos..], "in") {
            return Err(ParseError {
                message: "Expected `in` after the `for` loop variable".to_string(),
                position: self.pos,
            });
        }
        self.pos += 2;
        self.skip_whitespace();

        // The iterable stops at the `{`, since nothing in an expression continues
        // into a brace.
        let iterable = Box::new(self.parse_or_expr()?);
        self.skip_whitespace();
        if !self.input[self.pos..].starts_with('{') {
            return Err(ParseError {
                message: "Expected '{' to open the `for` loop body".to_string(),
                position: self.pos,
            });
        }
        let body = Box::new(self.parse_block()?);

        Ok(Expr::For { id: self.node(), name, iterable, body })
    }

    /// Parse `while condition { body }`. Its value is `{}`.
    fn parse_while(&mut self) -> Result<Expr, ParseError> {
        self.pos += 5; // Skip "while"
        self.skip_whitespace();

        let condition = Box::new(self.parse_or_expr()?);
        self.skip_whitespace();
        if !self.input[self.pos..].starts_with('{') {
            return Err(ParseError {
                message: "Expected '{' to open the `while` loop body".to_string(),
                position: self.pos,
            });
        }
        let body = Box::new(self.parse_block()?);

        Ok(Expr::While { id: self.node(), condition, body })
    }

    /// Parse whatever the `{` at the cursor opens: a record literal or a block.
    ///
    /// Every site that accepts a braced construct must go through here. A lambda
    /// body used to call `parse_block` directly, so `|n| { v: n }` parsed the record
    /// as a block and failed on `v: n`.
    fn parse_braced(&mut self) -> Result<Expr, ParseError> {
        if self.looks_like_record() {
            self.parse_record()
        } else {
            self.parse_block()
        }
    }

    /// Does the `{` at the cursor open a record literal rather than a block?
    ///
    /// The discriminator is the same one the annotation skipper uses: a record field
    /// is `name: value` with **no** space before the colon, while a block statement
    /// that happens to be annotated is `name : Type`. A block statement otherwise
    /// starts with `name =`, or with something that is not an identifier at all.
    fn looks_like_record(&self) -> bool {
        let rest = &self.input[self.pos + 1..]; // past the '{'
        // A record written one field per line often opens with a comment, so the first
        // thing after the `{` is not necessarily the first field.
        let mut trimmed = rest.trim_start();
        while trimmed.starts_with('#') {
            trimmed = match trimmed.find('\n') {
                Some(i) => trimmed[i..].trim_start(),
                None => "",
            };
        }

        // `{}` is the unit value, handled by parse_block.
        if trimmed.starts_with('}') {
            return false;
        }
        // `{ ..base, field: value }` is a record UPDATE, not a block.
        if trimmed.starts_with("..") {
            return true;
        }
        let ident_len = trimmed
            .char_indices()
            .take_while(|(i, c)| {
                if *i == 0 { is_ident_start(*c) } else { is_ident_char(*c) }
            })
            .count();
        if ident_len == 0 {
            return false;
        }
        let after = &trimmed[ident_len..];
        // No whitespace allowed between the field name and its colon.
        if after.starts_with(':') && !after.starts_with("::") {
            return true;
        }
        // Field punning: `{ name, age }` is `{ name: name, age: age }`. A COMMA is
        // what marks it — roc reads a lone `{ x }` as a block whose value is `x`, not
        // as a one-field record.
        after.trim_start().starts_with(',')
    }

    /// Parse a record literal: `{ x: 1, y: f(2) }`.
    fn parse_record(&mut self) -> Result<Expr, ParseError> {
        self.pos += 1; // Skip '{'
        let mut fields = Vec::new();
        // `{ ..base, field: value }` is an UPDATE. roc rejects `{ base & field: v }`,
        // so this spelling is the only one.
        let mut base: Option<Expr> = None;

        loop {
            self.skip_whitespace();
            let rest = &self.input[self.pos..];
            if rest.starts_with("..") {
                self.pos += 2;
                self.skip_whitespace();
                base = Some(self.parse_or_expr()?);
                self.skip_whitespace();
                if self.input[self.pos..].starts_with(',') {
                    self.pos += 1;
                }
                continue;
            }
            if rest.starts_with('}') {
                self.pos += 1;
                self.skip_whitespace();
                break;
            }

            let (remaining, ident) = parse_identifier(rest)?;
            let name = match ident {
                Expr::Ident(n, _) => n,
                _ => {
                    return Err(ParseError {
                        message: "Expected a field name in record literal".to_string(),
                        position: self.pos,
                    })
                }
            };
            self.pos += rest.len() - remaining.len();
            self.skip_inline_whitespace();

            // Field punning: `{ name, birth_year }` is `{ name: name, ... }`. The
            // field takes the value of the binding that shares its name.
            if !self.input[self.pos..].starts_with(':') {
                let next = self.input[self.pos..].chars().next();
                if matches!(next, Some(',') | Some('}')) || next.is_none() {
                    fields.push((name, Expr::Ident(name, self.node())));
                    self.skip_whitespace();
                    if self.input[self.pos..].starts_with(',') {
                        self.pos += 1;
                        continue;
                    }
                    if self.input[self.pos..].starts_with('}') {
                        self.pos += 1;
                        self.skip_whitespace();
                        break;
                    }
                    continue;
                }
                return Err(ParseError {
                    message: format!("Expected ':' after record field '{}'", name),
                    position: self.pos,
                });
            }
            self.pos += 1; // Skip ':'
            self.skip_whitespace();

            // Field values are full expressions.
            fields.push((name, self.parse_or_expr()?));

            self.skip_whitespace();
            let rest = &self.input[self.pos..];
            if rest.starts_with(',') {
                self.pos += 1; // A trailing comma before `}` is legal.
            } else if !rest.starts_with('}') {
                return Err(ParseError {
                    message: "Expected ',' or '}' in record literal".to_string(),
                    position: self.pos,
                });
            }
        }

        Ok(match base {
            Some(base) => Expr::RecordUpdate { id: self.node(), base: Box::new(base), fields },
            None => Expr::Record(fields, self.node()),
        })
    }

    /// Parse a block `{ stmt \n stmt \n expr }` into nested `Let`s.
    ///
    /// Roc blocks are a sequence of statements ending in an expression. Rather than
    /// add a `Block` AST node, each statement becomes a `Let`: `name = value`
    /// binds `name`, and a bare statement (typically an effect call like
    /// `echo!("hi")`) binds the throwaway name `_`. The final expression is the
    /// innermost body, so the block's value is the last expression's value.
    ///
    /// `{}` is the empty record, Roc's unit value.
    fn parse_block(&mut self) -> Result<Expr, ParseError> {
        self.pos += 1; // Skip '{'
        self.skip_whitespace();

        if self.input[self.pos..].starts_with('}') {
            self.pos += 1;
            self.skip_whitespace();
            return Ok(Expr::Unit(self.node()));
        }

        // (binding target, annotation, value, uses `?`) per statement; the last is
        // the block's result.
        // `(target, annotation, value, propagates, error mapper)`.
        type Stmt = (BindTarget, Option<Type>, Expr, bool, Option<Expr>);
        let mut stmts: Vec<Stmt> = Vec::new();

        loop {
            // Blocks carry annotations and comments too (`add5 : I64 -> I64`), so the
            // same trivia rules apply here as at the top level.
            self.skip_trivia();
            let rest = &self.input[self.pos..];

            if rest.is_empty() {
                return Err(ParseError {
                    message: "Unexpected end of input inside block; expected '}'".to_string(),
                    position: self.pos,
                });
            }
            if rest.starts_with('}') {
                self.pos += 1;
                self.skip_whitespace();
                break;
            }

            // `var` is a binding form, so it has to be handled before the ordinary
            // binding check — otherwise `var x` reads as the name `var`. Loops and
            // `break` need no special case here: they are expressions, handled by
            // `parse_primary_expr`, and a statement position accepts any expression.
            if starts_with_keyword(rest, "var") {
                self.pos += 3;
                self.skip_whitespace();
                let rest = &self.input[self.pos..];
                let (remaining, ident) = parse_identifier(rest)?;
                self.pos += rest.len() - remaining.len();
                let name = match ident {
                    Expr::Ident(n, _) => n,
                    other => {
                        return Err(ParseError {
                            message: format!("Expected a name after `var`, got {}", other),
                            position: self.pos,
                        })
                    }
                };
                if !self.mutable_names.iter().any(|n| n == name) {
                    self.mutable_names.push(name.to_string());
                }
                self.skip_whitespace();
                if !self.input[self.pos..].starts_with('=') {
                    return Err(ParseError {
                        message: format!("Expected '=' after `var {}`", name),
                        position: self.pos,
                    });
                }
                self.pos += 1;
                self.skip_whitespace();
                let value = self.parse_or_expr()?;
                stmts.push((BindTarget::Var(name), None, value, false, None));
                continue;
            }

            // `name = value`, or a destructuring `(a, b) = value`.
            let mut bound: Option<BindTarget> = None;
            let mut stmt_annotation: Option<Type> = None;

            // Destructuring: try to read a pattern followed by `=`. A statement may
            // legitimately START with `(` as a grouped expression, so this backtracks
            // rather than committing.
            // Copied out so `rest`'s borrow ends before the mutable parse calls below.
            // A record destructuring `{ x, y } = r` is the same shape as a tuple one.
            let opens_paren = rest.starts_with('(');
            let opens_brace = rest.starts_with('{');
            if opens_paren || opens_brace {
                let saved = self.pos;
                let parsed = if opens_paren {
                    self.parse_tuple_pattern()
                } else {
                    self.parse_record_pattern()
                };
                if let Ok(pattern) = parsed {
                    self.skip_whitespace();
                    let after = &self.input[self.pos..];
                    if after.starts_with('=') && !after.starts_with("==") && !after.starts_with("=>")
                    {
                        self.pos += 1; // Skip '='
                        self.skip_whitespace();
                        bound = Some(BindTarget::Destructure(pattern));
                    } else {
                        self.pos = saved;
                    }
                } else {
                    self.pos = saved;
                }
            }

            if bound.is_none() {
                let rest = &self.input[self.pos..];
                if let Ok((remaining, ident)) = parse_identifier(rest) {
                    let consumed = rest.len() - remaining.len();
                    let after = rest[consumed..].trim_start();
                    if after.starts_with('=')
                        && !after.starts_with("==")
                        && !after.starts_with("=>")
                    {
                        if let Expr::Ident(name, _) = ident {
                            self.pos += consumed;
                            self.skip_whitespace();
                            self.pos += 1; // Skip '='
                            self.skip_whitespace();
                            // A name declared by `var` is mutable, so `x = e`
                            // REASSIGNS rather than shadows. Shadowing inside a loop
                            // body would be discarded when its scope pops. `$` carries
                            // no meaning of its own — it is an ordinary identifier
                            // character — but `var $sum` lands in the same set.
                            bound = Some(if self.mutable_names.iter().any(|n| n == name) {
                                BindTarget::Assign(name)
                            } else {
                                BindTarget::Name(name)
                            });
                            stmt_annotation = self.claim_annotation(name);
                        }
                    }
                }
            }

            let value = self.parse_or_expr()?;

            // Postfix `?` on a statement's value. Not `??`, which the expression
            // parser has already consumed by this point.
            let propagates = self.input[self.pos..].starts_with('?');
            let mut mapper = None;
            if propagates {
                self.pos += 1;
                self.skip_whitespace();
                // `expr ? |e| MyError(e)` REPLACES the error before propagating it.
                // Without the mapper the original error travels unchanged.
                if self.input[self.pos..].starts_with('|') {
                    mapper = Some(self.parse_lambda()?);
                }
                self.skip_whitespace();
            }

            stmts.push((
                bound.unwrap_or(BindTarget::Name("_")),
                stmt_annotation,
                value,
                propagates,
                mapper,
            ));
        }

        // Fold from the end: every statement wraps the one after it, so the
        // "rest of the block" is whatever has been folded so far.
        let (result_target, _result_annotation, result, result_propagates, _result_mapper) =
            stmts.pop().ok_or_else(|| ParseError {
            message: "Empty block body".to_string(),
            position: self.pos,
        })?;
        if result_propagates {
            // `?` unwraps, so a block ending in `expr?` evaluates to the unwrapped
            // value rather than a Try. roc rejects that against a Try return type,
            // and there is no continuation to put in the Ok arm.
            return Err(ParseError {
                message: "`?` cannot be used on the final expression of a block: it                           unwraps the value, leaving nothing to propagate into"
                    .to_string(),
                position: self.pos,
            });
        }
        // A block's value is its LAST expression — but if that last statement is an
        // assignment or a `var`, it still has to run, and the block's value is unit.
        // Dropping the target here silently discarded the mutation, so
        // `for n in xs { $sum = $sum + n }` became `for n in xs { $sum + n }` and the
        // loop did nothing.
        let mut body = match result_target {
            BindTarget::Assign(name) => Expr::Assign { id: self.node(),
                name,
                value: Box::new(result),
                body: Box::new(Expr::Unit(self.node())),
            },
            BindTarget::Var(name) => Expr::VarDecl { id: self.node(),
                name,
                value: Box::new(result),
                body: Box::new(Expr::Unit(self.node())),
            },
            BindTarget::Destructure(pattern) => Expr::Match { id: self.node(),
                scrutinee: Box::new(result),
                arms: vec![MatchArm {
                    patterns: vec![pattern],
                    guard: None,
                    body: Expr::Unit(self.node()),
                }],
            },
            BindTarget::Name(_) => result,
        };
        while let Some((target, annotation, value, propagates, mapper)) = stmts.pop() {
            // A destructuring binding is a one-arm match: `(a, b) = v` then the rest
            // is exactly `match v { (a, b) => rest }`. No new AST node needed.
            if let BindTarget::Destructure(pattern) = target {
                if propagates {
                    // `{ before: a } = expr ? |e| ...` — unwrap the Ok, then
                    // destructure what was inside it.
                    body = Self::propagate_error_pattern(
                        Pattern::Tag { name: "Ok", args: vec![pattern] },
                        value,
                        body,
                        mapper,
                    );
                    continue;
                }
                body = Expr::Match { id: self.node(),
                    scrutinee: Box::new(value),
                    arms: vec![MatchArm { patterns: vec![pattern], guard: None, body }],
                };
                continue;
            }
            let name = match target {
                BindTarget::Name(name) => name,
                BindTarget::Var(name) => {
                    body = Expr::VarDecl { id: self.node(),
                        name,
                        value: Box::new(value),
                        body: Box::new(body),
                    };
                    continue;
                }
                BindTarget::Assign(name) => {
                    body = Expr::Assign { id: self.node(),
                        name,
                        value: Box::new(value),
                        body: Box::new(body),
                    };
                    continue;
                }
                BindTarget::Destructure(_) => unreachable!("handled above"),
            };
            body = if propagates {
                // This is the whole point of `?`: the continuation moves INSIDE the
                // Ok arm, so an Err skips it entirely.
                //
                //     x = expr?        match expr {
                //     <rest>      =>       Ok(x) => <rest>
                //                          Err(e) => Err(e)
                //                      }
                Self::propagate_error_pattern(
                    Pattern::Tag {
                        name: "Ok",
                        args: vec![if name == "_" {
                            Pattern::Wildcard
                        } else {
                            Pattern::Binding(name)
                        }],
                    },
                    value,
                    body,
                    mapper,
                )
            } else {
                Expr::Let { id: self.node(),
                    name,
                    annotation,
                    value: Box::new(value),
                    body: Box::new(body),
                }
            };
        }
        Ok(body)
    }

    /// Parse lambda expression: |params| body
    fn parse_lambda(&mut self) -> Result<Expr, ParseError> {
        self.skip_whitespace();

        let rest = &self.input[self.pos..];
        if !rest.starts_with('|') {
            return Err(ParseError {
                message: "Expected '|' to start lambda".to_string(),
                position: self.pos,
            });
        }
        self.pos += 1; // Skip '|'
        self.skip_whitespace();

        let mut params: Vec<&'static str> = Vec::new();
        // Parameters that are patterns rather than plain names, paired with the
        // generated parameter they destructure.
        let mut destructured: Vec<(&'static str, Pattern)> = Vec::new();

        // Parse parameters. Each is a PATTERN: `|Point.{ x, y }|` and `|(a, b)|` are
        // both legal, not just `|name|`.
        let has_params = !self.input[self.pos..].starts_with('|');
        if has_params {
            loop {
                let pattern = match self.parse_pattern() {
                    Ok(pattern) => pattern,
                    Err(_) => break,
                };

                match pattern {
                    // A plain name is used directly, which keeps the common case's
                    // AST unchanged.
                    Pattern::Binding(name) => params.push(name),
                    other => {
                        let generated: &'static str = Box::leak(
                            format!("__param{}", params.len()).into_boxed_str(),
                        );
                        params.push(generated);
                        destructured.push((generated, other));
                    }
                }

                self.skip_whitespace();
                if self.input[self.pos..].starts_with(',') {
                    self.pos += 1;
                    self.skip_whitespace();
                } else {
                    break;
                }
            }
        }

        self.skip_whitespace();
        let rest = &self.input[self.pos..];
        if !rest.starts_with('|') {
            return Err(ParseError {
                message: "Expected '|' to end lambda parameters".to_string(),
                position: self.pos,
            });
        }
        self.pos += 1; // Skip '|'
        self.skip_whitespace();

        // Parse body. A `{ ... }` block is a primary expression; anything else
        // falls through to the normal expression parser.
        let mut body = if self.input[self.pos..].starts_with('{') {
            self.parse_braced()?
        } else {
            self.parse_expr()?
        };

        // A pattern parameter is a one-arm match on the generated name, the same shape
        // a destructuring binding uses inside a block.
        for (generated, pattern) in destructured.into_iter().rev() {
            body = Expr::Match { id: self.node(),
                scrutinee: Box::new(Expr::Ident(generated, self.node())),
                arms: vec![MatchArm { patterns: vec![pattern], guard: None, body }],
            };
        }

        Ok(Expr::Lambda { id: self.node(), params: std::rc::Rc::new(params), body: std::rc::Rc::new(body) })
    }

    /// Skip whitespace
    fn skip_whitespace(&mut self) {
        loop {
            let rest = &self.input[self.pos..];
            let trimmed = rest.trim_start();
            self.pos += rest.len() - trimmed.len();

            // A `#` comment runs to the end of the line and is trivia EVERYWHERE — a
            // comment can sit between a binding's `=` and its value, which is where
            // skipping only whitespace used to leave the parser looking at the `#`.
            if self.input[self.pos..].starts_with('#') {
                self.skip_to_line_end();
                continue;
            }
            break;
        }
    }

    /// Parse string literal: "..."
    fn parse_string(&mut self) -> Result<Expr, ParseError> {
        let rest = &self.input[self.pos..];
        // The sub-parser for each `${...}` needs the nominal declarations too, or
        // `Animal.Dog(x)` inside an interpolation parses as a qualified CALL instead of
        // a tag. Any parser state a nested expression depends on has to be passed down.
        match parse_string_literal(rest, &self.nominals, &self.nominal_defaults) {
            Ok((remaining, expr)) => {
                self.pos += rest.len() - remaining.len();
                Ok(expr)
            }
            Err(e) => Err(ParseError {
                message: e.message,
                position: self.pos + e.position,
            }),
        }
    }
}

/// Parse string literal with interpolation
fn parse_string_literal<'input>(
    input: &'input str,
    nominals: &[(&'static str, Type)],
    nominal_defaults: &[(String, Vec<(String, Expr)>)],
) -> Result<(&'input str, Expr), ParseError> {
    // Check for opening quote
    if !input.starts_with('"') {
        return Err(ParseError {
            message: "Expected '\"'".to_string(),
            position: 0,
        });
    }

    let input = &input[1..]; // Skip opening quote
    let (content, remaining) = parse_string_content(input)?;

    if !remaining.starts_with('"') {
        return Err(ParseError {
            message: "Expected closing '\"'".to_string(),
            position: input.len() - remaining.len(),
        });
    }

    let remaining = &remaining[1..]; // Skip closing quote

    if content.is_empty() {
        Ok((remaining, Expr::Str(string_pool::intern(""), crate::ast::fresh_node_unlocated())))
    } else if content.contains("${") {
        // Parse interpolation expressions
        let parts = parse_interpolation_parts(&content, nominals, nominal_defaults)?;
        Ok((remaining, Expr::StrInterp(parts, crate::ast::fresh_node_unlocated())))
    } else {
        // Plain string
        Ok((remaining, Expr::Str(string_pool::intern(&content), crate::ast::fresh_node_unlocated())))
    }
}

/// Parse string content (everything between quotes, handling escapes)
fn parse_string_content(input: &str) -> Result<(String, &str), ParseError> {
    let mut result = String::new();
    let mut pos = 0;
    let input_bytes = input.as_bytes();

    while pos < input_bytes.len() {
        // An interpolation may contain string literals of its own, as in
        //     "${render(Foo(42, "answer"))}"
        // so `${ ... }` is copied across verbatim, tracking brace depth and skipping
        // over nested strings. Without this the inner quote ends the outer string.
        if input_bytes[pos] == b'$' && pos + 1 < input_bytes.len() && input_bytes[pos + 1] == b'{' {
            result.push_str("${");
            pos += 2;

            let mut brace_depth = 1;
            while pos < input_bytes.len() && brace_depth > 0 {
                match input_bytes[pos] {
                    b'{' => {
                        brace_depth += 1;
                        result.push('{');
                        pos += 1;
                    }
                    b'}' => {
                        brace_depth -= 1;
                        result.push('}');
                        pos += 1;
                    }
                    b'"' => {
                        // Copy a nested string literal whole, honouring escapes so a
                        // `\"` inside it does not look like its terminator.
                        result.push('"');
                        pos += 1;
                        while pos < input_bytes.len() && input_bytes[pos] != b'"' {
                            if input_bytes[pos] == b'\\' && pos + 1 < input_bytes.len() {
                                result.push(input_bytes[pos] as char);
                                pos += 1;
                            }
                            let ch = input[pos..].chars().next().expect("char boundary");
                            result.push(ch);
                            pos += ch.len_utf8();
                        }
                        if pos < input_bytes.len() {
                            result.push('"');
                            pos += 1;
                        }
                    }
                    _ => {
                        let ch = input[pos..].chars().next().expect("char boundary");
                        result.push(ch);
                        pos += ch.len_utf8();
                    }
                }
            }

            if brace_depth != 0 {
                return Err(ParseError {
                    message: "Unclosed ${ in string interpolation".to_string(),
                    position: pos,
                });
            }
            continue;
        }

        match input_bytes[pos] {
            b'"' => break,
            b'\\' => {
                pos += 1;
                if pos < input_bytes.len() {
                    match input_bytes[pos] {
                        b'n' => result.push('\n'),
                        b't' => result.push('\t'),
                        b'r' => result.push('\r'),
                        b'\\' => result.push('\\'),
                        b'"' => result.push('"'),
                        // `\u(e9)` inserts a code point by its hex value.
                        b'u' if input_bytes.get(pos + 1) == Some(&b'(') => {
                            let start = pos + 2;
                            match input[start..].find(')') {
                                Some(offset) => {
                                    let hex = &input[start..start + offset];
                                    match u32::from_str_radix(hex, 16)
                                        .ok()
                                        .and_then(char::from_u32)
                                    {
                                        Some(c) => result.push(c),
                                        None => {
                                            return Err(ParseError {
                                                message: format!(
                                                    "Invalid unicode escape \\u({})",
                                                    hex
                                                ),
                                                position: pos,
                                            })
                                        }
                                    }
                                    // Past the closing paren; the loop's own `pos += 1`
                                    // steps over the final character.
                                    pos = start + offset;
                                }
                                None => {
                                    return Err(ParseError {
                                        message: "Unclosed \\u( escape".to_string(),
                                        position: pos,
                                    })
                                }
                            }
                        }
                        c => {
                            result.push('\\');
                            result.push(c as char);
                        }
                    }
                    pos += 1;
                }
            }
            // A multi-byte character must be pushed WHOLE: `b as char` reinterprets
            // one UTF-8 byte as a code point, which turned `σ` into mojibake.
            _ => {
                let ch = input[pos..].chars().next().expect("pos is a char boundary");
                result.push(ch);
                pos += ch.len_utf8();
            }
        }
    }

    Ok((result, &input[pos..]))
}

/// Check if character can start an identifier
/// Allows both lowercase and uppercase for module names
fn is_ident_start(c: char) -> bool {
    // `$` starts a mutable name. `$sum` and `sum` are DIFFERENT names in roc — the
    // sigil is part of the identifier, like the trailing `!` on an effectful one.
    c.is_ascii_alphabetic() || c == '_' || c == '$'
}

/// Check if character can be in an identifier
/// Allows both lowercase and uppercase
fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Parse number literal: decimal, hex (0xFF), octal (0o77), binary (0b1010)
/// With optional type suffixes: .U8, .I32, .F32, .Dec
/// Scan a run of digits, allowing `_` separators, and return the digits without them.
///
/// Roc permits `_` anywhere inside a number for readability — `1_000_000`,
/// `0xFF_FF`, `1_0.2_5`. Stopping at the `_` silently truncated the value: `1_000_000`
/// parsed as `1`.
fn scan_digits(bytes: &[u8], mut pos: usize, accept: impl Fn(u8) -> bool) -> (usize, String) {
    let mut digits = String::new();
    let mut last_was_digit = false;
    while pos < bytes.len() {
        let c = bytes[pos];
        if accept(c) {
            digits.push(c as char);
            last_was_digit = true;
            pos += 1;
        } else if c == b'_' && last_was_digit {
            // A separator must sit between digits, so `_1` and a trailing `_` end the
            // run rather than being absorbed.
            if pos + 1 < bytes.len() && accept(bytes[pos + 1]) {
                pos += 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    (pos, digits)
}

/// Parse a numeric literal: decimal, hex (`0xFF`), octal (`0o77`), binary (`0b1010`),
/// fractional, scientific (`1.5e3`), and type-suffixed (`255.U8`).
///
/// Digit separators are allowed throughout. Verified against roc: `1_000_000` is
/// 1000000, `1e3` is 1000, `1.5e-3` is 0.0015, `0xFF_FF` is 65535.
fn parse_number_literal(input: &str) -> Result<(&str, Expr), ParseError> {
    let bytes = input.as_bytes();
    let mut pos = 0;

    let is_negative = if pos < bytes.len() && bytes[pos] == b'-' {
        pos += 1;
        true
    } else {
        false
    };

    if pos >= bytes.len() || !bytes[pos].is_ascii_digit() {
        return Err(ParseError { message: "Expected digit".to_string(), position: 0 });
    }

    // Radix prefixes. Each yields an integer, so no fraction or exponent follows.
    if bytes[pos] == b'0' && pos + 1 < bytes.len() {
        let radix = match bytes[pos + 1] {
            b'x' | b'X' => Some((16u32, "hex")),
            b'o' | b'O' => Some((8, "octal")),
            b'b' | b'B' => Some((2, "binary")),
            _ => None,
        };
        if let Some((radix, label)) = radix {
            pos += 2;
            let (next, digits) = scan_digits(bytes, pos, |c| match radix {
                16 => c.is_ascii_hexdigit(),
                8 => (b'0'..=b'7').contains(&c),
                _ => c == b'0' || c == b'1',
            });
            if digits.is_empty() {
                return Err(ParseError {
                    message: format!("Expected a {} digit", label),
                    position: next,
                });
            }
            let value = i64::from_str_radix(&digits, radix).map_err(|_| ParseError {
                message: format!("Invalid {} number: {}", label, digits),
                position: 0,
            })?;
            let value = if is_negative { -value } else { value };
            return Ok((&input[next..], Expr::Int(value, crate::ast::fresh_node_unlocated())));
        }
    }

    // Integer part.
    let (next, mut number) = scan_digits(bytes, pos, |c| c.is_ascii_digit());
    pos = next;
    if number.is_empty() {
        return Err(ParseError { message: "Expected digit".to_string(), position: pos });
    }

    // A `.` followed by a digit is a fraction; followed by a letter it is a type
    // suffix (`255.U8`), which belongs to the integer.
    let mut is_fractional = false;
    if pos < bytes.len() && bytes[pos] == b'.' && pos + 1 < bytes.len() && bytes[pos + 1].is_ascii_digit()
    {
        is_fractional = true;
        number.push('.');
        let (next, fraction) = scan_digits(bytes, pos + 1, |c| c.is_ascii_digit());
        number.push_str(&fraction);
        pos = next;
    }

    // Exponent: `e`/`E`, an optional sign, then digits. Valid with or without a
    // fraction — `1e3` is 1000.
    if pos < bytes.len() && (bytes[pos] == b'e' || bytes[pos] == b'E') {
        let mut probe = pos + 1;
        let mut sign = String::new();
        if probe < bytes.len() && (bytes[probe] == b'+' || bytes[probe] == b'-') {
            sign.push(bytes[probe] as char);
            probe += 1;
        }
        let (next, exponent) = scan_digits(bytes, probe, |c| c.is_ascii_digit());
        // Only an exponent with digits counts; otherwise the `e` starts something else.
        if !exponent.is_empty() {
            is_fractional = true;
            number.push('e');
            number.push_str(&sign);
            number.push_str(&exponent);
            pos = next;
        }
    }

    if is_fractional {
        let value = number.parse::<f64>().map_err(|_| ParseError {
            message: format!("Invalid number: {}", number),
            position: 0,
        })?;
        let value = if is_negative { -value } else { value };

        // A fractional type suffix, e.g. `3.14.F64`.
        let remaining = &input[pos..];
        for suffix in [".F32", ".F64", ".Dec"] {
            if let Some(rest) = remaining.strip_prefix(suffix) {
                return Ok((rest, Expr::Float(value, crate::ast::fresh_node_unlocated())));
            }
        }
        return Ok((remaining, Expr::Float(value, crate::ast::fresh_node_unlocated())));
    }

    let value = number.parse::<i64>().map_err(|_| ParseError {
        message: format!("Invalid number: {}", number),
        position: 0,
    })?;
    let value = if is_negative { -value } else { value };

    // An integer type suffix, e.g. `255.U8`. The value is unchanged: the interpreter
    // keeps one integer representation, and the suffix is the annotation.
    let remaining = &input[pos..];
    if remaining.starts_with('.') {
        let after = &remaining[1..];
        for suffix in [
            "U128", "I128", "U64", "I64", "U32", "I32", "U16", "I16", "U8", "I8", "Dec",
            "F64", "F32",
        ] {
            if let Some(rest) = after.strip_prefix(suffix) {
                // Only if the suffix ends there — `255.U8x` is not a suffix.
                if !rest.starts_with(is_ident_char) {
                    return if suffix.starts_with('F') || suffix == "Dec" {
                        Ok((rest, Expr::Float(value as f64, crate::ast::fresh_node_unlocated())))
                    } else {
                        Ok((rest, Expr::Int(value, crate::ast::fresh_node_unlocated())))
                    };
                }
            }
        }
    }

    Ok((remaining, Expr::Int(value, crate::ast::fresh_node_unlocated())))
}

/// Parse identifier: x, main, birds
fn parse_identifier(input: &str) -> Result<(&str, Expr), ParseError> {
    let mut pos = 0;
    let mut chars = input.chars();

    // First character must be lowercase letter or underscore
    match chars.next() {
        Some(c) if is_ident_start(c) => pos += c.len_utf8(),
        _ => {
            return Err(ParseError {
                message: "Expected identifier start".to_string(),
                position: 0,
            })
        }
    }

    // Rest can be alphanumeric or underscore
    for c in chars {
        if is_ident_char(c) {
            pos += c.len_utf8();
        } else {
            break;
        }
    }

    // A trailing `!` is part of the identifier, not an operator: upstream's
    // `chompIdentGeneral` (roc-compiler/src/parse/tokenize.zig) chomps it, and an
    // effectful binding whose name lacks it is a warning. So `echo!` is one name.
    // Guard against `!=`, which is its own operator.
    if input[pos..].starts_with('!') && !input[pos..].starts_with("!=") {
        pos += 1;
    }

    let ident = &input[..pos];
    let remaining = &input[pos..];

    Ok((remaining, Expr::Ident(string_pool::intern(ident), crate::ast::fresh_node_unlocated())))
}

/// Parse string interpolation: "text ${expr} more"
/// Returns vector of literal strings and expressions
fn parse_interpolation_parts(
    content: &str,
    nominals: &[(&'static str, Type)],
    nominal_defaults: &[(String, Vec<(String, Expr)>)],
) -> Result<Vec<StrPart>, ParseError> {
    let mut parts = Vec::new();
    let mut current_literal = String::new();
    let mut pos = 0;
    let bytes = content.as_bytes();

    while pos < bytes.len() {
        // Look for ${
        if pos + 1 < bytes.len() && bytes[pos] == b'$' && bytes[pos + 1] == b'{' {
            // Save current literal if any
            if !current_literal.is_empty() {
                parts.push(StrPart::Literal(string_pool::intern(&current_literal)));
                current_literal.clear();
            }

            // Find matching }
            pos += 2; // Skip ${
            let expr_start = pos;
            let mut brace_depth = 1;

            while pos < bytes.len() && brace_depth > 0 {
                match bytes[pos] {
                    b'{' => brace_depth += 1,
                    b'}' => brace_depth -= 1,
                    // A `}` inside a nested string is not our closing brace.
                    b'"' => {
                        pos += 1;
                        while pos < bytes.len() && bytes[pos] != b'"' {
                            if bytes[pos] == b'\\' {
                                pos += 1;
                            }
                            pos += 1;
                        }
                    }
                    _ => {}
                }
                pos += 1;
            }

            if brace_depth != 0 {
                return Err(ParseError {
                    message: "Unclosed ${ in string interpolation".to_string(),
                    position: 0,
                });
            }

            // Parse expression
            let expr_str = &content[expr_start..pos - 1];
            let mut expr_parser = Parser::new(expr_str);
            // Every piece of parser state a nested expression depends on has to be
            // passed down. `nominals` alone was not enough: a construction inside an
            // interpolation also needs the field DEFAULTS, or its omitted fields are
            // silently left out.
            expr_parser.nominals = nominals.to_vec();
            expr_parser.nominal_defaults = nominal_defaults.to_vec();
            let expr = expr_parser.parse_expr()?;
            parts.push(StrPart::Expr(Box::leak(Box::new(expr))));
        } else {
            // Push the whole character, not one byte of it: `as char` on a byte
            // splits multi-byte UTF-8 and turns `é` into mojibake. Advancing must use
            // the character's own width for the same reason, or the next read lands
            // mid-character.
            let ch = content[pos..].chars().next().expect("pos is a char boundary");
            current_literal.push(ch);
            pos += ch.len_utf8();
        }
    }

    // Add final literal if any
    if !current_literal.is_empty() {
        parts.push(StrPart::Literal(string_pool::intern(&current_literal)));
    }

    Ok(parts)
}

/// Check if '-' is followed by a digit (negative literal) vs subtraction operator
fn is_next_digit(rest: &str) -> bool {
    if rest.len() < 2 {
        return false;
    }
    let after_minus = &rest[1..];
    after_minus.chars().next().map_or(false, |c| c.is_ascii_digit())
}

/// Does `rest` start with the bare keyword `kw`, not merely a word beginning with it?
///
/// Without the boundary check, `android` would parse as `and` followed by `roid`.
fn starts_with_keyword(rest: &str, kw: &str) -> bool {
    rest.strip_prefix(kw)
        .map(|after| after.chars().next().is_none_or(|c| !is_ident_char(c)))
        .unwrap_or(false)
}

/// What a block statement binds: a plain name, or a pattern to destructure.
///
/// Destructuring is folded into a one-arm `match`, so it needs no AST node of its own.
enum BindTarget {
    Name(&'static str),
    /// `var x = value` — a rebindable binding.
    Var(&'static str),
    /// `x = value` where `x` is an existing `var` — updates it in place.
    Assign(&'static str),
    Destructure(Pattern),
}

/// Map a capitalised type name plus arguments to a `Type`.
///
/// `Try(a, b)` becomes the tag union `[Ok(a), Err(b)]`, because in roc that is
/// literally what it is — which is what makes `main!`'s annotation checkable.
///
/// ponytail: an unrecognised name yields a fresh type variable rather than an error,
/// so an annotation mentioning a type the interpreter does not model stays harmless
/// instead of rejecting the file. Nominal types (phase 14) will need real entries.
/// Replace type variables by id, used to instantiate a parameterised nominal.
fn substitute_type_vars(ty: &Type, pairs: &[(u32, Type)]) -> Type {
    match ty {
        Type::TypeVar(id) => pairs
            .iter()
            .find(|(p, _)| p == id)
            .map(|(_, t)| t.clone())
            .unwrap_or_else(|| ty.clone()),
        Type::List(inner) => Type::List(Box::new(substitute_type_vars(inner, pairs))),
        Type::Optional(inner) => Type::Optional(Box::new(substitute_type_vars(inner, pairs))),
        Type::Nominal { name, backing } => Type::Nominal {
            name: name.clone(),
            backing: Box::new(substitute_type_vars(backing, pairs)),
        },
        Type::Record { fields, .. } => Type::closed_record(
            fields
                .iter()
                .map(|(n, t)| (n.clone(), substitute_type_vars(t, pairs)))
                .collect(),
        ),
        Type::Tuple(items) => {
            Type::Tuple(items.iter().map(|t| substitute_type_vars(t, pairs)).collect())
        }
        Type::Function(a, b) => Type::Function(
            Box::new(substitute_type_vars(a, pairs)),
            Box::new(substitute_type_vars(b, pairs)),
        ),
        Type::TagUnion { tags, open } => Type::TagUnion {
            tags: tags
                .iter()
                .map(|(n, ts)| {
                    (n.clone(), ts.iter().map(|t| substitute_type_vars(t, pairs)).collect())
                })
                .collect(),
            open: *open,
        },
        other => other.clone(),
    }
}

fn named_type(name: &str, args: Vec<Type>, mut fresh: impl FnMut() -> Type) -> Type {
    match (name, args.len()) {
        ("Str", 0) => Type::Str,
        ("Bool", 0) => Type::Bool,
        ("U8", 0) => Type::U8,
        ("U16", 0) => Type::U16,
        ("U32", 0) => Type::U32,
        ("U64", 0) => Type::U64,
        ("U128", 0) => Type::U128,
        ("I8", 0) => Type::I8,
        ("I16", 0) => Type::I16,
        ("I32", 0) => Type::I32,
        ("I64", 0) => Type::I64,
        ("I128", 0) => Type::I128,
        ("F32", 0) => Type::F32,
        ("F64", 0) => Type::F64,
        ("Dec", 0) => Type::Dec,
        ("List", 1) => Type::List(Box::new(args.into_iter().next().expect("arity 1"))),
        ("Try", 2) => {
            let mut it = args.into_iter();
            let ok = it.next().expect("arity 2");
            let err = it.next().expect("arity 2");
            // Sorted by tag name, like every other union.
            Type::TagUnion {
                tags: vec![("Err".to_string(), vec![err]), ("Ok".to_string(), vec![ok])],
                open: false,
            }
        }
        _ => fresh(),
    }
}

/// How a top-level destructuring reaches one element of its temporary.
///
/// A tuple pattern indexes by position, a record pattern by field name; both otherwise
/// lower to the same chain of bindings.
enum Accessor {
    Index(usize),
    Field(&'static str),
}

/// Leak a field name so it lives as long as the AST.
fn leak_field(name: &str) -> &'static str {
    Box::leak(name.to_string().into_boxed_str())
}
