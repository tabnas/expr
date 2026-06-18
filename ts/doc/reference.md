# Reference

Exact API surface of `@tabnas/expr`. Dry and complete. Behaviour is grounded
in `ts/src/expr.ts` and the test suite.

## Package

```js
const { Expr, evaluation, testing } = require('@tabnas/expr')
```

ESM / TypeScript:

```ts
import { Expr, evaluation, testing } from '@tabnas/expr'
import type { ExprOptions, OpDef, Op, Evaluate } from '@tabnas/expr'
```

The package is library-only — there is no CLI. `main` is `dist/expr.js`,
types are `dist/expr.d.ts`.

## Exports

| Export | Kind | Description |
|---|---|---|
| `Expr` | Plugin | The expression plugin. Apply with `new Tabnas().use(jsonic).use(Expr, options?)`. |
| `evaluation` | function | Standalone reducer for a parsed S-expression tree (see below). |
| `testing` | object | Internal test hooks: `{ prattify, opify }`. Not part of the stable API. |
| `Expr.defaults` | object | The default options (`{ op: { … } }`), see [Default operators](#default-operators). |

Type-only exports: `ExprOptions`, `OpDef`, `Op`, `Evaluate`.

## Using the plugin

`Expr` is a jsonic/tabnas plugin and must be stacked on the `jsonic` base
grammar:

```ts
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Expr } from '@tabnas/expr'

const j = new Tabnas().use(jsonic).use(Expr, options)
const result = j.parse(source)
```

`result` is either the raw S-expression tree (no `evaluate` option) or the
evaluated value (with `evaluate`).

## `ExprOptions`

```ts
type ExprOptions = {
  op?: { [name: string]: OpDef }
  evaluate?: Evaluate
}
```

| Field | Type | Description |
|---|---|---|
| `op` | `{ [name]: OpDef }` | Operator definitions, keyed by a name you choose. Merged over `Expr.defaults.op`. Set an entry to `null` to remove a default operator. |
| `evaluate` | `Evaluate` | If set, each expression is reduced to a value during parsing, and `parse` returns values instead of S-expression trees. |

Options are merged with the defaults; you only specify what differs. The
operator name you pick is decorated by kind (see [Operator names](#operator-names)).

## `OpDef`

The shape of each `op` entry. Which fields apply depends on the operator
kind flag (`infix` / `prefix` / `suffix` / `ternary` / `paren`).

```ts
type OpDef = {
  src?: string | string[]
  osrc?: string
  csrc?: string
  left?: number
  right?: number
  use?: any
  prefix?: boolean
  suffix?: boolean
  infix?: boolean
  ternary?: boolean
  paren?: boolean
  preval?: {
    active?: boolean
    required?: boolean
    allow?: string[]
  }
}
```

| Field | Applies to | Description |
|---|---|---|
| `src` | infix, prefix, suffix | The operator token text, e.g. `'+'`. For `ternary`, a two-element array `[open, close]`, e.g. `['?', ':']`. |
| `osrc` | paren | The opening token text, e.g. `'('`, `'['`, `'<'`. |
| `csrc` | paren | The closing token text, e.g. `')'`, `']'`, `'>'`. |
| `left` | infix, suffix | Left binding power. Higher binds tighter. Defaults to `Number.MIN_SAFE_INTEGER` (loosest). |
| `right` | infix, prefix | Right binding power. Higher binds tighter. Defaults to `Number.MAX_SAFE_INTEGER` (tightest). |
| `infix` | — | Mark as a binary infix operator (2 terms). |
| `prefix` | — | Mark as a unary prefix operator (1 term). |
| `suffix` | — | Mark as a unary suffix/postfix operator (1 term). |
| `ternary` | — | Mark as a ternary operator (3 terms). Requires `src: [open, close]`. |
| `paren` | — | Mark as a paren/grouping operator. Requires `osrc`/`csrc`. |
| `preval` | paren | Allow a preceding value (function-call / index syntax). See [Preval](#preval). |
| `use` | any | Arbitrary data carried through onto the resolved `Op.use`. |

`left`/`right` are compared **only by order**, never by magnitude. See
[Concepts → binding-power scale](concepts.md#the-binding-power-scale).
`left < right` is left-associative; `left > right` is right-associative.

The same `src` may appear under two entries of different kinds — e.g. `+`
is both a `prefix` (`positive`) and an `infix` (`addition`). They share a
token; the parser disambiguates by position.

### Preval

`preval` makes a paren operator able to absorb the value immediately to its
left as its first operand — the shape of `f(args)` and `a[i]`.

| Field | Default | Description |
|---|---|---|
| `active` | `true` when a `preval` object is present | Enable preval for this paren. |
| `required` | `false` | If `true`, the paren *must* have a preceding value to match (so `[1]` is a list literal but `a[1]` is an index op). |
| `allow` | unset | If set, the preceding value must be one of these strings for preval to apply. |

`preval: true` is shorthand for `preval: { active: true }`.

## `Op`

The fully-resolved operator object. It is the **first element** of every
expression array, and the third argument to an `Evaluate` callback.

```ts
type Op = {
  name: string       // decorated name, e.g. 'addition-infix'
  src: string        // operator source, e.g. '+'  (osrc for parens)
  left: number
  right: number
  use: any
  prefix: boolean
  suffix: boolean
  infix: boolean
  ternary: boolean
  paren: boolean
  terms: number      // operand count: 1 (unary), 2 (infix), 3 (ternary)
  tkn: string        // token name
  tin: number        // token id
  osrc: string       // paren open source
  csrc: string       // paren close source
  otkn: string; otin: number   // paren open token
  ctkn: string; ctin: number   // paren close token
  preval: { active: boolean; required: boolean; allow?: string[] }
  token: Token       // the matched source Token (location info)
  OP_MARK: object    // internal marker identifying plugin-owned ops
}
```

The most useful fields in an evaluator are `name`, `src`, and the kind
booleans (`prefix`, `infix`, `paren`, …).

### Operator names

The `name` you give an op is decorated with its kind, unless it already ends
that way:

| Kind | Suffix added | Example name → resolved `Op.name` |
|---|---|---|
| infix | `-infix` | `addition` → `addition-infix` |
| prefix | `-prefix` | `negative` → `negative-prefix` |
| suffix | `-suffix` | `factorial` → `factorial-suffix` |
| ternary | `-ternary` | `cond` → `cond-ternary` |
| paren | `-paren` | `plain` → `plain-paren` |

Dispatch on `op.name` (kind-precise) or `op.src` + kind booleans.

## `Evaluate`

```ts
type Evaluate = (rule: Rule, ctx: Context, op: Op, terms: any[]) => any
```

The callback you pass as `options.evaluate`, or to `evaluation()`. It is
invoked **bottom-up**: by the time it sees an `op`, each entry of `terms` is
the already-evaluated value of the corresponding operand. Return the value
this sub-expression reduces to.

- `rule`, `ctx` — the current parse `Rule` and `Context` (often unused).
- `op` — the resolved `Op` (above).
- `terms` — array of evaluated operands: length 1 (unary), 2 (infix), 3
  (ternary). For a preval paren call like `f(1,2)`, terms are
  `[funcName, argsValue]`.

## `evaluation(rule, ctx, node, evaluate)`

```ts
function evaluation(rule: Rule, ctx: Context, node: any, evaluate: Evaluate): any
```

Reduce a parsed S-expression tree to a value, outside of parsing. Use it
when you parsed with no `evaluate` option (to defer or repeat evaluation).
`rule`/`ctx` may be `null` if your callback ignores them. Non-expression
nodes (plain values, plain arrays/maps) pass through unchanged.

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Expr, evaluation } = require('@tabnas/expr')

const j = new Tabnas().use(jsonic).use(Expr)
const tree = j.parse('1+2*3')           // raw S-expression, not yet reduced

const mathFn = (rule, ctx, op, terms) =>
  op.src === '+' ? terms[0] + terms[1]
  : op.src === '*' ? terms[0] * terms[1]
  : NaN

evaluation(null, null, tree, mathFn)    // => 7
```

## Default operators

`Expr.defaults.op` provides these out of the box. Powers are on the
base-1,000,000 ladder; only their order matters.

| Name | Kind | `src` | `left` | `right` | Notes |
|---|---|---|---|---|---|
| `positive` | prefix | `+` | — | `4000000` | unary plus |
| `negative` | prefix | `-` | — | `4000000` | unary minus |
| `addition` | infix | `+` | `2000000` | `2100000` | left-assoc |
| `subtraction` | infix | `-` | `2000000` | `2100000` | left-assoc |
| `multiplication` | infix | `*` | `3000000` | `3100000` | left-assoc |
| `division` | infix | `/` | `3000000` | `3100000` | left-assoc |
| `remainder` | infix | `%` | `3000000` | `3100000` | left-assoc |
| `plain` | paren | `(` `)` | — | — | grouping |

Precedence order (loosest → tightest): addition/subtraction <
multiplication/division/remainder < unary prefix. Parens have neither power
(they are structural).

To change or remove a default, supply the same key in your `op` map (with a
new `OpDef`, or `null` to remove it).

## S-expression output shape

With no `evaluate`, `parse` returns arrays of the form
`[op, ...operands]` where `op` is an `Op`. Reference shapes (operators shown
as their `src` for readability):

| Input | Shape |
|---|---|
| `1+2` | `['+', 1, 2]` |
| `1+2*3` | `['+', 1, ['*', 2, 3]]` |
| `-1+2` | `['+', ['-', 1], 2]` |
| `(1+2)*3` | `['*', ['(', ['+', 1, 2]], 3]` |
| `1?2:3` (ternary) | `['?', 1, 2, 3]` |
| `foo(1,2)` (preval) | `['(', 'foo', [1, 2]]` |
| `a[0]` (required preval) | `['[', 'a', 0]` |

Top-level comma/space sequences become implicit lists (`1,2` → `[1, 2]`),
and expressions compose inside maps and lists like any jsonic value.
