// The tree layer's fixtures, `../test/tree/*.tsv`: the Rust half of
// `ts/test/tree-spec.test.ts`.
//
// They are kept beside, not inside, `test/spec/`, because that directory
// is the three-runtime parity contract and the Go port does not carry
// `fold` yet; the files move into `test/spec/` once both ports run them.
// Each file folds the parse of its input with its own operator table and
// resolver, the ones the TypeScript runner uses, and each cell is the
// folded value's canonical JSON, byte for byte, as the shared fixtures
// are held.
//
// The TypeScript file has one more runner, "at-pointer, deferred", which
// folds through the plugin's `fold` option. Rust has the function form
// only, so that runner has no counterpart here; the file it runs is run
// in the function form below.

// A resolver returns the engine's `TabnasError`, which is large by design
// (it carries the whole report); the engine and this crate allow the lint
// at their roots for the same reason.
#![allow(clippy::result_large_err)]

mod common;

use std::path::PathBuf;

use tabnas::{TabnasError, Value};
use tabnas_expr::{fold, ExprOptions, FoldCycle, FoldError, FoldOptions, OpDef};
use tabnas_support::{load_spec, Failure, Runner, SpecOptions};

use common::{canonical, key_parser, pointer, repo_root, resolving, table, text_of, to_failure};

fn tree_dir() -> PathBuf {
    repo_root().join("test").join("tree")
}

fn fold_failure(error: FoldError) -> Failure {
    Failure::new(error.code()).with_message(error.to_string())
}

fn parse_failure(error: TabnasError) -> Failure {
    to_failure(error)
}

/// Run one tree fixture: every cell that is not an error is held to the
/// canonical JSON of the folded value, byte for byte, and the whole file,
/// error rows included, goes through the shared runner.
fn run_tree(name: &str, run: impl Fn(&str) -> Result<Value, Failure> + 'static) {
    let path = tree_dir().join(name);
    assert!(path.is_file(), "missing tree fixture {}", path.display());
    let spec = load_spec(&path, &SpecOptions::default())
        .unwrap_or_else(|error| panic!("{}: {}", path.display(), error.0));
    let mut failures = Vec::new();
    for row in &spec.rows {
        if row.col(1).starts_with("ERROR") {
            continue;
        }
        match run(&row.unesc(0)) {
            Ok(value) => {
                let got = canonical(&value);
                if got != row.col(1) {
                    failures.push(format!(
                        "{}\n  got      {got}\n  expected {}",
                        row.location(),
                        row.col(1)
                    ));
                }
            }
            Err(failure) => failures.push(format!("{}\n  failed {failure}", row.location())),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} rows differ from the canonical JSON:\n{}",
        failures.len(),
        spec.rows.len(),
        failures.join("\n")
    );
    Runner::new(move |input| run(input).map(|value| tabnas_support::Value::from(value.to_json())))
        .file(&path);
}

/// Parse with the parser given, realized, and fold the result.
fn fold_parse(
    parser: &tabnas::Tabnas,
    options: &FoldOptions<'_>,
    input: &str,
) -> Result<Value, Failure> {
    let raw = tabnas_expr::parse_with(parser, input).map_err(parse_failure)?;
    fold(&raw, options).map_err(fold_failure)
}

// A recording resolver: a site reads as `[src, value, siblings]`.
#[test]
fn tree_key_basic() {
    let ops = [
        ("ref", OpDef::key("$ref")),
        ("zed", OpDef::key("$zed")),
        ("dyn", OpDef::dynamic_key("$dyn")),
    ];
    let parser = key_parser(&ops);
    let options = FoldOptions::new(|_site, op, terms| {
        let mut row = vec![Value::String(op.src.clone())];
        row.extend(terms.iter().cloned());
        Ok(Value::array(row))
    })
    .with_ops(table(&ops));
    run_tree("key-basic.tsv", move |input| {
        fold_parse(&parser, &options, input)
    });
}

#[test]
fn tree_at_pointer() {
    let ops = [
        ("ref", OpDef::key("$ref")),
        ("dyn", OpDef::dynamic_key("$dynamicRef")),
    ];
    let parser = key_parser(&ops);
    let options = FoldOptions::new(resolving).with_ops(table(&ops));
    run_tree("at-pointer.tsv", move |input| {
        fold_parse(&parser, &options, input)
    });
}

// Token operators and key operators reduce in one fold. A token operator
// is recognised only on a live expression node, so the fold runs inside
// `parse_scope`, and the folded value is realized there before the scope
// releases the arena.
#[test]
fn tree_tokens() {
    let parser = tabnas_expr::make_with(ExprOptions::new().with_op("ref", OpDef::key("$ref")));
    let options = FoldOptions::new(|site, op, terms| {
        if op.key {
            return resolving(site, op, terms);
        }
        let term = |index: usize| match terms.get(index) {
            Some(Value::Number(number)) => *number,
            _ => f64::NAN,
        };
        Ok(Value::Number(match op.src.as_str() {
            "+" => term(0) + term(1),
            "*" => term(0) * term(1),
            _ => f64::NAN,
        }))
    })
    .with_op("ref", OpDef::key("$ref"));
    run_tree("tokens.tsv", move |input| {
        tabnas_expr::parse_scope(&parser, input, |value| {
            fold(&value, &options).map(|out| tabnas_expr::realize(&out))
        })
        .map_err(parse_failure)?
        .map_err(fold_failure)
    });
}

// The inherited bag: a `base` member sets the bag for its subtree, and a
// site reached early through a reference gets its own ancestors'.
#[test]
fn tree_inherit() {
    let ops = [("ref", OpDef::key("$ref"))];
    let parser = key_parser(&ops);
    let options = FoldOptions::new(|site, _op, terms| {
        if let Some(reference) = terms.first().and_then(text_of) {
            if reference.starts_with("#/") {
                let hit = site.at(&pointer(reference), None)?;
                return Ok(hit.value.unwrap_or(Value::Undefined));
            }
        }
        let base = match site.k()? {
            Value::Object(bag) => bag.get("base").cloned().unwrap_or(Value::Undefined),
            _ => Value::Undefined,
        };
        Ok(Value::array(vec![terms[0].clone(), base]))
    })
    .with_ops(table(&ops))
    .with_inherit(|_site, node, k| {
        let base = match node {
            Value::Object(members) => members.get("base").and_then(text_of),
            _ => None,
        };
        Ok(match (base, k) {
            (Some(base), Value::Object(bag)) => {
                let mut bag = (**bag).clone();
                bag.insert("base".into(), Value::String(base.to_string()));
                Value::object(bag)
            }
            _ => k.clone(),
        })
    });
    run_tree("inherit.tsv", move |input| {
        fold_parse(&parser, &options, input)
    });
}

// The fold's own failures, pinned by code: a cycle under
// `FoldCycle::Error`, and nesting past `max_depth` (3 here).
#[test]
fn tree_errors() {
    let ops = [("ref", OpDef::key("$ref"))];
    let parser = key_parser(&ops);
    let options = FoldOptions::new(resolving)
        .with_ops(table(&ops))
        .with_cycle(FoldCycle::Error)
        .with_max_depth(3);
    run_tree("errors.tsv", move |input| {
        fold_parse(&parser, &options, input)
    });
}

/// Every tree fixture on disk is run by a test in this file, read the way
/// `every_shared_fixture_is_run` in `parity_test.rs` reads its own: a
/// fixture is run when its name appears here as a string literal.
#[test]
fn every_tree_fixture_is_run() {
    let source = std::fs::read_to_string(
        repo_root()
            .join("rs")
            .join("tests")
            .join("tree_spec_test.rs"),
    )
    .expect("this test file is readable");
    let mut fixtures: Vec<String> = std::fs::read_dir(tree_dir())
        .expect("the tree fixture directory is readable")
        .map(|entry| entry.expect("a readable directory entry").file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tsv"))
        .collect();
    fixtures.sort();
    assert!(
        !fixtures.is_empty(),
        "no fixtures in {}; the coverage gate would pass over nothing",
        tree_dir().display()
    );
    let missing: Vec<&String> = fixtures
        .iter()
        .filter(|name| !source.contains(&format!("\"{name}\"")))
        .collect();
    assert!(
        missing.is_empty(),
        "no Rust runner for {} of {} tree fixtures: {:?}",
        missing.len(),
        fixtures.len(),
        missing
    );
}
