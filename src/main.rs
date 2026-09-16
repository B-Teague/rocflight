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
//! 5. Evaluate with tree-walk interpreter

use std::env;
use std::error::Error;
use std::process;

use rocflight::parser::Parser;
use rocflight::types::TypeChecker;
use rocflight::eval::Evaluator;
use rocflight::eval::Value;
use rocflight::desugaring::Desugarer;

/// Stack for the interpreter thread.
///
/// A tree-walker spends many Rust frames per Roc call, so the 8 MB a main thread gets
/// by default runs out at a couple of hundred levels of Roc recursion — well short of
/// what ordinary Roc programs do (the LeastSquares example recurses 501 times).
const INTERPRETER_STACK: usize = 256 * 1024 * 1024;

fn main() {
    // Everything runs on a thread with a stack big enough for deep recursion.
    let worker = std::thread::Builder::new()
        .stack_size(INTERPRETER_STACK)
        .spawn(cli)
        .expect("failed to start the interpreter thread");
    match worker.join() {
        Ok(()) => {}
        // The thread already reported whatever went wrong.
        Err(_) => process::exit(1),
    }
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
        eprintln!("  --vm                Run on the register VM instead of the tree-walker");
        eprintln!("  --clear-cache       Delete .rocflight/cache/desugared and exit");
        process::exit(1);
    }

    // Parse command-line options
    let mut show_desugared = false;
    let mut show_ast = false;
    let mut ast_only = false;
    let mut emit_desugared = false;
    let mut show_platforms = false;
    let mut test_mode = false;
    let mut use_vm = false;
    let mut filename = None;

    for arg in &args[1..] {
        match arg.as_str() {
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
            "--vm" => {
                use_vm = true;
            }
            "--show-platforms" => {
                show_platforms = true;
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
        use_vm,
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
    use_vm: bool,
) -> Result<(), Box<dyn Error>> {
    // Step 1: Load and desugar file.
    //
    // The source is read and parsed on every run. Nothing is cached between runs:
    // `.rocflight/cache/desugared/` is a write-only dump for inspection, never read
    // back, so editing a .roc file always takes effect immediately.
    let source = std::fs::read_to_string(filename)?;
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
    let mut parser = Parser::new(&desugared);
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
        let mut module_parser = Parser::new(module_source);
        let module_ast = module_parser.parse_expr()?;
        // The last segment is the type the module's method block hangs its names on:
        // `Dir/Hello` exposes them as `Hello.hello`.
        let type_name = path.rsplit('/').next().unwrap_or(path).to_string();
        module_asts.push((module_ast, type_name, exposed.clone()));
    }

    // Step 3: Type check
    let mut type_checker = TypeChecker::new();
    type_checker.allow_dispatch(parser.where_methods());
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

    // Step 4 (--vm): compile to bytecode and run that instead.
    //
    // Anything the VM cannot compile is an error naming the construct, never a silent
    // fall-through to the tree-walker: which engine ran a program has to be knowable.
    if use_vm {
        let unit = rocflight::vm::compile::Unit {
            // A module's top level is compiled into the SAME program, ahead of the
            // app's, which is how `hello` from `import Hello exposing [hello]` ends up
            // in scope — the tree-walker gets there by evaluating each module into the
            // shared global scope first.
            modules: module_asts
                .iter()
                .map(|(module_ast, type_name, exposed)| rocflight::vm::compile::Module {
                    ast: module_ast,
                    type_name: Box::leak(type_name.clone().into_boxed_str()),
                    exposed: exposed
                        .iter()
                        .map(|name| &*Box::leak(name.clone().into_boxed_str()) as &'static str)
                        .collect(),
                })
                .collect(),
            app: &ast,
            entry: app_entry_point.as_deref(),
            ingested,
        };
        let program = std::rc::Rc::new(rocflight::vm::compile_unit(&unit)?);
        let value = rocflight::vm::run(&program)?;

        // `--test` reports the `expect` tally the way `roc test` does. The tally is
        // process-wide and both engines feed the same one, so this is the same report.
        if test_mode {
            return report_tests();
        }
        // A module's own value is its output, exactly as below. An app's output comes
        // from its effects, so there is nothing to print.
        if app_entry_point.is_none() {
            println!("{}", value);
        }
        return Ok(());
    }

    // Step 4: Evaluate
    let mut evaluator = Evaluator::new();
    for (module_ast, type_name, exposed) in &module_asts {
        evaluator.eval(module_ast)?;
        // `exposing [hello]` makes `Hello.hello` reachable as plain `hello`.
        for name in exposed {
            let qualified = format!("{}.{}", type_name, name);
            match evaluator.env.lookup(&qualified) {
                Some(value) => evaluator
                    .env
                    .bind(Box::leak(name.clone().into_boxed_str()), value),
                None => {
                    return Err(format!(
                        "module `{}` does not expose `{}`",
                        type_name, name
                    )
                    .into())
                }
            }
        }
    }
    for (name, text) in ingested {
        evaluator
            .env
            .bind(name, rocflight::eval::str_value(text));
    }
    let _value = evaluator.eval(&ast)?;

    // Step 5: Execute app entry point if present
    // A file with no entry point is a MODULE: walking it has already run its
    // `expect`s, which is all roc does for one too. Several of the language's own
    // examples are modules of nothing but expects.
    let is_module = match &app_entry_point {
        Some(name) => evaluator.env.lookup(name).is_none(),
        None => true,
    };

    if test_mode {
        return report_tests();
    }

    match app_entry_point {
        Some(entry_name) if !is_module => invoke_app_entry_point(&mut evaluator, &entry_name)?,
        // A module produces no output of its own.
        Some(_) => {}
        None => println!("{}", _value),
    }

    Ok(())
}

/// Report the `expect` tally, the way `roc test` does.
///
/// The tally is process-wide, and both engines add to the same one, so this is one
/// function rather than one per engine.
fn report_tests() -> Result<(), Box<dyn Error>> {
    let (ran, failed) = rocflight::eval::expect_tally();
    if failed == 0 {
        println!("All ({}) tests passed", ran);
    } else {
        println!("Ran {} tests:", ran);
        println!("    {} passed", ran - failed);
        println!("    {} failed", failed);
        process::exit(1);
    }
    Ok(())
}

/// Invoke the app entry point function
fn invoke_app_entry_point(
    evaluator: &mut Evaluator,
    entry_name: &str,
) -> Result<(), Box<dyn Error>> {
    // Validate entry point name is not empty
    if entry_name.is_empty() {
        return Err("Empty app entry point name".into());
    }

    // `!` is part of the name (e.g. `main!`), so look it up verbatim.
    let entry_fn = evaluator
        .env
        .lookup(entry_name)
        .ok_or_else(|| format!("App entry point '{}' not found", entry_name))?;

    // Match on the entry point type
    match entry_fn {
        Value::Lambda(l) => {
            let arity = l.params.len();

            // The entry point takes the command-line arguments — a LIST, matching
            // `main! : List(Str) => ...`. Passing a string here made `args` the wrong
            // shape for anything that inspected it.
            //
            // ponytail: always empty. Real argv needs the host to supply it.
            let args: Vec<Value> = match arity {
                0 => Vec::new(),
                1 => vec![Value::List(Vec::new())],
                n => {
                    return Err(format!(
                        "App entry point expects {} arguments, only 0 or 1 supported",
                        n
                    )
                    .into())
                }
            };

            // Shared with every other call, so `return` unwinds here too.
            rocflight::eval::apply(Value::Lambda(l), args)?;

            Ok(())
        }
        _ => Err(format!("App entry point '{}' is not a function", entry_name).into()),
    }
}
