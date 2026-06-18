# Guide: expression recipes (Go)

The Go port of the [TypeScript guide](../../ts/doc/guide.md). Task-focused
recipes, each self-contained. They assume you did the
[tutorial](tutorial.md) and know that the plugin stacks on `jsonic`, that
the result is an S-expression slice (use `Simplify` to read it), and that an
`evaluate` callback has signature
`func(*jsonic.Rule, *jsonic.Context, *tabnasexpr.Op, []interface{}) interface{}`.

Options are a `map[string]interface{}`; the operator map lives under the
`"op"` key, the evaluator under `"evaluate"`.

```go
import (
    jsonic "github.com/tabnas/jsonic/go"
    tabnasexpr "github.com/tabnas/expr/go"
)
```

A `toF` numeric coercion helper is reused throughout:

```go
func toF(v interface{}) float64 {
    switch n := v.(type) {
    case float64:
        return n
    case int:
        return float64(n)
    }
    return 0
}
```

---

## Add a custom infix operator

Each `op` entry is an operator definition (a `map[string]interface{}`). For
an infix operator give `"infix": true`, the source string `"src"`, and a
binding-power pair (`"left"`, `"right"`). `left < right` is
left-associative; `left > right` is right-associative.

Pick a tier base `N * 1000000` that does not collide with the built-ins
(addition `2000000`, multiplication `3000000`, unary prefix `4000000`). Here
`^` (exponent) is tighter than multiplication and **right**-associative:

```go
j := tabnasexpr.MakeJsonic(map[string]interface{}{
    "op": map[string]interface{}{
        "power": map[string]interface{}{
            "infix": true, "src": "^", "left": 5100000, "right": 5000000,
        },
    },
})

a, _ := j.Parse("2^3^2")
fmt.Println(tabnasexpr.Simplify(a)) // ["^" 2 ["^" 3 2]]

b, _ := j.Parse("1+2^3")
fmt.Println(tabnasexpr.Simplify(b)) // ["+" 1 ["^" 2 3]]
```

The resolved name is decorated with its kind: `power` becomes
`power-infix`.

## Add a prefix and a suffix operator

Prefix operators take only `"right"`; suffix operators take only `"left"`.
The unset side falls back to the loosest/tightest extreme.

```go
j := tabnasexpr.MakeJsonic(map[string]interface{}{
    "op": map[string]interface{}{
        "factorial": map[string]interface{}{"suffix": true, "src": "!", "left": 6000000},
        "at":        map[string]interface{}{"prefix": true, "src": "@", "right": 5000000},
    },
})

a, _ := j.Parse("1+2!")
fmt.Println(tabnasexpr.Simplify(a)) // ["+" 1 ["!" 2]]

b, _ := j.Parse("@1+2")
fmt.Println(tabnasexpr.Simplify(b)) // ["+" ["@" 1] 2]
```

Names: `factorial` → `factorial-suffix`, `at` → `at-prefix`.

## Define a function-call syntax (paren-preval)

A "preval" paren absorbs the value to its left as a first operand — the shape
of a function call `f(args)`. Add `"preval"` to a paren op. With
`"preval": {"active": true}` the paren works with or without a leading
value:

```go
j := tabnasexpr.MakeJsonic(map[string]interface{}{
    "op": map[string]interface{}{
        "plain": map[string]interface{}{
            "paren": true, "osrc": "(", "csrc": ")",
            "preval": map[string]interface{}{"active": true},
        },
    },
})

a, _ := j.Parse("foo(1,2)")
fmt.Println(tabnasexpr.Simplify(a)) // ["(" "foo" [1 2]]

b, _ := j.Parse("(1+2)")
fmt.Println(tabnasexpr.Simplify(b)) // ["(" ["+" 1 2]]
```

`foo(1,2)` becomes `["(" "foo" [1 2]]`: operator, function name, then the
implicit-list argument slice. A bare `(1+2)` (no preceding value) is still
just grouping.

To **evaluate** calls, dispatch on the paren op (`plain-paren`) and treat the
first term as the function name. The argument list arrives as a single
`[]interface{}` operand:

```go
func evaluate(r *jsonic.Rule, ctx *jsonic.Context, op *tabnasexpr.Op, terms []interface{}) interface{} {
    switch op.Name {
    case "addition-infix":
        return toF(terms[0]) + toF(terms[1])
    case "multiplication-infix":
        return toF(terms[0]) * toF(terms[1])
    case "func-paren":
        if name, ok := terms[0].(string); ok {
            args := terms[1:]
            if len(args) == 1 {
                if sl, ok := args[0].([]interface{}); ok {
                    args = sl
                }
            }
            switch name {
            case "max":
                if toF(args[0]) > toF(args[1]) {
                    return toF(args[0])
                }
                return toF(args[1])
            case "min":
                if toF(args[0]) < toF(args[1]) {
                    return toF(args[0])
                }
                return toF(args[1])
            }
        }
        return terms[0]
    }
    return nil
}

func runCalls() {
    j := tabnasexpr.MakeJsonic(map[string]interface{}{
        "op": map[string]interface{}{
            "func": map[string]interface{}{
                "paren": true, "osrc": "(", "csrc": ")",
                "preval": map[string]interface{}{"active": true},
            },
        },
        "evaluate": evaluate,
    })

    v, _ := j.Parse("max(1,2)")
    fmt.Println(v) // 2

    w, _ := j.Parse("1+max(2,5)")
    fmt.Println(w) // 6
}
```

(This is exactly the shape the `evaluate-math.tsv` conformance fixture
exercises in `go/expr_test.go`.)

Use `"preval": {"required": true}` for an index-style paren that *must* have
a leading value (so `[1]` is a literal list, but `a[1]` is an index op). Add
`"allow": []string{"foo"}` inside `preval` to restrict which leading names
qualify.

## Chain calls and indexes

Preval parens chain: each successive paren takes the previous result as its
preval, giving `f(x)(y)`, `a[0][1]`, and mixed forms.

```go
j := tabnasexpr.MakeJsonic(map[string]interface{}{
    "op": map[string]interface{}{
        "index": map[string]interface{}{
            "osrc": "[", "csrc": "]", "paren": true,
            "preval": map[string]interface{}{"required": true},
        },
        "call": map[string]interface{}{
            "osrc": "(", "csrc": ")", "paren": true,
            "preval": map[string]interface{}{"active": true},
        },
        "plain": nil, // disable the default `(`…`)` so `(` only means `call`
    },
})

a, _ := j.Parse("a[0][1]")
fmt.Println(tabnasexpr.Simplify(a)) // ["[" ["[" "a" 0] 1]

b, _ := j.Parse("f(x)[i]")
fmt.Println(tabnasexpr.Simplify(b)) // ["[" ["(" "f" "x"] "i"]
```

Setting an op entry to `nil` removes it — here it removes the default
`plain` paren so `(` is unambiguously `call`.

## Add a ternary (conditional) operator

A ternary op has two source markers in `"src"` (open and close). Ternaries
are right-associative. The Go option map takes the markers as
`[]interface{}{"?", ":"}`.

```go
j := tabnasexpr.MakeJsonic(map[string]interface{}{
    "op": map[string]interface{}{
        "cond": map[string]interface{}{
            "ternary": true, "src": []interface{}{"?", ":"},
        },
    },
})

a, _ := j.Parse("1?2:3")
fmt.Println(tabnasexpr.Simplify(a)) // ["?" 1 2 3]

b, _ := j.Parse("1?2: 0?4:5")
fmt.Println(tabnasexpr.Simplify(b)) // ["?" 1 2 ["?" 0 4 5]]
```

The S-expression has three operands: condition, then-branch, else-branch.
Evaluate by branching on `cond-ternary`:

```go
func evalTernary(r *jsonic.Rule, ctx *jsonic.Context, op *tabnasexpr.Op, terms []interface{}) interface{} {
    if op.Name == "cond-ternary" {
        if b, ok := terms[0].(bool); ok && b {
            return terms[1]
        }
        if toF(terms[0]) != 0 {
            return terms[1]
        }
        return terms[2]
    }
    return nil
}
```

## Restrict to strict math (drop a default operator)

The defaults include `+ - * / %` and the `plain` paren. Set an op entry to
`nil` to remove it. Here `%` and `/` are dropped:

```go
j := tabnasexpr.MakeJsonic(map[string]interface{}{
    "op": map[string]interface{}{
        "remainder": nil,
        "division":  nil,
    },
})

a, _ := j.Parse("1+2*3")
fmt.Println(tabnasexpr.Simplify(a)) // ["+" 1 ["*" 2 3]]
```

`+`, `-`, `*` still work; `/` and `%` become ordinary text.

## Walk the tree yourself with `Evaluation`

Parse with no `evaluate` to get the raw S-expression, then reduce it later
(or repeatedly) with the exported `Evaluation` function. It folds the tree
bottom-up and calls your resolver for each op. `rule`/`ctx` may be `nil`:

```go
j := tabnasexpr.MakeJsonic() // no evaluate option
tree, _ := j.Parse("1+2*3")  // raw S-expression

math := func(r *jsonic.Rule, ctx *jsonic.Context, op *tabnasexpr.Op, terms []interface{}) interface{} {
    switch op.Name {
    case "addition-infix":
        return toF(terms[0]) + toF(terms[1])
    case "multiplication-infix":
        return toF(terms[0]) * toF(terms[1])
    }
    return nil
}

result := tabnasexpr.Evaluation(nil, nil, tree, math)
fmt.Println(result) // 7
```

This separates *parse* from *evaluate*: parse once, evaluate many times. It
is the same pattern `TestEvaluation` and `TestExampleDotpath` use in
`go/expr_test.go`.
