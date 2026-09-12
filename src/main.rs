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
use std::process;

use rocflight::parser::Parser;
use rocflight::types::TypeChecker;
use rocflight::eval::Evaluator;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <file.roc>", args[0]);
        process::exit(1);
    }

    let filename = &args[1];

    // Parse (includes desugaring + parsing)
    let (ast, app_entry_point) = match Parser::from_file(filename) {
        Ok((expr, entry)) => (expr, entry),
        Err(e) => {
            eprintln!("Parse error: {}", e);
            process::exit(1);
        }
    };

    // Type check
    let mut type_checker = TypeChecker::new();
    if let Err(e) = type_checker.synth(&ast) {
        eprintln!("{}", e);
        process::exit(1);
    }

    // Evaluate
    let mut evaluator = Evaluator::new();
    match evaluator.eval(&ast) {
        Ok(value) => {
            // If there's an app entry point, invoke it
            if let Some(entry_name) = app_entry_point {
                // Validate entry point name is not empty
                if entry_name.is_empty() {
                    eprintln!("Error: Empty app entry point name");
                    process::exit(1);
                }

                // Note: desugarer removes trailing !, so entry_name might be "main" or "main!"
                // Handle both cases by stripping ! if present
                let lookup_name = if entry_name.ends_with('!') {
                    &entry_name[..entry_name.len() - 1]
                } else {
                    &entry_name
                };

                // Try to find the entry point in the environment and call it
                if let Some(entry_fn) = evaluator.env.lookup(lookup_name) {
                    match entry_fn {
                        rocflight::eval::Value::Lambda { params, body, env: lambda_env } => {
                            // Create a new evaluator with the lambda's environment
                            let mut lambda_eval = Evaluator { env: lambda_env };
                            lambda_eval.env.push_scope();

                            // Bind parameters to arguments
                            if params.len() == 1 {
                                // Pass empty string as args (Roc CLI args not yet supported)
                                let args_value = rocflight::eval::Value::Str("");
                                lambda_eval.env.bind(params[0], args_value);

                                match lambda_eval.eval(&body) {
                                    Ok(_result) => {
                                        // App entry point executed successfully
                                        // Output is handled by the program itself (e.g., Stdout.line!)
                                    }
                                    Err(e) => {
                                        eprintln!("Runtime error in app entry point: {}", e);
                                        process::exit(1);
                                    }
                                }
                            } else if params.is_empty() {
                                match lambda_eval.eval(&body) {
                                    Ok(_result) => {
                                        // App entry point executed successfully
                                        // Output is handled by the program itself
                                    }
                                    Err(e) => {
                                        eprintln!("Runtime error in app entry point: {}", e);
                                        process::exit(1);
                                    }
                                }
                            } else {
                                eprintln!("App entry point expects {} arguments, only 0 or 1 supported", params.len());
                                process::exit(1);
                            }
                        }
                        _ => {
                            eprintln!("App entry point '{}' is not a function", entry_name);
                            process::exit(1);
                        }
                    }
                } else {
                    eprintln!("App entry point '{}' not found", entry_name);
                    process::exit(1);
                }
            } else {
                // No app entry point - for non-app files, print the result
                // (useful for REPL-like testing without "Result:" prefix)
                println!("{}", value);
            }
        }
        Err(e) => {
            eprintln!("Runtime error: {}", e);
            process::exit(1);
        }
    }
}
