# tabnas-expr (Rust)

The Pratt expression-operator plugin for the
[`tabnas`](https://github.com/tabnas/parser) parsing engine, crate
`tabnas_expr`.

It adds infix, prefix, suffix, ternary and paren operators with
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
evaluator can read the name, the binding powers and the token the
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
values rather than trees. It is a typed callback, so it travels in the
options struct rather than in the JSON bag:

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
This plugin declares no error codes of its own: a malformed expression
raises one of the base codes the engine and the jsonic grammar define.

## Install

Neither the engine nor the jsonic base is published to a registry, so
both are consumed as **sibling checkouts**, the standard tabnas
development model. Clone `https://github.com/tabnas/parser`,
`https://github.com/tabnas/json` and `https://github.com/tabnas/jsonic`
next to this repository and point at them:

```toml
[dependencies]
tabnas-expr = { path = "../expr/rs" }
tabnas-jsonic = { path = "../jsonic/rs" }
tabnas = { path = "../parser/rs" }
serde_json = "1"
```

All four entries are needed. A crate's dependencies are not passed on to
its dependents, so `tabnas-expr` alone does not put `tabnas`,
`tabnas-jsonic` or `serde_json` in your extern prelude, and the examples
above that name `tabnas::Value`, `tabnas_jsonic::make` or
`serde_json::json!` would not resolve. Only `ExprError` is re-exported.
A program that declares its operators through `ExprOptions` can leave
`serde_json` out. The test suite additionally needs
`https://github.com/tabnas/support` beside the repository, for the
shared fixture runner.

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
  rule's `paren_preval` flag, the live context and the token of the
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
  than failing. TypeScript raises a JavaScript `RangeError` of its own a
  few thousand terms later and Go keeps going, both measured in
  [`../DIVERGENCE.md`](../DIVERGENCE.md); the number is the one the
  jsonic base grammar already applies to nested containers. An
  UNTERMINATED nesting builds no expression, so a second limit bounds the
  rule stack at 1024 frames, which is more than twice the deepest
  document the base grammar accepts.
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

## Build and test

The engine, the jsonic base, the JSON core it needs and the fixture
runner are path dependencies on sibling checkouts, so there is nothing
to fetch for the build:

```bash
cargo test --all-targets && cargo test --doc
```

Or, from the repository root, `make test-rs`. For what CI would say,
including formatting, clippy and the lockfile check, run
`ci/rust/run.sh`.

The suite runs every shared `../test/spec/*.tsv` fixture, each with the
operator table its file assumes, the same way the TypeScript and Go
suites do. Beside them are the in-language tests: the Pratt core through
its exported entry point, comma-operator suppression, the evaluator over
a small configuration language, the ternary after-close, the instance
token binding, the serialized shape of a parsed expression, thread
safety, hostile input, instance reuse and the version sites.

## License

MIT.
