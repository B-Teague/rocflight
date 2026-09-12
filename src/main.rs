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

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <file.roc>", args[0]);
        process::exit(1);
    }

    let filename = &args[1];

    // Run the interpreter with proper error handling
    if let Err(e) = run(filename) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

/// Main interpreter pipeline with Result-based error handling
fn run(filename: &str) -> Result<(), Box<dyn Error>> {
    // Step 1: Parse (includes desugaring + parsing)
    let (ast, app_entry_point) = Parser::from_file(filename)?;

    // Step 2: Type check
    let mut type_checker = TypeChecker::new();
    type_checker.synth(&ast)?;

    // Step 3: Evaluate
    let mut evaluator = Evaluator::new();
    let _value = evaluator.eval(&ast)?;

    // Step 4: Execute app entry point if present
    if let Some(entry_name) = app_entry_point {
        invoke_app_entry_point(&mut evaluator, &entry_name)?;
    } else {
        // No app entry point - for non-app files, print the result
        println!("{}", _value);
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

    // Note: desugarer removes trailing !, so entry_name might be "main" or "main!"
    // Handle both cases by stripping ! if present
    let lookup_name = if entry_name.ends_with('!') {
        &entry_name[..entry_name.len() - 1]
    } else {
        entry_name
    };

    // Look up the entry point function
    let entry_fn = evaluator
        .env
        .lookup(lookup_name)
        .ok_or_else(|| format!("App entry point '{}' not found", entry_name))?;

    // Match on the entry point type
    match entry_fn {
        Value::Lambda { params, body, env: lambda_env } => {
            // Create a new evaluator with the lambda's captured environment
            let mut lambda_eval = Evaluator { env: lambda_env };
            lambda_eval.env.push_scope();

            // Handle different arities
            match params.len() {
                1 => {
                    // Pass empty string as args (Roc CLI args not yet supported)
                    let args_value = Value::Str("");
                    lambda_eval.env.bind(params[0], args_value);
                    lambda_eval.eval(&body)?;
                }
                0 => {
                    // No parameters needed
                    lambda_eval.eval(&body)?;
                }
                n => {
                    return Err(
                        format!("App entry point expects {} arguments, only 0 or 1 supported", n)
                            .into(),
                    );
                }
            }

            Ok(())
        }
        _ => Err(format!("App entry point '{}' is not a function", entry_name).into()),
    }
}
