# Concepts: how `tabnasexpr` works and why (Go)

Understanding-oriented. This is the Go companion to the
[TypeScript concepts doc](../../ts/doc/concepts.md). The TypeScript
implementation is canonical; the Go package `tabnasexpr` is a faithful port
and produces identical results. This page covers the same ideas — the engine
relationship, the S-expression AST, the Pratt binding-power scale, and
paren/ternary/preval semantics — and then lists where the Go port differs
mechanically.

---

## The engine relationship

`tabnasexpr` is **not** a standalone parser. Three layers stack:

1. **The Tabnas engine** — a configurable, rule-based parser with a lexer.
   In Go it is re-exported through `github.com/tabnas/jsonic/go`
   (`jsonic.Make`, `jsonic.Rule`, `jsonic.Context`, …), so the plugin imports
   only `jsonic`.
2. **jsonic** — the relaxed-JSON grammar (`val`, `map`, `list`, `pair`,
   `elem` rules). This understands objects, arrays, bare words, comments, and
   implicit structure.
3. **`tabnasexpr`** — this plugin. It hooks new alternates onto jsonic's
   `val` rule and adds its own `expr` and `paren` rules (plus `ternary` when
   configured), so operator syntax is available anywhere a jsonic *value* can
   appear.

That layering is why an expression can sit inside a JSON object value, a list
element, or at the top level: the expression grammar is woven into the value
rule, not bolted on beside it. The plugin edits rules imperatively
(`j.Rule(...)`, `rs.PrependOpen(...)`, `rs.AddClose(...)`) rather than using a
declarative grammar spec, because expression nodes are rewritten as parsing
proceeds.

## The S-expression AST

A parsed expression is a LISP-style **S-expression**: a slice whose first
element is the operator and whose remaining elements are the operands.

```text
1+2*3   →   [ +op, 1.0, [ *op, 2.0, 3.0 ] ]
```

The head is a rich `*Op` (its source text is `op.Src`); the rest are
operands, which may be sub-expressions, plain values, slices, or maps. Use
`Simplify` to turn this into plain slices with the operator's source string
in the head slot.

The array-of-op-first shape is uniform (every operator kind is the same shape
with a different operand count, `op.Terms`), inspectable (plain data you can
transform, print, or evaluate), and metadata-carrying (the head is the full
`*Op`, so an evaluator gets precedence, kind flags, and your `Use` data).

Evaluation (`Evaluation`, or the `"evaluate"` option) is a bottom-up fold:
recurse into operands, then call the resolver with the op and the evaluated
operand slice. Parse and evaluate are deliberately separable.

## Pratt parsing in one paragraph

Operator precedence is resolved with **Pratt parsing**. Each operator carries
a *binding power* on each side it binds. When a new operator arrives, the
parser compares its binding power against the operator currently holding the
expression and decides whether to **wrap** the existing expression (the new
op is looser, so it takes the whole thing as a left operand) or **drill**
into the last operand (the new op is tighter, so it grabs only the right
edge). Applied repeatedly, that single comparison yields the correct tree for
any mix of precedences and associativities. The core lives in `prattify`
(and `prattifySuffix`).

## The binding-power scale

This is the most important concept for configuring operators.

A binding power is just an integer. The Pratt core compares powers **only**
with `<` and `<=` — never the *distance* between two numbers, only their
**order**:

> Only the ORDER of the magnitudes is a contract. The magnitudes themselves
> are free and volatile.

### Associativity is left vs right

Each infix operator has a `Left` and `Right` power:

- `Left < Right` → **left-associative**. `2+3+4` parses as `(2+3)+4` because
  `addition.Left (2000000) < addition.Right (2100000)`.
- `Left > Right` → **right-associative**. Swap them and `2^3^2` parses as
  `2^(3^2)`.

Prefix operators have only `Right` (no left term), suffix operators only
`Left`. The unset side falls back to the loosest/tightest extreme.

### The default ladder

The built-ins occupy a compact low block on a **base-1,000,000** ladder,
leaving the range above the tightest built-in open for your operators:

```text
   <1000000  looser client ops (assignment, ternary, logical, comparison, …)
    1000000  sequence / comma
    2000000  addition / subtraction                 <-- built-in
    3000000  multiplication / division / remainder  <-- built-in
    4000000  unary prefix      (+ -)                 <-- built-in (tightest)
    5000000  exponent          (** ^, right-assoc)
    6000000  postfix / suffix  (! ? ++)
    7000000  call / index / member  (f() a[i] a.b)
    8000000+ free — the whole range above 4000000 is open for client ops
```

Tiers are `1000000` apart; the `+100000` offset on a left-associative op's
`Right` keeps a tier inside a `100000`-wide band within its `1000000`-wide
slot, so adjacent tiers never overlap.

### How to extend it

Pick a tier base `N * 1000000`:

- **Left-associative infix**: `left = base`, `right = base + 100000`.
- **Right-associative infix**: `left = base + 100000`, `right = base`.
- **Tighter than the built-ins**: `5000000` and up.
- **Looser than addition**: below `2000000`.
- **Sub-tier in a gap**: `base + 200000`, `base + 400000`, … (~4 per gap).

You never rescale existing numbers to insert an operator — just pick a free
tier. A full rescale, if ever done, must be an **order-preserving** remap of
both the defaults and every operator power baked into the tests.

## Parens, ternaries, and preval

These are variations on one idea: a structural operator bracketing a
sub-expression.

- **Parens** (`plain` `(`…`)`) are grouping. They carry no binding power; they
  override precedence by being an explicit boundary. The AST keeps the paren
  node (`["(" inner]`) so an evaluator can act on it; a typical evaluator
  returns the inner value.

- **Ternaries** are implemented as a special bracket rule, like parens but
  with two markers (`["?", ":"]`) and three operands. They are
  right-associative, so `a?b: c?d:e` chains naturally.

- **Preval** parens absorb the value immediately to their left as a first
  operand: `foo(1,2)` → `["(" "foo" [1 2]]`. `required: true` forces a
  leading value (so `[1]` is a list literal but `a[1]` is an index op);
  `allow` restricts which leading names qualify. Preval parens **chain** —
  each picks up the previous result — giving `f(x)(y)`, `a[0][1]`, and mixed
  forms.

## Implicit structure

jsonic allows implicit lists and maps (`a,b` → `["a","b"]`, `x:1 y:2` →
`{x:1, y:2}`). This plugin extends implicits to work inside parens too, so
`foo(1,2)` and `(1 2 3)` produce list operands. That required extra
context counters (`expr_paren`, `expr_ternary`, …) and context-sensitive edge
handling — correctness details of the grammar wiring, not configuration.

## Differences from the TS version

The Go port is behaviourally identical (the shared `test/spec/*.tsv`
fixtures are the parity contract, run by both runtimes). The differences are
mechanical, stemming from Go's lack of JavaScript's shared-array identity and
from Go's static typing:

- **Shared-mutation via `*jsonic.ListRef`.** In TS, expression nodes are
  plain arrays rewritten **in place**; every rule that captured the array
  sees the change because JS arrays share identity. Go slices do **not** share
  a header, so the port wraps each expression in a `*jsonic.ListRef` (a
  pointer to a struct holding the slice). Re-pointing `ListRef.Val` in one
  rule's action is then visible to every holder of the pointer — recovering
  the property TS arrays get for free. `Simplify` and `Evaluation` unwrap
  these boxes for you.

- **Pre-allocated slots and an `_unfilled` sentinel.** Because Go can't grow a
  shared array by reference, the port pre-allocates an expression's operand
  slots (`op.Terms + 1`) and fills them depth-first via `fillNextSlot`,
  marking empty slots with an internal `_unfilled` sentinel. `cleanExpr`
  strips these when an expression escapes into an enclosing list/map.

- **Entry points.** The Go port exposes package-level `Parse`, `MakeJsonic`,
  `Simplify`, and `Evaluation` (capitalised, exported). The no-options
  `Parse` reuses one lazily-built, concurrency-safe default instance because
  building the grammar dominates parse time. The TS package is library-only
  (`new Tabnas().use(jsonic).use(Expr)`); there is no `Parse`/`MakeJsonic`
  convenience there.

- **Options are an untyped map.** Go callers pass
  `map[string]interface{}` with `"op"` / `"evaluate"` keys (decoded into
  `OpDef`/`ExprOptions`), where TS uses a typed `ExprOptions` object. Numbers
  in `"left"`/`"right"` accept both `int` and `float64`.

- **Numbers are `float64`.** jsonic numbers arrive as `float64` in Go, so
  evaluators coerce (`terms[0].(float64)`); TS uses JS numbers directly.

- **Resolved op type.** The evaluate callback receives `*Op` (a struct
  pointer) rather than TS's `Op` object, and field names are capitalised
  (`op.Name`, `op.Src`, `op.Prefix`).

- **`Op.OP_MARK`.** TS tags plugin-owned ops with an `OP_MARK` identity to
  distinguish them from foreign arrays; Go uses a Go type assertion to `*Op`
  instead, so there is no `OP_MARK` field.

None of these change the grammar, the operator set, the precedence rules, or
the output shapes — they are how the same behaviour is achieved in Go.

## See also

- [Tutorial](tutorial.md) — the happy path end to end.
- [Guide](guide.md) — recipes for custom operators, calls, ternaries.
- [Reference](reference.md) — exact functions, types, and the default table.
