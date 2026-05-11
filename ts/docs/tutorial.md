# Tutorial

A walk-through of parsing and evaluating your first expression with
`@jsonic/expr`. By the end you will have:

1. Parsed `1+2*3` into an S-expression AST
2. Plugged in an evaluator to compute the numeric result
3. Seen how expressions compose with the rest of Jsonic JSON

The TypeScript and Go variants are kept side-by-side. Pick one language
and follow the column; the inputs and outputs match.

---

## 1. Set up

```sh
# TypeScript
npm install @jsonic/expr jsonic

# Go
go get github.com/jsonicjs/expr/go
```

## 2. Parse an expression

TypeScript:

```ts
import { Jsonic } from 'jsonic'
import { Expr } from '@jsonic/expr'

const j = Jsonic.make().use(Expr)

console.log(j('1+2*3'))
// [ { src: '+', ... }, 1, [ { src: '*', ... }, 2, 3 ] ]
```

Go:

```go
package main

import (
  "fmt"
  expr "github.com/jsonicjs/expr/go"
)

func main() {
  result, _ := expr.Parse("1+2*3")
  fmt.Println(expr.Simplify(result))
  // ["+" 1 ["*" 2 3]]
}
```

The AST is an array whose first element is the operator. Operator
precedence (`*` binds tighter than `+`) shapes the tree without
parentheses.

## 3. Add an evaluator

An evaluator is a function that receives the op and its evaluated terms
and returns a value. Supply it via the plugin's `evaluate` option.

TypeScript:

```ts
import { Jsonic } from 'jsonic'
import { Expr } from '@jsonic/expr'

const math = (rule: any, ctx: any, op: any, ...terms: any[]) => {
  switch (op.src) {
    case '+': return terms[0] + terms[1]
    case '-': return op.prefix ? -terms[0] : terms[0] - terms[1]
    case '*': return terms[0] * terms[1]
    case '/': return terms[0] / terms[1]
  }
}

const j = Jsonic.make().use(Expr, { evaluate: math })

console.log(j('1+2*3'))  // 7
console.log(j('-4+10'))  // 6
```

Go:

```go
package main

import (
  "fmt"
  jsonic "github.com/jsonicjs/jsonic/go"
  expr "github.com/jsonicjs/expr/go"
)

func math(r *jsonic.Rule, ctx *jsonic.Context, op *expr.Op, terms []interface{}) interface{} {
  a, _ := terms[0].(float64)
  switch op.Src {
  case "+":
    b, _ := terms[1].(float64)
    return a + b
  case "-":
    if op.Prefix {
      return -a
    }
    b, _ := terms[1].(float64)
    return a - b
  case "*":
    b, _ := terms[1].(float64)
    return a * b
  case "/":
    b, _ := terms[1].(float64)
    return a / b
  }
  return nil
}

func main() {
  result, _ := expr.Parse("1+2*3", map[string]interface{}{"evaluate": math})
  fmt.Println(result)  // 7
}
```

## 4. Compose with JSON

Expressions live inside any Jsonic value slot, so you can drop them
into objects and arrays:

```ts
j('{ total: 1+2*3, flags: [!true, -4] }')
// { total: 7, flags: [false, -4] }   (after an evaluator for ! and - is added)
```

## Next steps

- [How-to guides](how-to.md) for recipes: adding a custom operator,
  using parens for grouping, building function-call syntax via
  `paren-preval`.
- [Reference](reference.md) for the full `OpDef` schema and the default
  operator table.
- [Explanation](explanation.md) for how the Pratt parser and AST work.
