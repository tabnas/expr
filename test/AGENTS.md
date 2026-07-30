# Agents Guide — shared spec fixtures

`spec/*.tsv` holds the cross-runtime conformance fixtures. Both runtimes run
the same files, so a change here affects TypeScript and Go together — edit
with that in mind.

## Format

Tab-separated, one case per line. Blank lines and lines whose first
non-space character is `#` are skipped; each file opens with a title line and
a column legend:

    # <what this file covers>
    # input	expected_output
    1+2	["+",1,2]

Both loaders trim the whole line, then split at the **first** tab.
`expected_output` is parsed as JSON — it is the value the expression
evaluates to, with operator trees written as `[op, ...operands]`.

**The `input` column is used verbatim** and is line-trimmed: no escape
sequence is decoded, and leading or trailing whitespace cannot be tested
here. A case needing either belongs in an in-language test.

Files are named after the feature they pin (`ternary-*`, `paren-*`,
`unary-*`, …); the plugin configuration a file assumes belongs to the runner
that names it.

## Who runs what

- TypeScript: `ts/test/*.test.ts` via `loadSpec` in `ts/test/spec-util.ts`.
- Go: `go/*_test.go` via `loadSpec` in `go/expr_test.go`.

Both name the same files. A fixture only one runtime runs proves nothing, so
wire a new file into both.

## Rules

- Prefer adding a fixture here over a one-off in-language assertion when a
  case is expressible as input → output. That is what keeps the two runtimes
  honest against each other.
- TypeScript is canonical. If the two runtimes disagree, the TS behaviour is
  the expected value — unless Go has exposed a genuine TS defect, or the
  difference is one of the intentional divergences the root `AGENTS.md`
  records, which stay out of these shared fixtures.
- A new fixture must pass in BOTH runtimes before it counts:
  `go test ./...` from `go/`, and **`npm run build && npm test`** from `ts/`.
  Plain `npm test` runs the previously compiled `dist-test/`, so it can pass
  without ever loading a newly added fixture.
