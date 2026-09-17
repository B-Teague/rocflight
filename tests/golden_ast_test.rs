//! The golden pairs must describe the same program.
//!
//! Every syntax feature under `tests/roc/` is a pair: a sugared file and a
//! `.desugared.roc` sibling that spells out by hand what desugaring is supposed to
//! produce. They differ only in sugar, so they must build the same AST — that is the
//! definition of the desugarer being right, and it is what proves the parser handles
//! both syntaxes.
//!
//! This used to be `rocflight --ast-only` run twice from `tests/check_roc.sh`. It needs
//! neither the `roc` compiler nor a built binary, only the library, so it belongs here:
//! the check is not a thing the interpreter does, and does not need a flag to ask for.
//!
//! Only the AST STRUCTURE is compared, not the inferred type. Annotations are not
//! sugar: the desugared file declares types the sugared one leaves to inference, so its
//! type is legitimately more specific (`List(Str) -> ...` versus `$0 -> ...`).

use std::path::{Path, PathBuf};

use rocflight::desugaring::Desugarer;
use rocflight::parser::Parser;

/// Desugar and parse one file, rendering its AST.
///
/// `None` means the file does not parse. A pair whose feature is not implemented yet
/// fails loudly in `check_roc.sh`, against `roc` itself, which is the gate that should
/// report it; repeating it here would report the same gap twice.
fn ast_of(path: &Path) -> Option<String> {
    let source = std::fs::read_to_string(path).ok()?;
    let desugared = Desugarer::new(source).desugar().ok()?;
    let mut parser = Parser::named(&path.display().to_string(), &desugared);
    Some(format!("{}", parser.parse_expr().ok()?))
}

/// Every `.roc` under `tests/roc/` that is not itself a `.desugared.roc`.
fn sugared_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", dir.display(), e))
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        // `examples/` and `bench/` are whole programs, not pairs; `check_roc.sh` skips
        // them on the same two names.
        if path.is_dir() {
            if matches!(path.file_name().and_then(|n| n.to_str()), Some("examples") | Some("bench")) {
                continue;
            }
            sugared_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "roc")
            && !path.to_string_lossy().ends_with(".desugared.roc")
        {
            out.push(path);
        }
    }
}

#[test]
fn golden_pairs_build_the_same_ast() {
    let mut sugared = Vec::new();
    sugared_files(Path::new("tests/roc"), &mut sugared);
    assert!(sugared.len() > 50, "expected the golden corpus, found {} files", sugared.len());

    let (mut compared, mut differ) = (0, Vec::new());
    for path in &sugared {
        let des = PathBuf::from(format!("{}.desugared.roc", path.display().to_string().trim_end_matches(".roc")));
        assert!(des.exists(), "{} has no .desugared.roc sibling", path.display());

        // Both sides must parse for the comparison to mean anything.
        let (Some(a), Some(b)) = (ast_of(path), ast_of(&des)) else { continue };
        compared += 1;
        if a != b {
            differ.push(format!("{}\n  sugared:   {}\n  desugared: {}", path.display(), a, b));
        }
    }

    assert!(
        differ.is_empty(),
        "{} of {} pairs build different ASTs:\n{}",
        differ.len(),
        compared,
        differ.join("\n")
    );
    assert!(compared > 50, "only {} pairs parsed on both sides", compared);
}
