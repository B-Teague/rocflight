//! Instructions the compiler emits in pairs that the machine can run as one.
//!
//! Every fusion here is a rewrite IN PLACE — one instruction becomes another, none is
//! added or removed — so no jump target moves and nothing has to be renumbered. That is
//! the whole reason these are the fusions that got written and not others.

use super::Op;

/// Fuse what can be fused. Runs on finished code, before liveness.
pub fn fuse(code: &mut [Op]) {
    fuse_loop_back_edges(code);
}

/// A `for` loop's back edge is a `Jump` to the `IterNext` that tests it. That is two
/// dispatches per iteration where one will do.
///
/// The compiler lays a loop out as
///
/// ```text
///   i     IterNext { dst, iter, idx, to: exit }   // fall through with an element
///   ...   the body
///   j     Jump { to: i }
///   exit  ...
/// ```
///
/// so the `Jump` at `j` and the `IterNext` at `i` always run back to back, and `exit`
/// is always `j + 1`. Replacing the `Jump` with an `IterNextBack` that jumps to the
/// BODY when there is an element and falls through when there is not does both in one
/// instruction: a third of everything `tests/bench/loop.roc` executes, a quarter of
/// `iter_range`.
///
/// Each of those three facts is checked rather than assumed, and a loop that does not
/// have this exact shape is left alone.
fn fuse_loop_back_edges(code: &mut [Op]) {
    // An instruction that something JUMPS to cannot be rewritten into one that means
    // something else on arrival — a `continue` lands on the back edge, and arriving
    // there must still mean "go round again", not "step the iterator twice".
    let mut jumped_to = vec![false; code.len()];
    for (j, op) in code.iter().enumerate() {
        super::liveness::successors(op, j, code.len(), |s| {
            if s != j + 1 && s < jumped_to.len() {
                jumped_to[s] = true;
            }
        });
    }
    for j in 0..code.len() {
        let Op::Jump { to } = code[j] else { continue };
        let head = to as usize;
        // A back edge, to an `IterNext` whose exit is exactly where this `Jump` falls
        // through to. Anything else is not the loop this fuses.
        if head >= j || jumped_to[j] {
            continue;
        }
        let Op::IterNext { dst, iter, idx, to: exit } = code[head] else { continue };
        if exit as usize != j + 1 {
            continue;
        }
        code[j] = Op::IterNextBack { dst, iter, idx, to: (head + 1) as u32 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loop_shape() -> Vec<Op> {
        vec![
            Op::IterNext { dst: 4, iter: 2, idx: 3, to: 3 },
            Op::BinInt { dst: 1, a: 1, b: 4, op: super::super::BinOp::Add, width: 192 },
            Op::Jump { to: 0 },
            Op::Ret { src: 1 },
        ]
    }

    #[test]
    fn a_loops_back_edge_becomes_the_step() {
        let mut code = loop_shape();
        fuse(&mut code);
        assert!(matches!(code[2], Op::IterNextBack { dst: 4, iter: 2, idx: 3, to: 1 }), "{:?}", code[2]);
    }

    /// `continue` lands on the back edge. Arriving there must still mean "go round",
    /// and an `IterNextBack` would step the iterator a second time instead.
    #[test]
    fn a_back_edge_something_jumps_to_is_left_alone() {
        let mut code = loop_shape();
        code.insert(1, Op::Jump { to: 3 }); // a `continue`, targeting the back edge
        code[0] = Op::IterNext { dst: 4, iter: 2, idx: 3, to: 4 };
        code[3] = Op::Jump { to: 0 };
        fuse(&mut code);
        assert!(matches!(code[3], Op::Jump { to: 0 }), "{:?}", code[3]);
    }

    /// A plain backward `Jump` that is not a loop's iterator test.
    #[test]
    fn an_ordinary_backward_jump_is_left_alone() {
        let mut code = vec![Op::LoadK { dst: 0, k: 0 }, Op::Jump { to: 0 }, Op::Ret { src: 0 }];
        fuse(&mut code);
        assert!(matches!(code[1], Op::Jump { to: 0 }), "{:?}", code[1]);
    }
}
