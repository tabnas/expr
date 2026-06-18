# Tutorial: parse and evaluate your first expression

This is a learning-oriented walk-through. Follow it top to bottom and you
will go from nothing to a working expression evaluator. One happy path, no
detours.

By the end you will have:

1. Parsed `1+2*3` into an S-expression tree.
2. Seen how operator precedence shapes that tree.
3. Plugged in an evaluator to compute a number.
4. Dropped an expression inside ordinary JSON.

`@tabnas/expr` is a plugin for the Tabnas/jsonic parser. It does not parse
on its own — you stack it on top of the relaxed-JSON `jsonic` grammar.

---

## 1. Install

```sh
npm install @tabnas/expr @tabnas/parser @tabnas/jsonic
```

`@tabnas/parser` is the engine, `@tabnas/jsonic` is the base JSON grammar,
and `@tabnas/expr` is this plugin.

## 2. Parse an expression

Build a parser by stacking the two plugins, then call `parse`:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Expr } = require('@tabnas/expr')

const j = new Tabnas().use(jsonic).use(Expr)

const tree = j.parse('1+2*3')
```

The result is a LISP-style S-expression: an array whose **first element is
the operator** and whose remaining elements are the operands. The operator
element is a rich `Op` object, but its source string is on `op.src`, so the
shape of `1+2*3` is:

```text
[ +op, 1, [ *op, 2, 3 ] ]
```

Notice that `*` binds tighter than `+`, so `2*3` became a nested
sub-expression. You did not write any parentheses — precedence did that.

To make the tree easy to read (and to assert on), replace each `Op` with
its `src` string. Here is a tiny helper plus a check the test harness
verifies:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Expr } = require('@tabnas/expr')

// Replace each Op with its source string for a readable tree.
const S = (x) =>
  Array.isArray(x) && x.length
    ? [x[0].src || x[0].osrc || S(x[0]), ...x.slice(1).map(S)]
    : x

const j = new Tabnas().use(jsonic).use(Expr)
const show = (s) => JSON.parse(JSON.stringify(S(j.parse(s))))

show('1+2')      // => ['+', 1, 2]
show('1+2*3')    // => ['+', 1, ['*', 2, 3]]
show('-1+2')     // => ['+', ['-', 1], 2]
```

The last line shows the unary prefix `-`: `-1` is the one-operand
expression `['-', 1]`.

## 3. Evaluate to a value

A parse tree is structure, not a number. To compute, supply an
`evaluate` callback. It is called bottom-up — operands are already
evaluated by the time your callback sees an operator. Its signature is
`(rule, ctx, op, terms)`, where `terms` is the **array** of
already-evaluated operands.

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Expr } = require('@tabnas/expr')

const math = (rule, ctx, op, terms) => {
  switch (op.src) {
    case '+': return op.prefix ? +terms[0] : terms[0] + terms[1]
    case '-': return op.prefix ? -terms[0] : terms[0] - terms[1]
    case '*': return terms[0] * terms[1]
    case '/': return terms[0] / terms[1]
    case '(': return terms[0]            // plain grouping parens
    default:  return NaN
  }
}

const j = new Tabnas().use(jsonic).use(Expr, { evaluate: math })

j.parse('1+2*3')     // => 7
j.parse('(1+2)*3')   // => 9
j.parse('-4+10')     // => 6
```

Two things to note:

- `op.prefix` is `true` for the unary `-`/`+`, so the callback knows to
  negate rather than subtract.
- `(1+2)*3` evaluates to `9`: parentheses are the built-in `plain` paren
  operator (`op.src === '('`), which here just returns its inner value.

## 4. Expressions inside JSON

Because `Expr` layers on jsonic, expressions slot into any value
position — map values, list elements, nested objects — and your
evaluator runs on each one:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Expr } = require('@tabnas/expr')

const math = (rule, ctx, op, terms) => {
  switch (op.src) {
    case '+': return op.prefix ? +terms[0] : terms[0] + terms[1]
    case '-': return op.prefix ? -terms[0] : terms[0] - terms[1]
    case '*': return terms[0] * terms[1]
    case '(': return terms[0]
    default:  return NaN
  }
}

const j = new Tabnas().use(jsonic).use(Expr, { evaluate: math })

j.parse('{ a: 1+2, b: [3*4, -5] }')   // => { a: 3, b: [12, -5] }
```

That is the whole happy path: install, parse, evaluate, embed.

## Next steps

- [Guide](guide.md) — recipes: add a custom operator, build function-call
  syntax, restrict to strict math, walk the tree yourself.
- [Reference](reference.md) — the exact exports, `OpDef` options, the
  default operator table, and the `Op`/`Evaluate` types.
- [Concepts](concepts.md) — how Pratt parsing and the binding-power scale
  work, and why the AST is built the way it is.
