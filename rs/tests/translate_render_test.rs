// The render, run: trees written as expression documents by the parts the
// crate carries (`translate()`), the embedding and then the render, as a
// host composes them for a plain tree, through the `alchemy` command
// (tabnas/alchemy-cli), which this crate does not depend on. Set
// TABNAS_ALCHEMY to the command's path to run these tests; without it each
// test says so on standard error and passes having run nothing, as the
// TypeScript half, `ts/test/translate-render.test.ts`, skips.
//
// What the render writes reads back through `parse_simplified` as the tree
// it was given, under the conventions the manifest's loss list declares. A
// list led by an object whose src a reader would read as an operator that
// is no default one has no document that reads back as it, and the render
// refuses it with TARGET_VALUE_UNREPRESENTABLE, naming where.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{json, Value as Json};
use tabnas::Value;
use tabnas_support::{load_spec, SpecOptions};

use common::{norm, parse_simplified, parser_default, spec_dir};

/// The alchemy command TABNAS_ALCHEMY names, or `None` where it names
/// none, which the test reports as skipped.
fn alchemy(test: &str) -> Option<PathBuf> {
    match std::env::var_os("TABNAS_ALCHEMY").filter(|path| !path.is_empty()) {
        Some(path) => Some(PathBuf::from(path)),
        None => {
            eprintln!("{test}: skipped, TABNAS_ALCHEMY does not name the alchemy command");
            None
        }
    }
}

/// What the render does with a tree: the document it writes, or the
/// failure the command reports.
enum Rendered {
    Text(String),
    Refused { code: String, message: String },
}

/// A directory of its own for each run, since the tests run at once.
static RUNS: AtomicUsize = AtomicUsize::new(0);

/// The embedding and the render over a tree's JSON text, as a host
/// composes them for a plain tree.
fn render(alchemy: &Path, tree: &str) -> Rendered {
    let parts = tabnas_expr::translate().expect("the expression format carries translation parts");
    let render = parts.render.and_then(|part| part.source).expect("a render");
    let embed = parts
        .embed
        .and_then(|part| part.source)
        .expect("an embedding");
    let dir = std::env::temp_dir().join(format!(
        "expr-render-{}-{}",
        std::process::id(),
        RUNS.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).expect("a temporary directory");
    let program = dir.join("translate.alc");
    std::fs::write(
        &program,
        format!("{render}\n{embed}\ndef export [input] (expr-render (expr-embed input))\n"),
    )
    .expect("the program is written");
    let input = dir.join("tree.json");
    std::fs::write(&input, tree).expect("the tree is written");
    let output = Command::new(alchemy)
        .arg("run")
        .arg(&program)
        .arg(&input)
        .output()
        .unwrap_or_else(|error| panic!("{}: {error}", alchemy.display()));
    std::fs::remove_dir_all(&dir).ok();
    if output.status.success() {
        return Rendered::Text(String::from_utf8(output.stdout).expect("the document is UTF-8"));
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let failure: Json = serde_json::from_str(&stderr)
        .unwrap_or_else(|_| panic!("the command failed without a JSON failure: {stderr}"));
    Rendered::Refused {
        code: failure["code"].as_str().unwrap_or_default().to_string(),
        message: failure["message"].as_str().unwrap_or_default().to_string(),
    }
}

/// The text the render writes for a tree, which fails the test where the
/// render refuses it.
fn written(alchemy: &Path, tree: &Json) -> String {
    match render(alchemy, &tree.to_string()) {
        Rendered::Text(text) => text,
        Rendered::Refused { code, message } => panic!("{tree} is refused: {code}: {message}"),
    }
}

/// A document read back as the reader with the default operators reads
/// it, the reader the render writes for.
fn read_back(text: &str) -> Json {
    norm(
        parse_simplified(&parser_default(), text).unwrap_or_else(|error| panic!("{text}: {error}")),
    )
}

/// What the reader reads back from a document the render wrote of a tree,
/// by the loss list: each operator's description as its source text, which
/// `simplify` gives, and a negative number as the operator `-` applied to
/// its magnitude. JSON carries no number that is not finite.
fn reading(tree: &Json) -> Json {
    fn back(value: Json) -> Json {
        match value {
            Json::Number(number) if number.as_f64().is_some_and(|n| n < 0.0) => {
                json!(["-", -number.as_f64().unwrap_or_default()])
            }
            Json::Array(items) => Json::Array(items.into_iter().map(back).collect()),
            Json::Object(members) => Json::Object(
                members
                    .into_iter()
                    .map(|(key, member)| (key, back(member)))
                    .collect(),
            ),
            other => other,
        }
    }
    norm(back(
        tabnas_expr::simplify(&Value::from_json(tree)).to_json(),
    ))
}

/// Where the render refuses a list, and the src it names there.
fn refuses(alchemy: &Path, tree: &str, at: &str, src: &str) {
    match render(alchemy, tree) {
        Rendered::Refused { code, message } => {
            assert_eq!(code, "TARGET_VALUE_UNREPRESENTABLE", "{message}");
            let lead = format!(
                "the object at {at} is the first element of a list and has the src {src}, "
            );
            assert!(
                message.starts_with(&lead),
                "{message:?} does not begin {lead:?}"
            );
        }
        Rendered::Text(text) => panic!("{tree} is written as {text:?}"),
    }
}

/// The tree tabnas-c's reader builds for `int x;`, as alchemy-cli's
/// `translate --from c` reads it: the children of a node begin with a
/// token, an object whose src is the token's text.
const C_INT_X: &str = r#"{"kind":"translation_unit","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"external_declaration","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"declaration_specifiers","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"token","tname":"KW_INT","src":"int","span":{"start":0,"end":3,"line":1,"col":1}}],"trivia":{"leading":[],"trailing":[]}},{"kind":"init_declarator_list","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"init_declarator","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"declarator","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"direct_declarator","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"token","tname":"ID","src":"x","span":{"start":4,"end":5,"line":1,"col":5}}],"trivia":{"leading":[],"trailing":[]},"declaredName":"x"}],"trivia":{"leading":[],"trailing":[]},"declaredName":"x"}],"trivia":{"leading":[],"trailing":[]},"declaredName":"x"}],"trivia":{"leading":[],"trailing":[]}},{"kind":"token","tname":"PUNC_SEMI","src":";","span":{"start":5,"end":6,"line":1,"col":6}}],"trivia":{"leading":[],"trailing":[]},"declKind":"declaration","viaPath":"grammar"}],"trivia":{"leading":[],"trailing":[]}}"#;

#[test]
fn the_render_refuses_a_c_syntax_tree_naming_the_token_and_its_src() {
    let Some(alchemy) = alchemy("the_render_refuses_a_c_syntax_tree_naming_the_token_and_its_src")
    else {
        return;
    };
    refuses(
        &alchemy,
        C_INT_X,
        ".children[0].children[0].children[0]",
        r#""int""#,
    );
}

/// The reader keeps the src of an object at a list's head alone wherever
/// it would read one: the canonical any src that is not empty, zero, false
/// or null, as the render writes it, so a negative number, written with its
/// sign, is an operation there, a list.
#[test]
fn the_render_refuses_a_list_led_by_an_object_with_a_src_the_reader_reads() {
    let Some(alchemy) =
        alchemy("the_render_refuses_a_list_led_by_an_object_with_a_src_the_reader_reads")
    else {
        return;
    };
    for (tree, at, src) in [
        (json!([{"kind": "token", "src": "x"}, 1]), ".[0]", r#""x""#),
        (
            json!({"a": [1, {"b c": [{"src": "q"}]}]}),
            r#".a[1]."b c"[0]"#,
            r#""q""#,
        ),
        (json!(["+", [{"src": "int"}, 1], 2]), ".[1][0]", r#""int""#),
        (
            json!([[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, [{"src": "k"}]]]),
            ".[0][11][0]",
            r#""k""#,
        ),
        (json!([{"src": "NaN"}]), ".[0]", r#""NaN""#),
        (json!([{"src": 1}, 2]), ".[0]", "1"),
        (json!([{"src": -1}]), ".[0]", "-1"),
        (json!([{"src": true}]), ".[0]", "true"),
        (json!([{"src": {"a": 1}}]), ".[0]", r#"{"a":1}"#),
        (json!([{"src": []}]), ".[0]", "[]"),
    ] {
        refuses(&alchemy, &tree.to_string(), at, src);
    }
}

#[test]
fn an_object_whose_src_no_reader_reads_or_that_leads_no_list_is_written_as_it_is() {
    let Some(alchemy) =
        alchemy("an_object_whose_src_no_reader_reads_or_that_leads_no_list_is_written_as_it_is")
    else {
        return;
    };
    for tree in [
        json!([{"a": 1, "src": ""}, 1]),
        json!([{"src": 0}, 1]),
        json!([{"src": false}, 1]),
        json!([{"src": null}, 1]),
        json!({"a": {"src": "x"}}),
        json!([1, {"src": "x"}]),
    ] {
        assert_eq!(
            read_back(&written(&alchemy, &tree)),
            norm(tree.clone()),
            "{tree}"
        );
    }
}

/// Each default operator, as its source text, which `simplify` gives, and
/// as the reader's realized value describes it, an object whose src is
/// that text.
#[test]
fn every_default_operator_is_written_and_reads_back_unchanged() {
    let Some(alchemy) = alchemy("every_default_operator_is_written_and_reads_back_unchanged")
    else {
        return;
    };
    for src in [
        "1+2-3*4/5%6",
        "-(1+2)*+3",
        "()",
        "(1)",
        "8/2%3-1",
        "--1",
        "-+1",
        "1-(2-3)",
        "(1+2)*(3-4)/(5%6)",
        "{a:1+2,b:[3*4,(5)]}",
    ] {
        let tree = read_back(src);
        let own = tabnas_expr::parse(src)
            .unwrap_or_else(|error| panic!("{src}: {error}"))
            .to_json();
        for given in [tree.clone(), own] {
            assert_eq!(read_back(&written(&alchemy, &given)), tree, "{src}");
        }
    }
}

/// Nothing the reader builds is refused. Every shared fixture row's value,
/// as the reader builds it with its file's operators (the row's expected
/// cell), is written in one list after a null, which makes the list no
/// operation, and reads back as itself through the reader the render
/// writes for, the default operators'. So is the realized value of each
/// row the default operators read, its operators described.
#[test]
fn nothing_the_reader_builds_from_the_shared_fixtures_is_refused() {
    let Some(alchemy) = alchemy("nothing_the_reader_builds_from_the_shared_fixtures_is_refused")
    else {
        return;
    };
    let mut paths: Vec<PathBuf> = std::fs::read_dir(spec_dir())
        .expect("the shared fixture directory is readable")
        .map(|entry| entry.expect("a readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "tsv"))
        .collect();
    paths.sort();
    let mut values = vec![Json::Null];
    let mut own = vec![Json::Null];
    for path in &paths {
        let spec = load_spec(path, &SpecOptions::default())
            .unwrap_or_else(|error| panic!("{}: {}", path.display(), error.0));
        for row in &spec.rows {
            values.push(
                serde_json::from_str(row.col(1))
                    .unwrap_or_else(|error| panic!("{}: {error}", row.location())),
            );
            if let Ok(value) = tabnas_expr::parse(&row.unesc(0)) {
                own.push(value.to_json());
            }
        }
    }
    for batch in [Json::Array(values), Json::Array(own)] {
        assert_eq!(read_back(&written(&alchemy, &batch)), reading(&batch));
    }
}
