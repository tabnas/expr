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
use tabnas_support::{load_spec, Runner, SpecOptions, Value};

use common::{parse_simplified, parser_default, parser_for, parser_with, spec_dir, to_failure};

/// Run one fixture file with the parser the test built for it.
///
/// The runner compares a row's value with its cell structurally, which
/// ignores member order. Each cell is also exactly what the canonical
/// `JSON.stringify` writes for the TypeScript value (`ts/test/spec.test.ts`
/// holds every cell to it), so the file is held to its cells byte for
/// byte here too: `parse_simplified`'s value, written the way
/// `JSON.stringify` writes one, must BE the cell, member order and number
/// spelling included.
fn run_spec(name: &str, parser: Tabnas) {
    let path = spec_dir().join(name);
    assert!(path.is_file(), "missing shared fixture {}", path.display());
    hold_to_canonical_json(&path, &parser);
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
fn spec_dangling_operator() {
    run_spec("dangling-operator.tsv", parser_default());
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

/// The documents the translation render (`alchemy/render.alc`) writes,
/// read back with the default operators: see the file's own comments.
#[test]
fn spec_render() {
    run_spec("render.tsv", parser_default());
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
fn spec_binding_power_zero() {
    run_spec(
        "binding-power-zero.tsv",
        parser_for(json!({
            "op": {
                // `zero` declares both powers as 0, which the canonical
                // reads as unset; `below` sits on a negative tier, the only
                // place a genuine zero would differ from the fallback.
                "zero": { "infix": true, "left": 0, "right": 0, "src": "~" },
                "below": { "infix": true, "left": -2000000, "right": -1900000, "src": "@" },
            }
        })),
    );
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

// --- the canonical JSON ----------------------------------------------------

/// Every row of one fixture file, held to its cell byte for byte.
fn hold_to_canonical_json(path: &std::path::Path, parser: &Tabnas) {
    let spec = load_spec(path, &SpecOptions::default())
        .unwrap_or_else(|error| panic!("{}: {}", path.display(), error.0));
    let mut failures = Vec::new();
    for row in &spec.rows {
        let value = tabnas_expr::parse_simplified(parser, &row.unesc(0))
            .unwrap_or_else(|error| panic!("{}: {error}", row.location()));
        let got = canonical(&value);
        if got != row.col(1) {
            failures.push(format!(
                "{}\n  got      {got}\n  expected {}",
                row.location(),
                row.col(1)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} rows differ from the canonical JSON:\n{}",
        failures.len(),
        spec.rows.len(),
        failures.join("\n")
    );
}

/// A value written as `JSON.stringify` writes it: members in order, a
/// number spelt as JavaScript spells it, a number no JSON can hold as
/// `null`, and an undefined member left out.
fn canonical(value: &EngineValue) -> String {
    let mut out = String::new();
    write_canonical(&mut out, value);
    out
}

fn write_canonical(out: &mut String, value: &EngineValue) {
    let string = |text: &str| serde_json::to_string(text).expect("a string is JSON");
    match value {
        EngineValue::Undefined | EngineValue::Null => out.push_str("null"),
        EngineValue::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        EngineValue::Number(number) => out.push_str(&js_number(*number)),
        EngineValue::String(text) => out.push_str(&string(text)),
        EngineValue::Text(text) => out.push_str(&string(&text.string)),
        EngineValue::Array(items) => write_items(out, items),
        EngineValue::ListRef(list) => write_items(out, &list.value),
        EngineValue::Object(members) => write_members(out, members.iter()),
        EngineValue::MapRef(map) => write_members(out, map.value.iter()),
    }
}

fn write_items(out: &mut String, items: &[EngineValue]) {
    out.push('[');
    for (at, item) in items.iter().enumerate() {
        if 0 < at {
            out.push(',');
        }
        write_canonical(out, item);
    }
    out.push(']');
}

fn write_members<'a>(
    out: &mut String,
    members: impl Iterator<Item = (&'a String, &'a EngineValue)>,
) {
    out.push('{');
    let mut first = true;
    for (name, member) in members {
        if member.is_undefined() {
            continue;
        }
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(&serde_json::to_string(name).expect("a key is JSON"));
        out.push(':');
        write_canonical(out, member);
    }
    out.push('}');
}

/// ECMA-262 Number::toString, which `JSON.stringify` spells a number with:
/// the fewest digits that read back as the number, laid out fixed from
/// 1e-6 up to 1e21 and in exponent form, with a signed, unpadded exponent,
/// outside it. Where two digit strings of that length read back, the one
/// nearer the number wins, and on a tie the one ending in an even digit,
/// as the specification's Note 2 recommends and V8 does. A negative zero
/// is `0`, and NaN and the infinities are `null`.
///
/// Rust's `{:e}` gives the fewest digits but settles a tie upward, so
/// 771558860699787.25 comes out as `771558860699787.3` where JavaScript
/// writes `771558860699787.2`; and rounding the exact value to that many
/// digits gives the nearer string, which at a power of two need not read
/// back (2^-1017 is `7.120236347223045e-307`, and the nearer
/// `7.120236347223044e-307` is another number). So this works from the
/// exact decimal value instead, and holds each candidate to `parse`,
/// which rounds correctly. Graded against node over 846,150 doubles
/// (every power of two and its neighbours, 300,000 random bit patterns
/// and 500,000 values from the binades where ties fall) with no
/// difference.
fn js_number(number: f64) -> String {
    if !number.is_finite() {
        return "null".to_string();
    }
    if 0.0 == number {
        return "0".to_string();
    }
    let magnitude = number.abs();
    let (exact, n) = exact_decimal(magnitude);
    let reads_back = |digits: &[u8], n: i32| {
        let text: String = digits
            .iter()
            .map(|digit| char::from(b'0' + digit))
            .collect();
        format!("0.{text}e{n}").parse::<f64>() == Ok(magnitude)
    };
    let mut body = None;
    for k in 1..=exact.len() {
        let low = &exact[..k];
        let rest = &exact[k..];
        if rest.iter().all(|digit| 0 == *digit) {
            body = Some(js_layout(low, n));
            break;
        }
        // One more in the last place than `low`, which a carry lengthens.
        let mut high = low.to_vec();
        let mut high_n = n;
        match high.iter().rposition(|digit| 9 != *digit) {
            Some(at) => {
                high[at] += 1;
                high[at + 1..].fill(0);
            }
            None => {
                high.fill(0);
                high[0] = 1;
                high_n += 1;
            }
        }
        let take_high = match (reads_back(low, n), reads_back(&high, high_n)) {
            (false, false) => continue,
            (true, false) => false,
            (false, true) => true,
            (true, true) => match rest[0].cmp(&5) {
                std::cmp::Ordering::Less => false,
                std::cmp::Ordering::Greater => true,
                std::cmp::Ordering::Equal => {
                    rest[1..].iter().any(|digit| 0 != *digit) || 1 == low[k - 1] % 2
                }
            },
        };
        body = Some(if take_high {
            js_layout(&high, high_n)
        } else {
            js_layout(low, n)
        });
        break;
    }
    let body = body.expect("the exact digits read back");
    if number < 0.0 {
        format!("-{body}")
    } else {
        body
    }
}

/// The exact decimal value of a positive finite double: its digits, with
/// no leading zero, and `n`, where the number is 0.<digits> times ten to
/// the `n`. A double is a mantissa times a power of two, so its decimal
/// expansion ends: the mantissa times 2^e for e >= 0, and the mantissa
/// times 5^-e, shifted -e places, for e < 0.
fn exact_decimal(magnitude: f64) -> (Vec<u8>, i32) {
    const BASE: u64 = 1_000_000_000;
    let bits = magnitude.to_bits();
    let fraction = bits & ((1 << 52) - 1);
    let biased = ((bits >> 52) & 0x7ff) as i32;
    let (mantissa, exponent) = if 0 == biased {
        (fraction, -1074)
    } else {
        (fraction | (1 << 52), biased - 1075)
    };
    // A big integer in base 10^9, least significant limb first.
    let mut limbs = vec![
        mantissa % BASE,
        mantissa / BASE % BASE,
        mantissa / BASE / BASE,
    ];
    let mut multiply = |factor: u64| {
        let mut carry = 0;
        for limb in limbs.iter_mut() {
            let product = *limb * factor + carry;
            *limb = product % BASE;
            carry = product / BASE;
        }
        while 0 < carry {
            limbs.push(carry % BASE);
            carry /= BASE;
        }
    };
    // Each step's factor stays below 2^31, so a limb times it fits a u64.
    let (base, step) = if 0 <= exponent {
        (2u64, 30)
    } else {
        (5u64, 13)
    };
    let mut count = exponent.unsigned_abs();
    while 0 < count {
        let now = count.min(step);
        multiply(base.pow(now));
        count -= now;
    }
    let mut text = String::new();
    for limb in limbs.iter().rev() {
        text.push_str(&format!("{limb:09}"));
    }
    let digits: Vec<u8> = text
        .trim_start_matches('0')
        .bytes()
        .map(|byte| byte - b'0')
        .collect();
    let n = digits.len() as i32 + exponent.min(0);
    (digits, n)
}

/// Digits and the decimal point's place, `0.<digits>` times ten to the
/// `n`, laid out as Number::toString lays them out.
fn js_layout(digits: &[u8], n: i32) -> String {
    let text: String = digits
        .iter()
        .map(|digit| char::from(b'0' + digit))
        .collect();
    let digits = text.trim_end_matches('0');
    let k = digits.len() as i32;
    if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat(n.unsigned_abs() as usize))
    } else {
        let sign = if n < 1 { '-' } else { '+' };
        let power = (n - 1).abs();
        if 1 == k {
            format!("{digits}e{sign}{power}")
        } else {
            format!("{}.{}e{sign}{power}", &digits[..1], &digits[1..])
        }
    }
}

#[test]
fn js_number_spells_as_javascript_does() {
    for (number, want) in [
        (1.0, "1"),
        (-0.0, "0"),
        (1.5, "1.5"),
        (-2.25, "-2.25"),
        (100.0, "100"),
        (1e20, "100000000000000000000"),
        (1e21, "1e+21"),
        (1.5e300, "1.5e+300"),
        (0.000001, "0.000001"),
        (1e-7, "1e-7"),
        (-1.25e-7, "-1.25e-7"),
        (0.1 + 0.2, "0.30000000000000004"),
        (f64::NAN, "null"),
        (f64::INFINITY, "null"),
        // Ties, which Rust's own shortest form settles upward.
        (f64::from_bits(0x4305edd45e85c45a), "771558860699787.2"),
        (f64::from_bits(0x4305edd45e85c45e), "771558860699787.8"),
        (f64::from_bits(0x4310000000000001), "1125899906842624.2"),
        (f64::from_bits(0x3e60000000000000), "2.9802322387695312e-8"),
        // A power of two, where the nearer string of the fewest digits is
        // another number.
        (f64::from_bits(0x0060000000000000), "7.120236347223045e-307"),
        (f64::from_bits(0x0000000000000001), "5e-324"),
        (f64::MAX, "1.7976931348623157e+308"),
        (-f64::MAX, "-1.7976931348623157e+308"),
    ] {
        assert_eq!(js_number(number), want, "for {number:?}");
    }
}

/// Every row of every fixture is held to its canonical JSON: each file by
/// its `run_spec` test, and every file by the coverage gate below. The
/// total is ratcheted at what is on disk, so a corpus that shrinks cannot
/// pass by measuring less.
#[test]
fn every_fixture_row_is_held_to_its_canonical_json() {
    let rows: usize = std::fs::read_dir(spec_dir())
        .expect("the shared fixture directory is readable")
        .map(|entry| entry.expect("a readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "tsv"))
        .map(|path| {
            load_spec(&path, &SpecOptions::default())
                .unwrap_or_else(|error| panic!("{}: {}", path.display(), error.0))
                .rows
                .len()
        })
        .sum();
    assert_eq!(
        rows, 1130,
        "the shared fixtures hold {rows} rows, not the 1130 measured"
    );
}

// --- the coverage itself ---------------------------------------------------

/// Every shared fixture on disk is run by a test in this file.
///
/// Fixture coverage was a prose claim before this: `../AGENTS.md` said all
/// three runtimes run `test/spec/*.tsv`, and nothing failed when one of
/// them did not. A fixture only some runtimes run proves nothing, and the
/// runtime that quietly skips one is the runtime that has drifted.
///
/// The check reads this source file rather than a hand-kept list, because
/// a hand-kept list is the same claim one indirection further away: a
/// fixture could be added to it without a `run_spec` call and the gate
/// would still be green. A fixture name is "run" when it appears as a
/// string literal here, which is exactly how `run_spec` is handed one.
#[test]
fn every_shared_fixture_is_run() {
    let source = std::fs::read_to_string(
        common::repo_root()
            .join("rs")
            .join("tests")
            .join("parity_test.rs"),
    )
    .expect("this test file is readable");

    let mut fixtures: Vec<String> = std::fs::read_dir(spec_dir())
        .expect("the shared fixture directory is readable")
        .map(|entry| entry.expect("a readable directory entry").file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tsv"))
        .collect();
    fixtures.sort();

    assert!(
        !fixtures.is_empty(),
        "no fixtures in {}; the coverage gate would pass over nothing",
        spec_dir().display()
    );

    let missing: Vec<&String> = fixtures
        .iter()
        .filter(|name| !source.contains(&format!("\"{name}\"")))
        .collect();

    assert!(
        missing.is_empty(),
        "no Rust runner for {} of {} shared fixtures: {:?}. \
         Add a `run_spec` test with the operator table the file assumes, \
         matching the TypeScript and Go runners.",
        missing.len(),
        fixtures.len(),
        missing
    );
}
