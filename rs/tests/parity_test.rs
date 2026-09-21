// Cross-runtime conformance, driven by the shared `test/spec/*.tsv`
// fixtures at the repository root (see ../../test/AGENTS.md).
//
// The fixture loader, the escape codec and the row loop all come from
// tabnas_support, whose TypeScript half `ts/test/spec.test.ts` and Go half
// `go/expr_test.go` run the SAME files, so the three implementations
// cannot drift without one of them going red, and neither can the
// loaders.
//
// What varies per case is the CONFIGURED PARSER, which cannot live in a
// fixture column: several files need operators defined in the plugin's
// options. So each test builds its own, exactly as the TypeScript and Go
// runners do, and every fixture is named by the test that supplies it.

mod common;

use serde_json::json;
use tabnas::{Tabnas, Value as EngineValue};
use tabnas_expr::{ExprOptions, OpDef, PrevalDef};
use tabnas_support::{Runner, Value};

use common::{parse_simplified, parser_default, parser_for, parser_with, spec_dir, to_failure};

/// Run one fixture file with the parser the test built for it.
fn run_spec(name: &str, parser: Tabnas) {
    let path = spec_dir().join(name);
    assert!(path.is_file(), "missing shared fixture {}", path.display());
    Runner::new(move |input| {
        parse_simplified(&parser, input)
            .map(Value::from)
            .map_err(to_failure)
    })
    .file(&path);
}

// --- default operator table ------------------------------------------------

#[test]
fn spec_happy() {
    run_spec("happy.tsv", parser_default());
}

#[test]
fn spec_binary() {
    run_spec("binary.tsv", parser_default());
}

#[test]
fn spec_arithmetic_mixed() {
    run_spec("arithmetic-mixed.tsv", parser_default());
}

#[test]
fn spec_prefix_infix_mixed() {
    run_spec("prefix-infix-mixed.tsv", parser_default());
}

#[test]
fn spec_paren_deep_nest() {
    run_spec("paren-deep-nest.tsv", parser_default());
}

#[test]
fn spec_structure_arith() {
    run_spec("structure-arith.tsv", parser_default());
}

#[test]
fn spec_structure() {
    run_spec("structure.tsv", parser_default());
}

#[test]
fn spec_unary_prefix_basic() {
    run_spec("unary-prefix-basic.tsv", parser_default());
}

#[test]
fn spec_paren_basic() {
    run_spec("paren-basic.tsv", parser_default());
}

#[test]
fn spec_implicit_list_top_basic() {
    run_spec("implicit-list-top-basic.tsv", parser_default());
}

#[test]
fn spec_json_base() {
    run_spec("json-base.tsv", parser_default());
}

#[test]
fn spec_jsonic_base() {
    run_spec("jsonic-base.tsv", parser_default());
}

#[test]
fn spec_implicit_list_top_paren() {
    run_spec("implicit-list-top-paren.tsv", parser_default());
}

#[test]
fn spec_paren_implicit_list() {
    run_spec("paren-implicit-list.tsv", parser_default());
}

#[test]
fn spec_paren_implicit_map() {
    run_spec("paren-implicit-map.tsv", parser_default());
}

#[test]
fn spec_map_implicit_list_paren() {
    run_spec("map-implicit-list-paren.tsv", parser_default());
}

#[test]
fn spec_paren_list_implicit_structure_comma() {
    run_spec("paren-list-implicit-structure-comma.tsv", parser_default());
}

#[test]
fn spec_paren_list_implicit_structure_space() {
    run_spec("paren-list-implicit-structure-space.tsv", parser_default());
}

#[test]
fn spec_paren_map_implicit_structure_comma() {
    run_spec("paren-map-implicit-structure-comma.tsv", parser_default());
}

#[test]
fn spec_paren_map_implicit_structure_space() {
    run_spec("paren-map-implicit-structure-space.tsv", parser_default());
}

#[test]
fn spec_infix_in_paren_map() {
    run_spec("infix-in-paren-map.tsv", parser_default());
}

// --- per-file operator tables ----------------------------------------------

/// The `!` and `?` suffix pair several unary fixtures register.
fn suffix_ops() -> serde_json::Value {
    json!({
        "op": {
            "factorial": { "suffix": true, "left": 6000000, "src": "!" },
            "question": { "suffix": true, "left": 3500000, "src": "?" },
        }
    })
}

#[test]
fn spec_unary_prefix_edge() {
    run_spec(
        "unary-prefix-edge.tsv",
        parser_for(json!({
            "op": {
                "at": { "prefix": true, "right": 5000000, "src": "@" },
                "tight": { "infix": true, "left": 7000000, "right": 7100000, "src": "~" },
            }
        })),
    );
}

#[test]
fn spec_unary_suffix_basic() {
    run_spec("unary-suffix-basic.tsv", parser_for(suffix_ops()));
}

#[test]
fn spec_unary_suffix_arith() {
    run_spec("unary-suffix-arith.tsv", parser_for(suffix_ops()));
}

#[test]
fn spec_unary_suffix_structure() {
    run_spec("unary-suffix-structure.tsv", parser_for(suffix_ops()));
}

#[test]
fn spec_unary_suffix_prefix() {
    run_spec("unary-suffix-prefix.tsv", parser_for(suffix_ops()));
}

#[test]
fn spec_unary_suffix_paren() {
    run_spec("unary-suffix-paren.tsv", parser_for(suffix_ops()));
}

#[test]
fn spec_unary_suffix_edge() {
    run_spec(
        "unary-suffix-edge.tsv",
        parser_for(json!({
            "op": {
                "factorial": { "suffix": true, "left": 6000000, "src": "!" },
                "question": { "suffix": true, "left": 3500000, "src": "?" },
                "tight": { "infix": true, "left": 7000000, "right": 7100000, "src": "~" },
            }
        })),
    );
}

/// The suffix and ternary pair the basic ternary fixtures register.
fn ternary_ops() -> serde_json::Value {
    json!({
        "op": {
            "factorial": { "suffix": true, "src": "!", "left": 6000000 },
            "ternary": { "ternary": true, "src": ["?", ":"] },
        }
    })
}

#[test]
fn spec_ternary_basic() {
    run_spec("ternary-basic.tsv", parser_for(ternary_ops()));
}

#[test]
fn spec_ternary_implicit_list() {
    run_spec("ternary-implicit-list.tsv", parser_for(ternary_ops()));
}

#[test]
fn spec_ternary_many_2() {
    run_spec(
        "ternary-many-2.tsv",
        parser_for(json!({
            "op": {
                "foo": { "ternary": true, "src": ["?", ":"] },
                "bar": { "ternary": true, "src": ["QQ", "CC"] },
            }
        })),
    );
}

#[test]
fn spec_ternary_many_3() {
    run_spec(
        "ternary-many-3.tsv",
        parser_for(json!({
            "op": {
                "foo": { "ternary": true, "src": ["?", ":"] },
                "bar": { "ternary": true, "src": ["QQ", "CC"] },
                "zed": { "ternary": true, "src": ["%%", "@@"] },
            }
        })),
    );
}

#[test]
fn spec_ternary_paren_preval() {
    run_spec(
        "ternary-paren-preval.tsv",
        parser_for(json!({
            "op": {
                "ternary": { "ternary": true, "src": ["?", ":"] },
                "plain": {
                    "paren": true, "osrc": "(", "csrc": ")",
                    "preval": { "active": true },
                },
            }
        })),
    );
}

#[test]
fn spec_paren_preval_chain() {
    run_spec(
        "paren-preval-chain.tsv",
        parser_for(json!({
            "op": {
                "index": {
                    "paren": true, "osrc": "[", "csrc": "]",
                    "preval": { "required": true },
                },
                "call": {
                    "paren": true, "osrc": "(", "csrc": ")",
                    "preval": { "active": true },
                },
                "plain": null,
            }
        })),
    );
}

#[test]
fn spec_add_infix() {
    run_spec(
        "add-infix.tsv",
        parser_for(json!({
            "op": {
                "foo": { "infix": true, "left": 3500000, "right": 3600000, "src": "foo" },
            }
        })),
    );
}

#[test]
fn spec_add_paren() {
    run_spec(
        "add-paren.tsv",
        parser_for(json!({
            "op": { "angle": { "paren": true, "osrc": "<", "csrc": ">" } }
        })),
    );
}

#[test]
fn spec_paren_preval_basic() {
    run_spec(
        "paren-preval-basic.tsv",
        parser_for(json!({
            "op": {
                "angle": {
                    "osrc": "<", "csrc": ">", "paren": true,
                    "preval": { "active": true },
                },
            }
        })),
    );
}

#[test]
fn spec_paren_preval_overload() {
    run_spec(
        "paren-preval-overload.tsv",
        parser_for(json!({
            "op": {
                "factorial": { "suffix": true, "left": 6000000, "src": "!" },
                "square": {
                    "osrc": "[", "csrc": "]", "paren": true,
                    "preval": { "required": true },
                },
                "brace": {
                    "osrc": "{", "csrc": "}", "paren": true,
                    "preval": { "required": true },
                },
            }
        })),
    );
}

#[test]
fn spec_paren_preval_implicit() {
    run_spec(
        "paren-preval-implicit.tsv",
        parser_for(json!({ "op": { "plain": { "preval": true } } })),
    );
}

// --- the evaluate option ---------------------------------------------------

fn number_at(terms: &[EngineValue], index: usize) -> f64 {
    match terms.get(index) {
        Some(EngineValue::Number(number)) => *number,
        _ => 0.0,
    }
}

fn factorial(n: f64) -> f64 {
    if n <= 1.0 {
        return 1.0;
    }
    let mut out = 1.0;
    let mut step = 2.0;
    while step <= n {
        out *= step;
        step += 1.0;
    }
    out
}

/// The math expression grammar the TypeScript and Go runners build for
/// `evaluate-math.tsv`: the full pipeline of parse, S-expression, evaluate
/// and result, including preval function parens `min(x,y)` / `max(x,y)`.
#[test]
fn spec_evaluate_math() {
    let options = ExprOptions::new()
        .with_op("addition", OpDef::infix("+", 140, 150))
        .with_op("subtraction", OpDef::infix("-", 140, 150))
        .with_op("multiplication", OpDef::infix("*", 160, 170))
        .with_op("division", OpDef::infix("/", 160, 170))
        .with_op("negative", OpDef::prefix("-", 200))
        .with_op("positive", OpDef::prefix("+", 200))
        .with_op("factorial", OpDef::suffix("!", 300))
        .with_op(
            "func",
            OpDef::paren("(", ")").with_preval(PrevalDef {
                active: true,
                required: false,
                allow: None,
            }),
        )
        .with_evaluate(|_site, op, terms| {
            let left = number_at(terms, 0);
            let right = number_at(terms, 1);
            match op.name.as_str() {
                "addition-infix" => EngineValue::Number(left + right),
                "subtraction-infix" => EngineValue::Number(left - right),
                "multiplication-infix" => EngineValue::Number(left * right),
                "division-infix" => {
                    EngineValue::Number(if 0.0 == right { 0.0 } else { left / right })
                }
                "negative-prefix" => EngineValue::Number(-left),
                "positive-prefix" => EngineValue::Number(left),
                "factorial-suffix" => EngineValue::Number(factorial(left)),
                "func-paren" => {
                    let EngineValue::String(name) =
                        terms.first().unwrap_or(&EngineValue::Undefined)
                    else {
                        // Plain parens with no preval: the inner value.
                        return terms.first().cloned().unwrap_or(EngineValue::Undefined);
                    };
                    // A preval function call: the arguments follow the
                    // name, and an implicit list arrives as one array.
                    let rest = &terms[1..];
                    let args: Vec<EngineValue> = match rest {
                        [EngineValue::Array(members)] => members.to_vec(),
                        other => other.to_vec(),
                    };
                    // Variadic, as the TypeScript side's `Math.min(...args)`
                    // is: fixed at two arguments, a three-argument row would
                    // agree with TypeScript only when the third happens not
                    // to be the extremum.
                    match name.as_str() {
                        "min" | "max" => {
                            let mut best = number_at(&args, 0);
                            for index in 1..args.len() {
                                let candidate = number_at(&args, index);
                                if ("min" == name) == (candidate < best) && candidate != best {
                                    best = candidate;
                                }
                            }
                            EngineValue::Number(best)
                        }
                        _ => EngineValue::Number(number_at(&args, 0)),
                    }
                }
                "plain-paren" => EngineValue::Number(left),
                _ => EngineValue::Number(left),
            }
        });
    run_spec("evaluate-math.tsv", parser_with(options));
}
