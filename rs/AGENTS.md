# Agents Guide: rs/

The Rust port of the canonical TypeScript in [`../ts`](../ts). Read
[`../AGENTS.md`](../AGENTS.md) first: it holds the cross-runtime rules,
the binding-power ladder and the release path. This file only covers what
is specific to this crate.

## Layout

| Path | |
|---|---|
| `src/lib.rs` | the whole port: the operator model, the options, the expression arena, the Pratt core, every rule the plugin adds, the evaluator, `realize` / `simplify`, and `plugin` / `plugin_with` / `apply` / `make` / `make_with` / `parse` |
| `tests/parity_test.rs` | every shared `../test/spec/*.tsv` fixture through `tabnas_support::Runner`, each with the operator table its file assumes |
| `tests/expr_test.rs` | in-language behaviour: the Pratt core, comma-operator suppression, the evaluator, the ternary after-close, the instance token binding, the parsed shape, threads, hostile input |
| `tests/perf_test.rs` | the machine-independent instance-reuse guards |
| `tests/version_test.rs` | `Cargo.toml` equals `VERSION` equals `ts/package.json` |
| `tests/common/mod.rs` | shared helpers: the spec directory, the per-row parser, failure conversion, number normalization |
| `README.md` | the crate front page, prose-gated; its `rust` fences are doctests of this crate |

Crate `tabnas-expr`, library `tabnas_expr`. The engine (`tabnas`), the
jsonic base (`tabnas-jsonic`) and the fixture runner (`tabnas-support`,
dev only) are **path dependencies on sibling checkouts**
(`../../parser/rs`, `../../jsonic/rs`, `../../support/rs`). None is
published, so there is no registry version to fall back on.

```bash
cargo build --all-targets
cargo test --all-targets && cargo test --doc
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt
```

`make test-rs` from the repository root is the fast loop; `ci/rust/run.sh`
is the full gate and adds `fmt --check`, the lockfile check and the MSRV
pin.

## The expression arena, and why there is one

The canonical algorithm rewrites an expression node IN PLACE while
several rules hold it, and that is what keeps the overall AST intact
while an expression is still being assembled. JavaScript gets it from
array identity; the Go port buys it with a `*ListRef` box.

An engine `Value` is copied on write and has no interior mutability, so a
rewrite through one holder would be invisible to the others. Nodes
therefore live in a per-thread arena (`ARENA`) and the value that travels
through the parse is a HANDLE: a `ListRef` carrying the node's identity
under `meta.expr`. Cloning a handle shares the identity, which is exactly
the property the algorithm needs, and every operation the canonical port
performs on an array (`updateExprNode`, `dupNode`, `push`, `expr[i] =`)
has a one-line counterpart here.

Consequences to keep in mind:

- **A handle is a `ListRef`, not a string.** The base grammar's value
  coalescing asks whether a node is a container, and the canonical
  expression node, a JavaScript array, is one. A string handle would be
  read as a scalar and `@val-bc` would keep a stale parent-seeded node
  where TypeScript resolves the token. That is the defect the Go port
  works around with a `block-pair` alternate; this port does not need one.
- **The arena is released at the end of the outermost parse** this crate
  drives, and at the START of the next parse for a caller driving the
  engine directly. So a result read straight off `Tabnas::parse` must go
  through `realize` before the next parse on that thread. `parse`,
  `parse_with` and `parse_simplified` do it for you. `parse_scope` does
  NOT: holding the handles live is what it is for, and its closure returns
  a type parameter, so neither the signature nor the body can realize what
  comes back or refuse a handle in it. A handle that escapes it names a
  node the arena has dropped, and `realize` and `simplify` PANIC on one.
  They used to report it as an empty array, which handed a caller a
  well-formed value that had silently lost the whole expression: the worst
  of the three outcomes, because nothing downstream could tell it from a
  parse of an empty document. Node identities never repeat, so a stale
  handle can never be read as a live node of a later parse.
- **`NODE_LIMIT` is a crash fix, not a style choice.** The engine walks a
  value with the call stack to display, convert or drop it, and a flat
  sum of a few thousand terms builds a tree as deep as it is long: the
  process ABORTS rather than failing, and no caller can catch that. An
  expression past 127 nodes is refused with `cancel`, which is a recorded
  divergence (`../DIVERGENCE.md`, with the measurements: TypeScript
  raises its own `RangeError` a few thousand terms later, Go keeps
  going). The size is maintained in O(1): every node records the
  expression it belongs to, and a term taken in from another expression
  brings its size with it.

## The root and the attachment point

The canonical port keeps the ATTACHMENT POINT on `rule.node`, the
sub-expression the last operator now heads, and reads the parse result
off the FIRST rule of a replacement chain, where `prior` wrote the whole
expression. This engine returns the node of the LAST rule of that chain,
so an expr rule that kept only its attachment point hands a
sub-expression back as the whole parse: `1+2*3` came back as
`["*",2,3]`.

So an expr rule publishes both. `rule.node` is the attachment point, as
in the canonical port, because the rules pushed from here are seeded from
it and a chained prefix nests into it; the rule's after-close puts the
ROOT back on the node. The root is exact rather than inferred: every
arena node records the identity of the expression it belongs to, and
since the Pratt core only ever grows a tree downward from its root, that
identity never moves.

`linked_node` reads a linked rule through its recorded attachment point
rather than its node, so the Pratt core is handed the same node the
canonical port would hand it even after the after-close. `0!-1!*2!` is
the case that needs it: the trailing suffix binds to `2` only when the
core sees the sub-expression the last operator attached to.

## Three places the engine's node cells need care

A pushed or replaced rule SHARES its parent's `Rc<RefCell<Value>>`, so
writing through `rule.node.borrow_mut()` overwrites the parent's node
too. `set_node` installs a fresh cell instead, which is what `r.node = v`
means in the canonical engine.

1. **The val alternates this plugin adds detach.** The base grammar's own
   val alternates reach a private cell by resetting the node outright
   (`@reset$`); the prefix and paren alternates added here need the
   seeded value, a partly built expression the operator alternates read
   back, so they call `detach_node` instead. Without it, `prior_expr`
   writing the new expression to the val reached the expr rule above it
   as well, and `1+(2)` lost its second term.
2. **`paren` writes its result to its parent and grandparent**, as the
   canonical port does. Both are the same cell here whenever the val
   detached, which is why (1) is a precondition for this and not an
   independent choice.
3. **`elem` and `list` hand their finished node to the enclosing paren.**
   A container is copied on write, so the paren cannot hold the same
   array the collecting rules are appending to. The canonical port shares
   one array and needs nothing; the Go port added the same two hooks, for
   the same reason.

## Two install guards

`expr()` returns early when the instance already carries an `expr` rule,
so a second `use_plugin` on one instance does not give it a second copy
of every alternate. A derived instance is unaffected: `Tabnas::derive`
builds a child with an EMPTY rule set and re-runs every plugin, so the
base grammar and then this one install normally.

It refuses an instance with no `val` rule outright. This plugin hangs its
operator alternates on the base grammar's `val`, and on a bare engine
there is nothing to hang them on: without the check the parse failed
later, with a message about the document rather than about the missing
grammar.

## What the evaluator receives

`EvalSite` stands in for the canonical `(rule, ctx)` pair. It carries a
rule SNAPSHOT, because the canonical expr rule hands the evaluator its
PARENT, which this engine exposes as a snapshot; the ternary rule hands
the evaluator itself, and does so here too. `site.flag("paren_preval")`
is the test a function-paren evaluator needs to tell a preval call from a
plain group.

`site.token()` is the OCCURRENCE. The `Op` handed over is the shared
description, one `Arc` per entry in the operator table, so it is the same
value for every `+` in a document and cannot say which one is being
reduced. The canonical `makeOp` copies the description and attaches the
token for exactly that reason; this port keeps the token in the node
beside the op, and `evaluation` moves it onto the site around the
`evaluate` call, restoring what was there so an outer reduction keeps
pointing at its own operator. The position is the engine's `Site` triple:
`token.site.pos`, `.ri` and `.ci`. A node built by calling `prattify`
directly has no token, and the site reports `None`.

`EVALUATED` is the port of the canonical `WeakSet`: an implicit list is
reduced a member at a time as its members close and again as a whole once
it is complete, so a value the evaluator has already produced is returned
untouched rather than reduced twice. It is keyed by identity (a node's
own, or a container's pointer and length) and cleared with the arena.

## The operator table is ordered

Two operators claiming the same source resolve to the same token, and the
LATER entry wins. The table is an `IndexMap` and `resolve_options` merges
a caller's entries over the defaults by removing and re-inserting, which
puts them after the defaults, exactly as a JavaScript deep merge over an
insertion-ordered object does. `evaluate-math.tsv` depends on it: the
default `plain` paren and the fixture's `func` paren both claim `(`…`)`,
and `func` has to win.

The engine's plugin option merge does the same for the JSON bag, so
`plugin()` and `plugin_with()` agree.

## The docs are gated

`README.md` is in the published set: no em dashes in prose, no first
person singular, no banned phrases, and its `rust` fences are run as
doctests. rustdoc runs each fence as written, so a fence must be a
complete program: wrap it in
`fn main() -> Result<(), Box<dyn std::error::Error>> { ... Ok(()) }`
rather than using `?` at the top level, and never use hidden `# ` lines,
which render as garbage on GitHub. The `toml` and `bash` fences are not
run.

Numbers render differently through the two value printers:
`Value::to_string` is the JavaScript-shaped one (`1`), and
`Value::to_json().to_string()` goes through `serde_json` and prints
`1.0`. The doctests use the former; the tests normalize with
`common::norm` instead.

## This file is internal

It may be blunt, and it is not in the gated set. Keep the reader-facing
account in `README.md`.
