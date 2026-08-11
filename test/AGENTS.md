# Agents Guide — shared spec fixtures

`spec/*.tsv` holds the cross-runtime conformance fixtures. Both runtimes run
the same files, so a change here affects TypeScript and Go together — edit
with that in mind.

## Format

Tab-separated, one case per line, read by the shared
[`@tabnas/support`](https://github.com/tabnas/support) loader — one
loader, in two languages written to behave identically. **Line 1 is the
header**, and a `#`-leading line with no tab is a comment:

    input	expected
    # <what this file covers>
    1+2	["+",1,2]

That header line is new. These fixtures used to open with a title comment
and a `#`-prefixed column legend (`# input	expected_output`), which the
shared loader would have read as a data row, since a `#` line WITH a tab
is data — the rule that lets a fixture's input be a comment in the parsed
language. Every file was re-headed: the legend line is gone, the title
comment stays, no data row moved.

`expected` is parsed as JSON — the value the expression evaluates to, with
operator trees written as `[op, ...operands]`.

Escapes (`\n`, `\r`, `\t`, `\\`) are decoded in the `input` column. No
fixture uses one, but a case needing a real control character can now be
written; it could not before. Lines are no longer trimmed, so leading and
trailing whitespace in a cell is significant — no fixture has any.

Files are named after the feature they pin (`ternary-*`, `paren-*`,
`unary-*`, …); the plugin configuration a file assumes belongs to the runner
that names it.

## Who runs what

- TypeScript: `ts/test/spec.test.ts` — `runSpec(name, j)`, a
  `makeRunner(...)` over the file.
- Go: `go/expr_test.go` — `runSpec(t, name, j)`, a `support.Runner{...}`
  over the file.

A fixture is named by the test that supplies its parser, because what
varies per case is the CONFIGURED PARSER — several files need operators
defined in the plugin's options, which cannot live in a fixture column. So
a new file has to be wired into both runtimes by hand. A fixture only one
runtime runs proves nothing.

Each ROW is now its own test case rather than one assertion inside a
per-file test, so a failure names the file and line it came from.

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
