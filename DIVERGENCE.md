# Divergences — @tabnas/expr

Differences between the TypeScript and Go ports that are **recorded rather
than repaired**, each pinned by a test in both ports so the record cannot
outlive what it records (admin `DECISIONS.md` ADR-14).

A pin here fails when the divergence is REPAIRED, not only when it
regresses. That is deliberate: it is the signal to delete the entry along
with the pins.

## The parsed AST has a different shape in the two ports

**Not repaired** — closing it is a breaking change to this port's Go API,
so it is recorded with its measurement and left for a deliberate decision.

Measured on the simplest expression that shows it, `{a:1+2}`:

| | value of `a` |
| --- | --- |
| TypeScript | `[{"src":"+","left":2000000,"right":2100000,"name":"addition-infix", …}]` |
| Go | `{"Val":[{"Name":"addition-infix","Src":"+","Left":2000000, …}], "Child":null,"Implicit":false,"Meta":{"expr":true}}` |

Two **independently repairable** differences, so each has its own
paragraph and its own pair of pins. Closing one does not close the other,
and the pins say so: a partial repair deletes only the matching paragraph
and the matching pin.

### Wrapper

TypeScript yields the term list directly; Go yields an object carrying it
under `Val`, alongside `Child`, `Implicit` and `Meta`.

Pinned by `TestASTShapeWrapperDiverges` (`go/expr_test.go`) and
*"yields the term list directly, where Go wraps it"*
(`ts/test/ast-shape.test.ts`).

### Field naming

Go's AST structs (`Op` and friends in `go/expr.go`) carry **no `json`
tags**, so `encoding/json` emits the exported Go names — `Name`, `Src`,
`Left` — where TypeScript emits `name`, `src`, `left`.

Pinned by `TestASTShapeNamingDiverges` and *"serialises lower-case keys,
where Go emits Go names"*.

Adding `json` tags closes **this half only**. The wrapper survives it.

The second is not merely cosmetic and the first is not merely a wrapper:
lower-casing every Go key does **not** make the two key sets equal.
TypeScript additionally carries `OP_MARK` and a `token` object (`sI`,
`rI`, `cI`, `len`, `isToken`, `why`); Go additionally carries
`Preval.Allow`. So a consumer serialising the parse cannot read both ports
with one shape.

### Where the pins measure

Every pin marshals and round-trips through JSON — the **serialised**
shape, because that is what a consumer sees and what this entry is about.
Inspecting TypeScript's live object instead would keep passing if those
objects ever gained a `toJSON` emitting Go-compatible keys, while the
consumer-visible difference had in fact been repaired. Both ports must
measure the same boundary or the pair is not a pair.

### Why it is recorded and not fixed

`ADR-13` puts the repair on the Go side: TypeScript defines the language,
and the shape a consumer sees is part of it. But `Op` and the wrapper are
this port's public Go types, so changing either is a breaking change for
every Go consumer — the same class of decision as `Chars *string` in
`tabnas/parser` (`parser/DIVERGENCE.md`), and not one to take as a side
effect of a parity sweep.

Adding `json` tags alone would close the naming half without touching the
wrapper, and would still be breaking for anyone marshalling today.

### How it was found

`tasks/ax-parity-probe` in `tabnas/admin`, after this repo gained the
`pluginKind: "grammar"` descriptor field that had kept it out of the probe
(`@tabnas/expr#45`). Fourteen disagreements: **eleven are engine defects
already repaired in open `tabnas/parser` PRs** — six malformed-escape
(#123), three text-ender (#128), two line-separator (#125) — and will
close when those land. The remaining three are this entry.

Those three were reported under the probe's `signed-zero` input class,
which is what the inputs were, not what the difference is. Reading the
label rather than the payload would suggest a signed-zero bug in this
port. There is none.
