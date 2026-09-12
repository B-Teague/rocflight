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
    let ast = match Parser::from_file(filename) {
        Ok(expr) => expr,
        Err(e) => {
            eprintln!("Parse error: {}", e);
            process::exit(1);
        }
    };

    // Type check
    let mut type_checker = TypeChecker::new();
    match type_checker.synth(&ast) {
        Ok(ty) => println!("Type: {}", ty),
        Err(e) => {
            eprintln!("Type error: {}", e);
            process::exit(1);
        }
    }

    // Evaluate
    let mut evaluator = Evaluator::new();
    match evaluator.eval(&ast) {
        Ok(value) => println!("Result: {}", value),
        Err(e) => {
            eprintln!("Runtime error: {}", e);
            process::exit(1);
        }
    }
}
