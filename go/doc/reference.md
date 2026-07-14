# Reference (Go)

Exact API surface of the `tabnasexpr` package
(`github.com/tabnas/expr/go`). Grounded in `go/expr.go` and
`go/expr_test.go`. The TypeScript implementation is canonical; this is the
Go port and behaves identically.

## Import

```go
import (
    jsonic "github.com/tabnas/jsonic/go"
    tabnasexpr "github.com/tabnas/expr/go"
)
```

The plugin depends on `github.com/tabnas/jsonic/go`, which re-exports the
engine types (`jsonic.Make`, `jsonic.Rule`, `jsonic.Context`, …). There is
no separate `parser` import.

## Package-level functions

### `Parse`

```go
func Parse(src string, opts ...map[string]interface{}) (interface{}, error)
```

Convenience parser. With **no** options it reuses a single lazily-created,
concurrency-safe default instance (so repeated calls don't rebuild the
grammar). With options it builds a fresh instance per call. Returns the raw
result — pass it through `Simplify` for a readable tree, or supply an
`"evaluate"` option to get evaluated values.

### `MakeJsonic`

```go
func MakeJsonic(opts ...map[string]interface{}) *jsonic.Jsonic
```

Build a `*jsonic.Jsonic` instance configured with the Expr plugin. Use this
to configure once and call `j.Parse(src)` many times.

### `Simplify`

```go
func Simplify(node interface{}) interface{}
```

Convert a parse result containing internal `*Op` nodes into plain
`[]interface{}` / `map[string]interface{}` values whose operator slots are
the operator **source string** (`Op.Src`, or `Op.OSrc` for parens). It also
strips the internal `*jsonic.ListRef` wrappers. Use it to inspect or print a
parsed S-expression.

### `Evaluation`

```go
func Evaluation(
    rule *jsonic.Rule, ctx *jsonic.Context, node interface{},
    resolve func(*jsonic.Rule, *jsonic.Context, *Op, []interface{}) interface{},
) interface{}
```

Reduce a parsed S-expression tree to a value, outside of parsing. Use it when
you parsed with no `"evaluate"` option (to defer or repeat evaluation).
`rule`/`ctx` may be `nil` if your resolver ignores them. Non-expression
nodes pass through unchanged. Folds bottom-up: operands are evaluated before
the resolver is called for an operator.

### `Prattify`

```go
func Prattify(expr interface{}, op *Op) *jsonic.ListRef
```

The core Pratt algorithm, exported for unit testing (mirrors the TS module's
`testing.prattify`). Embeds `op` into the expression tree `expr` (mutated in
place) according to binding power, returning the sub-expression the new
operator now heads — the attachment point where the operator's next term
belongs. Build operator values with `Opify`.

### `Opify`

```go
func Opify(op *Op) *Op
```

Prepare a hand-built `*Op` for use as the head of an expression op-array
(mirrors the TS module's `testing.opify`). In TS, `opify` stamps the private
`OP_MARK` onto a plain object; in Go the `*Op` type itself is the mark, so
`Opify` normalises the operator (filling the derived `Terms` count when
unset) and returns it.

### `Expr`

```go
func Expr(j *jsonic.Jsonic, opts map[string]interface{}) error
```

The plugin itself. Apply with `j.Use(Expr, opts)`. `MakeJsonic` does this
for you.

### `Version`

```go
const Version = "0.1.3"
```

The Go module version of the plugin.

## Options map

Both `Parse` and `MakeJsonic` (and `j.Use(Expr, …)`) take a
`map[string]interface{}` with two recognised keys:

| Key | Type | Description |
|---|---|---|
| `"op"` | `map[string]interface{}` | Operator definitions, keyed by a name you choose. Merged over the built-in defaults. Set an entry to `nil` to remove a default. |
| `"evaluate"` | `func(*jsonic.Rule, *jsonic.Context, *Op, []interface{}) interface{}` | If set, each expression is reduced to a value during parsing, and `Parse` returns values instead of S-expression trees. |

Each `"op"` entry is itself a `map[string]interface{}`. Recognised fields:

| Field | Type | Applies to | Description |
|---|---|---|---|
| `"src"` | `string` or `[]interface{}` | infix/prefix/suffix; ternary | Operator text, e.g. `"+"`. For ternary, a two-element slice `[]interface{}{"?", ":"}`. |
| `"osrc"` | `string` | paren | Opening token text, e.g. `"("`. |
| `"csrc"` | `string` | paren | Closing token text, e.g. `")"`. |
| `"left"` | `int` (or `float64`) | infix, suffix | Left binding power. Higher binds tighter. |
| `"right"` | `int` (or `float64`) | infix, prefix | Right binding power. Higher binds tighter. |
| `"infix"` | `bool` | — | Binary infix operator (2 terms). |
| `"prefix"` | `bool` | — | Unary prefix operator (1 term). |
| `"suffix"` | `bool` | — | Unary suffix operator (1 term). |
| `"ternary"` | `bool` | — | Ternary operator (3 terms). Requires `"src": []interface{}{open, close}`. |
| `"paren"` | `bool` | — | Paren/grouping operator. Requires `"osrc"`/`"csrc"`. |
| `"preval"` | `bool` or `map[string]interface{}` | paren | Allow a preceding value (call/index syntax). See [Preval](#preval). |
| `"use"` | `interface{}` | any | Arbitrary data carried onto the resolved `Op.Use`. |

`left`/`right` are compared **only by order**, never magnitude. `left <
right` is left-associative; `left > right` is right-associative. See
[Concepts → binding-power scale](concepts.md#the-binding-power-scale).

### Preval

`"preval"` as a `bool` is shorthand for active. As a map:

| Field | Default | Description |
|---|---|---|
| `"active"` | `true` when a preval map is present | Enable preval for this paren. |
| `"required"` | `false` | Require a preceding value to match (so `[1]` is a list literal but `a[1]` is an index op). |
| `"allow"` | unset | `[]interface{}` / `[]string` of names the preceding value must match. |

## Exported types

### `OpDef`

```go
type OpDef struct {
    Src     interface{} // string or []string (ternary)
    OSrc    string
    CSrc    string
    Left    int
    Right   int
    Prefix  bool
    Suffix  bool
    Infix   bool
    Ternary bool
    Paren   bool
    Preval  interface{} // bool, map[string]interface{}, or PrevalDef
    Use     interface{}
}
```

The struct form of an operator definition. (In practice you usually pass the
`map[string]interface{}` form to the options map, which is decoded into
`OpDef` internally.)

### `Op`

The fully-resolved operator. It is the head of every expression slice and the
third argument to an evaluate/resolve callback.

```go
type Op struct {
    Name    string // decorated name, e.g. "addition-infix"
    Src     string // operator source, e.g. "+"
    Left    int
    Right   int
    Prefix  bool
    Suffix  bool
    Infix   bool
    Ternary bool
    Paren   bool
    Terms   int    // operand count: 1, 2, or 3
    Tkn     string
    Tin     int
    OSrc    string // paren open source
    CSrc    string // paren close source
    OTkn    string
    OTin    int
    CTkn    string
    CTin    int
    Preval  PrevalDef
    Use     interface{}
}
```

### `PrevalDef`

```go
type PrevalDef struct {
    Active   bool
    Required bool
    Allow    []string
}
```

### `ExprOptions`

```go
type ExprOptions struct {
    Op       map[string]*OpDef
    Evaluate func(rule *jsonic.Rule, ctx *jsonic.Context, op *Op, terms []interface{}) interface{}
}
```

The resolved options struct used internally. You normally supply options as a
`map[string]interface{}` (above), which is decoded into this.

## Operator names

The name you give an op is decorated with its kind, unless it already ends
that way:

| Kind | Suffix | Example |
|---|---|---|
| infix | `-infix` | `addition` → `addition-infix` |
| prefix | `-prefix` | `negative` → `negative-prefix` |
| suffix | `-suffix` | `factorial` → `factorial-suffix` |
| ternary | `-ternary` | `cond` → `cond-ternary` |
| paren | `-paren` | `plain` → `plain-paren` |

Dispatch on `op.Name`, or on `op.Src` plus the kind booleans.

## Default operators

These are provided out of the box (powers on the base-1,000,000 ladder; only
order matters):

| Name | Kind | Source | `Left` | `Right` |
|---|---|---|---|---|
| `positive` | prefix | `+` | — | `4000000` |
| `negative` | prefix | `-` | — | `4000000` |
| `addition` | infix | `+` | `2000000` | `2100000` |
| `subtraction` | infix | `-` | `2000000` | `2100000` |
| `multiplication` | infix | `*` | `3000000` | `3100000` |
| `division` | infix | `/` | `3000000` | `3100000` |
| `remainder` | infix | `%` | `3000000` | `3100000` |
| `plain` | paren | `(` `)` | — | — |

Precedence order (loosest → tightest): addition/subtraction <
multiplication/division/remainder < unary prefix. Supply the same key in your
`"op"` map (with a new definition, or `nil`) to change or remove a default.

## Evaluate / resolve callback

```go
func(rule *jsonic.Rule, ctx *jsonic.Context, op *Op, terms []interface{}) interface{}
```

Invoked bottom-up: `terms` holds the already-evaluated operands (length 1
unary, 2 infix, 3 ternary). For a preval paren call like `f(1,2)`, terms are
`[funcName, argsSlice]`. Return the reduced value. Numbers from jsonic arrive
as `float64`, so coerce accordingly.

## S-expression output shape

`Simplify` renders parse results as nested slices with the operator source
first:

| Input | `Simplify` output |
|---|---|
| `1+2` | `["+" 1 2]` |
| `1+2*3` | `["+" 1 ["*" 2 3]]` |
| `-1+2` | `["+" ["-" 1] 2]` |
| `(1+2)*3` | `["*" ["(" ["+" 1 2]] 3]` |
| `1?2:3` (ternary) | `["?" 1 2 3]` |
| `foo(1,2)` (preval) | `["(" "foo" [1 2]]` |
| `a[0]` (required preval) | `["[" "a" 0]` |
