//! A platform's own modules, loaded as the Roc they are.
//!
//! `Stdout.line!` is not something rocflight implements: it is Roc, in the platform's
//! `Stdout.roc`, and what it calls, `Host.stdout_line!`, is declared in `Host.roc`
//! with a type and no body. That is the split `Builtin.roc` has, and it gets the
//! same treatment: bodied members compile as modules ahead of the app, and a bodiless
//! member dispatches to whoever supplies it — here the host, through
//! `platform::hosted`.
//!
//! Only the modules the app reaches are loaded, transitively through their imports,
//! so a platform whose `Http.roc` needs a package the interpreter cannot fetch still
//! serves an app that never mentions `Http`.

use std::collections::{HashMap, HashSet};

use super::abi::Signature;
use super::layout::Declarations;
use super::real::{self, RealPlatform};
use crate::ast::Expr;
use crate::desugaring::Desugarer;
use crate::parser::Parser;
use crate::types::Type;

/// One platform module, parsed.
pub struct Module {
    pub name: &'static str,
    pub ast: Expr,
}

/// What loading a platform for an app produced.
pub struct Loaded {
    /// In dependency order: a module's imports come before it.
    pub modules: Vec<Module>,
    /// The entry the host calls (`provides`), as its own module, and its name. It
    /// references the app's `main!`, so it is checked after the app.
    pub entry: Option<(Module, &'static str)>,
    /// Every `Module.member : Type` the loaded modules declare.
    pub signatures: Vec<(String, Type)>,
    /// Every nominal the loaded modules declare, for the VM and for laying out.
    pub nominals: Vec<(&'static str, Type)>,
    /// Literals the loaded modules wrote as a nominal (`value : OsStr` given a
    /// string), for the checker; suffixed literals are collected thread-wide already.
    pub nominal_literals: Vec<(crate::ast::NodeId, Type)>,
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// Load the modules the app imports from `platform` (and, with `with_entry`, the
/// platform's entry point), each with everything it imports in turn.
pub fn load(platform: &RealPlatform, imported: &[String], with_entry: bool) -> Result<Loaded, String> {
    let mut parsed: HashMap<String, Parsed> = HashMap::new();
    let mut pending: Vec<String> = imported.to_vec();
    let mut entry = None;

    if with_entry {
        let name = platform
            .provides
            .clone()
            .ok_or_else(|| format!("platform `{}` provides no entry point", platform.alias))?;
        let main = platform.sources.join("main.roc");
        let text = std::fs::read_to_string(&main).map_err(|e| format!("cannot read `{}`: {}", main.display(), e))?;
        let tail_source = real::entry_source(&text);
        let tail = parse(&main.display().to_string(), tail_source.clone())?;
        // The entry imports every module the platform has, and uses two. Loading
        // only what its body names keeps a module the interpreter cannot check yet
        // from taking the whole platform down with it.
        let body: String = tail_source.lines().filter(|l| !l.starts_with("import ")).collect::<Vec<_>>().join("\n");
        pending.extend(tail.deps.into_iter().filter(|dep| body.contains(&format!("{}.", dep))));
        let ast = tail.ast;
        entry = Some((Module { name: leak(name.clone()), ast }, leak(name)));
    }

    while let Some(name) = pending.pop() {
        if parsed.contains_key(&name) {
            continue;
        }
        let file = platform.sources.join(format!("{}.roc", name));
        let text = std::fs::read_to_string(&file)
            .map_err(|e| format!("platform `{}` has no module `{}`: {}", platform.alias, name, e))?;
        let module = parse(&file.display().to_string(), text)?;
        pending.extend(module.deps.iter().cloned());
        parsed.insert(name, module);
    }

    // Dependency order: depth-first, a module after everything it imports.
    let mut order = Vec::new();
    let mut seen = HashSet::new();
    fn visit(name: &str, parsed: &HashMap<String, Parsed>, seen: &mut HashSet<String>, order: &mut Vec<String>) {
        if !seen.insert(name.to_string()) {
            return;
        }
        if let Some(module) = parsed.get(name) {
            for dep in &module.deps {
                visit(dep, parsed, seen, order);
            }
            order.push(name.to_string());
        }
    }
    let mut roots: Vec<&String> = parsed.keys().collect();
    roots.sort();
    for root in roots {
        visit(root, &parsed, &mut seen, &mut order);
    }

    let mut modules = Vec::new();
    let mut signatures = Vec::new();
    let mut nominals = Vec::new();
    let mut nominal_literals = Vec::new();
    for name in order {
        let module = parsed.remove(&name).expect("ordered from parsed");
        modules.push(Module { name: leak(name), ast: module.ast });
        signatures.extend(module.signatures);
        nominals.extend(module.nominals);
        nominal_literals.extend(module.nominal_literals);
    }
    Ok(Loaded { modules, entry, signatures, nominals, nominal_literals })
}

/// One module as parsed: its AST, the platform modules it imports, what it declares.
struct Parsed {
    ast: Expr,
    deps: Vec<String>,
    signatures: Vec<(String, Type)>,
    nominals: Vec<(&'static str, Type)>,
    nominal_literals: Vec<(crate::ast::NodeId, Type)>,
}

/// A platform module's top-level `expect`s are its own test suite. `roc run` never
/// runs them and neither does an app here, so they are dropped before parsing: a
/// test that exercises a feature the interpreter lacks must not stop the module's
/// functions from loading. `ponytail:` text-level — an `expect` at column 0 and its
/// indented continuation — because that is the only shape the platform writes.
fn without_top_level_expects(text: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut skipping = false;
    // `expect {` … `}` closes at column 0; the closer goes too.
    let mut in_block = false;
    for line in text.lines() {
        if line.starts_with("expect") {
            skipping = true;
            in_block = line.trim_end().ends_with('{');
            continue;
        }
        if skipping && in_block {
            if line == "}" {
                in_block = false;
            }
            continue;
        }
        if skipping && (line.is_empty() || line.starts_with(['\t', ' '])) {
            continue;
        }
        skipping = false;
        kept.push_str(line);
        kept.push('\n');
    }
    kept
}

/// Desugar and parse one module.
fn parse(path: &str, text: String) -> Result<Parsed, String> {
    let text = without_top_level_expects(&text);
    let source = Desugarer::new(text).desugar().map_err(|e| format!("{}: {}", path, e))?;
    let source: &'static str = leak(source);
    let mut parser = Parser::named(path, source);
    let ast = parser.parse_expr().map_err(|e| format!("{}: {}", path, e))?;
    // `import IOErr exposing [IOErr]` and bare `import Host` are sibling modules;
    // `import http.Request` is a package's, which nothing here can fetch.
    let deps = parser.local_modules().iter().map(|(m, _)| m.clone()).collect();
    let signatures = parser.signatures().iter().map(|(n, t)| (n.to_string(), t.clone())).collect();
    let nominals = parser.nominals().to_vec();
    let nominal_literals = parser.nominal_literals().to_vec();
    Ok(Parsed { ast, deps, signatures, nominals, nominal_literals })
}

/// Register every hosted effect the platform maps whose signature lays out.
///
/// One that does not — a type the loaded modules do not declare — is left out and
/// reported as a gap if reached, rather than failing the whole platform.
pub fn register_hosted(platform: &RealPlatform, loaded: &Loaded) -> Vec<String> {
    let mut decls: HashMap<String, Type> = HashMap::new();
    for (name, ty) in &loaded.nominals {
        decls.insert(name.to_string(), ty.clone());
        // `Host.NativeOsStr` is also `NativeOsStr` inside `Host.roc` itself.
        if let Some((_, bare)) = name.rsplit_once('.') {
            decls.entry(bare.to_string()).or_insert_with(|| ty.clone());
        }
    }
    let decls = Declarations::new(decls);
    let mut skipped = Vec::new();
    for (symbol, member) in &platform.hosted {
        let Some((_, ty)) = loaded.signatures.iter().find(|(n, _)| n == member) else {
            continue;
        };
        match Signature::of(ty, &decls) {
            Ok(signature) => super::hosted::register(member, symbol, signature),
            Err(e) => skipped.push(format!("{} ({}): {}", member, symbol, e)),
        }
    }
    skipped
}
