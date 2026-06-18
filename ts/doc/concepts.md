# Concepts: how `@tabnas/expr` works and why

Understanding-oriented. This explains the engine relationship, the
S-expression AST, the Pratt binding-power scale, paren/ternary/preval
semantics, and the design trade-offs behind them.

---

## The engine relationship

`@tabnas/expr` is **not** a standalone parser. There are three layers:

1. **`@tabnas/parser`** — the Tabnas engine: a configurable, rule-based
   parser with a lexer. It provides the `Rule`/`RuleSpec`/`Context`
   machinery and the plugin system.
2. **`@tabnas/jsonic`** — the relaxed-JSON grammar (`val`, `map`, `list`,
   `pair`, `elem` rules) built on the engine. This is what understands
   objects, arrays, bare words, comments, implicit structure, etc.
3. **`@tabnas/expr`** — this plugin. It hooks new alternates onto jsonic's
   existing `val` rule and adds two of its own rules (`expr`, `paren`, plus
   `ternary` when configured), so that operator syntax becomes available
   anywhere a jsonic *value* can appear.

That layering is why an expression can live inside a JSON object value, a
list element, or at the top level — the expression grammar is woven into the
value rule, not bolted on beside it.

The plugin uses the engine's `tn.rule(name, …)` API (imperative rule
editing) rather than the declarative `tn.grammar(…)` form that `json` and
`csv` use. It needs the fine control: expression nodes are **rewritten
in-place** as parsing proceeds (see [Why in-place rewriting](#why-in-place-rewriting)),
which the declarative form cannot express. Every alternate the plugin adds
is tagged with the group `expr` so the grammar model and railroad diagram
can identify the plugin's contributions.

## The S-expression AST

A parsed expression is a LISP-style **S-expression**: an array whose first
element is the operator and whose remaining elements are the operands.

```text
1+2*3   →   [ +op, 1, [ *op, 2, 3 ] ]
```

The first element is a rich `Op` object (its source text is `op.src`); the
rest are operands, which may themselves be sub-expressions, plain JSON
values, objects, or arrays.

Why an array-of-op-first? Three reasons:

- It is **uniform**: every operator — unary, binary, ternary, paren — is the
  same array shape, just with a different operand count (`op.terms`). One
  evaluator walks them all.
- It is **inspectable**: the tree is plain data. You can transform, print,
  or evaluate it however you like, and you can defer evaluation entirely.
- It **carries metadata**: because the head is the full `Op` (not just a
  string), an evaluator gets precedence, kind flags, source location, and
  your custom `use` data for free.

Evaluation (`evaluation()` or the `evaluate` option) is a simple bottom-up
fold: recurse into operands, then call the resolver with the op and the
evaluated operand array. Parsing and evaluation are deliberately separable —
parse once, evaluate zero or many times.

## Pratt parsing in one paragraph

Operator precedence is resolved with **Pratt parsing** (a.k.a. top-down
operator precedence). Each operator carries a *binding power* on each side
it can bind. When a new operator arrives, the parser compares its binding
power against the operator currently holding the expression and decides
whether to **wrap** the existing expression (the new op is looser, so it
takes the whole thing as a left operand) or **drill** into the last operand
(the new op is tighter, so it grabs only the right edge). That single
comparison, applied repeatedly, yields the correct tree for any mix of
precedences and associativities. The core lives in the `prattify` function.

## Why in-place rewriting

Tabnas builds one JSON AST incrementally as it consumes tokens. An
expression is part of that AST, so the plugin cannot simply swap a node out
for a reshaped one — other rules already hold references to it. Instead, the
partial expression array is **rewritten in place** (`updateExprNode`
overwrites slots and adjusts `length`), preserving referential integrity
while precedence reshapes the tree. This is an implementation detail you
never see in the output, but it explains the careful node bookkeeping in the
source and why the Go port needs a `*ListRef` indirection (Go slices do not
share a header the way JS arrays share identity).

## The binding-power scale

This is the most important concept for configuring operators.

A binding power is just an integer. The Pratt core compares powers **only**
with `<` and `<=` — it never depends on the *distance* between two numbers,
only on their **order**. So:

> Only the ORDER of the magnitudes is a contract. The magnitudes themselves
> are free and volatile.

### Associativity is left vs right

Each infix operator has a `left` and a `right` power:

- `left < right` → **left-associative**. `2+3+4` parses as `(2+3)+4` because
  `addition.left (2000000) < addition.right (2100000)`.
- `left > right` → **right-associative**. Swap the two and `2^3^2` parses as
  `2^(3^2)`.

Prefix operators have only a `right` (no left term to bind), suffix
operators only a `left`. The unset side falls back to
`Number.MIN_SAFE_INTEGER` (loosest) / `Number.MAX_SAFE_INTEGER` (tightest),
which is why prefix gives only `right` and parens give neither.

### The default ladder

The built-in operators occupy a compact low block on a **base-1,000,000**
ladder, leaving the entire range above the tightest built-in (and below
addition) open for your operators:

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

Tiers are `1000000` apart; the `+100000` offset on the `right` of a
left-associative op keeps a tier inside a `100000`-wide band within its
`1000000`-wide slot, so adjacent tiers can never overlap.

### How to extend it

To add an operator, pick a tier base `N * 1000000`:

- **Left-associative infix**: `left = base`, `right = base + 100000`.
- **Right-associative infix**: `left = base + 100000`, `right = base` (swap).
- **Tighter than the built-ins**: use `5000000` and up (exponent, postfix,
  call/member).
- **Looser than addition**: use below `2000000` (logical, comparison,
  assignment, ternary).
- **Need a sub-tier in a gap**: each `1000000`-wide gap holds ~4 sub-tiers —
  use `base + 200000`, `base + 400000`, etc.

You never need to rescale the existing numbers to insert a new operator;
just choose a free tier. If you ever *do* rescale the whole ladder, it must
be an **order-preserving** remap — change relative order and you change the
grammar.

## Parens, ternaries, and preval

These three are variations on the same idea: a structural operator that
brackets a sub-expression.

- **Parens** (`plain` `(`…`)`) are grouping. They are not infix/prefix/suffix
  and carry no binding power — they override precedence by being an explicit
  boundary. The AST keeps the paren as a node (`['(', inner]`) so an
  evaluator can act on grouping if it wants; a typical evaluator just returns
  the inner value.

- **Ternaries** are implemented as a special bracket rule, very like parens,
  but with *two* markers (`['?', ':']`) and *three* operands. They are
  right-associative so `a?b: c?d:e` chains naturally. Internally the ternary
  rule mirrors the paren rule's structure (it even has the same implicit-list
  edge-case handling), which is why the source calls the ternary rule "fancy
  parens".

- **Preval** parens absorb the value immediately to their left as a first
  operand. This is how `f(args)` and `a[i]` are expressed without special
  call/index syntax: a paren with `preval` turns `foo(1,2)` into
  `['(', 'foo', [1,2]]`. `required: true` forces a leading value (so `[1]`
  is a list literal but `a[1]` is an index op); `allow` restricts which
  leading names qualify. Preval parens also **chain** — each picks up the
  previous result — giving `f(x)(y)`, `a[0][1]`, and mixed forms.

## Implicit structure, and the edge cases

jsonic allows implicit lists and maps at the top level (`a,b` → `['a','b']`,
`x:1 y:2` → `{x:1, y:2}`). This plugin extends implicits to work **inside
parens** too, so `foo(1,2)` and `(1 2 3)` produce list operands. Supporting
that required extra counters (`expr_paren`, `expr_ternary`, …) on the parse
context and a fair amount of context-sensitive edge handling — most visibly,
care to *not* embed a surrounding implicit list inside an expression when the
expression is the first item of that list. These are correctness details of
the grammar wiring, not configuration you touch.

## Trade-offs

- **Imperative rule editing over declarative grammar.** More code and more
  intricate node bookkeeping, but it is the only way to express in-place
  precedence reshaping. The plugin accepts the complexity to keep the output
  a single, clean JSON AST.
- **S-expression output, not direct values.** Parsing returns structure by
  default; you opt into evaluation. This decouples syntax from semantics —
  the same parsed `1+2` can mean integer addition, set union, or string
  concatenation depending on the evaluator — at the cost of one extra fold to
  get a value.
- **Order-only binding powers.** Trades a tidy small enum for raw integers,
  but buys downstream clients unlimited headroom to slot operators between
  any two existing tiers without coordinating a global renumber.

## See also

- [Tutorial](tutorial.md) — the happy path end to end.
- [Guide](guide.md) — recipes for custom operators, calls, ternaries.
- [Reference](reference.md) — exact options, types, and default table.
