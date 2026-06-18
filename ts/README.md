# @tabnas/expr

An expression-syntax plugin for the [Tabnas](https://github.com/tabnas/jsonic)
parser, available in both TypeScript and Go.

Adds Pratt-parser expressions to Tabnas: infix, prefix, suffix, ternary,
and paren operators with a configurable binding-power (precedence) scale, so
values can include arithmetic/logical expressions. Expressions parse into
LISP-style S-expressions (arrays whose first element is the operator `Op`),
which a user-supplied evaluator can reduce to values.

[![npm version](https://img.shields.io/npm/v/@tabnas/expr.svg)](https://npmjs.com/package/@tabnas/expr)
[![build](https://github.com/tabnas/expr/actions/workflows/build.yml/badge.svg)](https://github.com/tabnas/expr/actions/workflows/build.yml)

## Install

```sh
npm install @tabnas/expr @tabnas/parser @tabnas/jsonic
```

## Tiny example

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Expr } = require('@tabnas/expr')

// Show parse trees with each Op replaced by its source string.
const S = (x) => Array.isArray(x) && x.length ? [x[0].src || x[0].osrc || S(x[0]), ...x.slice(1).map(S)] : x

const j = new Tabnas().use(jsonic).use(Expr)
const show = (s) => JSON.parse(JSON.stringify(S(j.parse(s))))

show('1+2*3')   // => ['+', 1, ['*', 2, 3]]
show('-1+2')    // => ['+', ['-', 1], 2]
```

Add an `evaluate` callback to compute values instead:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Expr } = require('@tabnas/expr')

const math = (rule, ctx, op, terms) => {
  switch (op.src) {
    case '+': return op.prefix ? +terms[0] : terms[0] + terms[1]
    case '*': return terms[0] * terms[1]
    case '(': return terms[0]
    default:  return NaN
  }
}

const j = new Tabnas().use(jsonic).use(Expr, { evaluate: math })

j.parse('1+2*3')     // => 7
j.parse('(1+2)*3')   // => 9
```

## Documentation

Docs follow the [Diátaxis](https://diataxis.fr) framework:

- **[Tutorial](doc/tutorial.md)** — parse and evaluate your first
  expression, step by step.
- **[Guide](doc/guide.md)** — recipes: add a custom operator, build
  function-call syntax with paren-preval, add a ternary, restrict to strict
  math, walk the tree yourself.
- **[Reference](doc/reference.md)** — exports, `OpDef` options, the `Op` /
  `Evaluate` types, and the default operator table.
- **[Concepts](doc/concepts.md)** — how Pratt parsing works, the
  binding-power scale, and the design trade-offs.

Go port docs: [tutorial](../go/doc/tutorial.md) ·
[guide](../go/doc/guide.md) · [reference](../go/doc/reference.md) ·
[concepts](../go/doc/concepts.md).

## Grammar diagram

The installed grammar as a railroad/syntax diagram, generated from the live
grammar with [`@tabnas/railroad`](https://github.com/tabnas/railroad):

![expr grammar railroad diagram](doc/grammar.svg)

A vertical ASCII version is in [`doc/grammar.txt`](doc/grammar.txt).

## License

MIT. See [LICENSE](LICENSE).
