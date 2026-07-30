# Agents Guide — shared spec fixtures

`spec/*.tsv` holds the cross-runtime conformance fixtures. Both runtimes run
the same files, so a change here affects TypeScript and Go together — edit
with that in mind.

## Format

Tab-separated, one case per line. Lines starting with `#` are comments; each
file opens with a title line and a column legend:

    # <what this file covers>
    # input	expected_output
    1+2	["+",1,2]

`input` is expression source, with `\n` `\r` `\t` `\\` decoded.
`expected_output` is the parse result as JSON — expressions build
`[op, ...operands]` trees.

Files are named after the feature they pin (`ternary-*`, `paren-*`,
`unary-*`, …); the plugin configuration each file assumes is part of the
runner that names it.

## Who runs what

- TypeScript: `ts/test/*.test.ts` via `ts/test/spec-util.ts`.
- Go: `go/*_test.go`.

Both name the same files. A fixture that only one runtime runs proves
nothing, so wire a new file into both.

## Rules

- Prefer adding a fixture here over a one-off in-language assertion when a
  case is expressible as input → output. That is what keeps the two runtimes
  honest against each other.
- TypeScript is canonical. If the two runtimes disagree, the TS behaviour is
  the expected value — unless Go has exposed a genuine TS defect, in which
  case fix TS first and pin the corrected behaviour here.
- A new fixture must pass in BOTH runtimes: run `go test ./...` (from `go/`)
  and `npm test` (from `ts/`) before considering it done.
