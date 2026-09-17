//! The pipeline: file in, value out.
//!
//! What `rocflight file.roc` does, as a library call, so a platform's host can run
//! the same thing from `roc_main` (see `host/` and PLATFORM_HOST_PLAN.md). Printing,
//! the test tally and exit codes stay with the caller.

use std::error::Error;
use std::time::{Duration, Instant};

use crate::desugaring::Desugarer;
use crate::eval::value::Value;
use crate::parser::Parser;
use crate::types::{Type, TypeChecker};

/// How to run a file. The four `show_*` are debugging aids and default off.
#[derive(Default)]
pub struct Options {
    pub show_desugared: bool,
    pub show_ast: bool,
    pub ast_only: bool,
    pub show_platforms: bool,
    /// Compile the top-level `expect`s and do not run the entry point: `roc test`.
    pub test_mode: bool,
    /// What the entry point is called with — the host's `args`, when there is a host.
    pub args: Option<Value>,
    /// Run the platform's own entry point (`provides { "roc_main": main_for_host! }`)
    /// instead of the app's `main!`. Set by `roc_main`, inside the host, where the
    /// platform's Roc is what maps `main!`'s answer to an exit code.
    pub host_entry: bool,
}

/// What a run produced.
pub struct Ran {
    /// The top level's value for a module; the entry point's result for an app.
    pub value: Value,
    /// Whether the file declared an entry point (an app) or is a module.
    pub is_app: bool,
    pub elapsed: Duration,
}

/// Run one file through the whole pipeline.
///
/// `None` when `ast_only` stopped it before running. Otherwise what came out, for the
/// caller to print, tally or turn into an exit code — this decides none of that, so
/// the same pipeline serves `rocflight file.roc`, `rocflight test`, and `roc_main`
/// inside a platform's host.
pub fn run_file(filename: &str, options: Options) -> Result<Option<Ran>, Box<dyn Error>> {
    let Options { show_desugared, show_ast, ast_only, show_platforms, test_mode, args, host_entry } = options;
    // `roc test` times the whole invocation, compile included, not just the expects.
    let started = Instant::now();

    // Step 1: Load and desugar file.
    //
    // The source is read and parsed on every run. Nothing is cached between runs, so
    // editing a .roc file always takes effect immediately.
    // Named, because the file may be the `main.roc` default that nobody typed.
    let source = std::fs::read_to_string(filename)
        .map_err(|e| format!("cannot read `{}`: {}", filename, e))?;
    // Which builtin members this file needs, read off the source before it is consumed.
    let needed = crate::builtin::needed_by(&source);
    let desugarer = Desugarer::new(source);
    let desugared = desugarer.desugar()?;

    // Optionally display desugared code
    if show_desugared {
        eprintln!("\n=== DESUGARED CODE ===");
        eprintln!("{}", desugared);
        eprintln!("=== END DESUGARED CODE ===\n");
    }

    // Step 2: Parse desugared code (includes AST building)
    let mut parser = Parser::named(filename, &desugared);
    let (ast, app_entry_point) = {
        let expr = parser.parse_expr()?;
        (expr, parser.app_entry_point())
    };
    // Every node so far is the app's; modules parsed from here on are not.
    let app_nodes = crate::ast::node_count();

    // Paths in a roc file are relative to the file itself.
    let source_dir = std::path::Path::new(filename)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();

    // Step 2b: Resolve any real platform the app names.
    //
    // Done before evaluation so a bad import is reported against the platform's own
    // sources rather than surfacing later as an unknown name.
    let platforms = crate::platform::real::verify_app(
        parser.dependencies(),
        parser.imports(),
    )?;
    crate::platform::real::record_declared(&platforms);
    if show_platforms {
        for platform in &platforms {
            eprintln!(
                "[Platform] {} from {}",
                platform.alias,
                platform.sources.display()
            );
            eprintln!("[Platform]   exposes {} modules", platform.exposes.len());
            eprintln!("[Platform]   declares {} hosted effects", platform.hosted.len());
            if let Some(requires) = &platform.requires {
                eprintln!("[Platform]   requires {}", requires);
            }
        }
    }

    // Step 2b2: the vendored builtin module.
    //
    // Its members are compiled into the SAME program, ahead of everything else, so a
    // definition Builtin.roc writes in Roc — `Dict.insert`, `List.join` — is an
    // ordinary top-level function by the time the app runs, and dispatch finds it the
    // way it finds any `Type.method`. Its annotation-only members stay builtins and
    // land in Rust, which is the same split `BuiltinLowLevel.zig` makes.
    //
    // Which members load is read off the source, never chosen from the command line:
    // the module is part of the interpreter, so asking for a different set of it would
    // only be a way to run a program against a runtime that is not the real one.
    let builtins = crate::builtin::load(&needed)?;

    // Step 2c: Local modules — `import Hello exposing [hello]`.
    //
    // Each is an ordinary .roc beside the importer. Its top level is evaluated into
    // the shared global scope BEFORE the app's, so `Hello.hello` is bound by the time
    // the app runs; `exposing` then aliases the named ones so they can be used bare.
    let mut module_asts = Vec::new();
    let mut module_nominals: Vec<(&'static str, Type)> = Vec::new();
    for (path, exposed) in parser.local_modules() {
        let file = source_dir.join(format!("{}.roc", path));
        let text = std::fs::read_to_string(&file)
            .map_err(|e| format!("cannot read module `{}`: {}", file.display(), e))?;
        let module_source = Desugarer::new(text).desugar()?;
        let module_source: &'static str = Box::leak(module_source.into_boxed_str());
        let mut module_parser = Parser::named(&file.display().to_string(), module_source);
        let module_ast = module_parser.parse_expr()?;
        // The last segment is the type the module's method block hangs its names on:
        // `Dir/Hello` exposes them as `Hello.hello`.
        let type_name = path.rsplit('/').next().unwrap_or(path).to_string();
        module_nominals.extend(module_parser.nominals().iter().cloned());
        module_asts.push((module_ast, type_name, exposed.clone()));
    }

    // Step 2d: the platform's own modules, as the Roc they are.
    //
    // `Stdout.line!` is Roc in the platform's `Stdout.roc`, calling `Host.stdout_line!`,
    // which `Host.roc` declares with a type and no body — `Builtin.roc`'s split, and
    // the same treatment: bodied members compile ahead of the app, and a bodiless one
    // dispatches to the host through `platform::hosted`. Only what the app imports
    // is loaded, with everything those modules import in turn.
    let mut platform_loaded = Vec::new();
    for platform in &platforms {
        let imported: Vec<String> = parser
            .imports()
            .iter()
            .filter(|(alias, _)| *alias == platform.alias)
            .map(|(_, module)| module.clone())
            .collect();
        let loaded = crate::platform::modules::load(platform, &imported, host_entry)?;
        let skipped = crate::platform::modules::register_hosted(platform, &loaded);
        if show_platforms {
            for module in &loaded.modules {
                eprintln!("[Platform] {} loaded module {}", platform.alias, module.name);
            }
            for gap in &skipped {
                eprintln!("[Platform] {} cannot lay out {}", platform.alias, gap);
            }
        }
        platform_loaded.push(loaded);
    }

    // Step 3: Type check
    let mut type_checker = TypeChecker::new();
    // Declarations first, so a name used before it is declared — or declared in a
    // module — resolves rather than standing as a placeholder.
    type_checker.declare_types(parser.nominals().iter().map(|(n, t)| (*n, t.clone())));
    type_checker.declare_types(module_nominals.iter().map(|(n, t)| (*n, t.clone())));
    for loaded in &platform_loaded {
        type_checker.declare_types(loaded.nominals.iter().map(|(n, t)| (*n, t.clone())));
    }
    type_checker.allow_dispatch(parser.where_methods());
    type_checker.declare_nominal_literals(parser.nominal_literals());
    type_checker.declare_suffixed_literals(&parser.suffixed_literals());
    for loaded in &platform_loaded {
        type_checker.declare_signatures(loaded.signatures.iter().cloned());
        type_checker.declare_nominal_literals(&loaded.nominal_literals);
        for module in &loaded.modules {
            type_checker.predeclare(&module.ast);
            type_checker.synth(&module.ast)?;
        }
    }
    for (module_ast, _, _) in &module_asts {
        // Checked first so the app sees the module's names with their real types.
        type_checker.predeclare(module_ast);
        type_checker.synth(module_ast)?;
    }
    type_checker.predeclare(&ast);
    let inferred = type_checker.synth(&ast)?;
    // The platform's entry references the app's `main!`, so it comes after the app.
    for loaded in &platform_loaded {
        if let Some((module, _)) = &loaded.entry {
            type_checker.predeclare(&module.ast);
            type_checker.synth(&module.ast)?;
        }
    }

    // Both files of a golden pair should build the same AST and infer the same type:
    // they differ only in sugar. `tests/golden_ast_test.rs` is the gate on that;
    // `--ast-only` makes one pair comparable by eye.
    if show_ast {
        println!("{}", ast);
        println!(":: {}", inferred);
        // Which numerals nothing pinned, so a `Dec` that should have been an `I64`
        // can be traced to the literal that became it.
        let mut floating: Vec<usize> = type_checker
            .fractional_literals()
            .into_iter()
            .filter(|id| id.index() < app_nodes)
            .filter_map(crate::ast::offset_of)
            .collect();
        floating.sort_unstable();
        for offset in floating {
            let line = desugared[..offset.min(desugared.len())].matches('\n').count() + 1;
            let col = offset - desugared[..offset.min(desugared.len())].rfind('\n').map_or(0, |i| i + 1) + 1;
            eprintln!(":: fractional literal at {}:{}", line, col);
        }
    }
    if ast_only {
        return Ok(None);
    }

    let ingested: Vec<(&'static str, String)> = parser
        .ingests()
        .iter()
        .map(|(name, path)| {
            let text = std::fs::read_to_string(source_dir.join(path))
                .map_err(|e| format!("cannot ingest `{}`: {}", path, e))?;
            Ok::<_, String>((
                &*Box::leak(name.clone().into_boxed_str()) as &'static str,
                text,
            ))
        })
        .collect::<Result<_, _>>()?;

    // Step 4: compile to bytecode and run it.
    //
    // Anything the compiler cannot lower is an error naming the construct. There is no
    // fallback interpreter to quietly take over, which is the point of there being one
    // engine: a program either compiles or says why.
    let unit = crate::vm::compile::Unit {
        // A module's top level is compiled into the SAME program, ahead of the app's,
        // which is how `hello` from `import Hello exposing [hello]` ends up in scope.
        modules: builtins
            .iter()
            // A member's own method blocks already qualified its names — the AST binds
            // `Str.is_empty`, not `is_empty` — so there is nothing to hang them on and
            // nothing to expose bare.
            .map(|loaded| crate::vm::compile::Module {
                ast: &loaded.ast,
                type_name: loaded.name,
                exposed: Vec::new(),
            })
            .chain(platform_loaded.iter().flat_map(|loaded| {
                // A platform module's `exposing [IOErr]` names a type, not a value;
                // its functions are always called qualified.
                loaded.modules.iter().map(|module| crate::vm::compile::Module {
                    ast: &module.ast,
                    type_name: module.name,
                    exposed: Vec::new(),
                })
            }))
            .chain(module_asts.iter().map(|(module_ast, type_name, exposed)| {
                crate::vm::compile::Module {
                    ast: module_ast,
                    type_name: Box::leak(type_name.clone().into_boxed_str()),
                    exposed: exposed
                        .iter()
                        .map(|name| &*Box::leak(name.clone().into_boxed_str()) as &'static str)
                        .collect(),
                }
            }))
            .chain(platform_loaded.iter().filter_map(|loaded| {
                loaded.entry.as_ref().map(|(module, _)| crate::vm::compile::Module {
                    ast: &module.ast,
                    type_name: module.name,
                    exposed: Vec::new(),
                })
            }))
            .collect(),
        app: &ast,
        // `roc test` runs the top-level `expect`s and NOTHING else: the entry point
        // does not run at all, which is why the test program has no entry.
        entry: if test_mode {
            None
        } else if host_entry {
            platform_loaded.iter().find_map(|l| l.entry.as_ref().map(|(_, name)| *name))
        } else {
            app_entry_point.as_deref()
        },
        ingested,
        // What the checker learned about each operator's operands, which is what lets
        // the compiler emit an integer-only opcode where it applies.
        integer_binops: type_checker.integer_binops(),
        dispatch_modules: type_checker.dispatch_modules(),
        binop_modules: type_checker.binop_modules(),
        dec_literals: type_checker.dec_literals(),
        fractional_literals: type_checker.fractional_literals(),
        parse_targets: type_checker.json_parse_targets(),
        // Every nominal in scope, the app's and each loaded builtin member's: the VM
        // needs their shapes to tell whose method a value can have meant.
        nominals: builtins
            .iter()
            .flat_map(|b| b.nominals.iter().cloned())
            .chain(platform_loaded.iter().flat_map(|l| l.nominals.iter().cloned()))
            .chain(parser.nominals().iter().cloned())
            .collect(),
        opaque_nominals: parser.opaque_nominals().to_vec(),
        intrinsics: builtins.iter().flat_map(|b| b.intrinsics.iter().copied()).collect(),
        test_mode,
    };
    let program = std::rc::Rc::new(crate::vm::compile_unit(&unit)?);

    // Step 5: run the top level, then the app's entry point if it declared one.
    //
    // Top-level `expect`s are compiled in only under `test`; an ordinary run skips
    // them the way `roc run` does, so this runs the declarations and the program.
    let value = crate::vm::run_with_args(&program, args)?;
    Ok(Some(Ran { value, is_app: app_entry_point.is_some(), elapsed: started.elapsed() }))
}

