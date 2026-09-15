# Syntax: operators dispatch to METHODS — `a + b` is `a.plus(b)`.
#
# Every operator in roc is spelled as a method on its left operand, so a type that
# defines the method gets the operator:
#
#     +  plus        /   div_by         <   is_lt     ==  is_eq
#     -  minus       //  div_trunc_by   >   is_gt     !=  is_eq, negated
#     *  times       %   rem_by         <=  is_lte    -x  negate
#                                       >=  is_gte
app [main!] {}

Money :: { cents: I64 }.{
    plus = |a, b| { cents: a.cents + b.cents }

    minus = |a, b| { cents: a.cents - b.cents }

    negate = |a| { cents: 0 - a.cents }

    is_eq = |a, b| a.cents == b.cents
}

main! = |_args| {
    a = Money.{ cents: 5 }
    b = Money.{ cents: 7 }
    sum = a + b
    diff = a - b
    neg = -a
    echo!("${sum.cents.to_str()},${diff.cents.to_str()},${neg.cents.to_str()}")
    echo!("${Str.inspect(a == b)},${Str.inspect(a != b)},${Str.inspect(a == a)}")
    Ok({})
}
