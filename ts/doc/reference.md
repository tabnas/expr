# Reference

Exact API surface of `@tabnas/expr`. Dry and complete. Behaviour is grounded
in `ts/src/expr.ts` and the test suite.

## Package

```js
const { Expr, evaluation, simplify, parseSimplified, fold, recordPositions, FoldError, opify, isOp, testing } = require('@tabnas/expr')
```

ESM / TypeScript:

```ts
import { Expr, evaluation, simplify, parseSimplified, fold, recordPositions, FoldError, opify, isOp } from '@tabnas/expr'
import type { ExprOptions, OpDef, Op, Evaluate, FoldOptions, Site, FoldReport } from '@tabnas/expr'
```

The package is library-only: there is no CLI. `main` is `dist/expr.js`,
types are `dist/expr.d.ts`.

## Exports

| Export | Kind | Description |
|---|---|---|
| `Expr` | Plugin | The expression plugin. Apply with `new Tabnas().use(jsonic).use(Expr, options?)`. |
| `evaluation` | function | Standalone reducer for a parsed S-expression tree (see below). |
| `simplify` | function | Reduce a parse to the plain S-expression form, each operator as its source text. |
| `parseSimplified` | function | Parse with an instance, then `simplify` the result. |
| `fold` | function | Fold a finished tree: resolve its key operators against the whole document (see [`fold`](#fold)). |
| `recordPositions` | function | Record the opening token of every container an instance parses, for the `positions` option of `fold`. |
| `FoldError` | class | The error a fold throws for its own failures, with a `code` (see [Fold errors](#fold-errors)). |
| `opify` | function | Mark a hand-built operator description as this plugin's, so `isOp` and `evaluation` recognise a node headed by it. |
| `isOp` | function | Whether a value is an operator node: an array headed by a marked operator. |
| `keyOpOf`, `makeKeyMap` | function | The key operator a map is an occurrence of, by shape, and the table (a `Map`, in resolution order) `fold` reads it from. |
| `testing` | object | Internal test hooks: `{ prattify, opify }`. Not part of the stable API. |
| `Expr.defaults` | object | The default options (`{ op: { … } }`), see [Default operators](#default-operators). |

Type-only exports: `ExprOptions`, `OpDef`, `Op`, `Evaluate`, `FoldOptions`,
`FoldEvaluate`, `FoldHit`, `FoldEntry`, `Inherit`, `Recognise`, `Positions`,
`Site`, `FoldReport`, `Path`, `KeyMap`.

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

## `simplify` and `parseSimplified`

```ts
function simplify(value: any): any
function parseSimplified(tn: Tabnas, src: string, meta?: any): any
```

`simplify` reduces a parse to the plain S-expression form the shared
fixtures and the translation parts read. An operation is an array whose
head is the operator object, and `simplify` replaces the head with the
operator's source text, which for a paren operator is its opening source,
and drops a term that was never filled. It reduces every other array and
object member by member into a new value, keeps their member order, and
returns anything else unchanged. A node that contains itself reads as the
string `'[CIRCLE]'`. `parseSimplified(tn, src, meta?)` is
`simplify(tn.parse(src, meta))`.

The Rust port's `simplify` and `parse_simplified` return the same value,
and so does the Go port's `SimplifyOrdered`. Every shared fixture holds
their JSON to this function's, byte for byte.

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Expr, simplify, parseSimplified } = require('@tabnas/expr')

const j = new Tabnas().use(jsonic).use(Expr)
simplify(j.parse('1+2*3'))                    // => ['+', 1, ['*', 2, 3]]
JSON.stringify(parseSimplified(j, '{b:1,a:2+3}')) // => '{"b":1,"a":["+",2,3]}'
```

## `ExprOptions`

```ts
type ExprOptions = {
  op?: { [name: string]: OpDef }
  evaluate?: Evaluate
  fold?: FoldOptions
}
```

| Field | Type | Description |
|---|---|---|
| `op` | `{ [name]: OpDef }` | Operator definitions, keyed by a name you choose. Merged over `Expr.defaults.op`. Set an entry to `null` to remove a default operator. |
| `evaluate` | `Evaluate` | If set, each expression is reduced to a value during parsing, and `parse` returns values instead of S-expression trees. |
| `fold` | `FoldOptions` | If set, the finished document is folded before `parse` returns: its key operators are resolved against the whole tree. See [`fold`](#fold). |

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
  key?: boolean
  dynamic?: boolean
}
```

| Field | Applies to | Description |
|---|---|---|
| `src` | infix, prefix, suffix, key | The operator token text, for example `'+'`. For `ternary`, a two-element array `[open, close]`, for example `['?', ':']`. For `key`, the member name that marks a site, for example `'$ref'`. |
| `osrc` | paren | The opening token text, for example `'('`, `'['`, `'<'`. |
| `csrc` | paren | The closing token text, for example `')'`, `']'`, `'>'`. |
| `left` | infix, suffix | Left binding power. Higher binds tighter. Defaults to `Number.MIN_SAFE_INTEGER` (loosest); a declared `0` takes that default too. |
| `right` | infix, prefix | Right binding power. Higher binds tighter. Defaults to `Number.MAX_SAFE_INTEGER` (tightest); a declared `0` takes that default too. |
| `infix` | (none) | Mark as a binary infix operator (2 terms). |
| `prefix` | (none) | Mark as a unary prefix operator (1 term). |
| `suffix` | (none) | Mark as a unary suffix/postfix operator (1 term). |
| `ternary` | (none) | Mark as a ternary operator (3 terms). Requires `src: [open, close]`. |
| `paren` | (none) | Mark as a paren/grouping operator. Requires `osrc`/`csrc`. |
| `preval` | paren | Allow a preceding value (function-call / index syntax). See [Preval](#preval). |
| `key` | (none) | Mark as a key operator: a map that owns the member `src` is an occurrence, recognised by shape when a tree is folded, never by a token. No binding power applies. See [Key operators](#key-operators). |
| `dynamic` | key | A key operator that a fold recognises and reports but never evaluates. |
| `use` | any | Arbitrary data carried through onto the resolved `Op.use`. |

`left`/`right` are compared **only by order**, never by magnitude. See
[Concepts → binding-power scale](concepts.md#the-binding-power-scale).
`left < right` is left-associative; `left > right` is right-associative.

The same `src` may appear under two entries of different kinds. For example `+`
is both a `prefix` (`positive`) and an `infix` (`addition`). They share a
token; the parser disambiguates by position.

### Preval

`preval` makes a paren operator able to absorb the value immediately to its
left as its first operand: the shape of `f(args)` and `a[i]`.

| Field | Default | Description |
|---|---|---|
| `active` | `true` when a `preval` object is present | Enable preval for this paren. |
| `required` | `false` | If `true`, the paren *must* have a preceding value to match (so `[1]` is a list literal but `a[1]` is an index op). |
| `allow` | unset | If set, the preceding value must be one of these strings for preval to apply. |

`preval: true` is shorthand for `preval: { active: true }`.

### Key operators

A key operator is recognised after the base grammar has built a map, by
the map's shape: it owns the member named by `src`. Nothing changes in
the parsed tree, which stays the document as written; the operator exists
for [`fold`](#fold), which hands the resolver two terms, the member's
value and a new map of the other members.

A map that owns the members of two key operators is an occurrence of
one of them, chosen by a rule every runtime computes alike: a static
operator before a dynamic one, then the member name that comes first by
Unicode code point. Declaration order plays no part, since a Go option
table has none and a JavaScript object lists integer-like names first.
Two key operators on one member name are refused when the table is
built, with a `FoldError` coded `fold_key`.

`dynamic: true` marks a key the fold reports and leaves alone, such as
`$dynamicRef`, whose meaning depends on a scope a parser does not have;
the map still folds as a plain map, so operators inside it reduce. A
host that wants to look at such a site (to record where it would point,
say) declares the key as an ordinary key operator instead and returns
`site.plain()` from its resolver.

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Expr, fold } = require('@tabnas/expr')

const op = { ref: { key: true, src: '$ref' } }
const j = new Tabnas().use(jsonic).use(Expr, { op })
const raw = j.parse('{ a: { $ref: "#/b", note: 1 }, b: { c: 2 } }')
JSON.stringify(raw)   // => '{"a":{"$ref":"#/b","note":1},"b":{"c":2}}'

const pointer = (ref) => ref.slice(2).split('/')
const out = fold(raw, {
  op,
  evaluate: (site, op, [value, siblings]) => {
    const hit = site.at(pointer(value))
    return hit.found && !hit.cycle ? { ...hit.value, ...siblings } : site.node
  },
})
JSON.stringify(out)   // => '{"a":{"c":2,"note":1},"b":{"c":2}}'
```

## `Op`

The fully resolved operator object. It is the **first element** of every
expression array, and the third argument to an `Evaluate` callback.

```ts
type Op = {
  name: string       // decorated name, for example 'addition-infix'
  src: string        // operator source, for example '+'  (osrc for parens)
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
  key: boolean       // a key operator, recognised by a map's shape
  dynamic: boolean   // reported by a fold, never evaluated
  token: Token       // the matched source Token (location info); for a key
                     // operator, the site's opening token when positions are
                     // recorded, and an empty placeholder otherwise
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
| key | `-key` | `ref` → `ref-key` |

Dispatch on `op.name` (kind-precise) or `op.src` + kind booleans.

## `Evaluate`

```ts
type Evaluate = (rule: Rule, ctx: Context, op: Op, terms: any[]) => any
```

The callback you pass as `options.evaluate`, or to `evaluation()`. It is
invoked **bottom-up**: by the time it sees an `op`, each entry of `terms` is
the already-evaluated value of the corresponding operand. Return the value
this sub-expression reduces to.

- `rule`, `ctx`. The current parse `Rule` and `Context` (often unused).
- `op`. The resolved `Op` (above).
- `terms`. Array of evaluated operands: length 1 (unary), 2 (infix), 3
  (ternary). For a preval paren call like `f(1,2)`, terms are
  `[funcName, argsValue]`.

## `evaluation(rule, ctx, node, evaluate, options?)`

```ts
function evaluation(rule: Rule, ctx: Context, node: any, evaluate: Evaluate, options?: { objects?: boolean }): any
```

Reduce a parsed S-expression tree to a value, outside of parsing. Use it
when you parsed with no `evaluate` option (to defer or repeat evaluation).
`rule`/`ctx` may be `null` if your callback ignores them. Plain values and
maps pass through unchanged; a plain array (an implicit list, such as the
parse of `1+2, 3+4`) is reduced element-wise into a new array. With
`options.objects` set, a plain map is reduced member by member into a new
map of the same prototype, which is what the `evaluate` option already
does for an expression inside a map. A map is plain when its prototype is
`null`, as the engine builds them, or `Object.prototype`, as `JSON.parse`
does; any other object, a `Date` or a class instance, passes through as it
is.

The tree passed in is never modified, so the same tree can be reduced
again with different semantics, and `evaluate` is called exactly once per
operator per reduction.

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

## `fold`

```ts
function fold(root: any, options: FoldOptions): any

type FoldOptions = {
  op?: { [name: string]: OpDef }     // the operator table; only `key` entries matter
  evaluate: (site: Site, op: Op, terms: any[]) => any
  inherit?: (site: Site, node: any, k: Record<string, any>) => Record<string, any>
  recognise?: (site: Site, op: Op) => boolean
  k?: Record<string, any>            // every document root's starting bag
  doc?: string                       // the key of the document folded; default ''
  load?: (doc: string, site: Site) => any   // another document's raw root, or undefined
  cycle?: 'verbatim' | 'error'       // default 'verbatim'
  tokens?: boolean                   // default true: token operator arrays met in the walk reduce too
  positions?: boolean | ((node: object) => Token | undefined)
  maxDepth?: number                  // default 1000
  report?: (report: FoldReport) => void
}

type Site = {
  root: any                          // the verbatim root of the site's document
  doc: string                        // that document's key
  path: (string | number)[]          // from the root to this map
  node: any                          // the verbatim map
  k: Record<string, any>             // the bag inherited down to and including this map
  token?: Token                      // the map's opening token, with `positions`
  at(path, doc?): { found: boolean; cycle: boolean; value?: any }
  locate(path, doc?): { found: boolean; value?: any }
  plain(): any                       // this map folded as a plain map
}

type FoldReport = {
  cycles: { doc: string; at: string; targetDoc: string; target: string }[]
  unresolved: { doc: string; at: string; targetDoc: string; target: string }[]
  dynamic: { doc: string; at: string; src: string }[]
}
```

`fold` walks a finished tree and returns a new one; the input is never
changed. Every key-operator site is reduced bottom-up through `evaluate`,
with `[value, siblings]` as its terms, both already folded. Members are
walked in each map's own order. Only plain records and arrays are walked:
a map whose prototype is neither `null` nor `Object.prototype`, a `Map`,
a `Date` or a class instance, is a leaf and comes out as it went in.

What a resolver returns is placed in the output as it is, so a resolver
that returns `site.node`, or another raw value, shares it with the input,
and editing that part of the output edits the input. A value, `siblings`,
and `site.node` may own a member named `__proto__`, which the fold keeps
as data: merge them with object spread or `Object.defineProperty`, never
with `Object.assign` or `target[name] = value`, which would set the
prototype of the result instead.

### Context: `inherit` and `recognise`

Values flow up through `evaluate`; context flows down through `inherit`.
It is called once for every map on a path the fold needs, from the root
down, with the bag the map's parent passes on, and returns the bag for
the map and everything below it; inside the call, `site.k` is the
parent's bag. A site's own `site.k` therefore includes its own map's
contribution: a `$id` beside a `$ref` sets the base that `$ref` resolves
against. Each bag is worked out from the map's own ancestors and kept, so
a site reached early through a reference gets its ancestors' bag and never
the referrer's. `inherit` must return an object, and must not fold the
part of the tree below the map it is called for; either mistake throws a
`FoldError` coded `fold_inherit`.

`recognise(site, op)` decides whether a map that owns a key operator's
member is an occurrence of it there. When it returns `false` the map
folds as a plain map, with no `evaluate` call and no report row: a
property named `$ref` in the `properties` of a schema, or a `$ref` inside
example data, is data. A resolver that decides only once it sees the
value (a `$ref` whose value is not a string) returns `site.plain()`: the
map folded as a plain map, members in their own order.

### Reaching the rest of the tree

`site.at(path)` folds another location and returns its folded value. It
follows raw members from the root; a key-operator map met on the way is
folded, and the rest of the path reads its folded value, so a reference
to a reference resolves and a pointer into a reference's target works.
There is one result per location, so every site naming one target shares
one value. A path names nothing (`found: false`) when a step does: an
array index is an RFC 6901 index: `0`, or digits with no leading zero, so
`01`, `1.0`, an empty segment, and `-` name nothing. `value` is present
only when the location was found and is not a cycle.
`site.locate(path)` reads the raw tree and follows nothing.

### Cycles

A site whose target is still being folded closes a cycle: the target
itself, an ancestor of the site, the root, or a map the walk entered on
the way. `site.at` then returns `cycle: true`, and the resolver keeps the
site as written, which is what the documented resolvers do; under
`cycle: 'error'` the fold throws a `FoldError` coded `fold_cycle` instead.

How far a cycle reaches depends on what it passes through:

- A cycle made of references alone, `x` naming `y` and `y` naming `x`,
  has nothing to resolve to: every site in it is a cycle, and so is every
  later reference into it.
- A cycle that passes through a map, `Tree` holding a site that names
  `Node`, which names `Tree`, is cut once, at the site whose target was
  still being folded. A later reference to that site gets its folded
  value, the reference as written; a path that has to pass through it to
  go further is a cycle.

Which site holds the cut follows the walk, which runs from the root in
member order, so a document gives the same cuts every time it is folded,
and reordering its members can move them.

### Several documents

`site.at(path, doc)` and `site.locate(path, doc)` reach a location in
another document, named by a key the host chooses, a URI say. The fold
asks `load(doc, site)` for that document's raw root once per key, and
treats `undefined` as a document it cannot have, reporting the location
unresolved. Deciding what a key may name, and fetching it, belongs to the
host: `load` is synchronous, so a host that fetches over a network loads first
and answers from what it has. Every document's root starts from
`options.k`, and `inherit` sees `site.doc`. Memory, cycles, and sharing
work across documents as within one, and every report row names the
document of the site (`doc`) and of the location it named (`targetDoc`).
A row is reported once however often a resolver asks.

### Token operators, positions, and depth

Token operator arrays met in the walk are reduced through the same
`evaluate` when `tokens` is on, so one fold handles `+` and `$ref`;
`op.key` tells the two apart. With `tokens` off, an operator array comes
out as an operator array, its terms folded.

`positions` gives sites their opening token. In the function form it is
a function from a container to its token, which `recordPositions(tn)`
builds by recording every container the instance parses; `true` is
accepted only in deferred mode, where it records them itself, and
anywhere else throws a `FoldError` coded `fold_option`.

`maxDepth` bounds how deep the walk goes. It counts nesting and reference
hops together. Past it, or if the JavaScript stack runs out first, the
fold throws a `FoldError` coded `fold_depth`, so a hostile document gets
a clean error and never a `RangeError`.

### Fold errors

`FoldError` carries `code`, and `doc`, `at`, `targetDoc`, and `target`
where they apply. Its message quotes member names JSON-escaped and cut
short, since they are document content.

| Code | Thrown when |
|---|---|
| `fold_cycle` | a cycle is met under `cycle: 'error'` |
| `fold_depth` | nesting and references go past `maxDepth`, or the stack runs out first |
| `fold_inherit` | `inherit` returns something that is not an object, or folds below its own map |
| `fold_key` | two key operators name one member, or one names none |
| `fold_option` | `positions: true` is given outside deferred mode |

These are the layer's own codes, not engine error codes: the layer is
TypeScript only for now, and they join the engine's catalogue and the
plugin descriptor when the Go and Rust ports carry it.

### Deferred mode

With `ExprOptions.fold` set, the plugin runs the same fold on the
finished root before `parse` returns, through the engine's post-process
rule event, in any grammar `Expr` is installed on (it goes after the base
grammar, as always). A parse that fails is not folded, and under the
engine's error recovery the recovered value is folded once. A resolver
that throws surfaces its error from `parse`, and the instance keeps
working.

## Default operators

`Expr.defaults.op` provides these out of the box. Powers are on the
base-1,000,000 ladder; only their order matters.

| Name | Kind | `src` | `left` | `right` | Notes |
|---|---|---|---|---|---|
| `positive` | prefix | `+` | (none) | `4000000` | unary plus |
| `negative` | prefix | `-` | (none) | `4000000` | unary minus |
| `addition` | infix | `+` | `2000000` | `2100000` | left-assoc |
| `subtraction` | infix | `-` | `2000000` | `2100000` | left-assoc |
| `multiplication` | infix | `*` | `3000000` | `3100000` | left-assoc |
| `division` | infix | `/` | `3000000` | `3100000` | left-assoc |
| `remainder` | infix | `%` | `3000000` | `3100000` | left-assoc |
| `plain` | paren | `(` `)` | (none) | (none) | grouping |

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
