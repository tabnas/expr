# Explanation

Background reading on how the plugin works. Useful before you customise
precedence tables or diagnose a surprising parse.

- [Pratt parsing in one page](#pratt-parsing-in-one-page)
- [Why S-expressions](#why-s-expressions)
- [Paren semantics and preval](#paren-semantics-and-preval)
- [Ternary as a two-token operator](#ternary-as-a-two-token-operator)
- [How the plugin plugs into Jsonic](#how-the-plugin-plugs-into-jsonic)
- [The `g=expr` tagging convention](#the-gexpr-tagging-convention)

---

## Pratt parsing in one page

Pratt parsing is a top-down operator-precedence algorithm where every
operator carries two numbers: a _left binding power_ (how strongly it
pulls in the term to its left) and a _right binding power_ (how strongly
it pulls to the right).

When the parser sits between two operators, it compares the left-side
operator's _right_ BP to the right-side operator's _left_ BP. Whichever
is higher wins the shared term.

That's enough to handle precedence (`*` beats `+`), associativity
(left-associative when `left < right`, right-associative when
`left > right`), and mixed pre/in/post-fix operators without separate
grammar productions per operator.

## Why S-expressions

The parse result is an array starting with the operator:
`['+', 1, ['*', 2, 3]]`. This has three properties we want:

- **Uniform shape**: infix, prefix, suffix, ternary, and paren all fit
  the same `[op, ...terms]` template.
- **Easy to walk**: a single recursive function (your `evaluate`) can
  fold the tree bottom-up.
- **Round-trippable**: `Simplify` converts to pure strings/arrays/maps,
  which survives `JSON.stringify` and deep-equality comparisons.

## Paren semantics and preval

Parens are modelled as unary operators whose single term is whatever is
inside. So `(1+2)` parses as `['(', ['+', 1, 2]]` rather than collapsing
the parens away. Keeping the paren op in the tree lets evaluators
distinguish grouping from implicit structure (important when you add
paren kinds like `[...]` that mean "list" rather than "group").

`preval` extends this: a paren op can consume the token preceding `(`
as an extra operand, turning `foo(1,2)` into `['(', 'foo', 1, 2]`.
Combined with `preval.allow`, this gives you named function-call syntax
without needing a full function grammar.

## Ternary as a two-token operator

`a?b:c` is a single operator with two syntactic tokens (`?` and `:`) and
three operands. The plugin exposes this as a normal `OpDef` with
`src: ['?', ':']` and `ternary: true`. Nesting is governed by the same
BP comparison as binary operators — `1?2:3?4:5` chains right because the
ternary op is right-associative by convention.

## How the plugin plugs into Jsonic

On `use`, the plugin:

1. Registers each operator token via `jsonic.options({ fixed: ... })`
   so the lexer recognises them.
2. Extends a few existing Jsonic rules (`val`, `list`, `map`, `pair`,
   `elem`) with alts that backtrack when an operator token appears and
   hand control to the plugin's own `expr` rule.
3. Defines three new rules — `expr`, `paren`, and (optional) `ternary`
   — that implement the Pratt algorithm as ordered alternates.

The result is that expression syntax is additive: it enters via
backtracking from the existing value/list/map rules, and returns the
same kind of node (a value) back to them when it finishes.

## The `g=expr` tagging convention

Every alt the plugin adds carries `expr` in its grammar group (`g`)
field. Users of a shared Jsonic instance can then strip the expression
grammar cleanly:

```ts
jsonic.options({ rule: { exclude: 'expr' } })
```

Internally the plugin tags alts by snapshotting `rs.Open`/`rs.Close`
before each rule modifier and appending `expr` only to newly-added
alts. Pre-existing alts (the base Jsonic grammar) are left untouched.

This mirrors jsonic's own `GrammarSetting.Rule.Alt.G` mechanism, which
applies when rules are installed via `jsonic.grammar()` (the plugin
uses `jsonic.rule()` instead, so the tag is applied manually).
