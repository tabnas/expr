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

Convenience parser. With **no** options it reuses a single lazily created,
concurrency-safe default instance (so repeated calls don't rebuild the
grammar). With options it builds a fresh instance per call, and returns the
plugin's error when the plugin refuses them (see [Fold errors](#fold-errors)).
Returns the raw result; pass it through `Simplify` for a readable tree, or
supply an `"evaluate"` option to get evaluated values.

### `MakeJsonic`

```go
func MakeJsonic(opts ...map[string]interface{}) *jsonic.Jsonic
```

Build a `*jsonic.Jsonic` instance configured with the Expr plugin. Use this
to configure once and call `j.Parse(src)` many times. It drops the error the
plugin returns for options it cannot take; call `j.Use(Expr, opts)` on a
`jsonic.Make()` instance to see it.

### `Simplify`

```go
func Simplify(node interface{}) interface{}
```

Convert a parse result containing internal `*Op` nodes into plain
`[]interface{}` / `map[string]interface{}` values whose operator slots are
the operator **source string** (`Op.Src`, or `Op.OSrc` for parens). It also
strips the internal `*jsonic.ListRef` wrappers. Use it to inspect or print a
parsed S-expression.

A plain map has no order, so `Simplify` loses the member order the parse
found, and it drops a `null` term. `SimplifyOrdered` keeps both.

### `SimplifyOrdered`

```go
func SimplifyOrdered(node interface{}) interface{}
```

Reduce a parse result the way the TypeScript `simplify` and the Rust
`simplify` do, to the S-expression form the translation parts read. Each
map the parse found comes back as a `*tabnas.OrderedMap`, its keys in the
order the parse found them. A `null` term stays, so `1+null` reads as `["+" 1 nil]`,
and it drops only a slot never filled. A list whose head is an object
with a non-empty string `src` member reads as an operation, with that text
as its head. A node that contains itself reads as `"[CIRCLE]"`. Every
shared fixture holds its JSON to the TypeScript output, byte for byte, so
a host walking the value reads the tree the other two ports give.
`DIVERGENCE.md` records where `Simplify` differs.

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
operator now heads: the attachment point where the operator's next term
belongs. Build operator values with `Opify`.

### `Opify`

```go
func Opify(op *Op) *Op
```

Prepare a hand-built `*Op` for use as the head of an expression op-array
(mirrors the TS module's `testing.opify`). In TS, `opify` stamps the private
`OP_MARK` onto a plain object; in Go the `*Op` type itself is the mark, so
`Opify` normalises the operator (filling the derived `Terms` count when
unset: 3 for a ternary, 2 for an infix or key operator, 1 otherwise) and
returns it.

### `IsOp`

```go
func IsOp(node interface{}) bool
```

Whether a value is an operator node: a `*jsonic.ListRef` or an
`[]interface{}` whose first element is an `*Op`, which is how a parse
result carries an operation. The TypeScript `isOp`.

### `Fold`

```go
func Fold(root interface{}, opts FoldOptions) (interface{}, error)
```

Fold a finished tree: resolve its key operators against the whole
document and return a new tree. See [The tree layer](#the-tree-layer-fold).

### `RecordPositions`

```go
func RecordPositions(j *jsonic.Jsonic) Positions
```

Record the opening token of every map and operator node a parse on `j`
builds, for the `Positions` option of `Fold`. See
[Token operators, positions, and depth](#token-operators-positions-and-depth).

### `MakeKeyMap` / `KeyOpOf`

```go
func MakeKeyMap(op map[string]*OpDef) (KeyMap, error)
func KeyOpOf(keys KeyMap, node interface{}) *Op

type KeyMap []*Op
```

`MakeKeyMap` builds the key-operator table `Fold` reads from an operator
table: one `*Op` for each `Key` entry, in resolution order (see
[Key operators](#key-operators)). Each has `Key` set, `Name` ending
`-key`, `Src` the member name and `Terms` 2. `KeyOpOf` is the key
operator a map is an occurrence of, by shape: the first in the table whose
member the map owns, or `nil`.

### `Expr`

```go
func Expr(j *jsonic.Jsonic, opts map[string]interface{}) error
```

The plugin itself. Apply with `j.Use(Expr, opts)`. `MakeJsonic` does this
for you.

### `VERSION`

```go
const VERSION = "0.5.8"
```

The plugin's version. It always equals the `version` in `ts/package.json`;
`TestVersionMatchesPackageJSON` fails the build if the two drift.

### `MinSafeInteger` / `MaxSafeInteger`

```go
const (
	MinSafeInteger int64 = -(1 << 53) + 1
	MaxSafeInteger int64 = (1 << 53) - 1
)
```

The binding powers an operator gets when it declares none, and when it
declares `0`. They are the integer equivalents of JavaScript's
`Number.MIN_SAFE_INTEGER` and `Number.MAX_SAFE_INTEGER`, which is what the
canonical falls back to, rather than `math.MinInt` / `math.MaxInt`: a
Go-width sentinel would order differently against a legitimately huge
binding power.

Both are typed `int64`, and so are the `Left` / `Right` fields that carry
them, because the pair needs 54 bits and `int` is 32 bits wide on `386`,
`arm`, `mips` and the other 32-bit targets. Typing them `int` there is not
a narrowing at run time but a build failure, so `make build-go`
cross-compiles the port with `GOARCH=386 go vet ./...`. The Rust port
carries the same powers as `i64`.

## Options map

Both `Parse` and `MakeJsonic` (and `j.Use(Expr, …)`) take a
`map[string]interface{}` with three recognised keys:

| Key | Type | Description |
|---|---|---|
| `"op"` | `map[string]interface{}` | Operator definitions, keyed by a name you choose. Merged over the built-in defaults. Set an entry to `nil` to remove a default. |
| `"evaluate"` | `func(*jsonic.Rule, *jsonic.Context, *Op, []interface{}) interface{}` | If set, each expression is reduced to a value during parsing, and `Parse` returns values instead of S-expression trees. |
| `"fold"` | `FoldOptions` or `*FoldOptions` | If set, the finished document is folded before `Parse` returns: its key operators are resolved against the whole tree. See [Deferred mode](#deferred-mode). |

Each `"op"` entry is itself a `map[string]interface{}`. Recognised fields:

| Field | Type | Applies to | Description |
|---|---|---|---|
| `"src"` | `string` or `[]interface{}` | infix/prefix/suffix; ternary; key | Operator text, for example `"+"`. For ternary, a two-element slice `[]interface{}{"?", ":"}`. For key, the member name that marks a site, for example `"$ref"`. |
| `"osrc"` | `string` | paren | Opening token text, for example `"("`. |
| `"csrc"` | `string` | paren | Closing token text, for example `")"`. |
| `"left"` | `int64` (or `int`, or `float64`) | infix, suffix | Left binding power. Higher binds tighter. Defaults to `MinSafeInteger` (loosest); a declared `0` takes that default too. |
| `"right"` | `int64` (or `int`, or `float64`) | infix, prefix | Right binding power. Higher binds tighter. Defaults to `MaxSafeInteger` (tightest); a declared `0` takes that default too. |
| `"infix"` | `bool` | (none) | Binary infix operator (2 terms). |
| `"prefix"` | `bool` | (none) | Unary prefix operator (1 term). |
| `"suffix"` | `bool` | (none) | Unary suffix operator (1 term). |
| `"ternary"` | `bool` | (none) | Ternary operator (3 terms). Requires `"src": []interface{}{open, close}`. |
| `"paren"` | `bool` | (none) | Paren/grouping operator. Requires `"osrc"`/`"csrc"`. |
| `"preval"` | `bool` or `map[string]interface{}` | paren | Allow a preceding value (call/index syntax). See [Preval](#preval). |
| `"key"` | `bool` | (none) | Key operator: a map that owns the member `"src"` names is an occurrence, recognised by shape when a tree is folded, never by a token. No binding power applies. See [Key operators](#key-operators). |
| `"dynamic"` | `bool` | key | A key operator that a fold recognises and reports but never evaluates. |
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
    Left    int64
    Right   int64
    Prefix  bool
    Suffix  bool
    Infix   bool
    Ternary bool
    Paren   bool
    Preval  interface{} // bool, map[string]interface{}, or PrevalDef
    Use     interface{}
    Key     bool        // a key operator, recognised by a map's shape
    Dynamic bool        // a key operator a fold reports and never evaluates
}
```

The struct form of an operator definition. (In practice you usually pass the
`map[string]interface{}` form to the options map, which is decoded into
`OpDef` internally. `FoldOptions.Op` takes this form.)

A definition with `Key` set and none of the kind flags is a key operator
only: it registers no token, so the base grammar's pair rule keeps the key
position. One with a kind flag as well is both kinds of operator, as in
TypeScript.

### Key operators

A key operator is recognised after the base grammar has built a map, by
the map's shape: it owns the member named by `Src`. Nothing changes in the
parsed tree, which stays the document as written; the operator exists for
[`Fold`](#the-tree-layer-fold), which hands the resolver two terms, the
member's value and a new map of the other members.

A map that owns the members of two key operators is an occurrence of one
of them, chosen by a rule every runtime computes alike: a static operator
before a dynamic one, then the member name that comes first by Unicode
code point (Go compares strings by byte, which for UTF-8 text is that
order). Neither declaration order nor the order of a Go map plays any
part. Two key operators on one member name, or one that names no member,
are refused when the table is built, with a `FoldError` coded `fold_key`:
`j.Use(Expr, opts)` returns it.

`Dynamic` marks a key the fold reports and leaves alone, such as
`$dynamicRef`, whose meaning depends on a scope a parser does not have;
the map still folds as a plain map, so operators inside it reduce. A host
that wants to look at such a site declares the key as an ordinary key
operator instead and returns `site.Plain()` from its resolver.

### `Op`

The fully resolved operator. It is the head of every expression slice and the
third argument to an evaluate/resolve callback.

```go
type Op struct {
    Name    string // decorated name, for example "addition-infix"
    Src     string // operator source, for example "+"
    Left    int64
    Right   int64
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
    Key     bool   // a key operator, recognised by a map's shape (see Fold)
    Dynamic bool   // a key operator a fold reports and never evaluates
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
    Fold     *FoldOptions
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
| key | `-key` | `ref` → `ref-key` |

Dispatch on `op.Name`, or on `op.Src` plus the kind booleans.

## Default operators

These are provided out of the box (powers on the base-1,000,000 ladder; only
order matters):

| Name | Kind | Source | `Left` | `Right` |
|---|---|---|---|---|
| `positive` | prefix | `+` | (none) | `4000000` |
| `negative` | prefix | `-` | (none) | `4000000` |
| `addition` | infix | `+` | `2000000` | `2100000` |
| `subtraction` | infix | `-` | `2000000` | `2100000` |
| `multiplication` | infix | `*` | `3000000` | `3100000` |
| `division` | infix | `/` | `3000000` | `3100000` |
| `remainder` | infix | `%` | `3000000` | `3100000` |
| `plain` | paren | `(` `)` | (none) | (none) |

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

The plugin calls it once per operation, in the order of the tree the
S-expression shows, and only after the expression has ended: `-1!` is
`[-, [!, 1]]`, so the call for `!` comes before the call for `-`.

## The tree layer: `Fold`

```go
func Fold(root interface{}, opts FoldOptions) (interface{}, error)

type FoldOptions struct {
    Op        map[string]*OpDef      // the operator table; only Key entries matter
    Evaluate  FoldEvaluate           // required
    Inherit   Inherit
    Recognise Recognise
    K         map[string]interface{} // every document root's starting bag; nil is empty
    Doc       string                 // the key of the document folded; default ""
    Load      func(doc string, site *Site) (root interface{}, ok bool, err error)
    Cycle     string                 // "verbatim" (the default) or "error"
    Tokens    *bool                  // nil is true: operator nodes met in the walk reduce too
    Positions interface{}            // a Positions lookup, or true in deferred mode
    MaxDepth  int                    // 0 is the default, 1000
    Report    func(report FoldReport)
}

type FoldEvaluate func(site *Site, op *Op, terms []interface{}) (interface{}, error)
type Inherit func(site *Site, node interface{}, k map[string]interface{}) (map[string]interface{}, error)
type Recognise func(site *Site, op *Op) (bool, error)
type Positions func(node interface{}) *jsonic.Token

type Site struct {
    Root  interface{}   // the verbatim root of the site's document
    Doc   string        // that document's key
    Node  interface{}   // the verbatim map, or operator node
    Token *jsonic.Token // the map's opening token, with Positions; nil otherwise
}

func (s *Site) Path() Path                                      // from the root to this map
func (s *Site) K() (map[string]interface{}, error)              // the bag down to and including this map
func (s *Site) At(path Path, doc ...string) (FoldHit, error)    // fold another location
func (s *Site) Locate(path Path, doc ...string) (FoldHit, error) // read one raw
func (s *Site) Plain() (interface{}, error)                     // this map folded as a plain map

type Path []interface{}      // member names as strings, indices as ints or index strings
type FoldHit struct {
    Found bool
    Cycle bool               // At only
    Value interface{}        // set only when found and not a cycle
}

type FoldReport struct {
    Cycles     []FoldEntry
    Unresolved []FoldEntry
    Dynamic    []FoldDynamic
}
type FoldEntry struct{ Doc, At, TargetDoc, Target string }
type FoldDynamic struct{ Doc, At, Src string }
```

`Fold` walks a finished tree and returns a new one; the input is never
changed. Every key-operator site is reduced bottom-up through `Evaluate`,
with `[]interface{}{value, siblings}` as its terms, both already folded;
`siblings` is always a new `*jsonic.OrderedMap`. The maps walked are
`*jsonic.OrderedMap`, whose members go in the order it holds them, and
`map[string]interface{}` and `jsonic.MapRef`, which have no order, so
their members go in code-point order. The lists walked are
`[]interface{}`, `jsonic.ListRef` and `*jsonic.ListRef`. A folded map or
list is a new one of the same kind. Any other value, a struct, a `time.Time`,
a map of another type, is a leaf and comes out as it went in. A nil map or
slice, which JSON writes as `null`, is a leaf too.

What a resolver returns is placed in the output as it is, so a resolver
that returns `site.Node`, or another raw value, shares it with the input,
and editing that part of the output edits the input. An error a callback
returns stops the fold, and `Fold` returns it as it is.

The report rows carry `json` tags (`doc`, `at`, `targetDoc`, `target`,
`src`), so a report serialises as the TypeScript one does.

### Context: `Inherit` and `Recognise`

Values flow up through `Evaluate`; context flows down through `Inherit`.
It is called once for every map on a path whose bag something reads
(`site.K()`), from the root down, with the bag the map's parent passes
on, and returns the bag for the map and everything below it; inside the
call, `site.K()` is the parent's bag. A site's own `site.K()` therefore
includes its own map's contribution. Each bag is worked out from the
map's own ancestors and kept, so a site reached early through a reference
gets its ancestors' bag and never the referrer's. `Inherit` must return a
bag, a nil map being none, and must not fold the part of the tree below
the map it is called for; either mistake is a `FoldError` coded
`fold_inherit`.

`Recognise(site, op)` decides whether a map that owns a key operator's
member is an occurrence of it there, once per map. When it returns
`false` the map folds as a plain map, with no `Evaluate` call and no
report row: a property named `$ref` in the `properties` of a schema, or a
`$ref` inside example data, is data. A resolver that can decide only
from the value (a `$ref` whose value is not a string) returns
`site.Plain()`: the map folded as a plain map, members in their own order.

### Reaching the rest of the tree

`site.At(path)` folds another location and returns its folded value. It
follows raw members from the root; a key-operator map met on the way is
folded, and the rest of the path reads its folded value, so a reference
to a reference resolves and a pointer into a reference's target works.
There is one result per location, so every site naming one target shares
one value. A path names nothing (`Found` false) when a step does. An
array index is an RFC 6901 index, `"0"` or digits with no leading zero,
or an `int` or an integral `float64`: `"01"`, `"1.0"`, an empty segment
and `"-"` name nothing. `Value` is set only when the location was found
and is not a cycle. `site.Locate(path)` reads the raw tree and follows
nothing.

### Cycles

A site whose target is still being folded closes a cycle: the target
itself, an ancestor of the site, the root, or a map the walk entered on
the way. `site.At` then returns `Cycle` true, and the resolver keeps the
site as written; under `Cycle: "error"` the fold returns a `FoldError`
coded `fold_cycle` instead. Any other value of `Cycle` reads as
`"verbatim"`.

A cycle made of references alone, `x` naming `y` and `y` naming `x`, has
nothing to resolve to: every site in it is a cycle, and so is every later
reference into it. A cycle that passes through a map, `Tree` holding a
site that names `Node`, which names `Tree`, is cut once, at the site
whose target was still being folded; a later reference to that site gets
its folded value, and a path that has to pass through it to go further is
a cycle. Which site holds the cut follows the walk, which runs from the
root in member order, so a document gives the same cuts every time it is
folded, and reordering its members can move them.

### Several documents

`site.At(path, doc)` and `site.Locate(path, doc)` reach a location in
another document, named by a key the host chooses. The fold asks
`Load(doc, site)` for that document's raw root once per key; `ok` false
is a document it cannot have, and the location is reported unresolved,
while an error stops the fold. Deciding what a key may name, and fetching
it, belongs to the host. Every document's root starts from `K`, and
`Inherit` is given `site.Doc`. Memory, cycles, and sharing work across
documents as within one, and every report row names the document of the
site (`Doc`) and of the location it named (`TargetDoc`). A row is reported
once however often a resolver asks.

### Token operators, positions, and depth

An operator node met in the walk (what `IsOp` reports) is reduced
through the same `Evaluate` unless `Tokens` is `jsonic.Bool(false)`, so
one fold handles `+` and `$ref`; `op.Key` is what differs, and each
term is folded at its own path first. With `Tokens` false an operator node
comes out as an operator node of the same kind, its terms folded.

`Positions` gives sites their opening token. `RecordPositions(j)`
subscribes to the engine's post-process rule event on `j`, records the
opening token of every map and operator node a parse builds, and returns
the function `Fold` takes. Go has no weak map, so each parse records into
a table of its own, keyed by the container's pointer, and the function
answers from the table of the last parse on `j` that finished, which it
keeps, with the containers it names, until the next one finishes. A
`[]interface{}` list has no pointer that stays put while it grows, so
only maps and operator nodes have a position. `Positions: true` is
accepted only in deferred mode, where the fold reads the table of the
parse it is folding; `Fold` refuses it, and any value that is neither
`true` nor a function, with `fold_option`.

`MaxDepth` bounds how deep the walk goes. It counts nesting and reference
hops together; past it the fold returns a `FoldError` coded `fold_depth`.
A Go value that contains itself is folded until it goes past `MaxDepth`.
Running out of stack is fatal in Go rather than an error, so `MaxDepth`
is the only guard: keep it within what a stack can hold, as the default
of 1000 is.

### Fold errors

`FoldError` carries `Code`, and `Doc`, `At`, `TargetDoc` and `Target`
where they apply. Its message, `Error()`, starts `expr fold: ` and quotes
member names JSON-escaped and cut short, since they are document content.

| Code | Returned when |
|---|---|
| `fold_cycle` | a cycle is met under `Cycle: "error"` |
| `fold_depth` | nesting and references go past `MaxDepth` |
| `fold_inherit` | `Inherit` returns a nil bag, or folds below its own map |
| `fold_key` | two key operators name one member, or one names none |
| `fold_option` | `Positions: true` is given to `Fold`, `Positions` is neither `true` nor a function, `Evaluate` is missing, or the `"fold"` option is no `FoldOptions` |

These are the layer's own codes, not engine error codes: they join the
engine's catalogue and the plugin descriptor when the Rust port carries
the layer as well.

### Deferred mode

With the `"fold"` option set, the plugin runs the same fold on the
finished root before `Parse` returns, through the engine's post-process
rule event, on the last pass of the parse: a close at depth 0 whose
alternate is nil (a rule with no close alternates) or neither pushes,
replaces nor failed, outside any probe, and not a close that recovery
forced. The fold writes the node of the rule that pass belongs to, which
is the node `Parse` returns, so a start rule replaced on the way is
followed. The plugin's own operator table is the one folded with,
whatever `FoldOptions.Op` holds.

A parse that fails is not folded. The engine reports a failed pass in
three ways the filter reads: an alternate that matched none, an
alternate that matched and then raised (the error is on the context),
and, with recovery off, a lexer error or a token left over after the
last pass, which fail the parse once the rule loop ends. Under the
engine's error recovery a recovered value is folded once, when its root
closes; a recovered parse whose root never closes is not folded.

A fold failure surfaces from `Parse` as a `*jsonic.JsonicError` (the
engine's `TabnasError`) with the fold's code and the `FoldError`'s
message as its `Detail`: the plugin panics it as a `TabnasError`, whose
code the engine keeps. Any other error a callback returns arrives coded
`internal`, its text kept, since the engine reports any other panic that
way. The instance keeps working.

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
