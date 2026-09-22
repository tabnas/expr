// Composition test: the expr operator plugin layered with the tabnas-debug
// introspection plugin, the Rust half of ts/test/debug-model.test.ts.
//
// The TypeScript suite resolves @tabnas/debug dynamically and SKIPS when
// it is absent. Here tabnas-debug is a dev-dependency on the sibling
// checkout, like every other crate this port takes, so the test FAILS to
// build when the checkout is missing rather than reporting green having
// run nothing, which is the rule the rest of this suite lives by.

use tabnas::Tabnas;
use tabnas_debug::{apply, model, DebugOptions};

/// A jsonic instance with expr installed and the debug plugin layered on
/// top, quiet: introspection only, no `USE:` dump and no tracing, which is
/// what `{ print: false, trace: false }` asks for in TypeScript.
fn build() -> Tabnas {
    let mut parser = tabnas_expr::make();
    apply(&mut parser, DebugOptions::quiet()).expect("the debug plugin installs over expr");
    parser
}

#[test]
fn parses_normally_with_the_debug_plugin_installed() {
    let parser = build();
    let value = tabnas_expr::parse_with(&parser, "1+2*3").expect("a mixed-precedence sum parses");
    assert_eq!(
        tabnas_expr::simplify(&value).to_string(),
        r#"["+",1,["*",2,3]]"#
    );
}

#[test]
fn the_model_is_the_structured_expr_grammar() {
    let parser = build();
    let m = model(&parser);

    // The structured rule set: the shared jsonic rules plus expr's own
    // `expr` and `paren`.
    let mut names: Vec<&str> = m.rules.iter().map(|rule| rule.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["elem", "expr", "list", "map", "pair", "paren", "val"]
    );

    // The entry rule of the grammar.
    assert_eq!(m.config.start, "val");

    // The expr plugin is registered, under the name it declares.
    assert!(
        m.plugins
            .iter()
            .any(|plugin| plugin.name == tabnas_expr::PLUGIN_NAME),
        "plugins should list {}: {:?}",
        tabnas_expr::PLUGIN_NAME,
        m.plugins
            .iter()
            .map(|plugin| &plugin.name)
            .collect::<Vec<_>>()
    );

    // `val` is a choice whose open alts push the expr rule (the operator
    // entry) as well as the shared map and list rules; `expr` pushes
    // `paren` for grouping and back into `val` for its operands; `paren`
    // re-enters `val` for the grouped expression.
    let pushes = |rule: &str, target: &str| {
        m.rules
            .iter()
            .find(|candidate| candidate.name == rule)
            .unwrap_or_else(|| panic!("a {rule} rule"))
            .open
            .iter()
            .any(|alt| alt.push.as_deref() == Some(target))
    };
    for (rule, target) in [
        ("val", "expr"),
        ("val", "map"),
        ("val", "list"),
        ("expr", "paren"),
        ("expr", "val"),
        ("paren", "val"),
    ] {
        assert!(pushes(rule, target), "{rule} should push {target}");
    }
}

#[test]
fn the_model_survives_json() {
    let m = model(&build());
    // The TypeScript half round-trips the rule names and the entry rule
    // through JSON, because the per-rule action functions do not survive
    // it and the structural skeleton is what a consumer reads.
    let round: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&m).expect("the model serialises"))
            .expect("and parses back");

    let mut names: Vec<&str> = round["rules"]
        .as_array()
        .expect("rules is an array")
        .iter()
        .map(|rule| rule["name"].as_str().expect("a rule name"))
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["elem", "expr", "list", "map", "pair", "paren", "val"]
    );
    assert_eq!(round["config"]["start"], "val");
}
