// The tree layer in the language: the Rust half of `ts/test/tree.test.ts`,
// for the behaviour a fixture cannot express, and what is particular to
// this port: the arena-live token operators, re-entry from a resolver, the
// error type and the stack bound.
//
// Not ported, since Rust has nothing they could observe: the deferred mode
// (the plugin's `fold` option, its filter on the engine's rule event, and
// a start rule with no close alternates), `recordPositions`, which this
// crate does not provide, and objects that are not plain records (a
// `Date`, a `Map`, a class instance), which an engine `Value` cannot be.

// A resolver returns the engine's `TabnasError`, which is large by design
// (it carries the whole report); the engine and this crate allow the lint
// at their roots for the same reason.
#![allow(clippy::result_large_err)]

mod common;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;
use serde_json::json;
use tabnas::{MapRef, TabnasError, Token, Value};
use tabnas_expr::{
    evaluation, evaluation_with, fold, is_op, key_op_of, make_key_map, op_of, opify, simplify,
    terms_of, EvalSite, Evaluate, EvaluationOptions, ExprOptions, FoldCycle, FoldDynamic,
    FoldEntry, FoldOptions, FoldReport, Op, OpDef, OpRef, Seg, Site, FOLD_CYCLE, FOLD_DEPTH,
    FOLD_INHERIT, FOLD_KEY,
};

use common::{key_parser, norm, pointer, resolving, table, text_of};

// --- helpers ---------------------------------------------------------------

fn ref_parser() -> tabnas::Tabnas {
    key_parser(&[("ref", OpDef::key("$ref"))])
}

/// A document with key operators and no token operators, realized.
fn parse(src: &str) -> Value {
    tabnas_expr::parse_with(&ref_parser(), src).unwrap_or_else(|error| panic!("{src}: {error}"))
}

/// A parser with the default operators and `$ref`.
fn token_parser() -> tabnas::Tabnas {
    tabnas_expr::make_with(ExprOptions::new().with_op("ref", OpDef::key("$ref")))
}

fn with_ref(options: FoldOptions<'_>) -> FoldOptions<'_> {
    options.with_op("ref", OpDef::key("$ref"))
}

fn data(value: serde_json::Value) -> Value {
    Value::from_json(&value)
}

fn json_of(value: &Value) -> serde_json::Value {
    norm(value.to_json())
}

fn expect(value: serde_json::Value) -> serde_json::Value {
    norm(value)
}

fn member(value: &Value, name: &str) -> Value {
    match value {
        Value::Object(members) => members.get(name).cloned(),
        Value::MapRef(map) => map.value.get(name).cloned(),
        _ => None,
    }
    .unwrap_or(Value::Undefined)
}

fn names(value: &Value) -> Vec<String> {
    match value {
        Value::Object(members) => members.keys().cloned().collect(),
        Value::MapRef(map) => map.value.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

/// The same container, not an equal one.
fn same(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Object(a), Value::Object(b)) => Arc::ptr_eq(a, b),
        (Value::Array(a), Value::Array(b)) => Arc::ptr_eq(a, b),
        (Value::MapRef(a), Value::MapRef(b)) => Arc::ptr_eq(a, b),
        (Value::ListRef(a), Value::ListRef(b)) => Arc::ptr_eq(a, b),
        _ => false,
    }
}

fn joined(path: &[Seg]) -> String {
    path.iter()
        .map(Seg::to_string)
        .collect::<Vec<_>>()
        .join("/")
}

fn number(value: &Value) -> f64 {
    match value {
        Value::Number(number) => *number,
        _ => f64::NAN,
    }
}

/// The folded value a hit carries, or the site's own node.
fn found_or_node(site: &Site<'_>, path: &[Seg], doc: Option<&str>) -> Result<Value, TabnasError> {
    let hit = site.at(path, doc)?;
    Ok(match hit.value {
        Some(value) if hit.found && !hit.cycle => value,
        _ => site.node().clone(),
    })
}

/// A `base` member sets the bag for its subtree.
fn base_inherit(_site: &Site<'_>, node: &Value, k: &Value) -> Result<Value, TabnasError> {
    Ok(match (text_of(&member(node, "base")), k) {
        (Some(base), Value::Object(bag)) => {
            let mut bag = (**bag).clone();
            bag.insert("base".into(), Value::String(base.to_string()));
            Value::object(bag)
        }
        _ => k.clone(),
    })
}

fn report_slot() -> RefCell<Option<FoldReport>> {
    RefCell::new(None)
}

fn taken(slot: RefCell<Option<FoldReport>>) -> FoldReport {
    slot.into_inner().expect("the report was handed over")
}

fn entry(doc: &str, at: &str, target_doc: &str, target: &str) -> FoldEntry {
    FoldEntry {
        doc: doc.into(),
        at: at.into(),
        target_doc: target_doc.into(),
        target: target.into(),
    }
}

fn math() -> Evaluate {
    Arc::new(|_site, op, terms| match (op.src.as_str(), terms) {
        ("+", [left, right]) => Value::Number(number(left) + number(right)),
        _ => Value::Number(f64::NAN),
    })
}

/// `a0` names `a1`, which names `a2`, and so on; the last is `{v: 1}`.
fn chain(length: usize) -> Value {
    let mut members = IndexMap::new();
    for index in 0..length {
        let link = if index + 1 < length {
            json!({ "$ref": format!("#/a{}", index + 1) })
        } else {
            json!({ "v": 1 })
        };
        members.insert(format!("a{index}"), data(link));
    }
    Value::object(members)
}

/// `{c: {c: ... {x: 1}}}`, `depth` maps deep around the innermost.
fn nest(depth: usize) -> Value {
    let mut value = data(json!({ "x": 1 }));
    for _ in 0..depth {
        let mut members = IndexMap::new();
        members.insert("c".to_string(), value);
        value = Value::object(members);
    }
    value
}

// --- fold ------------------------------------------------------------------

#[test]
fn a_fold_leaves_the_raw_tree_as_it_was_and_returns_a_new_one() {
    let raw = parse(r##"{a:{$ref:"#/b"},b:{c:1}}"##);
    let text = raw.to_string();
    let out = fold(&raw, &with_ref(FoldOptions::new(resolving))).expect("folds");
    assert_eq!(raw.to_string(), text);
    assert!(!same(&out, &raw));
    assert_eq!(json_of(&out), expect(json!({"a": {"c": 1}, "b": {"c": 1}})));
    assert!(matches!(out, Value::Object(_)), "the kind of map is kept");
}

#[test]
fn one_target_is_one_folded_object_shared_by_every_site() {
    let raw = parse(r##"{a:{$ref:"#/c"},b:{$ref:"#/c"},c:{d:1}}"##);
    let options = with_ref(FoldOptions::new(|site, _op, terms| {
        let hit = site.at(&pointer(text_of(&terms[0]).unwrap_or("")), None)?;
        Ok(hit.value.unwrap_or(Value::Undefined))
    }));
    let out = fold(&raw, &options).expect("folds");
    assert!(same(&member(&out, "a"), &member(&out, "c")));
    assert!(same(&member(&out, "b"), &member(&out, "c")));
}

#[test]
fn a_pointer_that_ends_at_a_site_and_one_that_passes_through_it() {
    let raw = parse(r##"{a:{$ref:"#/b"},b:{$ref:"#/c"},c:{d:{e:1}},f:{$ref:"#/b/d"}}"##);
    let out = fold(&raw, &with_ref(FoldOptions::new(resolving))).expect("folds");
    assert_eq!(
        json_of(&out),
        expect(
            json!({"a": {"d": {"e": 1}}, "b": {"d": {"e": 1}}, "c": {"d": {"e": 1}}, "f": {"e": 1}})
        )
    );
}

#[test]
fn cycles_are_reported_and_kept_verbatim_or_refused() {
    let src = r##"{a:{$ref:"#/a"},b:{c:{$ref:"#/b"}},x:{$ref:"#/y"},y:{$ref:"#/x"},r:{$ref:"#"}}"##;
    let slot = report_slot();
    let out = fold(
        &parse(src),
        &with_ref(FoldOptions::new(resolving).with_report(|report| {
            *slot.borrow_mut() = Some(report);
        })),
    )
    .expect("folds");
    assert_eq!(json_of(&out), json_of(&parse(src)));
    // A reference INTO a cycle is one: `x` names `y`, whose own resolution
    // met the cycle back to `x`, so both ring members stay verbatim.
    let mut rows: Vec<String> = taken(slot)
        .cycles
        .iter()
        .map(|row| format!("{}>{}", row.at, row.target))
        .collect();
    rows.sort();
    assert_eq!(rows, ["/a>/a", "/b/c>/b", "/r>", "/x>/y", "/y>/x"]);

    let error = fold(
        &parse(r##"{a:{$ref:"#/a"}}"##),
        &with_ref(FoldOptions::new(resolving).with_cycle(FoldCycle::Error)),
    )
    .expect_err("a cycle under FoldCycle::Error fails");
    assert_eq!(FOLD_CYCLE, error.code());
    assert_eq!(Some("/a"), error.at());
    assert!(
        error.message().contains(r#"cycle at "/a""#),
        "{}",
        error.message()
    );
    assert!(error.cause().is_none(), "the fold's own failure");
}

#[test]
fn a_target_that_names_nothing_is_reported_and_the_site_kept() {
    let slot = report_slot();
    let out = fold(
        &parse(r##"{a:{$ref:"#/nope/x"}}"##),
        &with_ref(FoldOptions::new(resolving).with_report(|report| {
            *slot.borrow_mut() = Some(report);
        })),
    )
    .expect("folds");
    assert_eq!(json_of(&out), expect(json!({"a": {"$ref": "#/nope/x"}})));
    assert_eq!(taken(slot).unresolved, [entry("", "/a", "", "/nope/x")]);
}

#[test]
fn a_dynamic_key_is_reported_and_folded_as_a_plain_map() {
    let ops = [
        ("ref", OpDef::key("$ref")),
        ("dyn", OpDef::dynamic_key("$dynamicRef")),
    ];
    let raw = tabnas_expr::parse_with(
        &key_parser(&ops),
        r##"{a:{$dynamicRef:"#/b",c:{$ref:"#/b"}},b:1}"##,
    )
    .expect("parses");
    let slot = report_slot();
    let out = fold(
        &raw,
        &FoldOptions::new(resolving)
            .with_ops(table(&ops))
            .with_report(|report| *slot.borrow_mut() = Some(report)),
    )
    .expect("folds");
    assert_eq!(
        json_of(&out),
        expect(json!({"a": {"$dynamicRef": "#/b", "c": 1}, "b": 1}))
    );
    assert_eq!(
        taken(slot).dynamic,
        [FoldDynamic {
            doc: String::new(),
            at: "/a".into(),
            src: "$dynamicRef".into(),
        }]
    );
}

#[test]
fn the_inherited_bag_comes_from_the_targets_own_ancestors() {
    // `d` is folded first and reaches `/b/c` through a reference; the bag
    // at `/b/c` is worked out from `/b`, not from `d`'s ancestors.
    let raw = parse(r##"{d:{$ref:"#/b/c"},base:x,b:{base:y,c:{$ref:r}}}"##);
    let seen = RefCell::new(Vec::new());
    let options = with_ref(
        FoldOptions::new(|site, _op, terms| {
            if let Some(reference) = text_of(&terms[0]).filter(|text| text.starts_with("#/")) {
                let hit = site.at(&pointer(reference), None)?;
                return Ok(hit.value.unwrap_or(Value::Undefined));
            }
            let base = member(&site.k()?, "base");
            seen.borrow_mut().push(format!(
                "{}={}",
                joined(site.path()),
                text_of(&base).unwrap_or("")
            ));
            Ok(terms[0].clone())
        })
        .with_inherit(base_inherit),
    );
    fold(&raw, &options).expect("folds");
    drop(options);
    assert_eq!(seen.into_inner(), ["b/c=y"]);
}

// The canonical composes its eager `evaluate` option with the deferred
// fold; Rust has the function form, over the parse's result.
#[test]
fn eager_evaluation_and_the_fold_compose() {
    let parser = tabnas_expr::make_with(
        ExprOptions::new()
            .with_op("ref", OpDef::key("$ref"))
            .with_evaluate(|_site, op, terms| match (op.src.as_str(), terms) {
                ("+", [left, right]) => Value::Number(number(left) + number(right)),
                _ => Value::Number(f64::NAN),
            }),
    );
    let raw = tabnas_expr::parse_with(&parser, r##"{a:1+2,b:{$ref:"#/a"}}"##).expect("parses");
    let out = fold(&raw, &with_ref(FoldOptions::new(resolving))).expect("folds");
    assert_eq!(json_of(&out), expect(json!({"a": 3, "b": 3})));
}

fn boom() -> TabnasError {
    let mut error = TabnasError::new("resolver_boom", "", "", 0, 1, 1);
    error.detail = "resolver: boom".into();
    error
}

// The canonical's throwing resolver surfaces from `parse`; here it comes
// back out of `fold`, unchanged, and nothing is left behind.
#[test]
fn a_resolver_error_is_returned_unchanged_and_the_parser_survives() {
    let parser = ref_parser();
    let options = with_ref(FoldOptions::new(|_site, _op, _terms| Err(boom())));
    let raw = tabnas_expr::parse_with(&parser, r##"{a:{$ref:"#/b"},b:1}"##).expect("parses");
    let error = fold(&raw, &options).expect_err("the resolver fails");
    assert_eq!("resolver_boom", error.code());
    assert_eq!(Some(&boom()), error.cause());
    assert!(
        error.at().is_none(),
        "a callback's error has no fold location"
    );
    assert_eq!(boom(), TabnasError::from(error));

    // A failure in a resolver the fold reached through another one's
    // `site.at` passes through that one's `?` and comes out as itself.
    let nested = with_ref(FoldOptions::new(|site, op, terms| {
        if [Seg::from("b")] == site.path() {
            return Err(boom());
        }
        resolving(site, op, terms)
    }));
    let raw = parse(r##"{a:{$ref:"#/b"},b:{$ref:"#/c"},c:1}"##);
    let error = fold(&raw, &nested).expect_err("the inner resolver fails");
    assert_eq!(Some(&boom()), error.cause());
    assert_eq!(boom(), TabnasError::from(error));

    let raw = tabnas_expr::parse_with(&parser, "{b:1}").expect("the parser still parses");
    let out = fold(&raw, &with_ref(FoldOptions::new(resolving))).expect("folds");
    assert_eq!(json_of(&out), expect(json!({"b": 1})));
}

#[test]
fn a_member_named_proto_is_data_in_the_folded_tree() {
    let raw = parse(r##"{"__proto__":{x:1},a:{$ref:"#/__proto__"}}"##);
    let out = fold(&raw, &with_ref(FoldOptions::new(resolving))).expect("folds");
    assert_eq!(names(&out), ["__proto__", "a"]);
    assert_eq!(json_of(&member(&out, "a")), expect(json!({"x": 1})));
}

// Neither declaration order nor member order decides: static before
// dynamic, then code points.
#[test]
fn a_map_that_owns_two_key_members_is_static_first_then_by_code_point() {
    let keys = make_key_map(&table(&[
        ("z", OpDef::key("$z")),
        ("dyn", OpDef::dynamic_key("$a")),
        ("two", OpDef::key("2")),
        ("astral", OpDef::key("\u{1F600}")),
        ("bmp", OpDef::key("\u{FF61}")),
    ]))
    .expect("a valid key table");
    let order: Vec<&str> = keys.iter().map(|op| op.src.as_str()).collect();
    assert_eq!(order, ["$z", "2", "\u{FF61}", "\u{1F600}", "$a"]);
    let src = |node: serde_json::Value| key_op_of(&keys, &data(node)).map(|op| op.src.clone());
    assert_eq!(Some("$z".into()), src(json!({"$a": 1, "$z": 2, "2": 3})));
    assert_eq!(Some("2".into()), src(json!({"$a": 1, "2": 3})));
    assert_eq!(
        Some("dyn-key".to_string()),
        key_op_of(&keys, &data(json!({"$a": 1}))).map(|op| op.name.clone())
    );
    assert_eq!(None, src(json!([{"$z": 1}])));
    assert_eq!(None, src(json!("x")));
}

#[test]
fn two_key_operators_on_one_member_name_are_refused() {
    let error = make_key_map(&table(&[
        ("ref", OpDef::key("$ref")),
        ("again", OpDef::key("$ref")),
    ]))
    .expect_err("one member, two operators");
    assert_eq!(FOLD_KEY, error.code());
    let error = make_key_map(&table(&[(
        "none",
        OpDef {
            key: true,
            ..Default::default()
        },
    )]))
    .expect_err("a key operator naming no member");
    assert_eq!(FOLD_KEY, error.code());

    // At install, as the canonical plugin refuses the table there.
    let mut parser = tabnas_jsonic::make();
    let refused = tabnas_expr::apply(
        &mut parser,
        ExprOptions::new()
            .with_op("a", OpDef::key("x"))
            .with_op("b", OpDef::key("x")),
    )
    .expect_err("the plugin refuses the table");
    assert!(refused.to_string().contains("fold_key"), "{refused}");

    // And by the fold, before it reads a member.
    let options = FoldOptions::new(resolving)
        .with_op("a", OpDef::key("x"))
        .with_op("b", OpDef::key("x"));
    let error = fold(&data(json!({})), &options).expect_err("the fold refuses it too");
    assert_eq!(FOLD_KEY, error.code());
}

// --- evaluation over objects -----------------------------------------------

#[test]
fn evaluation_passes_a_map_through_by_default() {
    tabnas_expr::parse_scope(&tabnas_expr::make(), "{a:1+2}", |tree| {
        let out = evaluation(&mut EvalSite::detached(), &tree, &math());
        assert!(same(&out, &tree));
        assert_eq!(json_of(&simplify(&out)), expect(json!({"a": ["+", 1, 2]})));
    })
    .expect("parses");
}

#[test]
fn evaluation_descends_into_maps_when_asked_into_new_ones() {
    tabnas_expr::parse_scope(&tabnas_expr::make(), "{a:1+2,b:{c:[3+4]}}", |tree| {
        let out = evaluation_with(
            &mut EvalSite::detached(),
            &tree,
            &math(),
            EvaluationOptions { objects: true },
        );
        assert!(!same(&out, &tree));
        assert_eq!(json_of(&out), expect(json!({"a": 3, "b": {"c": [7]}})));
        assert_eq!(
            json_of(&simplify(&tree)),
            expect(json!({"a": ["+", 1, 2], "b": {"c": [["+", 3, 4]]}})),
            "the input is untouched"
        );
    })
    .expect("parses");
}

#[test]
fn a_tree_passed_through_once_is_still_reducible() {
    tabnas_expr::parse_scope(&tabnas_expr::make(), "{a:1+2}", |tree| {
        evaluation(&mut EvalSite::detached(), &tree, &math());
        let out = evaluation_with(
            &mut EvalSite::detached(),
            &tree,
            &math(),
            EvaluationOptions { objects: true },
        );
        assert_eq!(json_of(&out), expect(json!({"a": 3})));
    })
    .expect("parses");
}

#[test]
fn a_hand_built_operator_node_is_recognised_by_is_op_and_evaluation() {
    let op = OpRef::new(opify(Op {
        name: "ref-key".into(),
        src: "$ref".into(),
        terms: 2,
        key: true,
        ..Default::default()
    }));
    let node = op.node(vec![Value::String("x".into()), data(json!({}))]);
    assert!(is_op(&node));
    assert!(!is_op(&data(json!(["$ref", "x", {}]))));
    let echo: Evaluate = Arc::new(|_site, op, terms| {
        let mut out = vec![Value::String(op.src.clone())];
        out.extend(terms.iter().cloned());
        Value::array(out)
    });
    let out = evaluation(&mut EvalSite::detached(), &node, &echo);
    assert_eq!(json_of(&out), expect(json!(["$ref", "x", {}])));
}

// --- token terms, rings and a __proto__ key --------------------------------

// A token operator is recognised only on a live expression node, so these
// fold inside `parse_scope`, and realize what they keep before it returns.
#[test]
fn a_site_inside_a_token_expression_is_folded_at_its_own_path() {
    let paths = RefCell::new(Vec::new());
    let out = tabnas_expr::parse_scope(&token_parser(), r##"{a:5,e:1+2*{$ref:"#/a"}}"##, |raw| {
        let options = with_ref(FoldOptions::new(|site, op, terms| {
            if op.key {
                paths.borrow_mut().push(site.path().to_vec());
                let hit = site.at(&pointer(text_of(&terms[0]).unwrap_or("")), None)?;
                return Ok(hit.value.unwrap_or(Value::Undefined));
            }
            let (left, right) = (number(&terms[0]), number(&terms[1]));
            Ok(Value::Number(if "+" == op.src {
                left + right
            } else {
                left * right
            }))
        }));
        fold(&raw, &options).map(|out| tabnas_expr::realize(&out))
    })
    .expect("parses")
    .expect("folds");
    assert_eq!(
        paths.into_inner(),
        [vec![Seg::from("e"), Seg::Index(2), Seg::Index(2)]]
    );
    assert_eq!(json_of(&out), expect(json!({"a": 5, "e": 11})));
}

#[test]
fn a_pointer_into_a_ring_is_a_cycle_in_every_member_order() {
    for src in [
        r##"{a:{$ref:"#/b/x"},b:{$ref:"#/c",x:7},c:{$ref:"#/b"}}"##,
        r##"{b:{$ref:"#/c",x:7},c:{$ref:"#/b"},a:{$ref:"#/b/x"}}"##,
    ] {
        let slot = report_slot();
        let out = fold(
            &parse(src),
            &with_ref(FoldOptions::new(resolving).with_report(|report| {
                *slot.borrow_mut() = Some(report);
            })),
        )
        .expect("folds");
        assert_eq!(
            json_of(&member(&out, "a")),
            expect(json!({"$ref": "#/b/x"})),
            "{src}"
        );
        assert!(
            taken(slot)
                .cycles
                .iter()
                .any(|row| "/a" == row.at && "/b" == row.target),
            "{src}"
        );
    }
}

#[test]
fn a_key_operator_may_be_named_proto() {
    let calls = RefCell::new(Vec::new());
    let options = FoldOptions::new(|site, op, terms| {
        calls.borrow_mut().push((op.src.clone(), terms[0].clone()));
        Ok(site.node().clone())
    })
    .with_op("p", OpDef::key("__proto__"));
    fold(&data(json!({"name": "ordinary", "src": "data"})), &options).expect("folds");
    assert!(
        calls.borrow().is_empty(),
        "a map owning a member of Op is not a site"
    );
    fold(&data(json!({"__proto__": 42, "k": 1})), &options).expect("folds");
    drop(options);
    assert_eq!(
        calls.into_inner(),
        [("__proto__".to_string(), Value::Number(42.0))]
    );
}

// --- the second review -----------------------------------------------------

#[test]
fn array_indices_are_rfc_6901() {
    let hit = |seg: Seg| -> String {
        let got = RefCell::new(None);
        let options = with_ref(FoldOptions::new(|site, _op, _terms| {
            *got.borrow_mut() = Some(site.at(&[Seg::from("b"), seg.clone()], None)?);
            Ok(Value::Number(1.0))
        }));
        let doc = data(json!({"b": ["zero", "one", "two"], "a": {"$ref": "x"}}));
        fold(&doc, &options).expect("folds");
        drop(options);
        let got = got.into_inner().expect("the resolver ran");
        match got.value {
            Some(value) if got.found => text_of(&value).unwrap_or("?").to_string(),
            _ => "-".to_string(),
        }
    };
    assert_eq!(
        ["0", "1", "2"].map(|seg| hit(Seg::from(seg))),
        ["zero", "one", "two"]
    );
    assert_eq!(hit(Seg::Index(1)), "one");
    for seg in [
        "", " ", "01", "1.0", "1e0", "0x1", "+1", "-0", "-", "3", "length",
    ] {
        assert_eq!(hit(Seg::from(seg)), "-", "{seg:?}");
    }
    assert_eq!(hit(Seg::Index(3)), "-");

    let located = RefCell::new(None);
    let options = with_ref(FoldOptions::new(|site, _op, _terms| {
        *located.borrow_mut() = Some(site.locate(&[Seg::from("b"), Seg::from("01")], None)?);
        Ok(Value::Number(1.0))
    }));
    fold(
        &data(json!({"b": ["zero", "one", "two"], "a": {"$ref": "x"}})),
        &options,
    )
    .expect("folds");
    drop(options);
    assert_eq!(located.into_inner(), Some(None));
}

#[test]
fn a_cycle_hit_never_carries_a_value_on_any_branch() {
    let hits = RefCell::new(Vec::new());
    let options = with_ref(FoldOptions::new(|site, _op, terms| {
        let hit = site.at(&pointer(text_of(&terms[0]).unwrap_or("")), None)?;
        hits.borrow_mut().push(hit.clone());
        Ok(match hit.value {
            Some(value) if hit.found && !hit.cycle => value,
            _ => site.node().clone(),
        })
    }));
    fold(
        &parse(
            r##"{a:{$ref:"#/a"},b:{$ref:"#/a"},x:{$ref:"#/y"},y:{$ref:"#/x"},z:{$ref:"#/x/q"}}"##,
        ),
        &options,
    )
    .expect("folds");
    drop(options);
    let hits = hits.into_inner();
    assert!(hits.len() >= 5, "{hits:?}");
    for hit in hits {
        assert!(hit.cycle, "{hit:?}");
        assert!(hit.value.is_none(), "{hit:?}");
    }
}

#[test]
fn the_bag_at_a_site_includes_its_own_map_and_inherit_must_return_one() {
    let out = fold(
        &parse("{base:x,a:{$ref:r,base:z}}"),
        &with_ref(
            FoldOptions::new(|site, _op, _terms| Ok(member(&site.k()?, "base")))
                .with_inherit(base_inherit),
        ),
    )
    .expect("folds");
    assert_eq!(member(&out, "a"), Value::String("z".into()));

    for bad in [Value::Undefined, Value::Null, Value::Number(1.0)] {
        let error = fold(
            &parse("{a:{b:{$ref:x}}}"),
            &with_ref(
                FoldOptions::new(|site, _op, _terms| Ok(site.k()?))
                    .with_inherit(move |_site, _node, _k| Ok(bad.clone())),
            ),
        )
        .expect_err("inherit returned no bag");
        assert_eq!(FOLD_INHERIT, error.code());
    }

    // The root's `inherit` folds `q`, whose bag needs the root's: refused.
    let error = fold(
        &parse("{a:{b:{$ref:x}},q:{$ref:y}}"),
        &with_ref(
            FoldOptions::new(|site, _op, _terms| Ok(site.k()?)).with_inherit(|site, _node, k| {
                if site.path().is_empty() {
                    site.at(&[Seg::from("q")], None)?;
                }
                Ok(k.clone())
            }),
        ),
    )
    .expect_err("inherit folded its own subtree");
    assert_eq!(FOLD_INHERIT, error.code());
}

#[test]
fn report_rows_are_kept_once_each() {
    let slot = report_slot();
    let options = with_ref(
        FoldOptions::new(|site, _op, terms| {
            let path = pointer(text_of(&terms[0]).unwrap_or(""));
            site.at(&path, None)?;
            site.at(&path, None)?;
            Ok(site.node().clone())
        })
        .with_report(|report| *slot.borrow_mut() = Some(report)),
    );
    fold(
        &data(json!({"a": {"$ref": "#/nope"}, "c": {"$ref": "#/c"}})),
        &options,
    )
    .expect("folds");
    drop(options);
    let report = taken(slot);
    assert_eq!(report.unresolved, [entry("", "/a", "", "/nope")]);
    assert_eq!(report.cycles, [entry("", "/c", "", "/c")]);
}

// Node -> Tree, Tree/properties/child -> Node: a cycle through a container
// is cut once, at the site whose target was being folded, in either member
// order; only the place of the cut follows the walk.
#[test]
fn a_cycle_through_a_container_is_cut_once_in_either_member_order() {
    let tree = r##"{properties:{child:{$ref:"#/Node"}}}"##;
    for (src, cut) in [
        (
            format!(r##"{{Node:{{$ref:"#/Tree"}},Tree:{tree}}}"##),
            "/Tree/properties/child>/Node",
        ),
        (
            format!(r##"{{Tree:{tree},Node:{{$ref:"#/Tree"}}}}"##),
            "/Node>/Tree",
        ),
    ] {
        let slot = report_slot();
        let out = fold(
            &parse(&src),
            &with_ref(FoldOptions::new(resolving).with_report(|report| {
                *slot.borrow_mut() = Some(report);
            })),
        )
        .expect("folds");
        let rows: Vec<String> = taken(slot)
            .cycles
            .iter()
            .map(|row| format!("{}>{}", row.at, row.target))
            .collect();
        assert_eq!(rows, [cut], "{src}");
        assert!(out.to_string().contains("properties"), "{src}");
    }
}

#[test]
fn a_pointer_through_a_site_that_was_cut_is_a_cycle_not_a_miss() {
    let slot = report_slot();
    let out = fold(
        &parse(r##"{Tree:{properties:{child:{$ref:"#/Node"}}},Node:{$ref:"#/Tree"},Q:{$ref:"#/Node/properties"}}"##),
        &with_ref(FoldOptions::new(resolving).with_report(|report| {
            *slot.borrow_mut() = Some(report);
        })),
    )
    .expect("folds");
    assert_eq!(
        json_of(&member(&out, "Q")),
        expect(json!({"$ref": "#/Node/properties"}))
    );
    let report = taken(slot);
    assert!(report.unresolved.is_empty(), "{:?}", report.unresolved);
    assert!(report
        .cycles
        .iter()
        .any(|row| "/Q" == row.at && "/Node" == row.target));
}

#[test]
fn a_path_through_a_dynamic_map_or_one_recognise_refused_reads_on() {
    let slot = report_slot();
    let options = FoldOptions::new(resolving)
        .with_ops(table(&[
            ("ref", OpDef::key("$ref")),
            ("dyn", OpDef::dynamic_key("$dynamicRef")),
        ]))
        .with_report(|report| *slot.borrow_mut() = Some(report));
    let out = fold(
        &data(json!({"a": {"$dynamicRef": "#/b", "c": {"$ref": "#/a/d"}, "d": 1}, "b": 2})),
        &options,
    )
    .expect("folds");
    drop(options);
    assert_eq!(member(&member(&out, "a"), "c"), Value::Number(1.0));
    assert!(taken(slot).cycles.is_empty());

    let asked = Cell::new(0);
    let slot = report_slot();
    let options = with_ref(
        FoldOptions::new(resolving)
            .with_recognise(|site, _op| {
                let path = site.path();
                if path.is_empty() || [Seg::from("P")] == path {
                    asked.set(asked.get() + 1);
                }
                Ok(path.last() != Some(&Seg::from("P")))
            })
            .with_report(|report| *slot.borrow_mut() = Some(report)),
    );
    let out = fold(
        &data(json!({"P": {"$ref": {"type": "string"}, "other": {"$ref": "#/P/third"}, "third": {"t": 3}}})),
        &options,
    )
    .expect("folds");
    drop(options);
    assert_eq!(
        json_of(&member(&member(&out, "P"), "other")),
        expect(json!({"t": 3}))
    );
    assert!(taken(slot).cycles.is_empty());
    assert_eq!(1, asked.get(), "recognise is asked once per map");
}

/// A reference as a document key and a path: `b.json#/Z`.
fn split(reference: &str) -> (Option<String>, Vec<Seg>) {
    let (doc, fragment) = reference.split_once('#').unwrap_or((reference, ""));
    let doc = (!doc.is_empty()).then(|| doc.to_string());
    (doc, pointer(&format!("#{fragment}")))
}

#[test]
fn several_documents_are_loaded_once_share_targets_and_cycle_across() {
    let docs: HashMap<&str, Value> = HashMap::from([(
        "b.json",
        data(json!({"Z": {"z": 1}, "X": {"$ref": "a.json#/Y"}})),
    )]);
    let loads = RefCell::new(Vec::new());
    let slot = report_slot();
    let options = with_ref(
        FoldOptions::new(|site, _op, terms| {
            let (doc, path) = split(text_of(&terms[0]).unwrap_or(""));
            found_or_node(site, &path, doc.as_deref())
        })
        .with_doc("a.json")
        .with_load(|key, _site| {
            loads.borrow_mut().push(key.to_string());
            Ok(if "a.json" == key {
                None
            } else {
                docs.get(key).cloned()
            })
        })
        .with_report(|report| *slot.borrow_mut() = Some(report)),
    );
    let out = fold(
        &data(json!({
            "P": {"$ref": "b.json#/Z"},
            "Q": {"$ref": "b.json#/Z"},
            "Y": {"$ref": "b.json#/X"},
            "R": {"$ref": "c.json#/W"},
        })),
        &options,
    )
    .expect("folds");
    drop(options);
    assert_eq!(loads.into_inner(), ["b.json", "c.json"]);
    assert!(
        same(&member(&out, "P"), &member(&out, "Q")),
        "one folded object per target, across documents"
    );
    assert_eq!(
        json_of(&member(&out, "Y")),
        expect(json!({"$ref": "b.json#/X"}))
    );
    let report = taken(slot);
    let mut cycles: Vec<String> = report
        .cycles
        .iter()
        .map(|row| format!("{} {} {} {}", row.doc, row.at, row.target_doc, row.target))
        .collect();
    cycles.sort();
    assert_eq!(cycles, ["a.json /Y b.json /X", "b.json /X a.json /Y"]);
    assert_eq!(report.unresolved, [entry("a.json", "/R", "c.json", "/W")]);
}

#[test]
fn recognise_and_plain_a_map_that_owns_ref_need_not_be_a_site() {
    let src = r##"{S:{properties:{$ref:{type:string},other:{$ref:"#/T"}}},T:{t:1},Q:{$ref:"#/S/properties/other"}}"##;
    let calls = RefCell::new(Vec::new());
    let options = with_ref(
        FoldOptions::new(|site, op, terms| {
            calls.borrow_mut().push(joined(site.path()));
            resolving(site, op, terms)
        })
        .with_recognise(|site, _op| Ok(site.path().last() != Some(&Seg::from("properties")))),
    );
    let out = fold(&parse(src), &options).expect("folds");
    drop(options);
    assert_eq!(
        json_of(&member(&member(&out, "S"), "properties")),
        expect(json!({"$ref": {"type": "string"}, "other": {"t": 1}}))
    );
    assert_eq!(json_of(&member(&out, "Q")), expect(json!({"t": 1})));
    let mut calls = calls.into_inner();
    calls.sort();
    assert_eq!(calls, ["Q", "S/properties/other"]);

    // A resolver that finds no operation returns `site.plain()`: the map,
    // folded, members in their own order.
    let plain = fold(
        &parse(r##"{a:{x:{$ref:"#/b"},$ref:42,y:1},b:2}"##),
        &with_ref(FoldOptions::new(|site, op, terms| {
            if text_of(&terms[0]).is_some() {
                resolving(site, op, terms)
            } else {
                Ok(site.plain()?)
            }
        })),
    )
    .expect("folds");
    assert_eq!(names(&member(&plain, "a")), ["x", "$ref", "y"]);
    assert_eq!(
        json_of(&member(&plain, "a")),
        expect(json!({"x": 2, "$ref": 42, "y": 1}))
    );
}

// The canonical records positions with `recordPositions`; this crate
// records none, and `site.token()` at a map is what the host's lookup says.
// At a token operator site it is the occurrence's token, as `EvalSite`
// gives it.
#[test]
fn a_site_token_is_the_hosts_lookup_or_the_operators_occurrence() {
    let seen = RefCell::new(Vec::new());
    tabnas_expr::parse_scope(
        &token_parser(),
        "{\n  a: { $ref: \"#/b\" },\n  b: 1+2\n}",
        |raw| {
            let target = member(&raw, "a");
            let mut token = Token::default();
            token.site.ri = 2;
            token.site.ci = 6;
            let options = with_ref(
                FoldOptions::new(|site, op, terms| {
                    let at = site.token().map(|token| (token.site.ri, token.site.ci));
                    seen.borrow_mut().push((joined(site.path()), at));
                    if op.key {
                        resolving(site, op, terms)
                    } else {
                        Ok(Value::Number(number(&terms[0]) + number(&terms[1])))
                    }
                })
                .with_positions(move |node| same(node, &target).then(|| token.clone())),
            );
            let out = fold(&raw, &options).expect("folds");
            assert_eq!(json_of(&out), expect(json!({"a": 3, "b": 3})));
        },
    )
    .expect("parses");
    assert_eq!(
        seen.into_inner(),
        [
            ("a".to_string(), Some((2, 6))),
            ("b".to_string(), Some((3, 7)))
        ]
    );
}

#[test]
fn tokens_off_keeps_an_operator_node_an_operator_node() {
    tabnas_expr::parse_scope(&token_parser(), r##"{a:1+{$ref:"#/b"},b:2}"##, |raw| {
        let out = fold(
            &raw,
            &with_ref(FoldOptions::new(resolving).with_tokens(false)),
        )
        .expect("folds");
        let (raw_a, out_a) = (member(&raw, "a"), member(&out, "a"));
        assert!(is_op(&out_a));
        assert!(Arc::ptr_eq(
            &op_of(&out_a).expect("an operator"),
            &op_of(&raw_a).expect("an operator")
        ));
        assert_eq!(
            terms_of(&out_a).expect("terms"),
            [Value::Number(1.0), Value::Number(2.0)]
        );
        assert_eq!(
            evaluation(&mut EvalSite::detached(), &out_a, &math()),
            Value::Number(3.0)
        );
    })
    .expect("parses");
}

// The canonical's limits, with room for them: `max_depth` hops on a thread
// with the stack for them, and `max_stack` raised to match.
#[test]
fn depth_a_deep_chain_or_nesting_is_fold_depth_never_a_crash() {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(|| {
            let options = |max_depth: usize| {
                with_ref(
                    FoldOptions::new(resolving)
                        .with_max_depth(max_depth)
                        .with_max_stack(32 << 20),
                )
            };
            let out = fold(&chain(500), &options(1000)).expect("500 hops fold");
            assert_eq!(json_of(&member(&out, "a0")), expect(json!({"v": 1})));
            for length in [1200, 20000] {
                let error = fold(&chain(length), &options(1000)).expect_err("too many hops");
                assert_eq!(FOLD_DEPTH, error.code(), "{length}");
                assert!(
                    error.message().contains("deeper than 1000"),
                    "{length}: {}",
                    error.message()
                );
            }
            let deep = nest(1200);
            let error = fold(&deep, &options(1000)).expect_err("too deep");
            assert_eq!(FOLD_DEPTH, error.code());
            assert!(fold(&deep, &options(1300)).is_ok());
        })
        .expect("a thread")
        .join()
        .expect("no panic");
}

// On a thread with the default 2 MiB of stack and the default options,
// nesting is bounded by `max_depth` alone, since the walk over containers
// costs no stack, and a chain of references by the stack it takes: past
// `max_stack` the fold refuses with `fold_depth` rather than overflow.
#[test]
fn on_a_default_thread_the_stack_bound_refuses_rather_than_aborts() {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(|| {
            let options = with_ref(FoldOptions::new(resolving));
            let out = fold(&nest(998), &options).expect("998 levels of nesting fold");
            // Read down iteratively: `Value`'s `Display` recurses once per
            // level, and 998 of them do not fit this thread in a debug
            // build.
            let mut inner = out;
            for _ in 0..998 {
                inner = member(&inner, "c");
            }
            assert_eq!(json_of(&inner), expect(json!({"x": 1})));
            let out = fold(&chain(100), &options).expect("100 hops fold");
            assert_eq!(json_of(&member(&out, "a0")), expect(json!({"v": 1})));
            // The divergence, pinned: TypeScript folds this chain (its own
            // depth test does), and this port refuses it on the stack bound,
            // well before `max_depth`.
            let error = fold(&chain(500), &options).expect_err("500 hops pass max_stack");
            assert_eq!(FOLD_DEPTH, error.code());
            assert!(
                error.message().contains("the stack ran out"),
                "{}",
                error.message()
            );
            for length in [1000, 5000] {
                let error = fold(&chain(length), &options).expect_err("too many hops");
                assert_eq!(FOLD_DEPTH, error.code(), "{length}");
            }
        })
        .expect("a thread")
        .join()
        .expect("no panic, and no abort");
}

// --- this port -------------------------------------------------------------

// A resolver re-enters the fold through its site: `site.at` folds a target
// that is itself a site, whose resolver does the same, while the outer one
// is still running; and `site.k`, `site.plain` and `site.locate` read the
// fold's state in between. None of it may hold a borrow across a callback.
#[test]
fn a_resolver_that_folds_its_target_re_enters_the_fold() {
    let depth = Cell::new(0);
    let deepest = Cell::new(0);
    let order = RefCell::new(Vec::new());
    let options = with_ref(
        FoldOptions::new(|site, op, terms| {
            depth.set(depth.get() + 1);
            deepest.set(deepest.get().max(depth.get()));
            order.borrow_mut().push(joined(site.path()));
            site.k()?;
            let plain = site.plain()?;
            assert_eq!(names(&plain), names(site.node()));
            assert_eq!(site.locate(&[], None)?.as_ref(), Some(site.root()));
            let out = resolving(site, op, terms);
            depth.set(depth.get() - 1);
            out
        })
        .with_inherit(|_site, _node, k| Ok(k.clone())),
    );
    let out = fold(
        &parse(r##"{a:{$ref:"#/b",x:1},b:{$ref:"#/c"},c:{d:{$ref:"#/e"},f:2},e:3}"##),
        &options,
    )
    .expect("folds");
    drop(options);
    assert_eq!(
        json_of(&out),
        expect(json!({
            "a": {"d": 3, "f": 2, "x": 1},
            "b": {"d": 3, "f": 2},
            "c": {"d": 3, "f": 2},
            "e": 3,
        }))
    );
    assert_eq!(order.into_inner(), ["a", "b", "c/d"]);
    assert_eq!(
        3,
        deepest.get(),
        "each resolution ran inside the one before"
    );
}

// A realized tree, which `parse` and `parse_with` return, carries its
// operators as plain arrays headed by their description. That is data to
// the fold, never an operator: a document can spell the same array.
#[test]
fn a_realized_operator_array_is_plain_data() {
    let calls = Cell::new(0);
    let options = with_ref(FoldOptions::new(|site, op, terms| {
        if !op.key {
            calls.set(calls.get() + 1);
        }
        resolving(site, op, terms)
    }));
    let raw =
        tabnas_expr::parse_with(&token_parser(), r##"{a:1+2,b:{$ref:"#/a"}}"##).expect("parses");
    let out = fold(&raw, &options).expect("folds");
    assert_eq!(
        json_of(&simplify(&member(&out, "a"))),
        expect(json!(["+", 1, 2]))
    );
    assert_eq!(
        json_of(&simplify(&member(&out, "b"))),
        expect(json!(["+", 1, 2]))
    );
    let forged = data(json!({"a": [{"src": "+", "infix": true, "terms": 2}, 1, 2]}));
    let out = fold(&forged, &options).expect("folds");
    assert_eq!(json_of(&out), json_of(&forged));
    drop(options);
    assert_eq!(0, calls.get(), "no token operator was evaluated");
}

#[test]
fn the_realized_description_carries_key_and_dynamic_in_the_canonical_order() {
    let value = tabnas_expr::parse("{a:1+2}").expect("parses");
    let op = value.to_json()["a"][0].clone();
    assert_eq!(op["key"], json!(false));
    assert_eq!(op["dynamic"], json!(false));
    let keys: Vec<&String> = op.as_object().expect("a description").keys().collect();
    let at = |name: &str| keys.iter().position(|key| *key == name).expect(name);
    assert!(at("preval") < at("content"));
    assert!(at("content") < at("separator"));
    assert!(at("separator") < at("cast"));
    assert!(at("cast") < at("key"));
    assert!(at("key") < at("dynamic"));
    assert!(at("dynamic") < at("token"));
    assert_eq!(
        op["content"],
        json!({"required": false, "separated": false})
    );
    assert_eq!(op["separator"], json!(false));
    assert_eq!(op["cast"], json!(false));
}

#[test]
fn a_mapref_is_a_map_to_the_fold_and_keeps_its_kind() {
    let mapref = |members: Vec<(&str, Value)>| {
        let mut meta = IndexMap::new();
        meta.insert("info".to_string(), Value::Bool(true));
        Value::MapRef(Arc::new(MapRef {
            value: members
                .into_iter()
                .map(|(name, value)| (name.to_string(), value))
                .collect(),
            implicit: false,
            meta,
        }))
    };
    let root = mapref(vec![
        (
            "a",
            mapref(vec![
                ("$ref", Value::String("#/b".into())),
                ("c", Value::Number(1.0)),
            ]),
        ),
        ("b", mapref(vec![("d", Value::Number(2.0))])),
    ]);
    let siblings = RefCell::new(None);
    let options = with_ref(FoldOptions::new(|site, op, terms| {
        *siblings.borrow_mut() = Some(terms[1].clone());
        resolving(site, op, terms)
    }));
    let out = fold(&root, &options).expect("folds");
    drop(options);
    let Some(Value::MapRef(siblings)) = siblings.into_inner() else {
        panic!("the siblings of a MapRef site are a MapRef");
    };
    assert_eq!(Some(&Value::Bool(true)), siblings.meta.get("info"));
    let Value::MapRef(folded) = &out else {
        panic!("a MapRef folds to a MapRef: {out:?}");
    };
    assert_eq!(Some(&Value::Bool(true)), folded.meta.get("info"));
    assert_eq!(
        json_of(&out),
        expect(json!({"a": {"d": 2, "c": 1}, "b": {"d": 2}}))
    );
}

// The cycle is raised inside `site.at`, passes through the resolver's `?`
// as a `TabnasError`, and still comes out of the fold with its location.
#[test]
fn a_fold_error_keeps_its_location_and_converts_to_a_tabnas_error() {
    let error = fold(
        &parse(r##"{a:{$ref:"#/a"}}"##),
        &with_ref(FoldOptions::new(resolving).with_cycle(FoldCycle::Error)),
    )
    .expect_err("a cycle");
    assert_eq!(Some(""), error.doc());
    assert_eq!(Some("/a"), error.at());
    assert_eq!(Some(""), error.target_doc());
    assert_eq!(Some("/a"), error.target());
    assert!(std::error::Error::source(&error).is_none());
    let converted = TabnasError::from(error.clone());
    assert_eq!(FOLD_CYCLE, converted.code);
    assert_eq!(error.message(), converted.detail);
    assert_eq!(error.message(), error.to_string());
}

#[test]
fn a_cycle_error_escapes_member_names_and_bounds_the_message() {
    let name = format!("l1\nl2\u{1b}[31m{}", "K".repeat(300));
    let mut members = IndexMap::new();
    members.insert(name.clone(), data(json!({"$ref": "self"})));
    let options = with_ref(
        FoldOptions::new(|site, _op, _terms| {
            let hit = site.at(&[Seg::Name(name.clone())], None)?;
            Ok(hit.value.unwrap_or(Value::Undefined))
        })
        .with_cycle(FoldCycle::Error),
    );
    let error = fold(&Value::object(members), &options).expect_err("a cycle");
    assert_eq!(FOLD_CYCLE, error.code());
    assert_eq!(Some(format!("/{name}").as_str()), error.at());
    assert!(!error.message().contains('\n'));
    assert!(!error.message().contains('\u{1b}'));
    let length = error.message().encode_utf16().count();
    assert!(length < 220, "{length}");
}

// A handle that outlived its parse is refused, as `realize` refuses it,
// rather than folded as an empty list.
#[test]
fn a_handle_that_escaped_its_scope_is_refused() {
    let escaped =
        tabnas_expr::parse_scope(&tabnas_expr::make(), "1+2", |value| value).expect("parses");
    let folded = std::panic::catch_unwind(|| {
        let options = FoldOptions::new(resolving);
        fold(&escaped, &options).map(|_| ())
    });
    assert!(folded.is_err(), "fold must refuse a released handle");
}
