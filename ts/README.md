# @tabnas/expr

An expression-syntax plugin for the [Tabnas](https://jsonic.senecajs.org)
parser, available in both TypeScript and Go.

Adds Pratt-parser expressions to Tabnas: infix, prefix, suffix, ternary,
and paren operators with configurable precedence. Expressions parse into
LISP-style S-expressions (arrays whose first element is the operator src),
which a user-supplied evaluator can reduce to values.

[![npm version](https://img.shields.io/npm/v/@tabnas/expr.svg)](https://npmjs.com/package/@tabnas/expr)
[![build](https://github.com/tabnas/expr/actions/workflows/build.yml/badge.svg)](https://github.com/tabnas/expr/actions/workflows/build.yml)

## Install

TypeScript:

```sh
npm install @tabnas/expr @tabnas/parser @tabnas/jsonic
```

Go:

```sh
go get github.com/tabnas/expr/go
```

## Documentation

Docs are organised following the [Diátaxis](https://diataxis.fr) framework:

- **[Tutorial](docs/tutorial.md)** — start here. Parse your first expression in
  TS and Go.
- **[How-to guides](docs/how-to.md)** — focused recipes: add an operator,
  plug in an evaluator, use paren-preval for function calls, restrict to
  strict math.
- **[Reference](docs/reference.md)** — exported types and functions,
  `OpDef` schema, default operator set, grammar group tags.
- **[Explanation](docs/explanation.md)** — design notes: Pratt algorithm,
  S-expression AST, paren/ternary/preval semantics, why `g=expr` tagging.

## License

MIT. See [LICENSE](LICENSE).
