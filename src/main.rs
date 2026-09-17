#![forbid(unsafe_code)]

//! Roc Interpreter CLI
//!
//! Usage: rocflight <file.roc>
//!
//! Pipeline:
//! 1. Load .roc file
//! 2. Desugar shorthand syntax
//! 3. Parse into AST
//! 4. Type check with Hindley-Milner inference
//! 5. Compile to bytecode and run it on the register VM

use std::env;
use std::error::Error;
use std::process;
use std::time::{Duration, Instant};

use rocflight::desugaring::Desugarer;
use rocflight::parser::Parser;
use rocflight::types::TypeChecker;

/// No thread with a giant stack any more.
///
/// The tree-walker spent many Rust frames per Roc call and needed 256 MB reserved to
/// recurse a few hundred levels. The VM's call frames are a `Vec`, so Roc recursion
/// costs heap rather than stack and the default main-thread stack is plenty.
fn main() {
    cli();
}

fn cli() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} [options] <file.roc>", args[0]);
        eprintln!("Options:");
        eprintln!("  --show-desugared    Print the desugared source before running");
        eprintln!("  --show-ast          Print the AST and its inferred type, then run");
        eprintln!("  --ast-only          Print the AST and its inferred type, do not run");
        eprintln!("  --emit-desugared    Write the desugared source to .rocflight/cache/");
        eprintln!("  --show-platforms    Report each real platform the app resolves");
        eprintln!("  --test              Run the file's `expect`s and report, like `roc test`");
        eprintln!("  --clear-cache       Delete .rocflight/cache/desugared and exit");
        eprintln!("  --builtins          Report the vendored Builtin.roc, member by member");
        eprintln!("  --builtins=names    ... and list every intrinsic it declares");
        eprintln!("  --load-builtins=A,B Load these Builtin.roc members ahead of the file");
        process::exit(1);
    }

    // Parse command-line options
    let mut show_desugared = false;
    let mut show_ast = false;
    let mut ast_only = false;
    let mut emit_desugared = false;
    let mut show_platforms = false;
    let mut test_mode = false;
    // `None` means "work it out from the source"; `--load-builtins=` makes it explicit.
    let mut load_builtins: Option<Vec<String>> = None;
    let mut filename = None;

    for arg in &args[1..] {
        match arg.as_str() {
            "--builtins" | "--builtins=names" => {
                report_builtins(arg.ends_with("names"));
                return;
            }
            "--clear-cache" => {
                // Clear cache and exit
                if let Err(e) = Desugarer::clear_cache() {
                    eprintln!("Error clearing cache: {}", e);
                    process::exit(1);
                }
                process::exit(0);
            }
            "--show-desugared" => {
                show_desugared = true;
            }
            "--show-ast" => {
                show_ast = true;
            }
            "--ast-only" => {
                show_ast = true;
                ast_only = true;
            }
            "--emit-desugared" => {
                emit_desugared = true;
            }
            "--test" => {
                test_mode = true;
            }
            "--show-platforms" => {
                show_platforms = true;
            }
            _ if arg.starts_with("--load-builtins=") => {
                let list = arg.trim_start_matches("--load-builtins=");
                load_builtins = Some(if list.is_empty() {
                    Vec::new()
                } else {
                    list.split(',').map(|m| m.trim().to_string()).collect()
                });
            }
            _ if !arg.starts_with("--") => {
                filename = Some(arg.clone());
            }
            _ => {
                eprintln!("Unknown option: {}", arg);
                process::exit(1);
            }
        }
    }

    let filename = match filename {
        Some(f) => f,
        None => {
            eprintln!("Error: No input file specified");
            process::exit(1);
        }
    };

    // Run the interpreter with proper error handling
    if let Err(e) = run(
        &filename,
        show_desugared,
        show_ast,
        ast_only,
        emit_desugared,
        show_platforms,
        test_mode,
        load_builtins.as_deref(),
    ) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

/// Main interpreter pipeline with Result-based error handling
fn run(
    filename: &str,
    show_desugared: bool,
    show_ast: bool,
    ast_only: bool,
    emit_desugared: bool,
    show_platforms: bool,
    test_mode: bool,
    load_builtins: Option<&[String]>,
) -> Result<(), Box<dyn Error>> {
    // `roc test` times the whole invocation, compile included, not just the expects.
    let started = Instant::now();

    // Step 1: Load and desugar file.
    //
    // The source is read and parsed on every run. Nothing is cached between runs:
    // `.rocflight/cache/desugared/` is a write-only dump for inspection, never read
    // back, so editing a .roc file always takes effect immediately.
    let source = std::fs::read_to_string(filename)?;
    // Which builtin members this file needs, read off the source before it is consumed.
    let needed = rocflight::builtin::needed_by(&source);
    let desugarer = Desugarer::new(source);
    let desugared = desugarer.desugar()?;

    // Writing the dump is opt-in: it is a debugging aid, not part of running a
    // program, and doing it on every run costs a file write and prints noise.
    if emit_desugared {
        desugarer.save_debug(filename, &desugared)?;
    }

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

    // Paths in a roc file are relative to the file itself.
    let source_dir = std::path::Path::new(filename)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();

    // Step 2b: Resolve any real platform the app names.
    //
    // Done before evaluation so a bad import is reported against the platform's own
    // sources rather than surfacing later as an unknown name.
    let platforms = rocflight::platform::real::verify_app(
        parser.dependencies(),
        parser.imports(),
    )?;
    rocflight::platform::real::record_declared(&platforms);
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
    let selected: Vec<&str> = match load_builtins {
        Some(chosen) => chosen.iter().map(String::as_str).collect(),
        None => needed,
    };
    let builtins = rocflight::builtin::load(&selected)?;

    // Step 2c: Local modules — `import Hello exposing [hello]`.
    //
    // Each is an ordinary .roc beside the importer. Its top level is evaluated into
    // the shared global scope BEFORE the app's, so `Hello.hello` is bound by the time
    // the app runs; `exposing` then aliases the named ones so they can be used bare.
    let mut module_asts = Vec::new();
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
        module_asts.push((module_ast, type_name, exposed.clone()));
    }

    // Step 3: Type check
    let mut type_checker = TypeChecker::new();
    type_checker.allow_dispatch(parser.where_methods());
    type_checker.declare_nominal_literals(parser.nominal_literals());
    type_checker.declare_suffixed_literals(&parser.suffixed_literals());
    for (module_ast, _, _) in &module_asts {
        // Checked first so the app sees the module's names with their real types.
        type_checker.synth(module_ast)?;
    }
    let inferred = type_checker.synth(&ast)?;

    // Both files of a golden pair should build the same AST and infer the same type:
    // they differ only in sugar. `--ast-only` makes that comparable without running.
    if show_ast {
        println!("{}", ast);
        println!(":: {}", inferred);
    }
    if ast_only {
        return Ok(());
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
    let unit = rocflight::vm::compile::Unit {
        // A module's top level is compiled into the SAME program, ahead of the app's,
        // which is how `hello` from `import Hello exposing [hello]` ends up in scope.
        modules: builtins
            .iter()
            // A member's own method blocks already qualified its names — the AST binds
            // `Str.is_empty`, not `is_empty` — so there is nothing to hang them on and
            // nothing to expose bare.
            .map(|loaded| rocflight::vm::compile::Module {
                ast: &loaded.ast,
                type_name: loaded.name,
                exposed: Vec::new(),
            })
            .chain(module_asts.iter().map(|(module_ast, type_name, exposed)| {
                rocflight::vm::compile::Module {
                    ast: module_ast,
                    type_name: Box::leak(type_name.clone().into_boxed_str()),
                    exposed: exposed
                        .iter()
                        .map(|name| &*Box::leak(name.clone().into_boxed_str()) as &'static str)
                        .collect(),
                }
            }))
            .collect(),
        app: &ast,
        // `roc test` runs the top-level `expect`s and NOTHING else: the entry point
        // does not run at all, which is why the test program has no entry.
        entry: if test_mode { None } else { app_entry_point.as_deref() },
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
            .chain(parser.nominals().iter().cloned())
            .collect(),
        opaque_nominals: parser.opaque_nominals().to_vec(),
        intrinsics: builtins.iter().flat_map(|b| b.intrinsics.iter().copied()).collect(),
        test_mode,
    };
    let program = std::rc::Rc::new(rocflight::vm::compile_unit(&unit)?);

    // Step 5: run the top level, then the app's entry point if it declared one.
    //
    // Top-level `expect`s are compiled in only under `--test`; an ordinary run skips
    // them the way `roc run` does, so this runs the declarations and the program.
    let value = rocflight::vm::run(&program)?;

    // `--test` reports the `expect` tally the way `roc test` does.
    if test_mode {
        return report_tests(started.elapsed());
    }
    // A module's own value is its output. An app's output comes from its effects, so
    // there is nothing to print.
    if app_entry_point.is_none() {
        println!("{}", value);
    }
    // A failed `expect` inside a function body does not stop the program, but it does
    // make the run fail, as it does under `roc run`.
    if rocflight::eval::assert_failed() {
        process::exit(1);
    }
    Ok(())
}

/// Report the vendored `Builtin.roc`: what parses, and where its builtin boundary is.
///
/// A member with a BODY is ordinary Roc that rocflight will run once it loads the
/// module; a member with only an annotation is an intrinsic that Rust has to supply,
/// exactly as the real compiler's `BuiltinLowLevel.zig` supplies it from Zig. The split
/// is read off the source, so this list is generated rather than maintained.
fn report_builtins(list_names: bool) {
    let (mut parsed, mut defined, mut intrinsics) = (0, 0, 0);
    let read = rocflight::builtin::read();
    for member in &read {
        match &member.error {
            Some(error) => {
                println!("  FAIL {:<10} {:>6} lines  {}", member.name, member.lines, error)
            }
            None => {
                parsed += 1;
                defined += member.defined.len();
                intrinsics += member.intrinsics.len();
                println!(
                    "  ok   {:<10} {:>6} lines  {:>4} defined  {:>4} intrinsic",
                    member.name,
                    member.lines,
                    member.defined.len(),
                    member.intrinsics.len()
                );
            }
        }
    }
    println!(
        "\n{} of {} members parse: {} definitions in Roc, {} intrinsics for Rust",
        parsed,
        read.len(),
        defined,
        intrinsics
    );
    if list_names {
        println!();
        for member in &read {
            for name in &member.intrinsics {
                println!("{}", name);
            }
        }
    }
}

/// Report the `expect` tally, the way `roc test` does.
///
/// Only top-level `expect`s are counted, and roc splits the two outcomes across the two
/// streams: the pass line goes to stdout, the failure report to stderr.
///
/// roc closes the line with ` in <d.d> ms.`, and appends ` (cached)` when every module
/// came from its cache. rocflight reads nothing back from `.rocflight/cache`, so the
/// run is never cached and that suffix never applies.
fn report_tests(elapsed: Duration) -> Result<(), Box<dyn Error>> {
    let ms = elapsed.as_secs_f64() * 1000.0;
    let (ran, failed) = rocflight::eval::expect_tally();
    if failed == 0 {
        println!("All ({}) tests passed in {:.1} ms.", ran, ms);
    } else {
        eprintln!("Ran {} tests in {:.1} ms.:", ran, ms);
        eprintln!("    {} passed", ran - failed);
        eprintln!("    {} failed", failed);
        // Anything that would raise this count is a parse or type error, which
        // rocflight reports and exits on before it gets here.
        eprintln!("    0 compiler errors");
        process::exit(1);
    }
    Ok(())
}
