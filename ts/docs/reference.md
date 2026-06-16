# Reference

Exported API for `@tabnas/expr` (TypeScript) and
`github.com/tabnas/expr/go`.

- [Exports](#exports)
- [Types](#types)
- [Plugin options](#plugin-options)
- [Default operators](#default-operators)
- [AST shape](#ast-shape)
- [Group tags](#group-tags)

---

## Exports

### TypeScript (`@tabnas/expr`)

| Symbol       | Kind  | Description                                               |
| ------------ | ----- | --------------------------------------------------------- |
| `Expr`       | plugin| Jsonic plugin. Pass to `tn.use(Expr, opts?)`.             |
| `evaluation` | fn    | Internal evaluator entry used by the plugin.              |
| `testing`    | obj   | Internal helpers exposed for the test suite.              |
| `ExprOptions`| type  | Shape of the plugin options argument.                     |
| `OpDef`      | type  | Shape of a single operator definition.                    |
| `Op`         | type  | Full operator descriptor passed to evaluators.            |
| `Evaluate`   | type  | Signature of a user evaluator.                            |

### Go (`github.com/tabnas/expr/go`)

| Symbol         | Kind   | Description                                                  |
| -------------- | ------ | ------------------------------------------------------------ |
| `Expr`         | func   | Jsonic plugin. `j.Use(Expr, opts...)`.                       |
| `Parse`        | func   | Convenience: parse a string, returning `(any, error)`.       |
| `MakeJsonic`   | func   | Build a `*jsonic.Jsonic` pre-configured with `Expr`.         |
| `Simplify`     | func   | Render an AST with operator `src` strings for inspection.    |
| `Version`      | const  | Module version string.                                       |
| `OpDef`        | type   | Operator definition (user-supplied).                         |
| `Op`           | type   | Full operator descriptor (plugin-resolved).                  |
| `ExprOptions`  | type   | Typed options struct.                                        |
| `PrevalDef`    | type   | Paren-preval settings.                                       |

## Types

### `OpDef`

Fields are optional; the combination of flags decides the operator kind.

| Field     | Type                        | Notes                                                 |
| --------- | --------------------------- | ----------------------------------------------------- |
| `src`     | `string \| string[]`        | Token source. `string[]` for ternary.                 |
| `osrc`    | `string`                    | Paren open src (when `paren:true`).                   |
| `csrc`    | `string`                    | Paren close src (when `paren:true`).                  |
| `left`    | `number`                    | Left binding power (infix/suffix).                    |
| `right`   | `number`                    | Right binding power (infix/prefix).                   |
| `prefix`  | `boolean`                   | Prefix operator, e.g. `-x`.                           |
| `suffix`  | `boolean`                   | Suffix operator, e.g. `x!`.                           |
| `infix`   | `boolean`                   | Infix operator, e.g. `a+b`.                           |
| `ternary` | `boolean`                   | Ternary operator; requires `src: [openTok, closeTok]`.|
| `paren`   | `boolean`                   | Parenthesis operator.                                 |
| `preval`  | `{active, required, allow}` | See [paren-preval recipe](how-to.md).                 |
| `use`     | `any`                       | Arbitrary user data forwarded to evaluators on `Op`.  |

### `ExprOptions`

```ts
type ExprOptions = {
  op?: { [name: string]: OpDef | null }  // null disables a default op
  evaluate?: Evaluate
}
```

### `Evaluate`

```ts
type Evaluate = (rule: Rule, ctx: Context, op: Op, ...terms: any[]) => any
```

Called bottom-up: by the time your evaluator runs, all term args are
already evaluated. Return the op's value.

## Plugin options

```ts
tn.use(Expr, { op: {...}, evaluate: fn })
```

```go
j.Use(expr.Expr, map[string]any{
  "op": map[string]any{...},
  "evaluate": fn,
})
```

Named ops in the `op` map merge with the defaults. Set a name to `null`
(TS) or omit / override it to disable a default op.

## Default operators

| Name            | Kind   | `src` | Precedence (left, right) |
| --------------- | ------ | ----- | ------------------------ |
| `addition`      | infix  | `+`   | 1200000, 1210000         |
| `subtraction`   | infix  | `-`   | 1200000, 1210000         |
| `multiplication`| infix  | `*`   | 1300000, 1310000         |
| `division`      | infix  | `/`   | 1300000, 1310000         |
| `remainder`     | infix  | `%`   | 1300000, 1310000         |
| `positive`      | prefix | `+`   | _, 1500000               |
| `negative`      | prefix | `-`   | _, 1500000               |
| `plain`         | paren  | `(` `)` | —                      |

Higher numeric precedence binds tighter. `left < right` gives
left-associativity (`a+b+c` → `(a+b)+c`); `left > right` gives
right-associativity.

### The binding-power scale

Precedence values are compared only by **order**, never by magnitude, so the
defaults sit on a deliberately roomy **base-100000 ladder** that leaves space
for client operators to slot between, below, and above the built-ins. Most
reserved tiers are **below** arithmetic, where grammars add the most operators:

| Base          | Tier                                       | Built-in?     |
| ------------- | ------------------------------------------ | ------------- |
| 100000        | sequence / comma                           |               |
| 200000        | assignment (right-assoc)                   |               |
| 300000        | ternary / conditional                      |               |
| 400000        | logical or                                 |               |
| 500000        | logical and                                |               |
| 600000–800000 | bitwise or / xor / and                     |               |
| 900000        | equality (`==` `!=`)                       |               |
| 1000000       | comparison (`<` `<=` `>` `>=`)             |               |
| 1100000       | shift (`<<` `>>`)                          |               |
| **1200000**   | **addition / subtraction**                 | ✔ `+` `-`     |
| **1300000**   | **multiplication / division / remainder**  | ✔ `*` `/` `%` |
| 1400000       | _(free)_                                   |               |
| **1500000**   | **unary prefix**                           | ✔ `+` `-`     |
| 1600000       | _(free — extra prefix)_                    |               |
| 1700000       | exponent (`**`, right-assoc)               |               |
| 1800000       | postfix / suffix (`!` `?`)                 |               |
| 1900000       | call / index / member                      |               |

To add an operator, pick a tier `base = N*100000` (`left = base`,
`right = base + 10000` for left-assoc; swap for right-assoc). Need an in-between
level? Use `base + 20000`, `base + 40000`, … — each gap holds ~4 sub-tiers.

## AST shape

Expressions are arrays whose first element is the operator descriptor.
Remaining elements are the operands (terms), in source order. Terms can
be literals, maps, arrays, or nested expressions.

```text
1+2            →  [Op('+'), 1, 2]
-3             →  [Op('-'), 3]          // prefix
(1+2)          →  [Op('('), [Op('+'), 1, 2]]
foo(1,2)       →  [Op('(','foo'), 1, 2] // with paren-preval
a?b:c          →  [Op('?'), a, b, c]
```

`Simplify` (Go) or `testing.simplify` (TS) substitutes each `Op` with its
`src` string to produce a compact shape suitable for comparison.

## Group tags

Every alt the plugin adds is tagged with `expr` in its grammar group
(`g`) field, in addition to alt-specific tags like `expr,prefix`,
`expr,paren,open`, etc. Use Jsonic's `rule.include` / `rule.exclude` to
filter by these tags.
