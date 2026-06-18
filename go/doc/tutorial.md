# Tutorial: parse and evaluate your first expression (Go)

This is the Go port of the [TypeScript tutorial](../../ts/doc/tutorial.md).
The TypeScript implementation is canonical; the Go package `tabnasexpr`
tracks it and behaves identically. Follow this top to bottom to go from
nothing to a working expression evaluator.

By the end you will have:

1. Parsed `1+2*3` into an S-expression tree.
2. Seen how operator precedence shapes that tree.
3. Plugged in an evaluator to compute a number.
4. Dropped an expression inside ordinary JSON.

`tabnasexpr` is a plugin for the Go port of the Tabnas/jsonic parser. It
layers on the relaxed-JSON `jsonic` grammar.

---

## 1. Install

```sh
go get github.com/tabnas/expr/go
```

Import it as `tabnasexpr` (the package name) and the engine as `jsonic`:

```go
import (
    jsonic "github.com/tabnas/jsonic/go"
    tabnasexpr "github.com/tabnas/expr/go"
)
```

## 2. Parse an expression

The simplest entry point is the package-level `Parse`. The raw result holds
internal `*Op` nodes, so pass it through `Simplify` to get plain
arrays whose first element is the operator's source string:

```go
package main

import (
    "fmt"

    tabnasexpr "github.com/tabnas/expr/go"
)

func main() {
    tree, _ := tabnasexpr.Parse("1+2*3")
    fmt.Println(tabnasexpr.Simplify(tree))
    // ["+" 1 ["*" 2 3]]
}
```

The result is a LISP-style S-expression: a slice whose **first element is
the operator** and whose remaining elements are the operands. `*` binds
tighter than `+`, so `2*3` became a nested sub-expression — precedence did
that without any parentheses.

A few more shapes:

```go
tabnasexpr.Simplify(must(tabnasexpr.Parse("1+2")))    // ["+" 1 2]
tabnasexpr.Simplify(must(tabnasexpr.Parse("-1+2")))   // ["+" ["-" 1] 2]
```

`-1` is the one-operand expression `["-" 1]` (unary minus).

## 3. Evaluate to a value

A parse tree is structure, not a number. To compute, supply an `evaluate`
callback through the options map. It runs bottom-up — operands are already
evaluated when your callback sees an operator. Its signature is
`func(rule *jsonic.Rule, ctx *jsonic.Context, op *tabnasexpr.Op, terms []interface{}) interface{}`,
where `terms` is the slice of already-evaluated operands.

```go
package main

import (
    "fmt"

    jsonic "github.com/tabnas/jsonic/go"
    tabnasexpr "github.com/tabnas/expr/go"
)

func toF(v interface{}) float64 {
    switch n := v.(type) {
    case float64:
        return n
    case int:
        return float64(n)
    }
    return 0
}

func math(r *jsonic.Rule, ctx *jsonic.Context, op *tabnasexpr.Op, terms []interface{}) interface{} {
    switch op.Name {
    case "addition-infix":
        return toF(terms[0]) + toF(terms[1])
    case "subtraction-infix":
        return toF(terms[0]) - toF(terms[1])
    case "multiplication-infix":
        return toF(terms[0]) * toF(terms[1])
    case "division-infix":
        return toF(terms[0]) / toF(terms[1])
    case "negative-prefix":
        return -toF(terms[0])
    case "positive-prefix":
        return toF(terms[0])
    case "plain-paren":
        return terms[0]
    }
    return nil
}

func main() {
    opts := map[string]interface{}{"evaluate": math}

    a, _ := tabnasexpr.Parse("1+2*3", opts)
    fmt.Println(a) // 7

    b, _ := tabnasexpr.Parse("(1+2)*3", opts)
    fmt.Println(b) // 9

    c, _ := tabnasexpr.Parse("-4+10", opts)
    fmt.Println(c) // 6
}
```

Note that the resolved operator name is decorated with its kind: the default
infix `+` is `addition-infix`, the unary `-` is `negative-prefix`, and the
grouping parens are `plain-paren`. Dispatch on `op.Name`, or on `op.Src`
plus the kind booleans (`op.Prefix`, `op.Infix`, `op.Paren`).

## 4. Expressions inside JSON

Because the plugin layers on jsonic, expressions slot into any value
position — map values, list elements, nested objects — and your evaluator
runs on each:

```go
opts := map[string]interface{}{"evaluate": math}
v, _ := tabnasexpr.Parse("{ a: 1+2, b: [3*4, -5] }", opts)
fmt.Println(v) // map[a:3 b:[12 -5]]
```

## Building a reusable parser

`Parse` with no options reuses a shared default instance; `Parse` with
options builds a fresh instance per call. To configure once and parse many
times, build an instance with `MakeJsonic` and call its `Parse`:

```go
j := tabnasexpr.MakeJsonic(map[string]interface{}{"evaluate": math})
x, _ := j.Parse("1+2*3") // 7
y, _ := j.Parse("10-4")  // 6
_ = x
_ = y
```

## Next steps

- [Guide](guide.md) — recipes: custom operators, function-call syntax,
  ternaries, strict math, walking the tree yourself.
- [Reference](reference.md) — exact exported types and functions, the
  `OpDef` options, and the default operator table.
- [Concepts](concepts.md) — Pratt parsing, the binding-power scale, and the
  differences from the TS version.
