# Divergences — @tabnas/expr

Differences between the TypeScript implementation and a port that are
**recorded rather than repaired**, each pinned by a test in the ports it
names so the record cannot outlive what it records (admin `DECISIONS.md`
ADR-14).

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
| Rust | `[{"src":"+","left":2000000.0,"right":2100000.0,"name":"addition-infix", …},1.0,2.0]` |

**The Rust port is on the TypeScript side of both halves**, deliberately:
the term list directly, and lower-case keys. It is pinned by
*"the AST shape is the TypeScript one"* (`rs/tests/expr_test.rs`), which
fails if either half ever drifts to the Go shape. The `.0` on the numbers
is the JSON renderer, not the shape: every engine number is an `f64`, and
`Value::to_string` prints the same values as TypeScript does.

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

## A comment marker adjacent to an operator lexes differently in all three

**Not repaired** — the residue is in the three engines' lexers, not in
this plugin, and the two halves below need changes in repositories this
plugin does not own.

The default `/` operator is a prefix of the comment openers `//` and
`/*`, so the fixed-token matcher and the comment matcher contend for the
same run of characters. Each runtime resolves that differently:
TypeScript sets `lex.match.comment.order` to `1e5` so the comment matcher
runs before the fixed one; the Rust port puts a `check` on the fixed
family, so the fixed matcher stands aside where a contested marker
begins; the Go port does neither.

Measured on the default operator table, with the operator objects reduced
to their `src`:

| input | TypeScript | Go | Rust |
| --- | --- | --- | --- |
| `1 //c` | `1` | `ERROR:unexpected` | `1` |
| `1/2 //c` | `["/",1,2]` | `ERROR:unexpected` | `["/",1,2]` |
| `1//c` | `ERROR:unexpected` | `ERROR:unexpected` | `1` |
| `1/2//c` | `ERROR:unexpected` | `ERROR:unexpected` | `["/",1,2]` |
| `a:1//c` | `ERROR:unexpected` | `ERROR:unexpected` | `{"a":1}` |
| `1/*c*/` | `ERROR:unexpected` | `ERROR:unexpected` | `1` |

Two **independently repairable** halves, so each has its own paragraph
and its own pin. Closing one does not close the other.

### Go lexes no comment at all once `/` is an operator

Rows 1 and 2: TypeScript and Rust read the comment, Go reports the `/`
as unexpected. This is the half `ADR-13` puts squarely on the port, and
it is not repairable from this repository today. `go/expr.go` reaches the
engine through `github.com/tabnas/jsonic/go`, whose `engine.go` re-exports
the engine types but not the matcher factories, so there is no
`MakeCommentMatcher` to register at an order below the fixed matcher's
`2000000`. The repair needs `tabnas/jsonic` to re-export it, after which
the Go plugin registers it the way TypeScript sets the order.

Pinned by `TestCommentAfterOperatorDiverges` (`go/expr_test.go`), which
fails when Go starts reading the comment.

### Rust reads a comment marker TypeScript does not

Rows 3 to 6: the marker sits immediately after a value, with no space
before it, and only Rust reads it. This half is NOT a port defect to
repair. Bare `jsonic` reads `1//c` as `1` in every runtime; it is
registering `/` as a fixed token that breaks it in TypeScript, and the
break survives the reorder. Measured on the canonical engine with no
plugin involved:

| parser | `1//c` |
| --- | --- |
| bare jsonic | `1` |
| bare jsonic, `/` added as a fixed token | `ERROR:unexpected` |
| the same, plus `lex.match.comment.order: 1e5` | `ERROR:unexpected` |

So the canonical is the defective side, and `ADR-13` puts the repair
there rather than in a port: making Rust reject `1//c` would copy a
TypeScript defect into a port, which is the one thing a port must not do.
The repair belongs to the TypeScript engine's fixed matcher, in
`tabnas/parser`.

Pinned by `a_comment_marker_adjacent_to_a_value_is_read`
(`rs/tests/expr_test.rs`) and *"comment-marker-adjacent-to-value"*
(`ts/test/expr.test.ts`), which fail together when the two agree again.

It is deliberately NOT a shared fixture: every row above is red in at
least one runtime, and [`test/AGENTS.md`](test/AGENTS.md) keeps
intentional divergences out of `test/spec`.

## The Rust port refuses a very large expression

**Not repaired** — it is a crash fix, and removing it would make a
pathological document end the process rather than fail.

`tabnas_expr::NODE_LIMIT` is 127. An expression that grows past that many
nodes is refused with the engine's `cancel` code. A flat sum of `n` terms
is `n-1` nodes, so 128 terms is the largest one this port accepts.

Measured on `0+1+2+…`, at the sizes that separate the three:

| terms | TypeScript | Go | Rust |
| --- | --- | --- | --- |
| 128 | the expression | the expression | the expression |
| 129 | the expression | the expression | `ERROR:cancel` |
| 3200 | the expression | the expression | `ERROR:cancel` |
| 5000 | `RangeError: Maximum call stack size exceeded` | the expression | `ERROR:cancel` |

None of the three is unbounded, and only one of them fails cleanly.
TypeScript raises a JavaScript `RangeError` rather than a tabnas error,
somewhere between 3200 and 5000 terms on the measuring machine, because
the value walk is recursive there too; Go grows its stacks and keeps
going.

The Rust engine walks a value with the CALL STACK to display it, convert
it to JSON or drop it, and a fixed-size thread stack turns that into an
ABORT rather than an error: the process ends, and no caller can catch it.
Refusing early is what turns the abort back into a parse error. The limit
is the one [`tabnas-jsonic`](https://github.com/tabnas/jsonic) already
applies to nested containers for the same reason, so a document parsed by
this plugin cannot nest deeper than this anyway.

`tabnas_expr::RULE_LIMIT` is the second half of the same fix. An
UNTERMINATED nesting, `(((((` with no closer, builds no expression at
all, so the node limit never sees it, and the engine's cost per step
grows with the rule stack: 1000 openers took 5 seconds and 2000 took 23,
which is the super-linear behaviour hostile input must not be able to
buy. A parse whose rule stack passes 1024 frames is refused with `cancel`
as well. The deepest document the base grammar accepts, 127 nested
containers, measures 378 frames.

Pinned by *"an expression past the node limit is refused"*
(`rs/tests/expr_test.rs`), which fails if either limit is removed as well
as if it regresses.

It is deliberately NOT in the shared fixtures: a row asserting `cancel`
would be red in TypeScript and Go, and a row asserting the expression
would be red in Rust. The rule in [`test/AGENTS.md`](test/AGENTS.md) keeps
intentional divergences out of `test/spec`.
