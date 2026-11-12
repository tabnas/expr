# tabnas-expr (Rust)

The Pratt expression-operator plugin for the
[`tabnas`](https://github.com/tabnas/parser) parsing engine, crate
`tabnas_expr`.

It adds infix, prefix, suffix, ternary, and paren operators with
configurable precedence, and parses an expression into a LISP-style
S-expression: an array whose first element describes the operator and
whose remaining elements are the operand terms, so `1+2*3` becomes
`["+", 1, ["*", 2, 3]]` once the descriptions are reduced to their source
text. A caller supplied evaluator can reduce the tree to a value as the
parse runs. The plugin is not standalone: it layers on the relaxed-JSON
grammar of [`tabnas-jsonic`](https://github.com/tabnas/jsonic) and reuses
the engine's lexer and rule lifecycle.

This is the Rust port of the canonical TypeScript implementation in
[`../ts`](../ts); the TypeScript version is authoritative and this crate
tracks it. The Go port is in [`../go`](../go), and the shared fixtures in
[`../test/spec`](../test/spec) hold all three to the same results.

## Use

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let value = tabnas_expr::parse("a:1+2*3")?;
    assert_eq!(
        tabnas_expr::simplify(&value).to_string(),
        r#"{"a":["+",1,["*",2,3]]}"#
    );
    Ok(())
}
```

`parse` builds one parser on first use and reuses it. Building a parser
costs far more than a small parse, so for anything but a one-off call
build an instance once and keep it:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let parser = tabnas_expr::make();
    let value = tabnas_expr::parse_with(&parser, "(1+2)*3")?;
    assert_eq!(
        tabnas_expr::simplify(&value).to_string(),
        r#"["*",["(",["+",1,2]],3]"#
    );
    Ok(())
}
```

A parsed expression carries the whole operator description, so an
evaluator can read the name, the binding powers, and the token the
operator was matched from. `simplify` reduces that to the source text,
which is the form the shared fixtures compare:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let value = tabnas_expr::parse("1+2")?;
    let terms = value.to_json();
    let op = &terms[0];
    assert_eq!(op["name"], "addition-infix");
    assert_eq!(op["src"], "+");
    assert_eq!(op["left"], 2000000.0);
    assert_eq!(op["right"], 2100000.0);
    assert_eq!(op["token"]["cI"], 2.0);
    Ok(())
}
```

Operators are declared in the options, as a typed table. The binding
powers are compared only by ORDER, never by magnitude: a higher number
binds tighter, and `left` below `right` is left-associative, so `a-b-c`
is `(a-b)-c`.

```rust
use tabnas_expr::{ExprOptions, OpDef};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let parser = tabnas_expr::make_with(
        ExprOptions::new()
            .with_op("exponent", OpDef::infix("^", 5100000, 5000000))
            .with_op("factorial", OpDef::suffix("!", 6000000)),
    );
    let value = tabnas_expr::parse_with(&parser, "2^3^4")?;
    assert_eq!(
        tabnas_expr::simplify(&value).to_string(),
        r#"["^",2,["^",3,4]]"#
    );
    Ok(())
}
```

The same table travels as a JSON bag through the engine's plugin
mechanism, which is how a host instance installs the grammar and how a
derived instance rebuilds it. A `null` entry deletes a default operator:

```rust
use tabnas::Value;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut parser = tabnas_jsonic::make();
    let options = Value::from_json(&serde_json::json!({
        "op": {
            "plain": null,
            "call": {
                "paren": true, "osrc": "(", "csrc": ")",
                "preval": { "active": true },
            },
        },
    }));
    parser.use_plugin(tabnas_expr::plugin(), Some(options))?;
    let value = tabnas_expr::realize(&parser.parse("f(1,2)")?);
    assert_eq!(
        tabnas_expr::simplify(&value).to_string(),
        r#"["(","f",[1,2]]"#
    );
    Ok(())
}
```

An evaluator reduces each expression as it closes, so the parse returns
values rather than trees. The plugin calls it once per operation, in the
order of the tree the S-expression shows: `-1!` is `[-, [!, 1]]`, so the
call for `!` comes before the call for `-`. It is a typed callback, so it
travels in the options struct rather than in the JSON bag:

```rust
use tabnas::Value;
use tabnas_expr::ExprOptions;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let parser = tabnas_expr::make_with(ExprOptions::new().with_evaluate(
        |_site, op, terms| {
            let (left, right) = match terms {
                [Value::Number(left), Value::Number(right)] => (*left, *right),
                [Value::Number(only)] => (*only, 0.0),
                _ => (0.0, 0.0),
            };
            match op.name.as_str() {
                "addition-infix" => Value::Number(left + right),
                "multiplication-infix" => Value::Number(left * right),
                "negative-prefix" => Value::Number(-left),
                "plain-paren" => Value::Number(left),
                _ => Value::Null,
            }
        },
    ));
    assert_eq!(
        tabnas_expr::parse_with(&parser, "x:(1+2)*-3")?.to_string(),
        r#"{"x":-9}"#
    );
    Ok(())
}
```

A tree can also be reduced after the parse, which is the parse-once,
evaluate-many workflow. The expression nodes are live only while the
arena of that parse is open, so the reduction runs inside `parse_scope`
and what the closure returns is the reduced value, never the tree:

```rust
use tabnas::Value;
use tabnas_expr::{evaluation, EvalSite, Evaluate, Op};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let parser = tabnas_expr::make();
    let add: Evaluate = std::sync::Arc::new(
        |_site: &mut EvalSite<'_>, _op: &Op, terms: &[Value]| match terms {
            [Value::Number(left), Value::Number(right)] => Value::Number(left + right),
            _ => Value::Null,
        },
    );
    let total = tabnas_expr::parse_scope(&parser, "1+2", |value| {
        evaluation(&mut EvalSite::detached(), &value, &add)
    })?;
    assert_eq!(total, Value::Number(3.0));
    Ok(())
}
```

The plugin layers on the base grammar rather than standing alone, so
installing it on an instance with no `val` rule is refused with a message
saying so, and installing it twice on one instance leaves one copy of the
grammar.

Parse errors are the engine's `TabnasError`, re-exported as `ExprError`,
with `code`, `row`, `col` and a report that shows the offending source.
This plugin declares no parse error codes of its own: a malformed
expression raises one of the base codes the engine and the jsonic grammar
define. A fold has codes of its own, described next.

## Calls, casts, and separated content

Seven options shape a table for a language like C, and each is off
unless the table sets it. `OpDef::with_left` gives a preval paren a
binding power, `OpDef::with_content` makes a paren's content required or
separated, and `OpDef::with_separator` makes an infix operator on `,`
give way to separators. `OpDef::cast` declares a cast, and
`ExprOptions::with_juxtapose` refuses values side by side.
`OpDef::with_left` and `OpDef::with_right` on a ternary place it as an
infix operator is, and `ExprOptions::with_values` names a host grammar's
own value tokens, which the parser refuses side by side too. The TypeScript
reference, `ts/doc/reference.md`, describes each one.

```rust
use tabnas::Value;
use tabnas_expr::{ContentDef, ExprOptions, OpDef, PrevalDef};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let preval = |active| PrevalDef {
        active,
        required: false,
        allow: None,
    };
    let parser = tabnas_expr::make_with(
        ExprOptions::new()
            .with_op("comma", OpDef::infix(",", 1000000, 1100000).with_separator())
            .with_op("dot", OpDef::infix(".", 5000000, 5100000))
            .with_op("paren", OpDef::paren("(", ")").with_preval(preval(false)))
            .with_op(
                "call",
                OpDef::paren("(", ")")
                    .with_preval(preval(true))
                    .with_left(5000000)
                    .with_content(ContentDef {
                        required: false,
                        separated: true,
                    }),
            )
            .with_op(
                "cast",
                OpDef::cast("(", ")", 4000000, |term| {
                    matches!(term, Value::String(name) if "int" == name)
                }),
            )
            .with_juxtapose(false),
    );
    let read = |src| tabnas_expr::parse_simplified(&parser, src).map(|value| value.to_string());
    assert_eq!(read("a.b(1, 2)")?, r#"["(",[".","a","b"],1,2]"#);
    assert_eq!(read("(int)a")?, r#"["()","int","a"]"#);
    assert!(read("f(a b)").is_err());
    Ok(())
}
```

## Folding a finished tree

A key operator is an operation spelled as a map, a `$ref` say, rather
than as a token. `OpDef::key` declares one by the member that marks it,
and a map that owns that member is an occurrence, recognised by its shape
when the finished document is folded. The parse itself is unchanged, and
`fold` hands a resolver the site, the operator and two terms, the
member's value and a new map of the other members, both already folded:

```rust
use tabnas::Value;
use tabnas_expr::{ExprOptions, FoldOptions, OpDef, Seg};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let parser = tabnas_expr::make_with(ExprOptions::new().with_op("ref", OpDef::key("$ref")));
    let raw = tabnas_expr::parse_with(&parser, r##"{ a: { $ref: "#/b", note: 1 }, b: { c: 2 } }"##)?;
    assert_eq!(raw.to_string(), r##"{"a":{"$ref":"#/b","note":1},"b":{"c":2}}"##);

    let options = FoldOptions::new(|site, _op, terms| {
        let Value::String(reference) = &terms[0] else {
            return Ok(site.node().clone());
        };
        let path: Vec<Seg> = reference
            .trim_start_matches("#/")
            .split('/')
            .map(Seg::from)
            .collect();
        let hit = site.at(&path, None)?;
        Ok(match (hit.value, &terms[1]) {
            (Some(Value::Object(target)), Value::Object(siblings)) => {
                let mut out = (*target).clone();
                out.extend(siblings.iter().map(|(name, value)| (name.clone(), value.clone())));
                Value::object(out)
            }
            _ => site.node().clone(),
        })
    })
    .with_op("ref", OpDef::key("$ref"));
    let out = tabnas_expr::fold(&raw, &options)?;
    assert_eq!(out.to_string(), r#"{"a":{"c":2,"note":1},"b":{"c":2}}"#);
    Ok(())
}
```

`site.at` folds the location it names first, so a reference to a
reference resolves, and every site that names one location shares one
folded value. A hit carries a value only when the location was found and
is not a cycle. The input is never changed: the result is a new tree.

The rest of the contract is the canonical one, and the TypeScript
reference documents it in full: `inherit` passes a bag of context down
from the root, `recognise` lets a host decide that a map owning `$ref` is
data, `load` reaches another document by a key the host chooses, a
dynamic key operator is reported and never evaluated, and the report
lists cycles, unresolved targets, and dynamic sites. A site whose target
is still being folded closes a cycle, which `at` reports with `cycle`
set, or which fails the fold under `FoldCycle::Error`.

One resolver can reduce the token operators too, and `op.key` sets the
two kinds apart. A token operator is recognised only on a live expression
node, so that fold runs inside `parse_scope`:

```rust
use tabnas::Value;
use tabnas_expr::{ExprOptions, FoldOptions, OpDef, Seg};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let parser = tabnas_expr::make_with(ExprOptions::new().with_op("ref", OpDef::key("$ref")));
    let out = tabnas_expr::parse_scope(&parser, r##"{ a: 1+2, b: { $ref: "#/a" } }"##, |raw| {
        let options = FoldOptions::new(|site, op, terms| {
            if op.key {
                let hit = site.at(&[Seg::from("a")], None)?;
                return Ok(hit.value.unwrap_or(Value::Null));
            }
            Ok(match terms {
                [Value::Number(left), Value::Number(right)] => Value::Number(left + right),
                _ => Value::Null,
            })
        })
        .with_op("ref", OpDef::key("$ref"));
        tabnas_expr::fold(&raw, &options)
    })??;
    assert_eq!(out.to_string(), r#"{"a":3,"b":3}"#);
    Ok(())
}
```

Outside the scope a parse is realized, and a realized operator is a plain
array led by its description. The fold reads that array as data, never
as an operator, because a document can spell the same array, and reading
it would let the text of a document choose what the resolver runs.

A fold that fails returns a `FoldError`. For the layer's own failures its
`code` is `fold_cycle`, `fold_depth`, `fold_inherit` or `fold_key`, and
`doc`, `at`, `target_doc` and `target` give the location, as pointers
into the document; member names in its message are escaped and cut
short, since they are document content. A resolver returns the engine's
`TabnasError`, and a resolver that fails stops the fold with its error
kept, unchanged, as the `cause`. `TabnasError::from` turns a `FoldError`
back into an engine error, which is what lets a resolver use `?` on
`site.at`, and the fold still knows its own failure when it comes back
out of a resolver that way.

`max_depth`, 1000 by default, bounds nesting and reference hops, the
two together. Nesting costs no stack, since the walk over containers keeps
its own. A reference hop does: a resolver that asks `site.at` enters the
fold again, and the hops of one chain pile up. `max_stack` bounds that,
at a megabyte by default, half the stack of a spawned thread, so a long
chain of references is refused with `fold_depth` rather than overflowing
the stack, which would end the process. A resolver like the first one
here makes about 240 hops in a debug build and about 320 in a release
build within the default. Raise `max_stack` only on a thread that has the
stack to give.

`site.token()` is the token a token operator occurrence was matched from.
At a map it is whatever `FoldOptions::positions` returns for the map:
this crate records no opening tokens of its own, because realizing a
parse builds every container afresh, so a container in the result was
never seen by the parse.

## Install

The engine and the jsonic base sit outside this crate. All three are
published on crates.io, the engine as `tabnas-parser`, whose library is
named `tabnas` in code, so add them, with `serde_json`:

```bash
cargo add tabnas-expr tabnas-jsonic tabnas-parser serde_json
```

All four entries are needed. A crate's dependencies are not passed on to
its dependents, so `tabnas-expr` alone does not put `tabnas`,
`tabnas-jsonic` or `serde_json` in your extern prelude, and the examples
above that name `tabnas::Value`, `tabnas_jsonic::make` or
`serde_json::json!` would not resolve. Only `ExprError` is re-exported.
A program that declares its operators through `ExprOptions` can leave
`serde_json` out.

In this repository, `Cargo.toml` takes the engine and `tabnas-jsonic` by
path from sibling checkouts instead, and `tabnas-jsonic` takes
`tabnas-json` the same way, so clone `https://github.com/tabnas/parser`,
`https://github.com/tabnas/json` and `https://github.com/tabnas/jsonic`
next to it. The test suite additionally needs
`https://github.com/tabnas/support` beside the repository, for the shared
fixture runner, and `https://github.com/tabnas/debug`, for the grammar
composition test. The release workflow swaps the runtime paths for
crates.io versions, and drops the test-only ones, when it publishes this
crate.

## Differences from the canonical TypeScript

Every parse result is the TypeScript one, and the shared fixtures hold
all three runtimes to it. What differs is the shape of the API and a few
points where the host language has no way to say what JavaScript says:

- **Options are a struct, or a JSON bag.** `make_with` takes an
  `ExprOptions`; `plugin()` reads the bag `use_plugin` merges over the
  declared defaults. An operator given in the struct replaces the default
  of the same name outright, where a bag entry extends it, because a
  struct has no partial form.
- **The evaluator is a typed callback.** A closure cannot travel in the
  option bag, so it sits on `ExprOptions` outside serialization and
  reaches the grammar through `plugin_with` and `make_with`. It receives
  an `EvalSite` in place of the rule and context pair, through which the
  rule's `paren_preval` flag, the live context, and the token of the
  operator OCCURRENCE being reduced are reachable. The `Op` itself is the
  shared description, one per entry in the operator table, so the token is
  what tells one `+` in a document from another; TypeScript attaches it to
  its copy of the description for the same reason.
- **Expressions are rewritten in place through a per-parse arena.** The
  algorithm rewrites a partly built expression while several rules hold
  it, which JavaScript gets from array identity. An engine value is
  copied on write, so a parse builds its expressions in a per-thread
  arena and the value that travels through the parse is a handle into it.
  `parse`, `parse_with` and `parse_simplified` resolve the handles at the
  parse boundary; a caller driving a `tabnas` instance directly calls
  `realize` on the result, before the next parse on that thread.
  `parse_scope` does not resolve them, because holding them live is its
  whole purpose, so a handle must not escape what its closure returns:
  `realize` and `simplify` PANIC on a handle whose nodes have been
  released rather than reporting the lost expression as an empty array.
- **An expression of more than 127 nodes is refused** with the engine's
  `cancel` code, so a flat sum of at most 128 terms. The engine walks a
  value with the call stack to display, convert or drop it, and a flat
  sum builds a tree as deep as it is long, which ends the process rather
  than failing. TypeScript and Go read it, both measured in
  [`../DIVERGENCE.md`](../DIVERGENCE.md); the number is the one the
  jsonic base grammar already applies to nested containers. An
  UNTERMINATED nesting builds no expression, so a second limit bounds the
  rule stack at 1024 frames, which is more than twice the deepest
  document the base grammar accepts. Both are a parse guard, checked at
  every step. A host whose evaluator reduces every expression into
  values of its own, with a depth it bounds itself, raises the first
  with `ExprOptions::with_node_limit`: the parse, the placing of an
  operator and the evaluation are loops, so they reach no deeper on the
  call stack however large an expression grows, and only `realize` and
  `simplify` build a value as deep as the tree.
- **`parse` is a convenience the other runtimes lack.** It keeps one
  default instance behind a `OnceLock`, the reuse the TypeScript suite
  tells a caller to arrange by hand and the Go port keeps behind a
  `sync.Once`.
- **A comment marker beats an operator token.** Where an operator source
  is a prefix of a comment opener, `/` and `//` for instance, the fixed
  matcher stands aside so the comment matcher takes the run. The engine
  allows one check on the fixed family, so a check a host had already
  configured is displaced by this one rather than chained to it. A host
  that needs both installs its own check after this plugin and skips the
  comment openers itself.

  This port reads MORE comments than the canonical, not the same ones.
  TypeScript reorders the two matchers instead, which reads a marker
  separated from the operator by a space (`1/2 // note`) but not one
  adjacent to a value (`1//note`); the Go port reads neither. Bare jsonic
  reads the adjacent one in every runtime, so the canonical is the
  defective side there and the repair belongs to its engine, not to this
  port. [`../DIVERGENCE.md`](../DIVERGENCE.md) measures all three.
- **Lone surrogates fold to U+FFFD**, and the regular expression dialect
  is the `regex` crate's. Both come from the engine, and both are
  recorded there.
- **The fold is a function, and only a function.** TypeScript can also
  fold during the parse, through the plugin's `fold` option, on the
  engine's rule event. The engine's rule events here observe a parse and
  cannot replace its result, so this port folds the value a parse
  returns, and it has no `recordPositions` either: a site's position is
  the host's to supply.
- **A token operator folds only inside `parse_scope`,** on the live
  expression nodes, because a realized operator is plain data here.
  TypeScript recognises its operator arrays wherever they are.
- **A long chain of references is refused sooner.** TypeScript follows
  reference hops up to `maxDepth` for as long as its stack lasts. This
  port stops at `max_stack`, which by default is about 240 hops in a
  debug build and 320 in a release build, with `fold_depth` in both
  cases. Nesting is bounded by `max_depth` alone in both.
- **A fold error is a value.** `FoldError` carries the code and the
  location, and an error a resolver returns rides in it unchanged as its
  `cause`, where TypeScript lets an exception a resolver throws through
  as it is. The code `fold_option` is TypeScript's refusal of a `positions`
  option this port cannot express, so a Rust fold never raises it.

## Build and test

The engine, the jsonic base, the JSON core it needs, and the fixture
runner are path dependencies on sibling checkouts, so there is nothing
to fetch for the build:

```bash
cargo test --all-targets && cargo test --doc
```

Or, from the repository root, `make test-rs`. For what CI would say,
including formatting, Clippy and the lockfile check, run
`ci/rust/run.sh`.

The suite runs every shared `../test/spec/*.tsv` fixture, each with the
operator table its file assumes, the same way the TypeScript and Go
suites do, and a test fails when a fixture on disk has no runner here.
The tree layer's fixtures in `../test/tree` are held the same way, each
with the operator table and the resolver its TypeScript runner uses.
Beside them are the in-language tests: the Pratt core through its
exported entry point, comma-operator suppression, the evaluator over a
small configuration language, the ternary after-close, the instance token
binding, the serialized shape of a parsed expression, the grammar
composed with the `tabnas-debug` plugin, thread safety, hostile input,
instance reuse, the fold and its key operators, and the version sites.

## License

MIT.
