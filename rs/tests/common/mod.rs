// Shared test helpers. Cargo compiles this module into EVERY integration
// test binary, so an item only one binary uses is dead code in the
// others; the allow keeps that from being a warning rather than hiding
// anything real.
#![allow(dead_code)]
// The engine's error is large by design (it carries the whole report), and
// the engine allows this lint at its own crate root for the same reason.
#![allow(clippy::result_large_err)]

use std::path::{Path, PathBuf};

use tabnas::{Tabnas, Value};
use tabnas_expr::{ExprOptions, OpDef, PrevalDef};
use tabnas_support::Failure;

/// The repository root: the parent of `rs/`.
pub fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rs/ has a parent")
}

/// The shared `test/spec` fixture directory.
pub fn spec_dir() -> PathBuf {
    repo_root().join("test").join("spec")
}

/// A fresh parser with the plugin options given as a JSON bag, the way
/// the Go runner builds one (`jsonic.Make()` then `j.Use(Expr, opts)`):
/// the bag is deep-merged over the plugin's declared defaults.
pub fn parser_for(options: serde_json::Value) -> Tabnas {
    let mut parser = tabnas_jsonic::make();
    parser
        .use_plugin(tabnas_expr::plugin(), Some(Value::from_json(&options)))
        .unwrap_or_else(|error| panic!("expr plugin: {error}"));
    parser
}

/// A fresh parser with the default operator table.
pub fn parser_default() -> Tabnas {
    parser_for(serde_json::json!({}))
}

/// A fresh parser over typed options, which is how an evaluator reaches
/// the grammar.
pub fn parser_with(options: ExprOptions) -> Tabnas {
    tabnas_expr::make_with(options)
}

/// A parse error as the runner's failure: the code the fixture pins, and
/// the rendered report for the failure message.
pub fn to_failure(error: tabnas::TabnasError) -> Failure {
    Failure::new(error.code.clone())
        .at(error.row, error.col)
        .with_message(error.to_string())
}

/// Parse and reduce to the S-expression form the shared fixtures compare,
/// the Rust half of the TypeScript runner's `C(S(x))` and the Go runner's
/// `simplifyAndNormalize`: operator descriptions become their source text,
/// and the result is flattened through JSON so numbers and the typed
/// metadata wrappers compare as plain values.
pub fn parse_simplified(
    parser: &Tabnas,
    src: &str,
) -> Result<serde_json::Value, tabnas::TabnasError> {
    tabnas_expr::parse_simplified(parser, src).map(|value| value.to_json())
}

/// Numbers as the engine renders them, so that a JSON literal written
/// with integers compares equal to a parse result carrying floats. The
/// shared fixture runner normalizes the same way.
pub fn norm(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Number(number) => {
            serde_json::Number::from_f64(number.as_f64().expect("a JSON number is representable"))
                .map_or(serde_json::Value::Null, serde_json::Value::Number)
        }
        serde_json::Value::Array(entries) => {
            serde_json::Value::Array(entries.into_iter().map(norm).collect())
        }
        serde_json::Value::Object(entries) => serde_json::Value::Object(
            entries
                .into_iter()
                .map(|(key, entry)| (key, norm(entry)))
                .collect(),
        ),
        other => other,
    }
}

/// An operator definition from the brief shape the fixture runners use.
pub fn preval(active: bool, required: bool, allow: Option<Vec<String>>) -> PrevalDef {
    PrevalDef {
        active,
        required,
        allow,
    }
}

/// A paren operator with preval active.
pub fn preval_paren(osrc: &str, csrc: &str) -> OpDef {
    OpDef::paren(osrc, csrc).with_preval(preval(true, false, None))
}
