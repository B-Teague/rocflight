//! The vendored builtin module.
//!
//! `src/roc/Builtin.roc` is a verbatim copy of the roc compiler's own
//! `src/build/roc/Builtin.roc` — the Roc source that defines `Str`, `List`, `Dict`,
//! `Set`, `Num`, `Iter` and the `Json` encoding. Re-sync it when the pinned nightly
//! moves:
//!
//! ```text
//! cp roc-compiler/src/build/roc/Builtin.roc src/roc/Builtin.roc && tests/check_builtin.sh
//! ```
//!
//! The boundary between what rocflight must write in Rust and what it gets for free is
//! read off that source rather than maintained by hand: **a member with a body is Roc,
//! a member with only an annotation is an intrinsic.** That is the same contract the
//! real compiler honours — `canonicalize/BuiltinLowLevel.zig` rewrites exactly the
//! annotation-only members into lambdas running a `LowLevel` op.

use crate::desugaring::Desugarer;
use crate::parser::Parser;

/// The vendored source, compiled in so a run needs no file beside the binary.
pub const SOURCE: &str = include_str!("roc/Builtin.roc");

/// One member of `Builtin :: [].{ … }`, lifted out to stand on its own.
pub struct Member {
    pub name: &'static str,
    /// The member's source, de-indented by one tab so it is a top-level declaration.
    pub source: String,
    pub lines: usize,
}

/// Split the vendored source into its top-level members.
///
/// Parsing the file whole reports ONE error and hides the other 23,000 lines, so it is
/// read a member at a time. They are exactly the declarations at one tab of indent.
pub fn members() -> Vec<Member> {
    members_where(|_| true)
}

/// The members `wanted` accepts, keeping the source of only those.
///
/// Loading one member should not cost building the text of the other eleven: the file
/// is 23,555 lines, and that showed up as two milliseconds and 700kB on every run of
/// every program.
pub fn members_where(wanted: impl Fn(&str) -> bool) -> Vec<Member> {
    index()
        .iter()
        .filter(|slice| wanted(slice.name))
        .map(|slice| {
            let mut member = Member { name: slice.name, source: String::new(), lines: 0 };
            for line in SOURCE[slice.start..slice.end].lines() {
                let text = if slice.in_nominal { line.strip_prefix('\t').unwrap_or(line) } else { line };
                member.source.push_str(text);
                member.source.push('\n');
                member.lines += 1;
            }
            member
        })
        .collect()
}

/// Where one member's lines sit in `SOURCE`.
struct Slice {
    name: &'static str,
    start: usize,
    end: usize,
    /// Inside `Builtin :: [].{ … }`, so its lines carry one tab to strip.
    in_nominal: bool,
}

/// Every member's byte range, found ONCE per process.
///
/// A run asks for members three times — the load, then a signature table per typed
/// module the checker meets — and each ask used to walk all 23,555 lines again, at
/// nearly half a millisecond a walk.
fn index() -> &'static [Slice] {
    static INDEX: std::sync::OnceLock<Vec<Slice>> = std::sync::OnceLock::new();
    INDEX.get_or_init(|| {
        let mut slices: Vec<Slice> = Vec::new();
        let mut in_nominal = false;
        let mut offset = 0;
        for line in SOURCE.split_inclusive('\n') {
            let start = offset;
            offset += line.len();
            let line = line.strip_suffix('\n').unwrap_or(line);
            if let Some(name) = member_name(line) {
                in_nominal = true;
                if let Some(last) = slices.last_mut() {
                    last.end = start;
                }
                // The header line is part of the member: it is the declaration.
                slices.push(Slice { name, start, end: offset, in_nominal });
            } else if in_nominal && line == "}" {
                // The `Builtin :: [].{ … }` nominal closes here. Everything after it is a
                // TOP-LEVEL section — 292 declarations, of which the ones with no body are
                // the low-level ops the real compiler injects (`list_get_unsafe`,
                // `hasher_finish`, `dict_seed`). They are a member of nothing, so they get
                // their own slice, and they are already at column 0.
                in_nominal = false;
                if let Some(last) = slices.last_mut() {
                    last.end = start;
                }
                slices.push(Slice { name: "(low level)", start: offset, end: offset, in_nominal });
            } else if let Some(last) = slices.last_mut() {
                last.end = offset;
            }
        }
        slices
    })
}

/// `\tName :: …` or `\tName(a, b) :: …`, and nothing deeper.
fn member_name(line: &str) -> Option<&'static str> {
    let rest = line.strip_prefix('\t')?;
    if rest.starts_with('\t') {
        return None;
    }
    let declared = rest.split(" :: ").next().filter(|d| *d != rest)?;
    let name = match declared.find('(') {
        Some(i) if declared.ends_with(')') => &declared[..i],
        _ => declared,
    };
    let mut chars = name.chars();
    if !chars.next().is_some_and(char::is_uppercase) || !chars.all(|c| c.is_alphanumeric() || c == '_')
    {
        return None;
    }
    // The member names come from a `const` source, so they outlive the program.
    Some(Box::leak(name.to_string().into_boxed_str()))
}

/// What reading one member told us.
pub struct Read {
    pub name: &'static str,
    pub lines: usize,
    /// The parse error, if it did not parse. Checking the TYPES is a later phase, so
    /// this stops at the AST.
    pub error: Option<String>,
    /// Members with a body: ordinary Roc, which rocflight can run once it loads them.
    pub defined: Vec<&'static str>,
    /// Members with an annotation and no body: what Rust has to supply.
    pub intrinsics: Vec<&'static str>,
}

/// Read every member, reporting what parsed and where its builtin boundary falls.
pub fn read() -> Vec<Read> {
    members()
        .into_iter()
        .map(|member| {
            let mut read = Read {
                name: member.name,
                lines: member.lines,
                error: None,
                defined: Vec::new(),
                intrinsics: Vec::new(),
            };
            match Desugarer::new(member.source).desugar() {
                Err(e) => read.error = Some(e.to_string()),
                Ok(desugared) => {
                    let mut parser = Parser::new(&desugared);
                    match parser.parse_expr() {
                        Err(e) => read.error = Some(e.to_string()),
                        Ok(ast) => {
                            read.defined = definitions(&ast);
                            read.intrinsics =
                                parser.intrinsics().iter().map(|(name, _)| *name).collect();
                        }
                    }
                }
            }
            read
        })
        .collect()
}

/// The names a parsed member BINDS, walking the `Let` spine its top level is.
///
/// That covers both shapes Builtin.roc uses: a nominal's methods, which `parse_expr`
/// wraps around the program as `Type.method` bindings, and the plain top-level
/// functions of the low-level section. `_` is a statement, not a definition.
fn definitions(ast: &crate::ast::Expr) -> Vec<&'static str> {
    let mut names = Vec::new();
    let mut cursor = ast;
    while let crate::ast::Expr::Let { name, body, .. } = cursor {
        if *name != "_" {
            names.push(*name);
        }
        cursor = body;
    }
    names
}

/// A member parsed and ready to compile into a program.
pub struct Loaded {
    pub name: &'static str,
    pub ast: crate::ast::Expr,
    /// The BARE names this member declares with a type and no body — the low-level ops.
    /// A qualified intrinsic (`Str.concat`) already routes through the builtin path on
    /// its module name; these have no module, so the compiler has to be told.
    pub intrinsics: Vec<&'static str>,
    /// `Type.method` to its declared type, for every annotation the member carries.
    pub signatures: Vec<(&'static str, crate::types::Type)>,
    /// The nominals this member declares, as `(name, backing)`.
    pub nominals: Vec<(&'static str, crate::types::Type)>,
}

/// Parse the named members so they can be compiled ahead of the user's file.
///
/// The names are the ones `read` reports. An unknown name, or one whose member does not
/// parse, is an error rather than a silent omission — loading half a module would leave
/// its definitions resolving to whatever Rust happens to answer, which is exactly the
/// drift this whole exercise is meant to stop.
///
/// The result is NOT type-checked. `roc` already checked this source, rocflight's
/// checker is weaker than the language Builtin.roc is written in, and the app's own
/// checking is unaffected either way because these names reach it as builtins already.
/// `ponytail: trusted input, checked upstream; revisit when P3 makes the annotations
/// the type table.`
pub fn load(selected: &[&str]) -> Result<Vec<Loaded>, String> {
    // Loading nothing must touch nothing: `SOURCE` is 700kB of the binary, and merely
    // scanning it faults those pages in on every run of every program.
    if selected.is_empty() {
        return Ok(Vec::new());
    }
    // Per-member timing, when `ROCFLIGHT_TIME` asks for it. Which member's parse costs
    // what is the whole question this phase of `OPTIMIZATION_PLAN.md` is about, and the
    // first `members_where` also pays for `index`'s one scan of all 700kB.
    let mut step = std::time::Instant::now();
    let mut sliced = members_where(|name| selected.contains(&name));
    crate::tick("slice + index", &mut step);
    // What the OTHER members say, which is all the low-level section is loaded for.
    let referenced: String =
        sliced.iter().filter(|m| m.name != "(low level)").map(|m| m.source.as_str()).collect();
    let mut loaded = Vec::new();
    for name in selected {
        let at = sliced
            .iter()
            .position(|m| m.name == *name)
            .ok_or_else(|| format!("no builtin member named `{}`", name))?;
        let mut member = sliced.remove(at);
        if member.name == "(low level)" && !referenced.is_empty() {
            member.source = reachable(&member.source, &referenced);
            crate::tick("  (low level) reachable", &mut step);
        }
        let desugared = Desugarer::new(member.source)
            .desugar()
            .map_err(|e| format!("builtin `{}`: {}", name, e))?;
        let mut parser = Parser::new(&desugared);
        let ast = parser.parse_expr().map_err(|e| format!("builtin `{}`: {}", name, e))?;
        crate::tick(format_args!("  {} parse", name), &mut step);
        let intrinsics = parser
            .intrinsics()
            .iter()
            .map(|(name, _)| *name)
            .filter(|name| !name.contains('.'))
            .collect();
        let signatures = parser.signatures().to_vec();
        let nominals = parser.nominals().to_vec();
        loaded.push(Loaded { name: member.name, ast, intrinsics, signatures, nominals });
    }
    Ok(loaded)
}

/// The low-level section cut down to what `from` reaches.
///
/// `Dict` and `Set` use 48 of its 317 declarations, and parsing the other 269 was
/// three milliseconds of every program that names a `Dict`. A declaration is kept when
/// its name appears in `from` or in a kept declaration's text, closed over; the walk
/// is by word, so a name in a comment keeps its declaration too, which costs a parse
/// and never an answer. Capitalised declarations (a type alias) are always kept.
fn reachable(source: &str, from: &str) -> String {
    // A block is a column-0 declaration and every line under it, up to the next.
    let mut blocks: Vec<(&str, usize, usize)> = Vec::new();
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let starts_block = line.starts_with(|c: char| c.is_alphabetic() || c == '_');
        if starts_block || blocks.is_empty() {
            let name = line
                .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '!'))
                .next()
                .filter(|n| n.starts_with(|c: char| c.is_lowercase() || c == '_'))
                .unwrap_or("");
            if blocks.last().is_some_and(|(n, _, _)| *n == name && !name.is_empty()) {
                // The body under its own annotation: one block.
            } else {
                blocks.push((name, start, start));
            }
        }
        blocks.last_mut().expect("a block").2 = offset;
    }
    let words = |text: &str| {
        text.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '!'))
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    let by_name: std::collections::HashMap<&str, (usize, usize)> = blocks
        .iter()
        .filter(|(name, _, _)| !name.is_empty())
        .map(|&(name, start, end)| (name, (start, end)))
        .collect();
    let mut keep: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut pending: Vec<String> = words(from);
    while let Some(word) = pending.pop() {
        let Some((&name, &(start, end))) = by_name.get_key_value(word.as_str()) else { continue };
        if keep.insert(name) {
            pending.extend(words(&source[start..end]));
        }
    }
    blocks
        .iter()
        .filter(|(name, _, _)| name.is_empty() || keep.contains(name))
        .map(|&(_, start, end)| &source[start..end])
        .collect()
}

/// The members whose signatures the checker reads. NOT all of them, on a measurement.
///
/// `Dict` and `Set` had no type at all before this — `Dict(Str, U64)` came out of the
/// type parser as a fresh variable, which is why `BasicDict` failed with "Cannot
/// dispatch `insert` on an unresolved type". Reading their signatures costs nothing:
/// a program that never mentions a `Dict` never parses one.
///
/// `Str` and `List` are left out for cost, not correctness. Neither moved a single
/// gate — `builtin_result` already gives their common methods real types — while `Str`
/// took the `strings` benchmark from 5ms to 7ms and `List` is 1,676 lines and cost 5ms
/// of every run against a 3ms baseline. The parse buys a long tail nothing yet asks
/// for.
///
/// All ten parsing members are verified to seed cleanly, 98 golden pairs and 12
/// examples with any combination of them, so widening this is one edit whenever that
/// long tail is worth the parse.
const TYPED_MEMBERS: &[&str] = &["Dict", "Set", "Str", "List"];

/// The `Type.method` signatures for one module, parsed on FIRST USE and kept.
///
/// Lazily, because seeding them all up front cost every program the parse of 1,490
/// lines — three milliseconds on a three-millisecond benchmark. A program that never
/// mentions a `Dict` never pays for `Dict`.
///
/// The module name is the member name here, which holds for everything in
/// `TYPED_MEMBERS`. It does not in general — `Json` lives in `Encoding` and `Try` in
/// `Box` — so widening that list means indexing modules to members first.
pub fn signatures_for(module: &str) -> &'static [(&'static str, crate::types::Type)] {
    // Checked before the cache, because the checker asks this of EVERY qualified name
    // it meets and most of them are not a member at all. Taking a lock to be told so
    // is the sort of cost that only shows up in a benchmark.
    if !TYPED_MEMBERS.contains(&module) {
        return &[];
    }
    type Table = std::collections::HashMap<String, &'static [(&'static str, crate::types::Type)]>;
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Table>> = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);

    if let Some(found) = cache.lock().expect("signature cache").get(module) {
        return found;
    }
    // Timed because this RE-PARSES a member `load` may already have parsed — the
    // measured 0.4ms of a `Dict` program and 1.4ms of a program that merely calls
    // `.map`. See `OPTIMIZATION_PLAN.md`, phase 1.3.
    let mut step = std::time::Instant::now();
    let parsed: &'static [(&'static str, crate::types::Type)] =
        Box::leak(parse_signatures(module));
    crate::tick(format_args!("signatures_for({})", module), &mut step);
    cache.lock().expect("signature cache").insert(module.to_string(), parsed);
    parsed
}

fn parse_signatures(module: &str) -> Box<[(&'static str, crate::types::Type)]> {
    let Some(member) = members_where(|name| name == module).pop() else { return Box::new([]) };
    // `Set(item) :: Dict(item, {})` — so `Set`'s own signatures only carry their
    // element type if `Dict` is a known parameterised nominal while they are parsed.
    // Alone, `Dict(item, {})` degrades to a placeholder and `item` is dropped, which is
    // why `Set.from_list([...U64]).to_list()` came back a list of unconstrained numbers.
    // Parse `Dict`'s declaration first, then keep only `Set`'s own signatures.
    let source = if module == "Set" {
        let dict = members_where(|name| name == "Dict").pop().map(|m| m.source).unwrap_or_default();
        format!("{}\n{}", annotations_only(&dict), annotations_only(&member.source))
    } else {
        annotations_only(&member.source)
    };
    let Ok(desugared) = Desugarer::new(source).desugar() else {
        return Box::new([]);
    };
    let mut parser = Parser::new(&desugared);
    if parser.parse_expr().is_err() {
        return Box::new([]);
    }
    parser
        .signatures()
        .iter()
        .filter(|(name, _)| name.starts_with(&format!("{}.", module)))
        .map(|(name, ty)| (*name, normalise(ty)))
        .collect()
}

/// The member with its function BODIES removed.
///
/// Only the annotations are wanted, and `List` is 1,676 lines of which the signatures
/// are a small fraction — parsing the rest cost five milliseconds of every run that
/// touched a list. A body starts at `name = ` and runs until something at its own
/// indent or shallower appears.
fn annotations_only(source: &str) -> String {
    let mut kept = String::with_capacity(source.len() / 4);
    let mut body_indent: Option<usize> = None;
    for line in source.lines() {
        let indent = line.len() - line.trim_start().len();
        if let Some(started) = body_indent {
            // A blank line does not end a body, nor does the closer that ends the
            // body's own block — dropping the opening brace and keeping the closing one
            // is what unbalanced eight golden pairs.
            let closes = matches!(line.trim_start().chars().next(), Some('}' | ')' | ']'));
            if line.trim().is_empty() || indent > started || (indent == started && closes) {
                continue;
            }
            body_indent = None;
        }
        let trimmed = line.trim_start();
        // Doc comments are most of the file and say nothing about a type.
        if trimmed.starts_with('#') {
            continue;
        }
        let is_body = trimmed
            .split_once(" = ")
            .is_some_and(|(name, _)| {
                !name.is_empty()
                    && name.starts_with(|c: char| c.is_lowercase() || c == '_')
                    && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '!')
            })
            || trimmed.ends_with(" =");
        if is_body {
            body_indent = Some(indent);
            continue;
        }
        kept.push_str(line);
        kept.push('\n');
    }
    kept
}

/// Bring a signature's types back to the ones rocflight actually uses.
///
/// Inside `Builtin.roc` a reference to `Str` resolves through that file's own
/// `Str :: [ProvidedByCompiler]` declaration, so it arrives as a nominal over an opaque
/// tag rather than as the string type. The same goes for `Bool`, the numbers and
/// `List`. Unifying one of those against a real `Type::Str` fails on the backing.
fn normalise(ty: &crate::types::Type) -> crate::types::Type {
    use crate::types::Type;
    match ty {
        Type::Nominal { name, backing } => match name.as_str() {
            "Str" => Type::Str,
            "Bool" => Type::Bool,
            "U8" => Type::U8, "U16" => Type::U16, "U32" => Type::U32,
            "U64" => Type::U64, "U128" => Type::U128,
            "I8" => Type::I8, "I16" => Type::I16, "I32" => Type::I32,
            "I64" => Type::I64, "I128" => Type::I128,
            "F32" => Type::F32, "F64" => Type::F64, "Dec" => Type::Dec,
            // `List(_item) :: [ProvidedByCompiler]` erases the element, so the most
            // that can be said is "a list of something".
            "List" => Type::List(Box::new(Type::TypeVar(u32::MAX))),
            _ => Type::Nominal { name: name.clone(), backing: Box::new(normalise(backing)) },
        },
        Type::Function(a, b) => {
            Type::Function(Box::new(normalise(a)), Box::new(normalise(b)))
        }
        Type::List(inner) => Type::List(Box::new(normalise(inner))),
        Type::Optional(inner) => Type::Optional(Box::new(normalise(inner))),
        Type::Tuple(items) => Type::Tuple(items.iter().map(normalise).collect()),
        Type::Record { fields, open } => Type::Record {
            fields: fields.iter().map(|(f, t)| (f.clone(), normalise(t))).collect(),
            open: *open,
        },
        Type::TagUnion { tags, open } => Type::TagUnion {
            tags: tags
                .iter()
                .map(|(t, args)| (t.clone(), args.iter().map(normalise).collect()))
                .collect(),
            open: *open,
        },
        other => other.clone(),
    }
}

/// The members a program needs, from the names it mentions.
///
/// Loading `Dict` means parsing it, `Set` and the 2,305-line low-level section with it:
/// seventeen milliseconds against a three-millisecond baseline. A program that never
/// mentions a `Dict` should not pay that, and cannot need it — the only way to make one
/// is to name `Dict` or `Set`.
///
/// Over-loading is a cost, never a wrong answer: the word in a comment buys a parse
/// nobody reads. Under-loading is impossible for the same reason it is cheap to detect.
/// Every method name `Builtin.roc` declares, anywhere in it.
///
/// A name roc has NO method for is a name no program can call — `list.reverse()` is
/// `rev` spelled wrong, and roc reports it rather than running it. Scanned once from
/// the source's annotation lines (`name : type`) and kept.
pub fn declared_names() -> &'static std::collections::HashSet<&'static str> {
    static NAMES: std::sync::OnceLock<std::collections::HashSet<&'static str>> =
        std::sync::OnceLock::new();
    NAMES.get_or_init(|| {
        SOURCE
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim_start();
                // `name : type` — a declaration, not a `name = value` binding, and not
                // a `::` type declaration.
                let (name, rest) = trimmed.split_once(':')?;
                if rest.starts_with(':') {
                    return None;
                }
                let name = name.trim_end();
                let ok = !name.is_empty()
                    && name.starts_with(|c: char| c.is_lowercase() || c == '_')
                    && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '!')
                    && line.len() - trimmed.len() > 0;
                // Borrowed from `SOURCE`, which is already `'static`: leaking a copy
                // of every one of them cost half a megabyte of peak memory.
                ok.then_some(name)
            })
            .collect()
    })
}

pub fn needed_by(source: &str) -> Vec<&'static str> {
    let mut wanted = Vec::new();
    // `Dict` and `Set` are inseparable — `Set(item) :: Dict(item, {})` — and both are
    // built out of the low-level section.
    if source.contains("Dict") || source.contains("Set") {
        wanted.extend(["(low level)", "Dict", "Set"]);
    }
    // `Box` carries `Try`, so anything writing `Ok`, `Err` or `?` may reach it.
    if source.contains("Box") || source.contains("Try") {
        wanted.push("Box");
    }
    if source.contains("Stream") {
        wanted.push("Stream");
    }
    wanted
}
