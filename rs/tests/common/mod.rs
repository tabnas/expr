// Shared test helpers. Cargo compiles this module into EVERY integration
// test binary, so an item only one binary uses is dead code in the
// others; the allow keeps that from being a warning rather than hiding
// anything real.
#![allow(dead_code)]
// The engine's error is large by design (it carries the whole report), and
// the engine allows this lint at its own crate root for the same reason.
#![allow(clippy::result_large_err)]

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use tabnas::{Tabnas, TabnasError, Value};
use tabnas_expr::{ExprOptions, Op, OpDef, PrevalDef, Seg, Site};
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

// --- the tree layer -----------------------------------------------------

/// The default operators, all deleted: the TypeScript tree suites'
/// `NO_DEFAULTS`, so a document holds key operators and nothing else.
pub fn no_defaults() -> ExprOptions {
    [
        "positive",
        "negative",
        "addition",
        "subtraction",
        "multiplication",
        "division",
        "remainder",
        "plain",
    ]
    .into_iter()
    .fold(ExprOptions::new(), |options, name| options.without_op(name))
}

/// An operator table from named definitions, in order.
pub fn table(ops: &[(&str, OpDef)]) -> IndexMap<String, Option<OpDef>> {
    ops.iter()
        .map(|(name, def)| ((*name).to_string(), Some(def.clone())))
        .collect()
}

/// A parser with no default operators and the given key operators.
pub fn key_parser(ops: &[(&str, OpDef)]) -> Tabnas {
    let options = ops.iter().fold(no_defaults(), |options, (name, def)| {
        options.with_op(*name, def.clone())
    });
    tabnas_expr::make_with(options)
}

/// A string's text, quoted or not.
pub fn text_of(value: &Value) -> Option<&str> {
    match value {
        Value::String(text) => Some(text),
        Value::Text(text) => Some(&text.string),
        _ => None,
    }
}

/// A `#/...` fragment as path steps (RFC 6901), as the TypeScript suites
/// read one: `#` is the root.
pub fn pointer(fragment: &str) -> Vec<Seg> {
    let mut rest = fragment.chars();
    rest.next();
    if rest.as_str().is_empty() {
        return Vec::new();
    }
    rest.next();
    rest.as_str()
        .split('/')
        .map(|part| Seg::Name(part.replace("~1", "/").replace("~0", "~")))
        .collect()
}

/// `{ ...target, ...siblings }`: the target's members, then the site's own
/// laid over them.
pub fn spread(target: &Value, siblings: &Value) -> Value {
    let mut out: IndexMap<String, Value> = IndexMap::new();
    for value in [target, siblings] {
        let members = match value {
            Value::Object(members) => (**members).clone(),
            Value::MapRef(map) => map.value.clone(),
            _ => IndexMap::new(),
        };
        for (name, member) in members {
            out.insert(name, member);
        }
    }
    Value::object(out)
}

/// The resolver the pointer fixtures use: a `#/...` target folded through
/// `site.at`, kept verbatim when it names nothing or is a cycle, with the
/// site's own members laid over an object target. A token operator reads
/// as NaN.
pub fn resolving(site: &Site<'_>, op: &Op, terms: &[Value]) -> Result<Value, TabnasError> {
    if !op.key {
        return Ok(Value::Number(f64::NAN));
    }
    let Some(reference) = terms.first().and_then(text_of) else {
        return Ok(site.node().clone());
    };
    let hit = site.at(&pointer(reference), None)?;
    let value = match hit.value {
        Some(value) if hit.found && !hit.cycle => value,
        _ => return Ok(site.node().clone()),
    };
    Ok(match (&value, terms.get(1)) {
        (Value::Object(_) | Value::MapRef(_), Some(siblings)) => spread(&value, siblings),
        _ => value,
    })
}

/// A value written as `JSON.stringify` writes it: members in order, a
/// number spelt as JavaScript spells it, a number no JSON can hold as
/// `null`, and an undefined member left out.
pub fn canonical(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(&mut out, value);
    out
}

fn write_canonical(out: &mut String, value: &Value) {
    let string = |text: &str| serde_json::to_string(text).expect("a string is JSON");
    match value {
        Value::Undefined | Value::Null => out.push_str("null"),
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) => out.push_str(&js_number(*number)),
        Value::String(text) => out.push_str(&string(text)),
        Value::Text(text) => out.push_str(&string(&text.string)),
        Value::Array(items) => write_items(out, items),
        Value::ListRef(list) => write_items(out, &list.value),
        Value::Object(members) => write_members(out, members.iter()),
        Value::MapRef(map) => write_members(out, map.value.iter()),
    }
}

fn write_items(out: &mut String, items: &[Value]) {
    out.push('[');
    for (at, item) in items.iter().enumerate() {
        if 0 < at {
            out.push(',');
        }
        write_canonical(out, item);
    }
    out.push(']');
}

fn write_members<'a>(out: &mut String, members: impl Iterator<Item = (&'a String, &'a Value)>) {
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
pub fn js_number(number: f64) -> String {
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
