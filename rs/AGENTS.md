# Agents Guide: rs/

The Rust port of the canonical TypeScript in [`../ts`](../ts). Read
[`../AGENTS.md`](../AGENTS.md) first: it holds the cross-runtime rules,
the binding-power ladder and the release path. This file only covers what
is specific to this crate.

## Layout

| Path | |
|---|---|
| `src/lib.rs` | the port but for the tree layer: the operator model, the options, the expression arena, the Pratt core, every rule the plugin adds, the evaluator, `realize` / `simplify`, and `plugin` / `plugin_with` / `apply` / `make` / `make_with` / `parse`; and the translation parts, `translate()` (its `embed` and `render`) with `manifest_text` and `render_text`, `include_str!` of the copies in `translate/` |
| `src/tree.rs` | the tree layer: key operators (`make_key_map`, `key_op_of`) and `fold`, with `FoldOptions`, `Site`, `FoldError` and the report; re-exported from the crate root |
| `translate/` | the crate's copies of `../tabnas.plugin.json` (as `manifest.json`), `../alchemy/render.alc` and `../alchemy/embed.alc`, which a packaged crate needs, written by `npm run embed` from `../ts`; `tests/translate_test.rs` holds them to the files |
| `tests/parity_test.rs` | every shared `../test/spec/*.tsv` fixture through `tabnas_support::Runner`, each with the operator table its file assumes, plus `every_shared_fixture_is_run`, which reads the fixture directory and fails when a file has no runner here |
| `tests/expr_test.rs` | in-language behaviour: the Pratt core, comma-operator suppression, the evaluator, the ternary after-close, the instance token binding, the parsed shape, threads, hostile input |
| `tests/debug_model_test.rs` | the grammar composed with `tabnas-debug`, asserting the rule set, the entry rule and the push edges: the Rust half of `ts/test/debug-model.test.ts` |
| `tests/perf_test.rs` | the machine-independent instance-reuse guards |
| `tests/version_test.rs` | `Cargo.toml` equals `VERSION` equals `ts/package.json` |
| `tests/translate_test.rs` | the translation parts: the render and the embedding the embedded manifest names are the ones the crate embeds, the manifest's shapes, root, schema and loss lines, every definition of both named `expr-...`, and the `src` the reader's realized value and `simplify` give each default operator, which the render reads |
| `tests/translate_render_test.rs` | the parts run, where `TABNAS_ALCHEMY` names the `alchemy` command (each test passes having run nothing, and says so, where it does not): the embedding and the render over trees, each document read back with `parse_simplified`, a list led by an object with a `src` the reader reads and no default operator has refused with its path, and every shared fixture's value and realized value written and read back; the Rust half of `ts/test/translate-render.test.ts` |
| `tests/tree_spec_test.rs` | every `../test/tree/*.tsv` fixture, each with the operator table and resolver `ts/test/tree-spec.test.ts` gives it, held to its cells byte for byte, plus `every_tree_fixture_is_run`; `tokens.tsv` folds inside `parse_scope` |
| `tests/tree_test.rs` | the tree layer in the language: the Rust half of `ts/test/tree.test.ts`, and what is this port's own: re-entry from a resolver, a realized operator array as data, `MapRef`, the error type, the stack bound on a default thread |
| `tests/common/mod.rs` | shared helpers: the spec directory, the per-row parser, failure conversion, number normalization, the canonical JSON writer (`canonical`, `js_number`) both fixture runners hold cells with, and the fold helpers the tree tests share (`resolving`, `pointer`, `no_defaults`) |
| `README.md` | the crate front page, prose-gated; its `rust` fences are doctests of this crate |

Crate `tabnas-expr`, library `tabnas_expr`. The engine (`tabnas`), the
jsonic base (`tabnas-jsonic`), the fixture runner (`tabnas-support`, dev
only) and the introspection plugin (`tabnas-debug`, dev only) are **path
dependencies on sibling checkouts** (`../../parser/rs`, `../../jsonic/rs`,
`../../support/rs`, `../../debug/rs`). They are on crates.io, but the
committed manifest names them by path alone, so there is no registry
version to fall back on.

The TypeScript `debug-model.test.ts` SKIPS when `@tabnas/debug` cannot be
resolved. The Rust half does not: a missing checkout fails the build
rather than reporting green having run nothing, which is how every other
sibling is treated here.

```bash
cargo build --all-targets
cargo test --all-targets && cargo test --doc
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt
```

`make test-rs` from the repository root is the fast loop; `ci/rust/run.sh`
is the full gate and adds `fmt --check`, the lockfile check, the MSRV pin
and `cargo doc --no-deps` under `RUSTDOCFLAGS=-D warnings`. rustdoc is the
only one of the three compilers over this crate that resolves an intra-doc
link, and a doctest fence is executed rather than linked, so a link naming
a type the crate does not have passed every other check and rendered as
plain text.

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
  divergence (`../DIVERGENCE.md`, with the measurements: TypeScript and
  Go read it). The size is maintained in O(1): every node records the
  expression it belongs to, and a term taken in from another expression
  brings its size with it, and the largest size of the parse is checked
  by a parse guard at every step (`LARGEST`, `bound_expression_size`).
  A host raises the limit with `ExprOptions::node_limit` when its
  evaluator reduces every expression into values of its own and bounds
  their depth itself, as tabnas/c does: nothing but `realize` and
  `simplify` walks a tree with the call stack, since evaluation
  (`EvalFrame`), the placing (`place_infix`, `prattify_postfix`) and the
  drilling of a suffix are loops.

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

## The tree layer

`src/tree.rs` ports the canonical `fold` and key operators. The contract
is the one `../AGENTS.md` states under "The tree layer"; these are the
choices this port made where the contract left room, and the hazards in
the code.

- **The function form only.** `fold(&root, &FoldOptions)` returns
  `Result<Value, FoldError>`. There is no deferred mode: the engine's
  `subscribe_rule_done` observes a parse and cannot replace its result.
- **Token operators only on arena-live handles.** The walk reduces an
  expression node through `evaluate` only when `node_id` finds it in the
  arena, so a fold that should see `+` runs inside `parse_scope`. A
  realized array, the description object followed by the terms, is plain
  data and is never read as an operator: a document can spell exactly
  that array, so recognising it would let input forge an operator. With
  `tokens` off a node is rebuilt as a NEW arena node around the same
  `Arc<Op>` and occurrence token, so the caller realizes the result inside
  the scope. Path index 0 of a live node is its realized description, a
  leaf the rest of a path reads raw, as the canonical treats its operator
  object; terms are indices 1 and up. A released handle panics, as
  `realize` does.
- **Key operators by map shape** on `Value::Object` and `Value::MapRef`.
  Siblings, a plain fold and every rebuilt map keep the kind of the map
  they replace; a `MapRef` keeps its `implicit` flag and `meta`. Lists
  likewise (`Array` or `ListRef`). The install (`expr()`) builds the key
  table once to refuse a bad one with a `PluginError` naming `fold_key`,
  as the canonical refuses it at `use`.
- **Memory.** A trie of `PNode`s in one `Vec`, addressed by index, the
  children of a map by member name and of a list by index. Never a
  pointer string per node (`ptr_of` makes one only for a report row or an
  error, and keeps it) and never a value's identity: `Arc` identity says
  nothing about a location, since values are copied on write.
- **Re-entry.** Every resolver can call back into the fold through its
  `Site` (`at`, `locate`, `plain`, `k`) while the fold is inside an outer
  callback. All state is one `RefCell<State>` and NO borrow of it may be
  held across a callback. The trap is a temporary in an `if let` or
  `match` scrutinee, which lives to the end of the whole statement: bind
  the cloned value to a local first, then call. A double borrow is a
  panic, not a compile error.
- **Errors.** `FoldError` is a boxed struct, eight bytes, with the code,
  the message and the location, and `cause` for a callback's own
  `TabnasError`. Callbacks return `TabnasError`, as decided for the API,
  so a fold failure raised inside `site.at` reaches the fold again as a
  `TabnasError` through the resolver's `?`. `raise` records each of the
  layer's own failures in `State::failure`, and `callback_error`
  recognises it by code and message coming back out of a callback and
  restores the located `FoldError`; anything else is the callback's, kept
  unchanged as `cause`, and `TabnasError::from` hands it back as it was.
  `fold_option` is declared and never raised: it is TypeScript's refusal
  of `positions: true`, which these options cannot express. An `inherit`
  result is a bag when it is a container, as the canonical's
  `typeof === 'object'` admits.
- **Hosts pay `clippy::result_large_err`.** `TabnasError` is 680 bytes,
  and every resolver closure returning it trips the lint, so the test
  files allow it at their roots as the crate does.

### The stack

A stack overflow aborts the process, so the fold may not recurse with
the document. The walk over containers is iterative (`walk`, `descend`,
`finish`, with the containers being folded as `Frame`s on a heap
`Vec`), so nesting costs heap. What still recurses is the re-entry: one
reference hop is `walk` → `finish` → `reduce_site` → `call_evaluate` →
the resolver → `Site::at` → `at` → `reach` → `walk`. Measured per hop,
with a resolver that records the stack address and calls the fixtures'
`resolving`: about 4.5 KB in a debug build and 3.3 KB in a release build,
of which the resolver's frames are about 1.5 KB in release, since a `?`
into a `TabnasError` result needs room for one.

The frames on that path are kept small on purpose, and a change there is
to be re-measured. `at` takes its steps in `find`, whose frame is gone
before the walk starts; `finish` reads the frame in place, as `pop` would
move it; `call_evaluate` inspects the 680-byte callback result by
reference, since moving it into a conversion copied it twice more; the
operator frame does not carry the 192-byte `Token`. Together that took a
hop from 10 KB to 4.5 KB in debug and from 4.3 KB to 3.3 KB in release.

`guard` refuses with `fold_depth` once the stack has grown
`FoldOptions::max_stack` bytes (a megabyte, `FOLD_MAX_STACK`) from where
the OUTERMOST fold on the thread started (`STACK_BASE`, so a fold inside a
resolver shares its caller's budget). It runs at every entry the re-entry
passes through: `walk`, `at`, `locate`, `plain_of`, `k_of` and a `load`.
On a 2 MiB thread with the defaults, 238 hops fold in a debug build, and
313 (rustc 1.85, the MSRV) or 324 (1.97) in a release build; 3,000 nested
maps fold with `max_depth` raised.
A `Value` itself drops and displays recursively, so a document nested
deeper than a few thousand levels aborts on a 2 MiB thread when it is
dropped, whatever the fold does (in a debug build 4,000 levels dropped and
8,000 aborted), and far less deep when it is displayed: a test that
printed the 998-deep result of a fold aborted under rustc 1.85, so the
test reads that result down iteratively instead.
`tree_test.rs` holds the canonical depth test to `max_depth` on a 64 MiB
thread with `max_stack` raised, and the default bound on a 2 MiB thread.

### Positions

`site.token()` is the occurrence token at a token operator site and the
host's `positions` lookup at a map; this crate records no opening tokens.
Measured before deciding, with a rule-event subscriber logging each
container's `Arc` pointer: inside `parse_scope` the `Arc` a container had
at its open pass was the one in the returned tree for every container of
the value tree probed (maps, lists, an implicit map), and a map inside an
expression, which the arena holds, kept its `Arc` from its open pass to
its close. But `realize`, which `parse`, `parse_with` and
`parse_simplified` apply, rebuilds every container, so no pointer
recorded during a parse survives into what they return; and the engine's
`unwrap_undefined` rebuilds every container whenever the tree holds an
`Undefined`. A table keyed by pointer must also pin each container
against address reuse, and a clone pinned at the open pass would make the
next `Arc::make_mut` copy the map and change the identity being recorded
(reasoned from `make_mut`, not measured). Recording at the close pass
instead meets `pair` closing before `map` (measured), so "first close
wins" records the key's token rather than the brace. And a table hung on
an instance grows across parses and is shared between threads. A port of
`recordPositions` that works on `parse_scope` values only is possible; it
was not worth those hazards without a host asking for it.

### Also in this change

`evaluation_with` and `EvaluationOptions { objects }` port the
canonical's `evaluation(..., { objects })`, which reduces a map member
by member into a new map, and the canonical's rule that only a value a
reduction PRODUCED is marked evaluated: a map passed through is not, so a
later reduction with `objects` on still reaches the operators inside it.

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
