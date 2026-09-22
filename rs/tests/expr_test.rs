// In-language behaviour: what a shared fixture cannot express.
//
// The Pratt core is exercised directly here, as the TypeScript
// `testing.prattify` cases and the Go `TestPrattifyBasic` /
// `TestPrattifyAssoc` do, along with the parse-level behaviours the `.tsv`
// files do not reach: comma-op suppression, the evaluator, the operator
// table's instance token binding, and the serialized shape of a parsed
// expression.

mod common;

use std::collections::HashMap;

use serde_json::json;
use tabnas::{Context, LexCheckResult, Rule, Tabnas, Value};
use tabnas_expr::{
    is_op, opify, prattify, simplify, EvalSite, ExprOptions, Op, OpDef, OpRef, PrevalDef,
};

use common::{norm, parse_simplified, parser_for, parser_with};

/// An operator built by hand, as the Go suite's `moT` does: the name comes
/// from the source when it is not given, and the term count is derived.
fn mo(mut spec: Op) -> OpRef {
    if spec.name.is_empty() {
        spec.name = spec.src.clone();
    }
    OpRef::new(opify(spec))
}

fn infix(src: &str, left: i64, right: i64) -> OpRef {
    mo(Op {
        infix: true,
        src: src.into(),
        left,
        right,
        ..Default::default()
    })
}

fn prefix(src: &str, right: i64) -> OpRef {
    mo(Op {
        prefix: true,
        src: src.into(),
        right,
        ..Default::default()
    })
}

fn suffix(src: &str, left: i64) -> OpRef {
    mo(Op {
        suffix: true,
        src: src.into(),
        left,
        ..Default::default()
    })
}

/// An expression node, the canonical `makeExpr`.
fn expr(op: &OpRef, terms: &[f64]) -> Value {
    op.node(terms.iter().map(|term| Value::Number(*term)).collect())
}

/// An expression node whose terms are already built.
fn nest(op: &OpRef, terms: Vec<Value>) -> Value {
    op.node(terms)
}

/// Compare a node's S-expression against a JSON literal, mirroring the
/// TypeScript tests' `C(S(x))` comparisons.
fn check(label: &str, node: &Value, want: &str) {
    let got = norm(simplify(node).to_json());
    let want: serde_json::Value = serde_json::from_str(want).expect("the expectation is JSON");
    assert_eq!(got, norm(want), "{label}");
}

/// Parse and compare the S-expression, mirroring the Go `parseSx`.
fn check_parse(parser: &Tabnas, src: &str, want: &str) {
    let got =
        parse_simplified(parser, src).unwrap_or_else(|error| panic!("parse {src:?}: {error}"));
    let want: serde_json::Value = serde_json::from_str(want).expect("the expectation is JSON");
    assert_eq!(norm(got), norm(want), "parse {src:?}");
}

// --- the Pratt core --------------------------------------------------------

/// The canonical `prattify-basic` cases: each one checks the returned
/// attachment point and the rewritten expression tree.
#[test]
fn prattify_basic() {
    let plus_la = infix("+", 140, 150);
    let plus_ra = infix("+", 150, 140);
    let mul_la = infix("*", 160, 170);
    let pipe_la = infix("|", 18000, 17000);
    let at_p = prefix("@", 1500);
    let per_p = prefix("%", 1300);
    let bang_s = suffix("!", 1600);
    let quest_s = suffix("?", 1400);

    // 1+2+N is (1+2)+N
    let tree = expr(&plus_la, &[1.0, 2.0]);
    check("1+2+N T", &prattify(&tree, &plus_la), r#"["+",["+",1,2]]"#);
    check("1+2+N E", &tree, r#"["+",["+",1,2]]"#);

    // 1+2+N is 1+(2+N) when the operator is right-associative
    let tree = expr(&plus_ra, &[1.0, 2.0]);
    check("1+2+N ra T", &prattify(&tree, &plus_ra), r#"["+",2]"#);
    check("1+2+N ra E", &tree, r#"["+",1,["+",2]]"#);

    // 1+2*N is 1+(2*N)
    let tree = expr(&plus_la, &[1.0, 2.0]);
    check("1+2*N T", &prattify(&tree, &mul_la), r#"["*",2]"#);
    check("1+2*N E", &tree, r#"["+",1,["*",2]]"#);

    // 1*2+N is (1*2)+N
    let tree = expr(&mul_la, &[1.0, 2.0]);
    check("1*2+N T", &prattify(&tree, &plus_la), r#"["+",["*",1,2]]"#);
    check("1*2+N E", &tree, r#"["+",["*",1,2]]"#);

    // @1+N is (@1)+N
    let tree = expr(&at_p, &[1.0]);
    check("@1+N T", &prattify(&tree, &plus_la), r#"["+",["@",1]]"#);
    check("@1+N E", &tree, r#"["+",["@",1]]"#);

    // 1!+N is (1!)+N
    let tree = expr(&bang_s, &[1.0]);
    check("1!+N T", &prattify(&tree, &plus_la), r#"["+",["!",1]]"#);
    check("1!+N E", &tree, r#"["+",["!",1]]"#);

    // @1|N is @(1|N)
    let tree = expr(&at_p, &[1.0]);
    check("@1|N T", &prattify(&tree, &pipe_la), r#"["|",1]"#);
    check("@1|N E", &tree, r#"["@",["|",1]]"#);

    // 1|@N is 1|(@N)
    let tree = expr(&pipe_la, &[1.0]);
    check("1|@N T", &prattify(&tree, &at_p), r#"["@"]"#);
    check("1|@N E", &tree, r#"["|",1,["@"]]"#);

    // 1!|N is (1!)|N
    let tree = expr(&bang_s, &[1.0]);
    check("1!|N T", &prattify(&tree, &pipe_la), r#"["|",["!",1]]"#);
    check("1!|N E", &tree, r#"["|",["!",1]]"#);

    // 1+@N is 1+(@N)
    let tree = expr(&plus_la, &[1.0]);
    check("1+@N T", &prattify(&tree, &at_p), r#"["@"]"#);
    check("1+@N E", &tree, r#"["+",1,["@"]]"#);

    // @@N is @(@N)
    let tree = expr(&at_p, &[]);
    check("@@N T", &prattify(&tree, &at_p), r#"["@"]"#);
    check("@@N E", &tree, r#"["@",["@"]]"#);

    // %@N is %(@N)
    let tree = expr(&per_p, &[]);
    check("%@N T", &prattify(&tree, &at_p), r#"["@"]"#);
    check("%@N E", &tree, r#"["%",["@"]]"#);

    // @%N is @(%N)
    let tree = expr(&at_p, &[]);
    check("@%N T", &prattify(&tree, &per_p), r#"["%"]"#);
    check("@%N E", &tree, r#"["@",["%"]]"#);

    // 1+2! is 1+(2!)
    let tree = expr(&plus_la, &[1.0, 2.0]);
    check("1+2! T", &prattify(&tree, &bang_s), r#"["+",1,["!",2]]"#);
    check("1+2! E", &tree, r#"["+",1,["!",2]]"#);

    // 1|2! is (1|2)!
    let tree = expr(&pipe_la, &[1.0, 2.0]);
    check("1|2! T", &prattify(&tree, &bang_s), r#"["!",["|",1,2]]"#);
    check("1|2! E", &tree, r#"["!",["|",1,2]]"#);

    // 1!! is (1!)!
    let tree = expr(&bang_s, &[1.0]);
    check("1!! T", &prattify(&tree, &bang_s), r#"["!",["!",1]]"#);
    check("1!! E", &tree, r#"["!",["!",1]]"#);

    // 1!? is (1!)?
    let tree = expr(&bang_s, &[1.0]);
    check("1!? T", &prattify(&tree, &quest_s), r#"["?",["!",1]]"#);
    check("1!? E", &tree, r#"["?",["!",1]]"#);

    // 1?! is (1?)!
    let tree = expr(&quest_s, &[1.0]);
    check("1?! T", &prattify(&tree, &bang_s), r#"["!",["?",1]]"#);
    check("1?! E", &tree, r#"["!",["?",1]]"#);

    // @1! is @(1!)
    let tree = expr(&at_p, &[1.0]);
    check("@1! T", &prattify(&tree, &bang_s), r#"["@",["!",1]]"#);
    check("@1! E", &tree, r#"["@",["!",1]]"#);

    // @1? is (@1)?
    let tree = expr(&at_p, &[1.0]);
    check("@1? T", &prattify(&tree, &quest_s), r#"["?",["@",1]]"#);
    check("@1? E", &tree, r#"["?",["@",1]]"#);

    // @@1! is @(@(1!))
    let tree = nest(&at_p, vec![expr(&at_p, &[1.0])]);
    check(
        "@@1! T",
        &prattify(&tree, &bang_s),
        r#"["@",["@",["!",1]]]"#,
    );
    check("@@1! E", &tree, r#"["@",["@",["!",1]]]"#);

    // @@1? is (@(@1))?
    let tree = nest(&at_p, vec![expr(&at_p, &[1.0])]);
    check(
        "@@1? T",
        &prattify(&tree, &quest_s),
        r#"["?",["@",["@",1]]]"#,
    );
    check("@@1? E", &tree, r#"["?",["@",["@",1]]]"#);
}

/// The canonical `prattify-assoc` cases: left- and right-associative
/// infix chains.
#[test]
fn prattify_assoc() {
    let at_la = infix("@", 14, 15);
    let per_ra = infix("%", 17, 16);

    let tree = expr(&at_la, &[1.0, 2.0]);
    check("1@2@N T", &prattify(&tree, &at_la), r#"["@",["@",1,2]]"#);
    check("1@2@N E", &tree, r#"["@",["@",1,2]]"#);

    let tree = nest(&at_la, vec![expr(&at_la, &[1.0, 2.0]), Value::Number(3.0)]);
    check(
        "1@2@3@N T",
        &prattify(&tree, &at_la),
        r#"["@",["@",["@",1,2],3]]"#,
    );
    check("1@2@3@N E", &tree, r#"["@",["@",["@",1,2],3]]"#);

    let tree = nest(
        &at_la,
        vec![
            nest(&at_la, vec![expr(&at_la, &[1.0, 2.0]), Value::Number(3.0)]),
            Value::Number(4.0),
        ],
    );
    check(
        "1@2@3@4@N T",
        &prattify(&tree, &at_la),
        r#"["@",["@",["@",["@",1,2],3],4]]"#,
    );
    check("1@2@3@4@N E", &tree, r#"["@",["@",["@",["@",1,2],3],4]]"#);

    let tree = nest(
        &at_la,
        vec![
            nest(
                &at_la,
                vec![
                    nest(&at_la, vec![expr(&at_la, &[1.0, 2.0]), Value::Number(3.0)]),
                    Value::Number(4.0),
                ],
            ),
            Value::Number(5.0),
        ],
    );
    check(
        "1@2@3@4@5@N T",
        &prattify(&tree, &at_la),
        r#"["@",["@",["@",["@",["@",1,2],3],4],5]]"#,
    );
    check(
        "1@2@3@4@5@N E",
        &tree,
        r#"["@",["@",["@",["@",["@",1,2],3],4],5]]"#,
    );

    let tree = expr(&per_ra, &[1.0, 2.0]);
    check("1%2%N T", &prattify(&tree, &per_ra), r#"["%",2]"#);
    check("1%2%N E", &tree, r#"["%",1,["%",2]]"#);

    let tree = nest(
        &per_ra,
        vec![Value::Number(1.0), expr(&per_ra, &[2.0, 3.0])],
    );
    check("1%2%3%N T", &prattify(&tree, &per_ra), r#"["%",3]"#);
    check("1%2%3%N E", &tree, r#"["%",1,["%",2,["%",3]]]"#);

    let tree = nest(
        &per_ra,
        vec![
            Value::Number(1.0),
            nest(
                &per_ra,
                vec![Value::Number(2.0), expr(&per_ra, &[3.0, 4.0])],
            ),
        ],
    );
    check("1%2%3%4%N T", &prattify(&tree, &per_ra), r#"["%",4]"#);
    check("1%2%3%4%N E", &tree, r#"["%",1,["%",2,["%",3,["%",4]]]]"#);

    let tree = nest(
        &per_ra,
        vec![
            Value::Number(1.0),
            nest(
                &per_ra,
                vec![
                    Value::Number(2.0),
                    nest(
                        &per_ra,
                        vec![Value::Number(3.0), expr(&per_ra, &[4.0, 5.0])],
                    ),
                ],
            ),
        ],
    );
    check("1%2%3%4%5%N T", &prattify(&tree, &per_ra), r#"["%",5]"#);
    check(
        "1%2%3%4%5%N E",
        &tree,
        r#"["%",1,["%",2,["%",3,["%",4,["%",5]]]]]"#,
    );
}

// --- comma-op suppression --------------------------------------------------

/// With `,` defined as an infix operator it is absorbed as the comma
/// operator everywhere; a host rule setting `n.no_comma_op` suppresses it,
/// so the bail alternates leave `,` for the enclosing rule to consume as a
/// separator.
#[test]
fn no_comma_op_suppression() {
    let comma_op = || {
        json!({
            "op": {
                "comma_op": { "infix": true, "src": ",", "left": 1000000, "right": 1100000 },
            }
        })
    };

    // Baseline: with the comma operator defined, `,` is absorbed
    // everywhere, including inside list and paren contexts where it would
    // otherwise have been a separator.
    let base = parser_for(comma_op());
    check_parse(&base, "1,2", r#"[",",1,2]"#);
    check_parse(&base, "[1,2]", r#"[[",",1,2]]"#);
    check_parse(&base, "(1,2)", r#"["(",[",",1,2]]"#);

    // Suppression: hook the base grammar's list rule so any expression
    // parsed inside `[...]` sees n.no_comma_op set. This mirrors how a
    // host grammar uses the counter around boundary expressions to keep
    // `,` out of the comma operator's reach.
    let mut parser = parser_for(comma_op());
    parser.define_rule("list", |spec| {
        spec.add_bo(|rule: &mut Rule, _context: &mut Context| {
            let depth = rule.n.get("no_comma_op").copied().unwrap_or(0);
            rule.n_mut().insert("no_comma_op".into(), depth + 1);
        });
    });

    // Inside `[...]`, `,` is a separator again, not the comma operator.
    check_parse(&parser, "[1,2]", "[1,2]");
    check_parse(&parser, "[1,2,3]", "[1,2,3]");
    // Other operators inside the list still work.
    check_parse(&parser, "[1+2,3+4]", r#"[["+",1,2],["+",3,4]]"#);
    // Outside the list the comma operator still applies: n.no_comma_op is
    // scoped to the list rule and the val and expr rules under it.
    check_parse(&parser, "1,2", r#"[",",1,2]"#);
}

// --- the evaluator ---------------------------------------------------------

fn number(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(number)) => *number,
        _ => 0.0,
    }
}

/// The ternary rule's after-close fires the evaluator even for a ternary
/// that is not wrapped in an expr rule. Without it the result would leak
/// as a raw S-expression.
#[test]
fn ternary_evaluate() {
    let parser = parser_with(
        ExprOptions::new()
            .with_op("q", OpDef::ternary("?", ":"))
            .with_evaluate(|_site, op, terms| {
                if "q-ternary" == op.name {
                    return if 0.0 != number(terms.first()) {
                        terms.get(1).cloned().unwrap_or(Value::Undefined)
                    } else {
                        terms.get(2).cloned().unwrap_or(Value::Undefined)
                    };
                }
                Value::Number(f64::NAN)
            }),
    );

    for (src, want) in [
        // A direct ternary at the top level evaluates through the
        // after-close.
        ("1?2:3", 2.0),
        ("0?2:3", 3.0),
        // Right-associative chains evaluate fully.
        ("1?2: 0?4:5", 2.0),
        ("0?2: 1?4:5", 4.0),
        ("0?2: 0?4:5", 5.0),
    ] {
        let got = tabnas_expr::parse_with(&parser, src)
            .unwrap_or_else(|error| panic!("parse {src:?}: {error}"));
        assert_eq!(got, Value::Number(want), "parse {src:?}");
    }
}

/// A small evaluated expression language: preval function parens with
/// custom delimiters and with overloaded `(...)`, plain parens, implicit
/// lists and map and list embedding, all through the evaluator.
#[test]
fn mini_config() {
    fn reduce(site: &EvalSite<'_>, name: &str, terms: &[Value]) -> Value {
        let mut terms = terms.to_vec();
        if "func-paren" == name && !site.flag("paren_preval") {
            terms.insert(0, Value::String(String::new()));
        }
        match name {
            "addition-infix" => Value::Number(number(terms.first()) + number(terms.get(1))),
            "subtraction-infix" => Value::Number(number(terms.first()) - number(terms.get(1))),
            "plain-paren" => terms.first().cloned().unwrap_or(Value::Null),
            "func-paren" => {
                let Some(Value::String(function)) = terms.first().cloned() else {
                    return terms.get(1).cloned().unwrap_or(Value::Null);
                };
                if function.is_empty() {
                    return terms.get(1).cloned().unwrap_or(Value::Null);
                }
                if "floor" != function {
                    return Value::Null;
                }
                match terms.get(1) {
                    Some(Value::Number(value)) => Value::Number(value.floor()),
                    _ => Value::Null,
                }
            }
            _ => Value::Null,
        }
    }

    let angle = parser_with(
        ExprOptions::new()
            .with_op(
                "func",
                OpDef::paren("<", ">").with_preval(PrevalDef {
                    active: true,
                    required: false,
                    allow: None,
                }),
            )
            .with_evaluate(|site, op, terms| reduce(site, &op.name, terms)),
    );

    for (src, want) in [
        ("11+22", "33"),
        ("44-33", "11"),
        ("(44-33)+11", "22"),
        ("44-(33+11)", "0"),
        ("44-33+11", "22"),
        ("(1.1)", "1.1"),
        ("[0,(1)]", "[0,1]"),
        ("[0 (1)]", "[0,1]"),
        ("floor<1.5>", "1"),
        ("a:floor<2.5>", r#"{"a":2}"#),
        ("{b:floor<3.5>}", r#"{"b":3}"#),
        ("[floor<4.5>]", "[4]"),
        ("[0 floor<5.5>]", "[0,5]"),
        ("1+floor<1.5>", "2"),
        ("1+floor<1.5>+3", "5"),
        ("floor<1.5>+4", "5"),
        ("a:floor<1.5>+4", r#"{"a":5}"#),
        ("a:(1+2) b:floor<1.9>", r#"{"a":3,"b":1}"#),
        ("()", "null"),
        ("<>", "null"),
        ("<1>", "1"),
        ("c:<2>", r#"{"c":2}"#),
        ("a:floor<>", r#"{"a":null}"#),
        ("floor<>", "null"),
        ("[floor<>]", "[null]"),
        (r#"floor<"a">"#, "null"),
        (r#"a:floor<"a">"#, r#"{"a":null}"#),
        ("[1 (2) (2+1) floor<4.5>]", "[1,2,3,4]"),
        ("1 (2) (2+1) floor<4.5>", "[1,2,3,4]"),
        ("bad<9>", "null"),
    ] {
        check_parse(&angle, src, want);
    }

    let round = parser_with(
        ExprOptions::new()
            .without_op("plain")
            .with_op(
                "func",
                OpDef::paren("(", ")").with_preval(PrevalDef {
                    active: true,
                    required: false,
                    allow: Some(vec!["floor".into()]),
                }),
            )
            .with_evaluate(|site, op, terms| reduce(site, &op.name, terms)),
    );

    for (src, want) in [
        ("()", "null"),
        ("(0)", "0"),
        ("(0+1)", "1"),
        ("[(0) 1]", "[0,1]"),
        ("[0,(1),2]", "[0,1,2]"),
        ("[0,(1)]", "[0,1]"),
        ("[(1)]", "[1]"),
        ("[(0),(1)]", "[0,1]"),
        ("(0),(1)", "[0,1]"),
        ("floor(1.1)", "1"),
        ("floor (1.1)", "1"),
        ("floor(0.5)", "0"),
        ("a:floor(2.5)", r#"{"a":2}"#),
        ("{b:floor(3.5)}", r#"{"b":3}"#),
        ("[floor(4.5)]", "[4]"),
        ("[0 floor(5.5)]", "[0,5]"),
        ("[(0) 1 floor(5.5)]", "[0,1,5]"),
        ("[(0) floor(5.5)]", "[0,5]"),
        ("[0,(1),floor(5.5)]", "[0,1,5]"),
        ("[1,(2),(2+1)]", "[1,2,3]"),
        ("[1,(2),(2+1),floor(4.5)]", "[1,2,3,4]"),
        ("a:floor(1.5)", r#"{"a":1}"#),
        ("[3+2]", "[5]"),
        ("[3+(2)]", "[5]"),
        ("[(3)+2]", "[5]"),
        ("[(3)+(2)]", "[5]"),
        ("[(3+2)]", "[5]"),
        ("[(3+(2))]", "[5]"),
        ("[((3)+2)]", "[5]"),
        ("[((3)+(2))]", "[5]"),
        ("[1,3+2]", "[1,5]"),
        ("[1,(3+(2))]", "[1,5]"),
        ("[3+2,4]", "[5,4]"),
        ("[((3)+(2)),4]", "[5,4]"),
        ("[1,3+2,4]", "[1,5,4]"),
        ("[1,((3)+(2)),4]", "[1,5,4]"),
        ("1+floor(1.1)", "2"),
        ("floor(1.1)+1", "2"),
        ("1+floor(1.1)+1", "3"),
        ("a:(2)+1", r#"{"a":3}"#),
        ("a:1+floor(1.1)", r#"{"a":2}"#),
        ("a:(1.1)+1", r#"{"a":2.1}"#),
        ("a:floor(1.1)+1", r#"{"a":2}"#),
        ("a:1+floor(1.1)+1", r#"{"a":3}"#),
        ("[1+floor(1.1)]", "[2]"),
        ("[floor(1.1)+2]", "[3]"),
        ("[3+floor(1.1)+2]", "[6]"),
        ("b:1.1+1,c:C0", r#"{"b":2.1,"c":"C0"}"#),
        ("b:(1.1+1),c:C0a", r#"{"b":2.1,"c":"C0a"}"#),
        ("b:(1.1)+1,c:C1", r#"{"b":2.1,"c":"C1"}"#),
        ("b:((1.1)+1),c:C1a", r#"{"b":2.1,"c":"C1a"}"#),
        ("b:1+floor(1.1),c:C2c", r#"{"b":2,"c":"C2c"}"#),
        ("b:floor(1.1)+1,c:C2d", r#"{"b":2,"c":"C2d"}"#),
        ("b:(floor(1.1)),c:C2a", r#"{"b":1,"c":"C2a"}"#),
        ("b:(1+floor(1.1)),c:C2b", r#"{"b":2,"c":"C2b"}"#),
        ("1+(floor(1.1))", "2"),
        ("(11,22)", "[11,22]"),
        ("21+31", "52"),
        ("(21)+31", "52"),
        ("(21+31)", "52"),
        ("(floor(2.2))", "2"),
        ("((floor(2.2)))", "2"),
        ("(floor(2.2))+1", "3"),
        ("floor(2.2)+3", "5"),
        ("(floor(1.1)+2)", "3"),
        ("b:(floor(1.1)+2),c:C2c", r#"{"b":3,"c":"C2c"}"#),
    ] {
        check_parse(&round, src, want);
    }
}

/// The evaluator is free to return another S-expression and to have
/// effects, so a member of an implicit list must not be reduced twice: it
/// is reduced when it closes and the list is reduced again as a whole.
#[test]
fn evaluate_called_once_per_operator() {
    for src in [
        "f(1+2,3+4,5+6)",
        "f(1+2,3+4)",
        "1+2,3+4,5+6",
        "f(1+2,3)",
        "f(-1+2,3+4)",
        "f(g(1+2,3+4),5+6)",
        "f(1+2,(3+4,5+6))",
        "(1+2,3+4)",
        "[f(1+2,3+4)]",
        "{k:f(1+2,3+4)}",
        "f(1+2,3+4),f(5+6,7+8)",
    ] {
        let counts: std::sync::Arc<std::sync::Mutex<HashMap<String, usize>>> =
            std::sync::Arc::new(std::sync::Mutex::new(HashMap::new()));
        let seen = counts.clone();
        let parser = parser_with(
            ExprOptions::new()
                .with_op(
                    "func",
                    OpDef::paren("(", ")").with_preval(PrevalDef {
                        active: true,
                        required: false,
                        allow: None,
                    }),
                )
                .with_evaluate(move |_site, op, terms| {
                    let key = format!(
                        "{}{}",
                        op.name,
                        simplify(&Value::array(terms.to_vec())).to_json()
                    );
                    *seen.lock().unwrap().entry(key).or_insert(0) += 1;
                    let mut out = vec![];
                    out.extend(terms.iter().cloned());
                    OpRef {
                        op: std::sync::Arc::new(op.clone()),
                        token: None,
                    }
                    .node(out)
                }),
        );

        tabnas_expr::parse_with(&parser, src).unwrap_or_else(|error| panic!("{src}: {error}"));

        for (key, count) in counts.lock().unwrap().iter() {
            assert_eq!(
                1, *count,
                "{src}: the evaluator ran {count} times for {key}"
            );
        }
    }
}

/// Reducing a finished tree outside a parse, the exported entry point.
#[test]
fn evaluation_reduces_a_finished_tree() {
    let parser = parser_for(json!({}));
    let math: tabnas_expr::Evaluate =
        std::sync::Arc::new(|_site: &mut EvalSite<'_>, op: &Op, terms: &[Value]| {
            let left = number(terms.first());
            let right = number(terms.get(1));
            match op.name.as_str() {
                "addition-infix" => Value::Number(left + right),
                "subtraction-infix" => Value::Number(left - right),
                "multiplication-infix" => Value::Number(left * right),
                "negative-prefix" => Value::Number(-left),
                "positive-prefix" => Value::Number(left),
                "plain-paren" => terms.first().cloned().unwrap_or(Value::Null),
                _ => Value::Null,
            }
        });

    for (src, want) in [
        ("1+2", 3.0),
        ("1+2+3", 6.0),
        ("1*2+3", 5.0),
        ("1+2*3", 7.0),
        ("(1+2)*3", 9.0),
        ("3*(1+2)", 9.0),
        ("(1)", 1.0),
        ("(1+2)", 3.0),
        ("3+(1+2)", 6.0),
        ("(1+2)+3", 6.0),
        ("111+222", 333.0),
        ("((1+2)*4)", 12.0),
        ("(1+(2*4))", 9.0),
        ("((114))", 114.0),
        ("(((115)))", 115.0),
        ("1-3", -2.0),
        ("-1", -1.0),
        ("+1", 1.0),
        ("1+(-3)", -2.0),
    ] {
        // The tree has to be reduced inside the parse's arena lifetime,
        // which `parse_scope` holds open.
        let got = tabnas_expr::parse_scope(&parser, src, |value| {
            tabnas_expr::evaluation(&mut EvalSite::detached(), &value, &math)
        })
        .unwrap_or_else(|error| panic!("parse {src:?}: {error}"));
        assert_eq!(got, Value::Number(want), "evaluate {src:?}");
    }
}

/// The evaluator is told which OCCURRENCE of an operator it is reducing.
///
/// The `Op` it receives is the shared description, one per entry in the
/// operator table, so it is the same value for every `+` in a document.
/// The canonical `makeOp` copies the description and attaches the token
/// for exactly that reason; this port keeps the token beside the node and
/// hands it over on the site. Without it an evaluator could not report the
/// row and column of the operator it is reducing.
#[test]
fn the_evaluator_is_given_the_occurrence_token() {
    let parser = parser_for(json!({}));
    let seen: std::sync::Arc<std::sync::Mutex<Vec<String>>> = Default::default();
    let record = seen.clone();
    let note: tabnas_expr::Evaluate =
        std::sync::Arc::new(move |site: &mut EvalSite<'_>, op: &Op, terms: &[Value]| {
            let token = site.token().expect("an occurrence carries its token");
            record.lock().expect("not poisoned").push(format!(
                "{} {} {} {} {}",
                op.name, token.src, token.site.pos, token.site.ri, token.site.ci
            ));
            terms.first().cloned().unwrap_or(Value::Null)
        });

    tabnas_expr::parse_scope(&parser, "1+2*3", |value| {
        tabnas_expr::evaluation(&mut EvalSite::detached(), &value, &note)
    })
    .expect("parses");

    // Innermost first, and each one carries its own token: the `*` at
    // column 4 and the `+` at column 2, the positions the canonical
    // attaches to its copy of the description.
    assert_eq!(
        *seen.lock().expect("not poisoned"),
        vec![
            "multiplication-infix * 3 1 4".to_string(),
            "addition-infix + 1 1 2".to_string(),
        ]
    );
}

/// A tree built without a parse behind it has no token to report, and
/// says so rather than inventing one.
#[test]
fn a_handmade_node_has_no_occurrence_token() {
    let add = infix("+", 2000000, 2100000);
    let node = expr(&add, &[1.0, 2.0]);
    let asked: tabnas_expr::Evaluate =
        std::sync::Arc::new(|site: &mut EvalSite<'_>, _op: &Op, _terms: &[Value]| {
            Value::Bool(site.token().is_none())
        });
    assert_eq!(
        tabnas_expr::evaluation(&mut EvalSite::detached(), &node, &asked),
        Value::Bool(true)
    );
}

// --- the operator table ----------------------------------------------------

/// An operator's token identity comes from the INSTANCE, not from a global
/// table. A host grammar registers its punctuation as instance-level fixed
/// tokens before this plugin runs, and an operator bound to a fresh
/// identity instead would leave its alternate waiting on a token the host
/// lexer never emits.
#[test]
fn operators_bind_to_instance_fixed_tokens() {
    let mut parser = tabnas_jsonic::make();
    let host = parser.token_with_source("#AT", "@");
    assert_eq!(Some(host), parser.fixed("@"));

    parser
        .use_plugin(
            tabnas_expr::plugin(),
            Some(Value::from_json(&json!({
                "op": { "at": { "infix": true, "left": 2000000, "right": 2100000, "src": "@" } }
            }))),
        )
        .expect("the plugin installs");

    // The operator matches the token the host lexer emits, so the
    // expression parses rather than failing as an unexpected character.
    assert_eq!(
        norm(parse_simplified(&parser, "1 @ 2").expect("1 @ 2 parses")),
        norm(json!(["@", 1, 2]))
    );
    // The host's identity is still the one bound to the source.
    assert_eq!(Some(host), parser.fixed("@"));
}

/// A host that already configured `options.fixed.check` still gets its
/// comments lexed as comments.
///
/// The default `/` operator is a prefix of both `//` and `/*`, and the
/// fixed matcher runs before the comment one, so the plugin makes the
/// fixed family stand aside where a comment starts. Omitting that because
/// the host had claimed the hook cut `// note` into two `/` operators and
/// the document failed with `unexpected`. The canonical reorders the
/// matchers instead, so a host check never sees a comment opener there
/// either, and `1/2 // note` parses in TypeScript whether or not a host
/// check is installed.
#[test]
fn a_host_fixed_check_keeps_the_comment_guard() {
    let mut parser = tabnas_jsonic::make();
    parser.lex_check_ref("@host-fixed", |_remaining: &str| LexCheckResult::Continue);
    parser
        .grammar_json(r#"{"options":{"fixed":{"check":"@host-fixed"}}}"#)
        .expect("the host check installs");
    parser
        .use_plugin(tabnas_expr::plugin(), None)
        .expect("the plugin installs");

    for (src, want) in [
        ("1/2 // note", json!(["/", 1, 2])),
        ("1/2 /* note */", json!(["/", 1, 2])),
        ("a:1/2 // note", json!({ "a": ["/", 1, 2] })),
    ] {
        assert_eq!(
            norm(parse_simplified(&parser, src).unwrap_or_else(|error| panic!("{src:?}: {error}"))),
            norm(want),
            "parse {src:?}"
        );
    }
}

/// A comment marker adjacent to a value is read as a comment.
///
/// The fixed matcher stands aside wherever a contested marker begins, so
/// `1//c` is the value `1` followed by a comment. TypeScript rejects the
/// same document, and `../DIVERGENCE.md` records why: bare jsonic reads it
/// in every runtime, and registering `/` as a fixed token is what breaks
/// it there, which puts the repair on the canonical engine rather than on
/// this port. This pin fails if this port is ever brought down to the
/// TypeScript behaviour instead, and the register entry says so.
#[test]
fn a_comment_marker_adjacent_to_a_value_is_read() {
    let parser = parser_for(json!({}));
    for (src, want) in [
        ("1//c", json!(1)),
        ("1/2//c", json!(["/", 1, 2])),
        ("a:1//c", json!({ "a": 1 })),
        ("1/*c*/", json!(1)),
    ] {
        assert_eq!(
            norm(parse_simplified(&parser, src).unwrap_or_else(|error| panic!("{src:?}: {error}"))),
            norm(want),
            "parse {src:?}"
        );
    }
}

/// Operator setup is deterministic, so precedence never varies between
/// runs: `1 + 2 * 3` is `1 + (2*3)` every time.
#[test]
fn precedence_is_stable() {
    for _ in 0..25 {
        let parser = parser_for(json!({}));
        assert_eq!(
            norm(parse_simplified(&parser, "1 + 2 * 3").expect("parses")),
            norm(json!(["+", 1, ["*", 2, 3]]))
        );
    }
}

/// A user operator claiming a default's source wins, because the
/// operator table is insertion-ordered and the later entry resolves the
/// token. The default `plain` paren and a `func` paren both on `(`…`)` is
/// the case that matters.
#[test]
fn a_later_operator_wins_a_shared_source() {
    let parser = parser_for(json!({
        "op": {
            "func": {
                "paren": true, "osrc": "(", "csrc": ")",
                "preval": { "active": true },
            }
        }
    }));
    assert_eq!(
        norm(parse_simplified(&parser, "f(1)").expect("parses")),
        norm(json!(["(", "f", 1]))
    );
}

/// A binding power of ZERO is an UNSET binding power, as it is in the
/// canonical `opdef.left || Number.MIN_SAFE_INTEGER` and
/// `opdef.right || Number.MAX_SAFE_INTEGER`: JavaScript's `||` is
/// falsy-based, so a zero falls through to the fallback exactly as an
/// absent power does.
///
/// It shows in the tree when a zero-power operator meets one with a
/// NEGATIVE power, the only way to sit below zero. Keeping the zero made
/// `1@2~3` parse as `["@",1,["~",2,3]]`, because `~`'s left of `0` no
/// longer bound looser than `@`'s right of `-2`. All three runtimes take
/// the fallback now, so the behaviour is also a shared fixture
/// (`binding-power-zero.tsv`); this case keeps the small powers the
/// original measurement used.
#[test]
fn a_zero_binding_power_is_unset() {
    let parser = parser_for(json!({
        "op": {
            "at": { "infix": true, "src": "@", "left": -2, "right": -2 },
            "tilde": { "infix": true, "src": "~", "left": 0, "right": 0 },
        }
    }));
    for (src, want) in [
        ("1@2~3", json!(["~", ["@", 1, 2], 3])),
        ("1~2@3", json!(["@", ["~", 1, 2], 3])),
        ("1~2~3", json!(["~", ["~", 1, 2], 3])),
        ("1@2@3", json!(["@", ["@", 1, 2], 3])),
    ] {
        assert_eq!(
            norm(parse_simplified(&parser, src).expect("parses")),
            norm(want),
            "parse {src:?}"
        );
    }
}

/// The plugin layers on the base grammar, and says so rather than
/// installing alternates on a rule that is not there.
#[test]
fn a_bare_engine_is_refused() {
    let mut bare = Tabnas::new();
    let Err(error) = bare.use_plugin(tabnas_expr::plugin(), None) else {
        panic!("a bare engine has no val rule to install on");
    };
    assert!(error.0.contains("val"), "{}", error.0);
}

/// Installing the plugin twice on one instance leaves one copy of the
/// grammar, not two.
#[test]
fn a_second_install_is_a_no_op() {
    let mut parser = tabnas_jsonic::make();
    assert!(parser.use_plugin(tabnas_expr::plugin(), None).is_ok());
    let once = parser.rule_names().len();
    assert!(parser.use_plugin(tabnas_expr::plugin(), None).is_ok());
    assert_eq!(once, parser.rule_names().len());
    assert_eq!(
        norm(parse_simplified(&parser, "1+2*3").expect("parses")),
        norm(json!(["+", 1, ["*", 2, 3]]))
    );
}

/// A derived instance rebuilds the grammar against its own options, so it
/// parses the same documents as its parent.
#[test]
fn a_derived_instance_keeps_the_grammar() {
    let parser = parser_for(json!({}));
    let derived = parser
        .derive(|options| options.tag = "derived".into())
        .expect("derives");
    assert_eq!(
        norm(parse_simplified(&derived, "1+2*3").expect("parses")),
        norm(json!(["+", 1, ["*", 2, 3]]))
    );
}

// --- the parsed shape ------------------------------------------------------

/// The parsed AST is TypeScript's: the term list directly, with the
/// operator described by an object whose keys are lower-case.
///
/// The Go port wraps the term list and emits Go field names, which
/// `DIVERGENCE.md` records; this port must not copy either half.
#[test]
fn the_ast_shape_is_the_typescript_one() {
    let value = tabnas_expr::parse("{a:1+2}").expect("parses");
    let doc = value.to_json();
    let term = doc.get("a").expect("the document has an `a` key");

    // The term list directly, not an object wrapping it.
    let terms = term
        .as_array()
        .unwrap_or_else(|| panic!("`a` is {term}, not the term list"));
    assert_eq!(3, terms.len());

    let op = terms[0]
        .as_object()
        .expect("the first term is the operator");
    // Lower-case keys, as TypeScript emits.
    assert_eq!(Some("addition-infix"), op["name"].as_str());
    assert_eq!(Some("+"), op["src"].as_str());
    assert_eq!(Some(2000000.0), op["left"].as_f64());
    assert_eq!(Some(2100000.0), op["right"].as_f64());
    assert!(op.contains_key("terms"));
    assert!(op.contains_key("preval"));
    assert!(!op.contains_key("Name"), "no Go field names");
    assert!(!op.contains_key("Val"), "no Go field names");

    // The operand terms follow the operator.
    assert_eq!(Some(1.0), terms[1].as_f64());
    assert_eq!(Some(2.0), terms[2].as_f64());

    // The operator carries the token it was matched from, so an evaluator
    // can report where a term came from.
    let token = op["token"].as_object().expect("the operator has a token");
    assert_eq!(Some(4.0), token["sI"].as_f64());
    assert_eq!(Some(1.0), token["rI"].as_f64());
    assert_eq!(Some(5.0), token["cI"].as_f64());
}

/// `simplify` reduces the same tree to the S-expression the shared
/// fixtures compare.
#[test]
fn simplify_reduces_to_source_text() {
    let value = tabnas_expr::parse("{a:1+2}").expect("parses");
    assert_eq!(
        norm(simplify(&value).to_json()),
        norm(json!({"a": ["+", 1, 2]}))
    );
}

/// The convenience parse reuses one instance and realizes its result.
#[test]
fn parse_is_a_convenience_over_one_instance() {
    let first = tabnas_expr::parse("1+2").expect("parses");
    let second = tabnas_expr::parse("a:1+2").expect("parses");
    assert_eq!(norm(simplify(&first).to_json()), norm(json!(["+", 1, 2])));
    assert_eq!(
        norm(simplify(&second).to_json()),
        norm(json!({"a": ["+", 1, 2]}))
    );
    // The first result survives the second parse: it was realized at the
    // parse boundary rather than left as a handle into the arena.
    assert_eq!(norm(simplify(&first).to_json()), norm(json!(["+", 1, 2])));
}

/// A parse driven straight through the engine hands back expression
/// handles, which `realize` turns into the plain arrays a caller reads.
#[test]
fn realize_converts_an_engine_parse() {
    let parser = parser_for(json!({}));
    let raw = parser.parse("1+2").expect("parses");
    assert!(
        is_op(&raw),
        "an unrealized expression is an expression node"
    );
    let value = tabnas_expr::realize(&raw);
    assert_eq!(norm(simplify(&value).to_json()), norm(json!(["+", 1, 2])));
}

/// A handle that escapes `parse_scope` is REFUSED, not quietly reported
/// as an empty expression.
///
/// The closure's return type is a type parameter, so the signature can
/// neither realize what comes back nor stop a handle from being in it.
/// The arena is released as `parse_scope` returns, and a later walk of an
/// escaped handle used to hand back `[]`: a well-formed value that had
/// silently lost the whole expression, which no caller could tell from a
/// parse of an empty document.
#[test]
fn a_handle_that_escapes_the_scope_is_refused() {
    let parser = parser_for(json!({}));

    // Returned directly.
    let escaped = tabnas_expr::parse_scope(&parser, "1+2*3", |value| value).expect("parses");
    let realized = std::panic::catch_unwind(|| tabnas_expr::realize(&escaped));
    assert!(realized.is_err(), "realize must refuse a released handle");

    // Embedded in another return value.
    let wrapped = tabnas_expr::parse_scope(&parser, "1+2*3", |value| vec![value]).expect("parses");
    let simplified = std::panic::catch_unwind(|| simplify(&wrapped[0]));
    assert!(simplified.is_err(), "simplify must refuse it too");

    // Realizing INSIDE the scope is what a caller does instead, and the
    // realized value outlives the arena.
    let kept = tabnas_expr::parse_scope(&parser, "1+2*3", |value| tabnas_expr::realize(&value))
        .expect("parses");
    assert_eq!(
        norm(simplify(&kept).to_json()),
        norm(json!(["+", 1, ["*", 2, 3]]))
    );
}

/// The expression arena is per-thread, so parsers are usable from several
/// threads at once, as the engine's own instances are.
#[test]
fn parsing_is_thread_safe() {
    let parser = std::sync::Arc::new(parser_for(json!({})));
    let mut threads = Vec::new();
    for index in 0..4 {
        let parser = parser.clone();
        threads.push(std::thread::spawn(move || {
            for _ in 0..50 {
                let got = parse_simplified(&parser, "1+2*3").expect("parses");
                assert_eq!(
                    norm(got),
                    norm(json!(["+", 1, ["*", 2, 3]])),
                    "thread {index}"
                );
            }
        }));
    }
    for thread in threads {
        thread.join().expect("the thread finished");
    }
}

// --- untrusted input -------------------------------------------------------

/// An expression past the node limit is refused, rather than building a
/// tree deep enough for the engine's recursive value walks to end the
/// process. The limit and its measurements are in `../DIVERGENCE.md`.
///
/// This fails if the limit is REMOVED as well as if it regresses, which
/// is what keeps the record honest.
#[test]
fn an_expression_past_the_node_limit_is_refused() {
    let parser = parser_for(json!({}));
    let sum = |terms: usize| {
        (0..terms)
            .map(|term| term.to_string())
            .collect::<Vec<_>>()
            .join("+")
    };

    // A flat sum of n terms is n-1 nodes, so the limit allows one more
    // term than it allows nodes.
    let largest = tabnas_expr::NODE_LIMIT + 1;
    assert!(
        parse_simplified(&parser, &sum(largest)).is_ok(),
        "a sum of {largest} terms is within the limit"
    );

    let error =
        parse_simplified(&parser, &sum(largest + 1)).expect_err("a sum past the limit is refused");
    assert_eq!("cancel", error.code, "{error}");

    // Far past it, the refusal is still an error rather than an abort.
    let error = parse_simplified(&parser, &sum(5000)).expect_err("still refused");
    assert_eq!("cancel", error.code);

    // Nesting counts the same way, because a paren expression takes the
    // expression inside it as a term.
    let deep = format!("{}1{}", "(".repeat(500), ")".repeat(500));
    let error = parse_simplified(&parser, &deep).expect_err("deep nesting is refused");
    assert_eq!("cancel", error.code);

    // An UNTERMINATED nesting builds no expression at all, so the node
    // limit never sees it: the rule stack is what bounds it. Without that
    // bound the parse was quadratic in the opener count, which is the
    // super-linear behaviour hostile input must not be able to buy.
    let opens = "(".repeat(8 * tabnas_expr::RULE_LIMIT);
    let error = parse_simplified(&parser, &opens).expect_err("deep openers are refused");
    assert_eq!("cancel", error.code, "{error}");
}

/// Deep nesting, long input, unterminated constructs, empty input and odd
/// characters must not panic, hang or overflow the stack. An error is a
/// fine answer; a crash is not.
#[test]
fn hostile_input_is_refused_rather_than_fatal() {
    let parser = parser_for(json!({}));
    let deep_open = "(".repeat(2000);
    let long_sum = (0..5000)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("+");
    for src in [
        "",
        "(",
        ")",
        "1+",
        "+",
        "((((",
        "1+2)",
        "\u{0}",
        "a\u{7f}b",
        "\u{feff}1+2",
        &deep_open,
        &"(1)".repeat(2000),
        &long_sum,
    ] {
        // Either answer is acceptable; neither may abort the process.
        let _ = parse_simplified(&parser, src);
    }
}
