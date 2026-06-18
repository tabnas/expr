# @tabnas/expr

This plugin allows the [Tabnas](https://github.com/tabnas/jsonic) JSON parser to support expression syntax.

This repository contains:

| Path | Description |
|---|---|
| [`ts/`](ts/) | TypeScript / JavaScript implementation. |
| [`go/`](go/) | Go port. |
| [`test/spec/`](test/spec/) | Shared conformance fixtures, exercised by both runtimes. |

See [`ts/README.md`](ts/README.md) for usage.

## Grammar diagram

The grammar as a railroad/syntax diagram, generated from the live grammar
with [`@tabnas/railroad`](https://github.com/tabnas/railroad):

![expr grammar railroad diagram](ts/doc/grammar.svg)

ASCII version: [`ts/doc/grammar.txt`](ts/doc/grammar.txt).

## License

MIT. Copyright (c) Richard Rodger.
