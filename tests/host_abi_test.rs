//! Every hosted function basic-cli declares has a call shape rocflight can make.
//!
//! The platform's `Host.roc` is the signatures, `main.roc`'s `hosted { … }` block is
//! the symbols, and `IOErr.roc` and the `Internal*.roc` modules are the types those
//! signatures name. All vendored under `tests/fixtures/basic-cli-0.22.0/`, so this
//! runs without roc's cache. The gate is `abi::Plan::of` accepting all sixty.

use std::collections::HashMap;
use std::path::Path;

use rocflight::desugaring::Desugarer;
use rocflight::parser::Parser;
use rocflight::platform::abi::{Plan, Signature};
use rocflight::platform::layout::Declarations;
use rocflight::types::Type;

const FIXTURES: &str = "tests/fixtures/basic-cli-0.22.0";

/// Parse one platform module for what it declares: its signatures and its types.
fn declarations(file: &str) -> (Vec<(&'static str, Type)>, Vec<(&'static str, Type)>) {
    let path = Path::new(FIXTURES).join(file);
    let source = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {}", path.display(), e));
    let desugared = Desugarer::new(source).desugar().expect("desugars");
    let desugared: &'static str = Box::leak(desugared.into_boxed_str());
    let mut parser = Parser::named(&path.display().to_string(), desugared);
    parser.parse_expr().unwrap_or_else(|e| panic!("{}: {}", file, e));
    (parser.signatures().to_vec(), parser.nominals().to_vec())
}

#[test]
fn every_basic_cli_hosted_function_has_a_call_shape() {
    let (host_sigs, _host_types) = declarations("Host.roc");
    let mut decls: HashMap<String, Type> = HashMap::new();
    for file in ["IOErr.roc", "InternalHttp.roc", "InternalSqlite.roc", "InternalDateTime.roc", "Host.roc"] {
        let (_, types) = declarations(file);
        for (name, ty) in types {
            // `Host.NativeOsStr` and bare `NativeOsStr` are the same declaration.
            let bare = name.rsplit('.').next().unwrap_or(name);
            decls.insert(bare.to_string(), ty.clone());
            decls.insert(name.to_string(), ty);
        }
    }
    let decls = Declarations::new(decls);

    let main = std::fs::read_to_string(Path::new(FIXTURES).join("main.roc")).expect("main.roc");
    let hosted: Vec<(&str, &str)> = main
        .lines()
        .filter_map(|l| {
            // `"hosted_stdin_bytes": Host.stdin_bytes!,`
            let l = l.trim();
            let (symbol, member) = l.strip_prefix('"')?.split_once("\": ")?;
            Some((symbol, member.trim_end_matches(',')))
        })
        .collect();
    assert_eq!(hosted.len(), 60, "basic-cli 0.22.0 declares 60 hosted functions");

    let mut failures = Vec::new();
    for (symbol, member) in &hosted {
        let Some((_, ty)) = host_sigs.iter().find(|(n, _)| n == member) else {
            failures.push(format!("{}: `{}` has no signature in Host.roc", symbol, member));
            continue;
        };
        match Signature::of(ty, &decls).and_then(|sig| Plan::of(&sig)) {
            Ok(_) => {}
            Err(e) => failures.push(format!("{}: {} — {}", symbol, ty, e)),
        }
    }
    assert!(failures.is_empty(), "{} of {} cannot be called:\n  {}", failures.len(), hosted.len(), failures.join("\n  "));
}
