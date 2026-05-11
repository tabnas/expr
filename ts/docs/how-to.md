# How-to guides

Focused recipes. Each guide assumes you've read the [Tutorial](tutorial.md).

- [Add a custom infix operator](#add-a-custom-infix-operator)
- [Add a prefix or suffix operator](#add-a-prefix-or-suffix-operator)
- [Group with parentheses](#group-with-parentheses)
- [Build function-call syntax with paren-preval](#build-function-call-syntax-with-paren-preval)
- [Add a ternary operator](#add-a-ternary-operator)
- [Disable a default operator](#disable-a-default-operator)
- [Strip expr alts with a group filter](#strip-expr-alts-with-a-group-filter)

---

## Add a custom infix operator

Define an operator by name under `op`. Give it an `src` token and a
`left`/`right` precedence pair (lower number = lower priority).
Left-associative operators have `left < right`; right-associative have
`left > right`.

TypeScript:

```ts
Jsonic.make().use(Expr, {
  op: {
    power: { infix: true, src: '^', left: 260, right: 250 },  // right-assoc
  },
})('2^3^2')  // ['^', 2, ['^', 3, 2]]
```

Go:

```go
expr.Parse("2^3^2", map[string]interface{}{
  "op": map[string]interface{}{
    "power": map[string]interface{}{
      "infix": true, "src": "^", "left": 260, "right": 250,
    },
  },
})
```

## Add a prefix or suffix operator

Prefix operators use `right` only; suffix operators use `left` only.

```ts
Expr, {
  op: {
    bang:   { prefix: true,  src: '!', right: 200 },   // !x
    factor: { suffix: true,  src: '!', left:  210 },   // x!
  },
}
```

Because both overloads share the `!` src, Jsonic disambiguates by
position: `!` appearing before a term is prefix, after a term is suffix.

## Group with parentheses

Parens are defined as operators with `paren: true`, `osrc`, `csrc`:

```ts
Expr, {
  op: {
    brace: { paren: true, osrc: '[', csrc: ']' },
  },
}
```

The default operator set already includes `plain: { paren:true, osrc:'(',
csrc:')' }`. Multiple paren kinds can coexist.

## Build function-call syntax with paren-preval

`preval` turns `foo(1,2)` into a paren expression whose `preval` value
is `foo`. Useful for call-like syntax.

```ts
Expr, {
  op: {
    call: {
      paren: true, osrc: '(', csrc: ')',
      preval: { active: true, required: true },
    },
  },
}
```

With `preval.required`, a paren only matches when preceded by a value.
Use `preval.allow: ['foo','bar']` to restrict which preceding names are
valid.

## Add a ternary operator

Ternary is defined by two tokens in `src`:

```ts
Expr, {
  op: {
    cond: { ternary: true, src: ['?', ':'], left: 80, right: 90 },
  },
}
```

Parses `a ? b : c` as `['?', a, b, c]`.

## Disable a default operator

Set the named op to `null` in your options:

```ts
Jsonic.make().use(Expr, { op: { remainder: null } })
```

`1%2` now fails to parse — no `%` operator is registered.

## Strip expr alts with a group filter

Every grammar alternate added by the plugin is tagged with `expr` in
its `g` (group) field. Use Jsonic's `rule.exclude` option to remove
the expression grammar entirely:

```ts
Jsonic.make().use(Expr).options({ rule: { exclude: 'expr' } })
```

This is how you temporarily reuse a shared Jsonic instance without
expression parsing.
