# Guide: expression recipes

Task-focused recipes for real problems. Each is self-contained. They assume
you already did the [tutorial](tutorial.md) and know that `Expr` stacks on
`jsonic`, that the parse result is an S-expression array, and that an
`evaluate` callback receives `(rule, ctx, op, terms)` with `terms` as an
array of evaluated operands.

A small `S` helper (replace each `Op` with its `src`) is used to show parse
trees readably:

```js
const S = (x) =>
  Array.isArray(x) && x.length
    ? [x[0].src || x[0].osrc || S(x[0]), ...x.slice(1).map(S)]
    : x
```

---

## Add a custom infix operator

Pass an `op` map. Each entry is an [`OpDef`](reference.md#opdef). For an
infix operator give `infix: true`, the source string `src`, and a
binding-power pair (`left`, `right`). `left < right` is left-associative.

Pick a tier base `N * 1000000` that does not collide with the built-ins
(addition is `2000000`, multiplication `3000000`, unary prefix `4000000`).
Here `^` (exponent) is tighter than multiplication and **right**-associative
(`left > right`):

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Expr } = require('@tabnas/expr')
const S = (x) => Array.isArray(x) && x.length ? [x[0].src || x[0].osrc || S(x[0]), ...x.slice(1).map(S)] : x

const j = new Tabnas().use(jsonic).use(Expr, {
  op: {
    power: { infix: true, src: '^', left: 5100000, right: 5000000 },
  },
})
const show = (s) => JSON.parse(JSON.stringify(S(j.parse(s))))

show('2^3^2')   // => ['^', 2, ['^', 3, 2]]
show('1+2^3')   // => ['+', 1, ['^', 2, 3]]
```

`2^3^2` nests to the right and `^` out-binds `+`, exactly as the powers say.

## Add a prefix and a suffix operator

Prefix operators take only a `right` power; suffix operators take only a
`left` power. The unset side falls back to the loosest/tightest extreme, so
within a run of unary operators only their *relative* tightness against
neighbouring infix operators matters.

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Expr } = require('@tabnas/expr')
const S = (x) => Array.isArray(x) && x.length ? [x[0].src || x[0].osrc || S(x[0]), ...x.slice(1).map(S)] : x

const j = new Tabnas().use(jsonic).use(Expr, {
  op: {
    factorial: { suffix: true, src: '!', left: 6000000 },
    at:        { prefix: true, src: '@', right: 5000000 },
  },
})
const show = (s) => JSON.parse(JSON.stringify(S(j.parse(s))))

show('1+2!')    // => ['+', 1, ['!', 2]]
show('@1+2')    // => ['+', ['@', 1], 2]
```

The factorial binds tighter than `+`, so it grabs only the `2`; the prefix
`@` binds to its single operand `1`.

## Define a function-call syntax (paren-preval)

A "preval" paren is a paren operator that may take a **preceding value** as
its first term — exactly the shape of a function call `f(args)` or an index
`a[i]`. Add `preval` to a `paren` op.

With `preval: { active: true }`, the paren works both with and without a
leading value:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Expr } = require('@tabnas/expr')
const S = (x) => Array.isArray(x) && x.length ? [x[0].src || x[0].osrc || S(x[0]), ...x.slice(1).map(S)] : x

const j = new Tabnas().use(jsonic).use(Expr, {
  op: {
    plain: { paren: true, osrc: '(', csrc: ')', preval: { active: true } },
  },
})
const show = (s) => JSON.parse(JSON.stringify(S(j.parse(s))))

show('foo(1,2)')   // => ['(', 'foo', [1, 2]]
show('(1+2)')      // => ['(', ['+', 1, 2]]
```

`foo(1,2)` becomes `['(', 'foo', [1,2]]`: the operator, then the function
name, then the implicit-list argument array. A bare `(1+2)` (no preceding
value) is still just a grouping paren.

To **evaluate** calls, dispatch on the paren op and treat the first term as
the function name:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Expr } = require('@tabnas/expr')

const fns = { max: Math.max, min: Math.min, sum: (...a) => a.reduce((x, y) => x + y, 0) }

const evaluate = (rule, ctx, op, terms) => {
  switch (op.name) {
    case 'addition-infix':       return terms[0] + terms[1]
    case 'multiplication-infix': return terms[0] * terms[1]
    case 'plain-paren': {
      if (typeof terms[0] === 'string') {           // f(args) call
        let args = terms.slice(1)
        if (args.length === 1 && Array.isArray(args[0])) args = args[0]
        const fn = fns[terms[0]]
        return fn ? fn(...args) : NaN
      }
      return terms[0]                                // plain grouping
    }
    default: return NaN
  }
}

const j = new Tabnas().use(jsonic).use(Expr, {
  op: { plain: { paren: true, osrc: '(', csrc: ')', preval: { active: true } } },
  evaluate,
})

j.parse('max(1,2)')       // => 2
j.parse('sum(1,2,3)')     // => 6
j.parse('1+max(2,5)')     // => 6
j.parse('(1+2)*3')        // => 9
```

Use `preval: { required: true }` for an index-style paren that *must* have a
leading value (so `[1]` is a literal list, but `a[1]` is an index op). You
can also restrict which names a preval paren accepts with
`preval: { active: true, allow: ['foo', 'bar'] }`.

## Chain calls and indexes

Preval parens chain: each successive paren takes the previous result as its
preval. This gives you `f(x)(y)`, `a[0][1]`, and mixed forms for free:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Expr } = require('@tabnas/expr')
const S = (x) => Array.isArray(x) && x.length ? [x[0].src || x[0].osrc || S(x[0]), ...x.slice(1).map(S)] : x

const j = new Tabnas().use(jsonic).use(Expr, {
  op: {
    index: { osrc: '[', csrc: ']', paren: true, preval: { required: true } },
    call:  { osrc: '(', csrc: ')', paren: true, preval: { active: true } },
    plain: null,   // disable the default `(`…`)` so `(` only means `call`
  },
})
const show = (s) => JSON.parse(JSON.stringify(S(j.parse(s))))

show('a[0][1]')   // => ['[', ['[', 'a', 0], 1]
show('f(x)[i]')   // => ['[', ['(', 'f', 'x'], 'i']
```

Setting an op entry to `null` removes it — here it removes the default
`plain` paren so that `(` is unambiguously the `call` operator.

## Add a ternary (conditional) operator

A ternary op has two source markers in `src` (open and close, e.g. `?` and
`:`). Ternaries are right-associative, so they chain like `cond ? a : cond2
? b : c`.

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Expr } = require('@tabnas/expr')
const S = (x) => Array.isArray(x) && x.length ? [x[0].src || x[0].osrc || S(x[0]), ...x.slice(1).map(S)] : x

const j = new Tabnas().use(jsonic).use(Expr, {
  op: { cond: { ternary: true, src: ['?', ':'] } },
})
const show = (s) => JSON.parse(JSON.stringify(S(j.parse(s))))

show('1?2:3')        // => ['?', 1, 2, 3]
show('1?2: 0?4:5')   // => ['?', 1, 2, ['?', 0, 4, 5]]
```

The S-expression for a ternary has three operands: condition, then-branch,
else-branch. Evaluate it by branching:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Expr } = require('@tabnas/expr')

const evaluate = (rule, ctx, op, terms) =>
  op.name === 'cond-ternary' ? (terms[0] ? terms[1] : terms[2]) : NaN

const j = new Tabnas().use(jsonic).use(Expr, {
  op: { cond: { ternary: true, src: ['?', ':'] } },
  evaluate,
})

j.parse('1?2:3')   // => 2
j.parse('0?2:3')   // => 3
```

## Restrict to strict math (drop a default operator)

The defaults include `+ - * / %` and the `plain` paren. To remove any of
them, set the op entry to `null`. Here `%` (remainder) and `/` (division)
are dropped:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Expr } = require('@tabnas/expr')
const S = (x) => Array.isArray(x) && x.length ? [x[0].src || x[0].osrc || S(x[0]), ...x.slice(1).map(S)] : x

const j = new Tabnas().use(jsonic).use(Expr, {
  op: { remainder: null, division: null },
})
const show = (s) => JSON.parse(JSON.stringify(S(j.parse(s))))

show('1+2*3')   // => ['+', 1, ['*', 2, 3]]
```

`+`, `-`, `*` still work; `/` and `%` are now ordinary text.

## Walk the tree yourself with `evaluation()`

If you parse with **no** `evaluate` option you get the raw S-expression, and
you can reduce it later (or repeatedly, with different semantics) by calling
the exported `evaluation` function. It walks the tree bottom-up and calls
your resolver for each op:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Expr, evaluation } = require('@tabnas/expr')

const j = new Tabnas().use(jsonic).use(Expr)
const tree = j.parse('1+2*3')   // raw S-expression, no evaluation

const math = (rule, ctx, op, terms) => {
  switch (op.src) {
    case '+': return terms[0] + terms[1]
    case '*': return terms[0] * terms[1]
    default:  return NaN
  }
}

evaluation(null, null, tree, math)   // => 7
```

This separates *parse* from *evaluate*: parse once, evaluate many times.
