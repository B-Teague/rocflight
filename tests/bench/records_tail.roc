# records.roc's work, written so BOTH engines can run it. 40,000 record updates and
# expect: 40000
# field reads.
go = |i, point| if i == 40000 { point.x } else { go(i + 1, { ..point, x: point.x + 1, y: point.y - 1 }) }

go(0, { x: 0, y: 0 })
