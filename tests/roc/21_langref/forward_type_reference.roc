# Syntax: a type named before it is declared.
#
# Declarations may come in any order: `State` refers to `Position`, which is only
# declared afterwards. What the interpreter must NOT do is treat the not-yet-seen
# name as an unknown and let every numeral that meets it default to a fraction —
# that turned `(pos.x + 3) % 20` into `Rem` on a `Dec`, and it was the whole game
# of Snake that found it.
#
# The declarations STAY: they are the syntax under test.
app [main!] {}

State : { pos : Position, tail : List(Position) }
Position : { x : I64, y : I64 }

initial = { pos: { x: 10, y: 10 }, tail: [{ x: 9, y: 10 }] }

step : State -> State
step = |s| { ..s, pos: { x: s.pos.x + 1, y: s.pos.y } }

main! = |_args| {
    moved = step(step(initial))
    echo!("${((moved.pos.x + 3) % 20).to_str()},${moved.tail.len().to_str()}")
    Ok({})
}
