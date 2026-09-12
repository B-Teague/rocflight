app [main] { pf: platform "https://github.com/roc-lang/basic-cli/releases/download/0.20.0/X73hGh05nNTkDHU06FHC0YfFaQB1pimX7gncRcao5mU.tar.br" }

import pf.Stdout

birds = -3

main = |_args|
Stdout.line("There are ${Num.to_str(birds)} birds.")
