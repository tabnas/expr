# tabnasexpr (`github.com/tabnas/expr/go`)

The Go port of [`@tabnas/expr`](../ts/README.md) — an expression-syntax
plugin for the Tabnas/jsonic parser.

It adds Pratt-parser expressions: infix, prefix, suffix, ternary, and paren
operators with a configurable binding-power (precedence) scale. Expressions
parse into LISP-style S-expressions (slices whose first element is the
operator), which a user-supplied evaluator can reduce to values. The
TypeScript implementation is canonical; this port behaves identically (the
shared `test/spec/*.tsv` fixtures are the parity contract).

## Install

```sh
go get github.com/tabnas/expr/go
```

```go
import (
    jsonic "github.com/tabnas/jsonic/go"
    tabnasexpr "github.com/tabnas/expr/go"
)
```

## Tiny example

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

Add an `evaluate` callback to compute values instead:

```go
opts := map[string]interface{}{
    "evaluate": func(r *jsonic.Rule, ctx *jsonic.Context, op *tabnasexpr.Op, terms []interface{}) interface{} {
        a, _ := terms[0].(float64)
        switch op.Name {
        case "addition-infix":
            b, _ := terms[1].(float64)
            return a + b
        case "multiplication-infix":
            b, _ := terms[1].(float64)
            return a * b
        case "negative-prefix":
            return -a
        case "plain-paren":
            return a
        }
        return nil
    },
}

v, _ := tabnasexpr.Parse("1+2*3", opts) // 7
w, _ := tabnasexpr.Parse("(1+2)*3", opts) // 9
```

## Documentation

Docs follow the [Diátaxis](https://diataxis.fr) framework:

- **[Tutorial](doc/tutorial.md)** — parse and evaluate your first
  expression.
- **[Guide](doc/guide.md)** — recipes: custom operators, function-call
  syntax, ternaries, strict math, walking the tree.
- **[Reference](doc/reference.md)** — exported functions and types, the
  options map, and the default operator table.
- **[Concepts](doc/concepts.md)** — Pratt parsing, the binding-power scale,
  and the differences from the TS version.

TypeScript (canonical) docs: [tutorial](../ts/doc/tutorial.md) ·
[guide](../ts/doc/guide.md) · [reference](../ts/doc/reference.md) ·
[concepts](../ts/doc/concepts.md).

## Grammar diagram

The grammar as a railroad/syntax diagram (generated from the live grammar
with [`@tabnas/railroad`](https://github.com/tabnas/railroad)):

![expr grammar railroad diagram](../ts/doc/grammar.svg)

ASCII version: [`../ts/doc/grammar.txt`](../ts/doc/grammar.txt).

## License

MIT. See [LICENSE](../LICENSE).
