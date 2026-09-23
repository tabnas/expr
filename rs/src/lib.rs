/* Copyright (c) 2021-2026 Richard Rodger and other contributors, MIT License */

//! Pratt expression-operator plugin for the
//! [tabnas](https://github.com/tabnas/parser) parser engine, layered on the
//! [jsonic](https://github.com/tabnas/jsonic) relaxed-JSON base grammar.
//!
//! The algorithm is Pratt parsing, and draws heavily from the explanation
//! written by Aleksey Kladov at
//! <https://matklad.github.io/2020/04/13/simple-but-powerful-pratt-parsing.html>.
//! See [`prattify`] for the core.
//!
//! Expressions parse into LISP-style S-expressions: an array whose first
//! element describes the operator and whose remaining elements are the
//! operand terms, so `1+2*3` becomes `["+", 1, ["*", 2, 3]]` once the
//! operator descriptions are reduced to their source text. A caller
//! supplied evaluator can reduce the tree to a value during the parse.
//!
//! TypeScript is canonical: `ts/src/expr.ts` defines the behaviour, the
//! option names, the defaults and the order of alternates. The shared
//! fixtures in `test/spec/*.tsv` are the parity contract across
//! TypeScript, Go and Rust.
//!
//! ```
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let value = tabnas_expr::parse("a:1+2*3")?;
//!     assert_eq!(tabnas_expr::simplify(&value).to_string(),
//!                r#"{"a":["+",1,["*",2,3]]}"#);
//!     Ok(())
//! }
//! ```
//!
//! # Untrusted input
//!
//! A parsed expression is data, never instructions. This plugin reads
//! expression text of unknown provenance and returns S-expressions for a
//! caller supplied evaluator to reduce, and that hand-off is where hostile
//! input could become execution. Treat every operand and operator as
//! untrusted text, and never let a document select which evaluator or
//! operator implementation runs.

// The engine's error type is large by design (it carries the whole
// rendered report), and the engine allows this lint at its own crate root
// for the same reason.
#![allow(clippy::result_large_err)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use indexmap::IndexMap;

use tabnas::{
    AltMatch, AltSpec, Context, ListRef, Plugin, PluginError, Rule, RuleSnapshot, RuleSpec, Tabnas,
    TabnasError, Tin, Token, Value, TIN_CA, TIN_CB, TIN_CL, TIN_CS, TIN_NR, TIN_ST, TIN_TX, TIN_VL,
    TIN_ZZ,
};

/// VERSION is this crate's version. It MUST equal `ts/package.json`
/// "version" and the `version` field in `rs/Cargo.toml`: the release
/// orchestrator rewrites all of them, and `tests/version_test.rs` fails
/// the build if they drift. Mirrors `VERSION` in `ts/src/expr.ts` and
/// `const VERSION` in `go/expr.go`.
pub const VERSION: &str = "0.5.9";

/// The name this plugin registers under.
pub const PLUGIN_NAME: &str = "Expr";

/// The README's Rust examples run as doctests, so a stale one fails the
/// gate rather than misleading the reader. Its `toml` and `bash` fences
/// are skipped; rustdoc runs only the `rust` ones.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme_examples {}

/// The error a failed parse produces, re-exported so callers need not
/// depend on the engine crate directly.
pub use tabnas::TabnasError as ExprError;

/// JavaScript's `Number.MIN_SAFE_INTEGER`, the binding power an operator
/// with no declared `left` takes: it binds loosest on the left.
pub const MIN_SAFE_INTEGER: i64 = -9_007_199_254_740_991;

/// JavaScript's `Number.MAX_SAFE_INTEGER`, the binding power an operator
/// with no declared `right` takes: it binds tightest on the right.
pub const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

// ---------------------------------------------------------------------------
// Operator model
// ---------------------------------------------------------------------------

/// Paren preval configuration: whether a value may precede the opening
/// paren (`foo(1)`, `a[1]`), whether one is required, and which values are
/// allowed to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrevalDef {
    pub active: bool,
    pub required: bool,
    pub allow: Option<Vec<String>>,
}

/// One entry of the `op` option map: the definition of an operator.
///
/// `src` carries one source string for a prefix, suffix or infix
/// operator and two for a ternary (`["?", ":"]`); a paren operator uses
/// `osrc` and `csrc` instead.
#[derive(Debug, Clone, PartialEq)]
pub struct OpDef {
    pub src: Vec<String>,
    pub osrc: String,
    pub csrc: String,
    pub left: Option<i64>,
    pub right: Option<i64>,
    pub prefix: bool,
    pub suffix: bool,
    pub infix: bool,
    pub ternary: bool,
    pub paren: bool,
    pub preval: Option<PrevalDef>,
    /// Custom operator data, handed back to an evaluator as `op.use`.
    pub use_data: Value,
}

impl Default for OpDef {
    fn default() -> Self {
        OpDef {
            src: Vec::new(),
            osrc: String::new(),
            csrc: String::new(),
            left: None,
            right: None,
            prefix: false,
            suffix: false,
            infix: false,
            ternary: false,
            paren: false,
            preval: None,
            use_data: Value::Undefined,
        }
    }
}

impl OpDef {
    /// An infix operator with the given source and binding powers.
    pub fn infix(src: impl Into<String>, left: i64, right: i64) -> Self {
        OpDef {
            src: vec![src.into()],
            infix: true,
            left: Some(left),
            right: Some(right),
            ..Default::default()
        }
    }

    /// A prefix operator with the given source and right binding power.
    pub fn prefix(src: impl Into<String>, right: i64) -> Self {
        OpDef {
            src: vec![src.into()],
            prefix: true,
            right: Some(right),
            ..Default::default()
        }
    }

    /// A suffix operator with the given source and left binding power.
    pub fn suffix(src: impl Into<String>, left: i64) -> Self {
        OpDef {
            src: vec![src.into()],
            suffix: true,
            left: Some(left),
            ..Default::default()
        }
    }

    /// A ternary operator with its two source strings.
    pub fn ternary(first: impl Into<String>, second: impl Into<String>) -> Self {
        OpDef {
            src: vec![first.into(), second.into()],
            ternary: true,
            ..Default::default()
        }
    }

    /// A paren operator with its opening and closing source strings.
    pub fn paren(osrc: impl Into<String>, csrc: impl Into<String>) -> Self {
        OpDef {
            osrc: osrc.into(),
            csrc: csrc.into(),
            paren: true,
            ..Default::default()
        }
    }

    /// Give a paren operator a preval configuration.
    pub fn with_preval(mut self, preval: PrevalDef) -> Self {
        self.preval = Some(preval);
        self
    }

    /// Attach custom operator data, handed back to an evaluator.
    pub fn with_use(mut self, use_data: Value) -> Self {
        self.use_data = use_data;
        self
    }
}

/// The full operator description carried by a parsed expression and handed
/// to an evaluator.
#[derive(Debug, Clone, PartialEq)]
pub struct Op {
    pub name: String,
    pub src: String,
    pub left: i64,
    pub right: i64,
    pub use_data: Value,
    pub prefix: bool,
    pub suffix: bool,
    pub infix: bool,
    pub ternary: bool,
    pub paren: bool,
    pub terms: usize,
    pub tkn: String,
    pub tin: Tin,
    pub osrc: String,
    pub csrc: String,
    pub otkn: String,
    pub otin: Tin,
    pub ctkn: String,
    pub ctin: Tin,
    pub preval: PrevalDef,
    /// Which of a ternary's two source strings this operator is, as the
    /// canonical `use.ternary.opI`. `None` for every other operator.
    pub ternary_index: Option<usize>,
}

impl Default for Op {
    fn default() -> Self {
        Op {
            name: String::new(),
            src: String::new(),
            left: MIN_SAFE_INTEGER,
            right: MAX_SAFE_INTEGER,
            use_data: Value::Undefined,
            prefix: false,
            suffix: false,
            infix: false,
            ternary: false,
            paren: false,
            terms: 0,
            tkn: String::new(),
            tin: -1,
            osrc: String::new(),
            csrc: String::new(),
            otkn: String::new(),
            otin: -1,
            ctkn: String::new(),
            ctin: -1,
            preval: PrevalDef::default(),
            ternary_index: None,
        }
    }
}

impl Op {
    /// The serialized operator description, with the canonical lower-case
    /// key names. `token` is the occurrence's own token, so it is supplied
    /// separately rather than carried on the shared description.
    fn to_value(&self, token: Option<&Token>) -> Value {
        let mut map: IndexMap<String, Value> = IndexMap::new();
        map.insert("src".into(), Value::String(self.src.clone()));
        map.insert("left".into(), number(self.left));
        map.insert("right".into(), number(self.right));
        map.insert("name".into(), Value::String(self.name.clone()));
        map.insert("infix".into(), Value::Bool(self.infix));
        map.insert("prefix".into(), Value::Bool(self.prefix));
        map.insert("suffix".into(), Value::Bool(self.suffix));
        map.insert("ternary".into(), Value::Bool(self.ternary));
        map.insert("tkn".into(), Value::String(self.tkn.clone()));
        map.insert("tin".into(), Value::Number(f64::from(self.tin)));
        map.insert("terms".into(), Value::Number(self.terms as f64));
        map.insert("use".into(), self.use_value());
        map.insert("paren".into(), Value::Bool(self.paren));
        map.insert("osrc".into(), Value::String(self.osrc.clone()));
        map.insert("csrc".into(), Value::String(self.csrc.clone()));
        map.insert("otkn".into(), Value::String(self.otkn.clone()));
        map.insert("ctkn".into(), Value::String(self.ctkn.clone()));
        map.insert("otin".into(), Value::Number(f64::from(self.otin)));
        map.insert("ctin".into(), Value::Number(f64::from(self.ctin)));
        map.insert("preval".into(), self.preval_value());
        map.insert("token".into(), token_value(token));
        Value::object(map)
    }

    fn use_value(&self) -> Value {
        let mut used: IndexMap<String, Value> = match &self.use_data {
            Value::Object(entries) => (**entries).clone(),
            _ => IndexMap::new(),
        };
        if let Some(index) = self.ternary_index {
            let mut ternary: IndexMap<String, Value> = IndexMap::new();
            ternary.insert("opI".into(), Value::Number(index as f64));
            used.insert("ternary".into(), Value::object(ternary));
        }
        Value::object(used)
    }

    fn preval_value(&self) -> Value {
        let mut preval: IndexMap<String, Value> = IndexMap::new();
        preval.insert("active".into(), Value::Bool(self.preval.active));
        preval.insert("required".into(), Value::Bool(self.preval.required));
        if let Some(allow) = &self.preval.allow {
            preval.insert(
                "allow".into(),
                Value::array(allow.iter().cloned().map(Value::String).collect()),
            );
        }
        Value::object(preval)
    }
}

/// A binding power as a `Value`. The powers are integers on a scale whose
/// ends are JavaScript's safe-integer bounds, and every one of them is
/// exactly representable as an `f64`.
fn number(value: i64) -> Value {
    Value::Number(value as f64)
}

/// The occurrence token an operator carries, under the canonical key
/// names, so that an evaluator can report where a term came from.
fn token_value(token: Option<&Token>) -> Value {
    let Some(token) = token else {
        return Value::object(IndexMap::new());
    };
    let mut map: IndexMap<String, Value> = IndexMap::new();
    map.insert("isToken".into(), Value::Bool(true));
    map.insert("name".into(), Value::String(token.name.as_str().into()));
    map.insert("tin".into(), Value::Number(f64::from(token.tin)));
    map.insert("src".into(), Value::String(token.src.as_str().into()));
    map.insert("len".into(), Value::Number(token.len as f64));
    map.insert("sI".into(), Value::Number(token.site.pos as f64));
    map.insert("rI".into(), Value::Number(token.site.ri as f64));
    map.insert("cI".into(), Value::Number(token.site.ci as f64));
    Value::object(map)
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// The site an evaluator is called from: the rule the reduction belongs to
/// and the live parse context, and neither when [`evaluation`] is called on
/// a finished tree.
///
/// The rule is a snapshot rather than the live rule, because the canonical
/// port hands the evaluator the PARENT of the rule that is closing, which
/// this engine exposes as a snapshot. Its state, its `u` flags and its node
/// cell are all reachable through it.
pub struct EvalSite<'a> {
    rule: Option<Rc<RuleSnapshot>>,
    context: Option<&'a mut Context>,
    token: Option<Token>,
}

impl<'a> EvalSite<'a> {
    /// A site with no live parse behind it.
    pub fn detached() -> Self {
        EvalSite {
            rule: None,
            context: None,
            token: None,
        }
    }

    /// A site inside a parse.
    pub fn new(rule: Option<Rc<RuleSnapshot>>, context: &'a mut Context) -> Self {
        EvalSite {
            rule,
            context: Some(context),
            token: None,
        }
    }

    /// The rule the reduction belongs to, when there is one.
    pub fn rule(&self) -> Option<&RuleSnapshot> {
        self.rule.as_deref()
    }

    /// The token this OCCURRENCE of the operator was matched from.
    ///
    /// The `Op` an evaluator receives is the shared description, one per
    /// entry in the operator table, so it cannot say which occurrence is
    /// being reduced. The canonical `makeOp` attaches the token to its
    /// copy of the description for exactly that reason; this port keeps
    /// the token beside the node instead, and hands it over here. The
    /// position is the engine's `Site` triple: `token.site.pos`, `.ri`
    /// (row) and `.ci` (column).
    ///
    /// It is `None` when the node was built without one, which is what
    /// [`prattify`] called directly produces.
    pub fn token(&self) -> Option<&Token> {
        self.token.as_ref()
    }

    /// The live parse context, when there is one.
    pub fn context(&mut self) -> Option<&mut Context> {
        self.context.as_deref_mut()
    }

    /// Whether the rule carries a `u` flag set to `true`. The
    /// `paren_preval` flag is what tells an evaluator that a paren
    /// operator's first term is the preceding value.
    pub fn flag(&self, name: &str) -> bool {
        self.rule
            .as_ref()
            .is_some_and(|rule| matches!(rule.u.get(name), Some(Value::Bool(true))))
    }

    /// Reborrow, so a recursive reduction passes the same site down. The
    /// token is not carried: it belongs to one occurrence, and the nested
    /// reduction sets the one it is reducing.
    fn reborrow(&mut self) -> EvalSite<'_> {
        EvalSite {
            rule: self.rule.clone(),
            context: self.context.as_deref_mut(),
            token: None,
        }
    }
}

/// Resolve the value of an operation. The Rust spelling of the canonical
/// `Evaluate` callback: it takes the operator and its already reduced
/// terms, and returns the value the expression stands for.
pub type Evaluate = Arc<dyn Fn(&mut EvalSite<'_>, &Op, &[Value]) -> Value + Send + Sync>;

/// Options for the plugin.
///
/// `op` is an insertion-ordered map, and the order matters: two operators
/// claiming the same source string resolve to the same token, and the
/// later entry wins, exactly as it does in the canonical TypeScript. A
/// `None` entry deletes an operator the defaults provide.
#[derive(Clone, Default)]
pub struct ExprOptions {
    pub op: IndexMap<String, Option<OpDef>>,
    pub evaluate: Option<Evaluate>,
}

impl fmt::Debug for ExprOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExprOptions")
            .field("op", &self.op)
            .field("evaluate", &self.evaluate.as_ref().map(|_| "<function>"))
            .finish()
    }
}

impl ExprOptions {
    /// Options carrying no operators of their own; the defaults still
    /// apply.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or replace one operator definition.
    pub fn with_op(mut self, name: impl Into<String>, def: OpDef) -> Self {
        self.op.insert(name.into(), Some(def));
        self
    }

    /// Delete a default operator, the Rust spelling of `plain: null`.
    pub fn without_op(mut self, name: impl Into<String>) -> Self {
        self.op.insert(name.into(), None);
        self
    }

    /// Set the evaluator that reduces each expression as it closes.
    pub fn with_evaluate(
        mut self,
        evaluate: impl Fn(&mut EvalSite<'_>, &Op, &[Value]) -> Value + Send + Sync + 'static,
    ) -> Self {
        self.evaluate = Some(Arc::new(evaluate));
        self
    }

    /// Read the operator table out of a serialized option bag, as the
    /// engine hands it to a plugin. Unreadable entries are ignored, the
    /// way the canonical plugin ignores what it cannot use.
    pub fn from_value(options: &Value) -> Self {
        let mut resolved = ExprOptions::new();
        let Some(op) = object_get(options, "op") else {
            return resolved;
        };
        let Value::Object(entries) = op else {
            return resolved;
        };
        for (name, def) in entries.iter() {
            match def {
                Value::Object(_) => {
                    resolved
                        .op
                        .insert(name.clone(), Some(op_def_from_value(def)));
                }
                // `plain: null` deletes a default operator.
                _ => {
                    resolved.op.insert(name.clone(), None);
                }
            }
        }
        resolved
    }
}

fn object_get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => map.get(key),
        Value::MapRef(map) => map.value.get(key),
        _ => None,
    }
}

fn value_bool(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Bool(true)))
}

fn value_i64(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(number)) => Some(*number as i64),
        _ => None,
    }
}

fn value_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Text(text)) => text.string.clone(),
        _ => String::new(),
    }
}

fn op_def_from_value(def: &Value) -> OpDef {
    let mut out = OpDef {
        osrc: value_string(object_get(def, "osrc")),
        csrc: value_string(object_get(def, "csrc")),
        left: value_i64(object_get(def, "left")),
        right: value_i64(object_get(def, "right")),
        prefix: value_bool(object_get(def, "prefix")),
        suffix: value_bool(object_get(def, "suffix")),
        infix: value_bool(object_get(def, "infix")),
        ternary: value_bool(object_get(def, "ternary")),
        paren: value_bool(object_get(def, "paren")),
        use_data: object_get(def, "use").cloned().unwrap_or(Value::Undefined),
        ..Default::default()
    };
    match object_get(def, "src") {
        Some(Value::String(src)) => out.src = vec![src.clone()],
        Some(Value::Array(srcs)) => {
            out.src = srcs
                .iter()
                .map(|src| value_string(Some(src)))
                .collect::<Vec<_>>()
        }
        _ => {}
    }
    out.preval = match object_get(def, "preval") {
        None | Some(Value::Undefined) | Some(Value::Null) => None,
        Some(Value::Bool(active)) => Some(PrevalDef {
            active: *active,
            required: false,
            allow: None,
        }),
        Some(preval) => Some(PrevalDef {
            // True by default when a preval object is specified at all.
            active: match object_get(preval, "active") {
                None | Some(Value::Undefined) | Some(Value::Null) => true,
                Some(Value::Bool(active)) => *active,
                Some(_) => true,
            },
            required: value_bool(object_get(preval, "required")),
            allow: match object_get(preval, "allow") {
                Some(Value::Array(allow)) => Some(
                    allow
                        .iter()
                        .map(|entry| value_string(Some(entry)))
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            },
        }),
    };
    out
}

// Default operator precedence: the "binding power" scale.
//
// Binding powers are compared only by ORDER, never by magnitude (every
// comparison in `prattify` is a `<` or `<=`): a higher number binds
// tighter. Associativity is set by left against right. Left below right is
// left-associative (`a-b-c` is `(a-b)-c`); left above right is
// right-associative. An unset left or right falls back to
// [`MIN_SAFE_INTEGER`] or [`MAX_SAFE_INTEGER`], which is why prefix
// operators give only `right` and parens give neither.
//
// The defaults occupy a compact low block on a base-1000000 ladder,
// leaving the whole range above the built-ins open for client operators.
// Tiers are 1000000 apart; the +100000 on `right` is the
// left-associativity offset, so a tier occupies a 100000-wide band inside
// its 1000000-wide slot and adjacent tiers can never overlap:
//
//    <1000000  looser client operators (assignment, ternary, logical)
//     1000000  sequence and comma
//     2000000  addition and subtraction                  built-in
//     3000000  multiplication, division, remainder       built-in
//     4000000  unary prefix                              built-in, tightest
//     5000000+ free for tighter client operators
/// The default operator table, as the plugin declares it to the engine.
pub fn defaults() -> Value {
    let mut ops: IndexMap<String, Value> = IndexMap::new();
    ops.insert("positive".into(), prefix_value("+", 4_000_000));
    ops.insert("negative".into(), prefix_value("-", 4_000_000));
    // NOTE: all of these are left-associative, as left is below right.
    // Example: 2+3+4 is (2+3)+4.
    ops.insert("addition".into(), infix_value("+", 2_000_000, 2_100_000));
    ops.insert("subtraction".into(), infix_value("-", 2_000_000, 2_100_000));
    ops.insert(
        "multiplication".into(),
        infix_value("*", 3_000_000, 3_100_000),
    );
    ops.insert("division".into(), infix_value("/", 3_000_000, 3_100_000));
    ops.insert("remainder".into(), infix_value("%", 3_000_000, 3_100_000));
    ops.insert("plain".into(), paren_value("(", ")"));

    let mut map: IndexMap<String, Value> = IndexMap::new();
    map.insert("op".into(), Value::object(ops));
    Value::object(map)
}

fn prefix_value(src: &str, right: i64) -> Value {
    let mut def: IndexMap<String, Value> = IndexMap::new();
    def.insert("prefix".into(), Value::Bool(true));
    def.insert("right".into(), number(right));
    def.insert("src".into(), Value::String(src.into()));
    Value::object(def)
}

fn infix_value(src: &str, left: i64, right: i64) -> Value {
    let mut def: IndexMap<String, Value> = IndexMap::new();
    def.insert("infix".into(), Value::Bool(true));
    def.insert("left".into(), number(left));
    def.insert("right".into(), number(right));
    def.insert("src".into(), Value::String(src.into()));
    Value::object(def)
}

fn paren_value(osrc: &str, csrc: &str) -> Value {
    let mut def: IndexMap<String, Value> = IndexMap::new();
    def.insert("paren".into(), Value::Bool(true));
    def.insert("osrc".into(), Value::String(osrc.into()));
    def.insert("csrc".into(), Value::String(csrc.into()));
    Value::object(def)
}

/// The default operator table as typed options.
fn default_ops() -> IndexMap<String, Option<OpDef>> {
    ExprOptions::from_value(&defaults()).op
}

/// Merge caller options over the defaults with the canonical semantics: a
/// name the defaults already carry is deep-merged and moves to the end of
/// the table, a new name is appended, and a `None` entry deletes.
fn resolve_options(options: &ExprOptions) -> IndexMap<String, Option<OpDef>> {
    let mut resolved = default_ops();
    for (name, def) in &options.op {
        resolved.shift_remove(name);
        match def {
            Some(def) => {
                resolved.insert(name.clone(), Some(def.clone()));
            }
            None => {
                resolved.insert(name.clone(), None);
            }
        }
    }
    resolved
}

// ---------------------------------------------------------------------------
// The expression node arena
// ---------------------------------------------------------------------------
//
// The canonical algorithm rewrites expression nodes IN PLACE, so that every
// rule holding a node observes the rewrite: that is what keeps the overall
// AST intact while an expression is still being assembled. JavaScript gets
// that from array identity and the Go port buys it with a `*ListRef` box.
//
// The engine's `Value` is a copy-on-write tree with no interior mutability,
// so a rewrite through one holder would be invisible to the others. Nodes
// therefore live in a per-thread arena and the value that travels through
// the parse is a HANDLE: a `ListRef` carrying the node's identity under
// `meta.expr`. Cloning a handle shares the identity, which is exactly the
// property the algorithm needs, and [`realize`] turns a finished tree of
// handles into plain values at the parse boundary.
//
// A handle is a `ListRef` rather than a string because the base grammar's
// value coalescing asks whether a node is a container, and the canonical
// expression node -- a JavaScript array -- is one.

/// One expression node: the operator, the token that introduced this
/// occurrence, and the operand terms.
///
/// The operator is optional because the canonical port rewrites a
/// completed ternary into a one-element LIST in place, keeping the node's
/// identity so that every holder sees the list. A node with no operator is
/// exactly that: a plain array of its terms.
#[derive(Debug, Clone)]
struct ExprNode {
    op: Option<Arc<Op>>,
    token: Option<Token>,
    terms: Vec<Value>,
    /// The node at the top of the expression this one belongs to, or this
    /// node itself.
    ///
    /// The Pratt core only ever grows a tree DOWNWARD from its root: a
    /// looser operator rewrites the root node in place, keeping its
    /// identity, and a tighter one builds a new node under it. So the
    /// root a node was created under stays its root for the life of the
    /// expression, and recording it is what lets a rule holding a
    /// sub-expression hand the whole expression back.
    root: u64,
    /// How many nodes the expression under this root holds, counted only
    /// on a root. A tree is never deeper than it is large, so this bounds
    /// its depth: see [`NODE_LIMIT`].
    size: usize,
}

#[derive(Default)]
struct Arena {
    nodes: HashMap<u64, ExprNode>,
    next_id: u64,
    /// Parses currently running on this thread through this crate's own
    /// entry points. While one is in flight the arena must not be cleared.
    active: usize,
}

thread_local! {
    static ARENA: RefCell<Arena> = RefCell::new(Arena::default());
    /// Set when an expression outgrows [`NODE_LIMIT`]. The parse budget
    /// reads it and cancels the parse.
    static OVER_LIMIT: Cell<bool> = const { Cell::new(false) };
}

/// The `meta` key a handle carries its node identity under.
const EXPR_META: &str = "expr";

/// The most nodes one expression may hold.
///
/// The engine walks a value with the CALL STACK to display it, convert it
/// to JSON or drop it, so a deeply nested result aborts the process rather
/// than failing: a flat sum of a few thousand terms builds a tree as deep
/// as it is long, and an abort is not something a caller can catch. An
/// expression past this size is refused with the engine's `cancel` code
/// instead, which is a recorded divergence (`../DIVERGENCE.md`):
/// TypeScript raises a JavaScript `RangeError` of its own a few thousand
/// terms later, and Go grows its stacks and keeps going. The number is
/// the one the jsonic base grammar already applies to nested containers,
/// so a document cannot nest deeper than this anyway.
pub const NODE_LIMIT: usize = 127;

/// The deepest rule stack a parse may reach.
///
/// [`NODE_LIMIT`] bounds an expression that is being BUILT, and an
/// unterminated one builds nothing: `((((` with no closer pushes a rule
/// frame per level and never reaches the limit, and the engine's cost per
/// step grows with the stack, so the parse turned quadratic. The base
/// grammar bounds its own nesting for the same reason, and the deepest
/// document it accepts, 127 nested containers, measures 378 frames, so
/// this one sits well above anything it lets through. Past it a parse is
/// refused with the engine's `cancel` code.
pub const RULE_LIMIT: usize = 1024;

/// Read a handle's node identity.
fn node_id(value: &Value) -> Option<u64> {
    let Value::ListRef(list) = value else {
        return None;
    };
    match list.meta.get(EXPR_META) {
        Some(Value::Number(id)) => Some(*id as u64),
        _ => None,
    }
}

/// Whether a value is an expression node, the port of the canonical
/// `isOp`.
pub fn is_op(value: &Value) -> bool {
    op_of(value).is_some()
}

/// The number of elements a node presents, the canonical `node.length`:
/// the operator plus its terms for an expression, and the terms alone for
/// a node rewritten into a plain list.
fn node_length(value: &Value) -> usize {
    if let Some(length) = with_node(value, |entry| {
        entry.terms.len() + usize::from(entry.op.is_some())
    }) {
        return length;
    }
    match value {
        Value::Array(entries) => entries.len(),
        Value::ListRef(list) => list.value.len(),
        _ => 0,
    }
}

fn is_op_kind(value: &Value, kind: fn(&Op) -> bool) -> bool {
    op_of(value).is_some_and(|op| kind(&op))
}

fn is_paren_op(value: &Value) -> bool {
    is_op_kind(value, |op| op.paren)
}

fn is_ternary_op(value: &Value) -> bool {
    is_op_kind(value, |op| op.ternary)
}

/// The operator heading an expression node.
pub fn op_of(value: &Value) -> Option<Arc<Op>> {
    let id = node_id(value)?;
    ARENA.with(|arena| {
        arena
            .borrow()
            .nodes
            .get(&id)
            .and_then(|node| node.op.clone())
    })
}

/// The operand terms of an expression node.
pub fn terms_of(value: &Value) -> Option<Vec<Value>> {
    let id = node_id(value)?;
    ARENA.with(|arena| arena.borrow().nodes.get(&id).map(|node| node.terms.clone()))
}

/// Allocate a node and return its handle. `under` is the expression the
/// node belongs to, when it is built inside one.
fn new_node_under(
    op: Option<Arc<Op>>,
    token: Option<Token>,
    terms: Vec<Value>,
    under: Option<&Value>,
) -> Value {
    let inherited = under.and_then(root_id);
    let terms_taken = terms.clone();
    let id = ARENA.with(|arena| {
        let mut arena = arena.borrow_mut();
        arena.next_id += 1;
        let id = arena.next_id;
        arena.nodes.insert(
            id,
            ExprNode {
                op,
                token,
                terms,
                root: inherited.unwrap_or(id),
                size: 1,
            },
        );
        id
    });
    let handle = handle_for(id);
    if under.is_some() {
        grow(&handle, 1);
    }
    adopt_all(&handle, &terms_taken);
    handle
}

fn new_node(op: Option<Arc<Op>>, token: Option<Token>, terms: Vec<Value>) -> Value {
    new_node_under(op, token, terms, None)
}

/// Count `count` more nodes against the expression `node` belongs to, and
/// record it when the expression has outgrown [`NODE_LIMIT`].
fn grow(node: &Value, count: usize) {
    let Some(root) = root_id(node) else {
        return;
    };
    let over = ARENA.with(|arena| {
        let mut arena = arena.borrow_mut();
        match arena.nodes.get_mut(&root) {
            Some(entry) => {
                entry.size += count;
                entry.size > NODE_LIMIT
            }
            None => false,
        }
    });
    if over {
        OVER_LIMIT.with(|flag| flag.set(true));
    }
}

/// The size of the expression a node belongs to.
fn expression_size(node: &Value) -> usize {
    let Some(root) = root_id(node) else {
        return 0;
    };
    ARENA.with(|arena| {
        arena
            .borrow()
            .nodes
            .get(&root)
            .map_or(0, |entry| entry.size)
    })
}

/// Account for a term taken into an expression. A term that is itself an
/// expression brings its whole tree with it, unless it already belongs to
/// this one.
fn adopt(container: &Value, term: &Value) {
    let (Some(into), Some(from)) = (root_id(container), root_id(term)) else {
        return;
    };
    if into == from {
        return;
    }
    grow(container, expression_size(term));
}

fn adopt_all(container: &Value, terms: &[Value]) {
    for term in terms {
        adopt(container, term);
    }
}

/// The handle for a node identity. Two handles carrying the same identity
/// are the same node, whichever one a rule is holding.
fn handle_for(id: u64) -> Value {
    let mut meta: IndexMap<String, Value> = IndexMap::new();
    meta.insert(EXPR_META.into(), Value::Number(id as f64));
    Value::ListRef(Arc::new(ListRef {
        value: Vec::new(),
        implicit: false,
        child: None,
        meta,
    }))
}

/// The identity of the expression a node belongs to.
fn root_id(value: &Value) -> Option<u64> {
    let id = node_id(value)?;
    ARENA.with(|arena| arena.borrow().nodes.get(&id).map(|node| node.root))
}

/// The whole expression a node belongs to.
fn root_node(value: &Value) -> Value {
    match root_id(value) {
        Some(id) => handle_for(id),
        None => value.clone(),
    }
}

fn with_node<R>(value: &Value, read: impl FnOnce(&ExprNode) -> R) -> Option<R> {
    let id = node_id(value)?;
    ARENA.with(|arena| arena.borrow().nodes.get(&id).map(read))
}

fn with_node_mut<R>(value: &Value, write: impl FnOnce(&mut ExprNode) -> R) -> Option<R> {
    let id = node_id(value)?;
    ARENA.with(|arena| arena.borrow_mut().nodes.get_mut(&id).map(write))
}

/// The port of the canonical `updateExprNode`: rewrite a node in place,
/// keeping its identity, and truncate it to the supplied terms.
fn update_expr_node(node: &Value, op: Option<Arc<Op>>, token: Option<Token>, terms: Vec<Value>) {
    adopt_all(node, &terms);
    with_node_mut(node, |entry| {
        entry.op = op;
        entry.token = token;
        entry.terms = terms;
    });
}

/// The port of the canonical `dupNode`: a shallow copy, sharing the terms
/// with the original but carrying its own identity.
fn dup_node(node: &Value) -> Value {
    let Some((op, token, terms)) = with_node(node, |entry| {
        (entry.op.clone(), entry.token.clone(), entry.terms.clone())
    }) else {
        return node.clone();
    };
    new_node_under(op, token, terms, Some(node))
}

/// `expr[index]` where index 1 is the first term: absent slots read as
/// `Undefined`, as a sparse JavaScript array does.
fn term_at(node: &Value, index: usize) -> Value {
    with_node(node, |entry| {
        entry
            .terms
            .get(index.saturating_sub(1))
            .cloned()
            .unwrap_or(Value::Undefined)
    })
    .unwrap_or(Value::Undefined)
}

/// `expr[index] = value`, extending the node when the slot is past its
/// end, as assigning past a JavaScript array's length does.
fn set_term(node: &Value, index: usize, value: Value) {
    adopt(node, &value);
    with_node_mut(node, |entry| {
        let slot = index.saturating_sub(1);
        while entry.terms.len() <= slot {
            entry.terms.push(Value::Undefined);
        }
        entry.terms[slot] = value;
    });
}

/// `expr.push(value)`.
fn push_term(node: &Value, value: Value) {
    adopt(node, &value);
    with_node_mut(node, |entry| entry.terms.push(value));
}

/// The number of terms a node carries, the canonical `node.length - 1`.
fn term_count(node: &Value) -> usize {
    with_node(node, |entry| entry.terms.len()).unwrap_or(0)
}

/// Refuse a handle whose node the arena no longer holds.
///
/// A node leaves the arena only when a parse releases it wholesale, so a
/// handle that names a missing one is a value that outlived the scope its
/// nodes were live in: taken out of [`parse_scope`], or read straight off
/// [`Tabnas::parse`] and kept across the next parse on this thread. Node
/// identities never repeat, so the handle cannot have been captured by a
/// later parse either: the expression it headed is simply gone.
///
/// This is loud on purpose. Reporting the missing node as an empty array
/// let a walk finish and hand back a well-formed value that had silently
/// lost the whole expression, which is worse than either a correct answer
/// or no answer: nothing downstream could tell the two apart.
fn released_handle(id: u64) -> ! {
    panic!(
        "tabnas-expr: expression node {id} has been released. The value \
         holding this handle outlived its parse: realize it with \
         `tabnas_expr::realize` inside the `parse_scope` closure, or before \
         the next parse on this thread, and let the realized value escape \
         instead."
    )
}

/// Drop every node this thread's arena holds.
///
/// The arena is per-thread and is released when the outermost parse
/// through this crate's own entry points returns, or at the start of the
/// next parse on the thread when a caller drives the engine directly. Its
/// high-water mark is therefore one parse's expression nodes.
fn release_arena() {
    ARENA.with(|arena| {
        let mut arena = arena.borrow_mut();
        if 0 == arena.active {
            arena.nodes.clear();
            EVALUATED.with(|set| set.borrow_mut().clear());
            OVER_LIMIT.with(|flag| flag.set(false));
        }
    });
}

/// Hold the arena open for the duration of one parse, and release it when
/// the outermost one returns.
struct ArenaGuard;

impl ArenaGuard {
    fn enter() -> Self {
        ARENA.with(|arena| arena.borrow_mut().active += 1);
        OVER_LIMIT.with(|flag| flag.set(false));
        ArenaGuard
    }
}

impl Drop for ArenaGuard {
    fn drop(&mut self) {
        ARENA.with(|arena| {
            let mut arena = arena.borrow_mut();
            arena.active = arena.active.saturating_sub(1);
            if 0 == arena.active {
                arena.nodes.clear();
                EVALUATED.with(|set| set.borrow_mut().clear());
                OVER_LIMIT.with(|flag| flag.set(false));
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Realizing and simplifying a parsed tree
// ---------------------------------------------------------------------------

/// Replace every expression handle in a parsed value with the plain array
/// the canonical port returns: the operator description followed by the
/// operand terms.
///
/// A parse through [`parse`], [`parse_with`] or [`parse_simplified`] is
/// realized already. A caller driving a [`Tabnas`] instance directly calls
/// this on the result.
pub fn realize(value: &Value) -> Value {
    let mut open: Vec<u64> = Vec::new();
    realize_seen(value, &mut open)
}

fn realize_seen(value: &Value, open: &mut Vec<u64>) -> Value {
    match value {
        Value::ListRef(list) => {
            let Some(id) = node_id(value) else {
                let mut out = (**list).clone();
                out.value = out
                    .value
                    .iter()
                    .map(|entry| realize_seen(entry, open))
                    .collect();
                return Value::ListRef(Arc::new(out));
            };
            // A rewrite can leave a node reachable from itself; the
            // canonical simplifier reports the same way rather than
            // recursing forever.
            if open.contains(&id) {
                return Value::String("[CIRCLE]".into());
            }
            let Some((op, token, terms)) = with_node(value, |entry| {
                (entry.op.clone(), entry.token.clone(), entry.terms.clone())
            }) else {
                released_handle(id)
            };
            open.push(id);
            let mut out = Vec::with_capacity(terms.len() + 1);
            if let Some(op) = &op {
                out.push(op.to_value(token.as_ref()));
            }
            for term in &terms {
                out.push(realize_seen(term, open));
            }
            open.pop();
            Value::array(out)
        }
        Value::Array(entries) => Value::array(
            entries
                .iter()
                .map(|entry| realize_seen(entry, open))
                .collect(),
        ),
        Value::Object(entries) => Value::object(
            entries
                .iter()
                .map(|(key, entry)| (key.clone(), realize_seen(entry, open)))
                .collect(),
        ),
        Value::MapRef(map) => {
            let mut out = (**map).clone();
            out.value = out
                .value
                .iter()
                .map(|(key, entry)| (key.clone(), realize_seen(entry, open)))
                .collect();
            Value::MapRef(Arc::new(out))
        }
        other => other.clone(),
    }
}

/// Reduce a parsed value to the S-expression form the shared fixtures
/// compare: every operator description becomes its source text, and a
/// term that was never filled is dropped.
///
/// This is the Rust spelling of the canonical `S` helper the TypeScript
/// suite uses and of the Go port's `Simplify`.
pub fn simplify(value: &Value) -> Value {
    let mut open: Vec<u64> = Vec::new();
    simplify_seen(value, &mut open)
}

fn simplify_seen(value: &Value, open: &mut Vec<u64>) -> Value {
    match value {
        Value::ListRef(list) => {
            let Some(id) = node_id(value) else {
                return Value::array(
                    list.value
                        .iter()
                        .map(|entry| simplify_seen(entry, open))
                        .collect(),
                );
            };
            if open.contains(&id) {
                return Value::String("[CIRCLE]".into());
            }
            let Some((op, terms)) =
                with_node(value, |entry| (entry.op.clone(), entry.terms.clone()))
            else {
                released_handle(id)
            };
            open.push(id);
            let mut out = Vec::with_capacity(terms.len() + 1);
            if let Some(op) = &op {
                // A paren operator's `src` is its opening source, so one
                // field covers both, as it does in the canonical port.
                out.push(Value::String(op.src.clone()));
            }
            for term in &terms {
                // An unfilled slot is `undefined` in the canonical port,
                // and the canonical simplifier filters those out.
                if term.is_undefined() {
                    continue;
                }
                out.push(simplify_seen(term, open));
            }
            open.pop();
            Value::array(out)
        }
        // A REALIZED expression: the operator description is an object
        // carrying its source text, which is what the canonical
        // simplifier reads (`x[0].src`).
        Value::Array(entries) if realized_source(entries.first()).is_some() => {
            let mut out = Vec::with_capacity(entries.len());
            out.push(Value::String(
                realized_source(entries.first()).unwrap_or_default(),
            ));
            for entry in entries.iter().skip(1) {
                if entry.is_undefined() {
                    continue;
                }
                out.push(simplify_seen(entry, open));
            }
            Value::array(out)
        }
        Value::Array(entries) => Value::array(
            entries
                .iter()
                .map(|entry| simplify_seen(entry, open))
                .collect(),
        ),
        Value::Object(entries) => Value::object(
            entries
                .iter()
                .map(|(key, entry)| (key.clone(), simplify_seen(entry, open)))
                .collect(),
        ),
        Value::MapRef(map) => Value::object(
            map.value
                .iter()
                .map(|(key, entry)| (key.clone(), simplify_seen(entry, open)))
                .collect(),
        ),
        Value::Text(text) => Value::String(text.string.clone()),
        other => other.clone(),
    }
}

/// The source text of a realized operator description, when a value is
/// one.
fn realized_source(value: Option<&Value>) -> Option<String> {
    match object_get(value?, "src") {
        Some(Value::String(src)) if !src.is_empty() => Some(src.clone()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// The Pratt core
// ---------------------------------------------------------------------------

/// One occurrence of an operator: the shared description and the token
/// that introduced it. The canonical `makeOp` builds the same pair by
/// copying the description and attaching the token.
#[derive(Debug, Clone)]
pub struct OpRef {
    pub op: Arc<Op>,
    pub token: Option<Token>,
}

impl OpRef {
    /// An occurrence with no token behind it, for direct use of
    /// [`prattify`].
    pub fn new(op: Op) -> Self {
        OpRef {
            op: Arc::new(opify(op)),
            token: None,
        }
    }

    fn at(op: &Arc<Op>, token: Option<Token>) -> Self {
        OpRef {
            op: op.clone(),
            token,
        }
    }

    /// A fresh expression node headed by this operator.
    pub fn node(&self, terms: Vec<Value>) -> Value {
        new_node(Some(self.op.clone()), self.token.clone(), terms)
    }

    /// A fresh expression node built inside an existing expression.
    fn node_under(&self, terms: Vec<Value>, under: &Value) -> Value {
        new_node_under(
            Some(self.op.clone()),
            self.token.clone(),
            terms,
            Some(under),
        )
    }
}

/// Normalize an operator built by hand, filling the derived term count
/// when it is unset, as the parser's own operator builder does. The Rust
/// spelling of the canonical `testing.opify`.
pub fn opify(mut op: Op) -> Op {
    if 0 == op.terms {
        op.terms = if op.ternary {
            3
        } else if op.infix {
            2
        } else {
            1
        };
    }
    op
}

/// Embed a new operator into an expression tree, in place, according to
/// operator precedence.
///
/// The node keeps its identity, so every rule holding it observes the
/// rewrite: that is what preserves the referential integrity of the root
/// expression while it is still being assembled. The returned handle is
/// the sub-expression the new operator now heads, which is where its next
/// term belongs.
pub fn prattify(expr: &Value, op: &OpRef) -> Value {
    let mut out = expr.clone();
    let Some(expr_op) = op_of(expr) else {
        return out;
    };
    let new = &op.op;

    if new.infix {
        if expr_op.suffix || new.left <= expr_op.right {
            // The new operator is looser: it takes the whole expression
            // built so far as its first term.
            let prior = dup_node(expr);
            update_expr_node(expr, Some(new.clone()), op.token.clone(), vec![prior]);
        } else {
            // The new operator is tighter: it takes only the last term.
            let end = expr_op.terms;
            let last = term_at(expr, end);
            let drill = op_of(&last).is_some_and(|last_op| last_op.right < new.left);
            if drill {
                out = prattify(&last, op);
            } else {
                out = op.node_under(vec![last], expr);
                set_term(expr, end, out.clone());
            }
        }
    } else if new.prefix {
        out = op.node_under(Vec::new(), expr);
        set_term(expr, expr_op.terms, out.clone());
    } else if new.suffix {
        if !expr_op.suffix && expr_op.right <= new.left {
            let end = expr_op.terms;
            let last = term_at(expr, end);
            // NOTE: special case: a higher precedence suffix "drills"
            // into lower precedence prefixes, so `@@1!` is `@(@(1!))`
            // rather than `@((@1)!)`.
            let drill =
                op_of(&last).is_some_and(|last_op| last_op.prefix && last_op.right < new.left);
            if drill {
                prattify(&last, op);
            } else {
                set_term(expr, end, op.node_under(vec![last], expr));
            }
        } else {
            let prior = dup_node(expr);
            update_expr_node(expr, Some(new.clone()), op.token.clone(), vec![prior]);
        }
    }

    out
}

/// Convert a prior rule's node into the start of a new expression, the
/// port of the canonical `prior`.
fn prior_expr(rule: &mut Rule, prior: Option<&Rc<RuleSnapshot>>, op: &OpRef) -> Value {
    let prior_value = prior.map_or(Value::Undefined, |prior| prior.node.borrow().clone());

    let node = if is_op(&prior_value) && !op.op.prefix {
        // Infix or suffix: the prior node is the genuine first term to
        // wrap (the `(1+2)` of `(1+2)+3`). Duplicate it so the in-place
        // rewrite below preserves referential integrity.
        let duplicate = dup_node(&prior_value);
        update_expr_node(
            &prior_value,
            Some(op.op.clone()),
            op.token.clone(),
            vec![duplicate],
        );
        prior_value
    } else {
        // A prefix operator never has a prior term, so any expression
        // sitting on the prior node here is a foreign parent-seed (a
        // ternary or paren wrapper the engine threaded down). Start
        // fresh rather than rewriting it.
        let terms = if op.op.prefix {
            Vec::new()
        } else {
            vec![prior_value]
        };
        let fresh = op.node(terms);
        if let Some(prior) = prior {
            // NOTE: nothing is written when there is no prior rule. The
            // canonical ports guard the same write, because a rule with
            // no parent would otherwise publish onto the no-rule
            // sentinel and leak the expression into every later parse.
            *prior.node.borrow_mut() = fresh.clone();
        }
        fresh
    };

    // Ensure the first term's rule contains the final expression.
    if let Some(prior) = prior {
        rule.parent_rule = Some(prior.clone());
        rule.parent_node = Some(prior.node.clone());
    }

    node
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

/// Values an evaluator has already produced, by identity.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum EvalKey {
    Node(u64),
    Shared(usize, usize),
}

thread_local! {
    static EVALUATED: RefCell<Vec<EvalKey>> = const { RefCell::new(Vec::new()) };
}

fn eval_key(value: &Value) -> Option<EvalKey> {
    if let Some(id) = node_id(value) {
        return Some(EvalKey::Node(id));
    }
    match value {
        Value::Array(entries) if !entries.is_empty() => Some(EvalKey::Shared(
            Arc::as_ptr(entries) as usize,
            entries.len(),
        )),
        Value::Object(entries) if !entries.is_empty() => Some(EvalKey::Shared(
            Arc::as_ptr(entries) as usize,
            entries.len(),
        )),
        Value::ListRef(list) => Some(EvalKey::Shared(
            Arc::as_ptr(list) as usize,
            list.value.len(),
        )),
        Value::MapRef(map) => Some(EvalKey::Shared(Arc::as_ptr(map) as usize, map.value.len())),
        _ => None,
    }
}

fn evaluated_seen(value: &Value) -> bool {
    eval_key(value).is_some_and(|key| EVALUATED.with(|set| set.borrow().contains(&key)))
}

fn evaluated_mark(value: &Value) {
    if let Some(key) = eval_key(value) {
        EVALUATED.with(|set| {
            let mut set = set.borrow_mut();
            if !set.contains(&key) {
                set.push(key);
            }
        });
    }
}

/// Reduce an expression tree with an evaluator, innermost first.
///
/// An implicit list is reduced a member at a time as its members close and
/// again as a whole once it is complete, so a value the evaluator has
/// already produced is returned untouched rather than reduced twice. That
/// record is per-thread and is cleared with the expression arena.
pub fn evaluation(site: &mut EvalSite<'_>, value: &Value, evaluate: &Evaluate) -> Value {
    if value.is_undefined() || matches!(value, Value::Null) {
        return Value::Null;
    }
    if evaluated_seen(value) {
        return value.clone();
    }

    let out = if is_op(value) {
        let Some((Some(op), token, terms)) = with_node(value, |entry| {
            (entry.op.clone(), entry.token.clone(), entry.terms.clone())
        }) else {
            return value.clone();
        };
        let reduced: Vec<Value> = terms
            .iter()
            .map(|term| evaluation(&mut site.reborrow(), term, evaluate))
            .collect();
        // The occurrence token travels on the site, not on the shared
        // description: `op` is one entry of the operator table and is the
        // same object for every occurrence. Restoring the previous token
        // keeps an outer reduction pointing at its own operator.
        let previous = std::mem::replace(&mut site.token, token);
        let reduction = evaluate(site, &op, &reduced);
        site.token = previous;
        reduction
    } else if let Value::Array(entries) = value {
        // An implicit list is a plain array, not an expression node, so
        // the branch above does not reach its members. Reduce them into a
        // NEW array: this is a documented entry point and the
        // parse-once, evaluate-many workflow needs a caller's tree to
        // survive a reduction intact.
        Value::array(
            entries
                .iter()
                .map(|entry| evaluation(&mut site.reborrow(), entry, evaluate))
                .collect(),
        )
    } else {
        value.clone()
    };

    evaluated_mark(&out);
    out
}

// ---------------------------------------------------------------------------
// Grammar construction
// ---------------------------------------------------------------------------

/// Which of the four token-led operator families a map is built for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Anyfix {
    Prefix,
    Suffix,
    Infix,
    Ternary,
}

impl Anyfix {
    fn name(self) -> &'static str {
        match self {
            Anyfix::Prefix => "prefix",
            Anyfix::Suffix => "suffix",
            Anyfix::Infix => "infix",
            Anyfix::Ternary => "ternary",
        }
    }

    fn selects(self, def: &OpDef) -> bool {
        match self {
            Anyfix::Prefix => def.prefix,
            Anyfix::Suffix => def.suffix,
            Anyfix::Infix => def.infix,
            Anyfix::Ternary => def.ternary,
        }
    }

    fn terms(self) -> usize {
        match self {
            Anyfix::Ternary => 3,
            Anyfix::Infix => 2,
            _ => 1,
        }
    }
}

/// Resolve the token identity for an operator source, preferring one this
/// INSTANCE already binds.
///
/// The canonical plugin looks a source up with `tabnas.fixed(src)`, an
/// instance lookup: a host grammar registers its punctuation as
/// instance-level fixed tokens, and binding an operator to a fresh
/// `#E<src>` identity instead would leave its alternate waiting on a token
/// the host lexer never emits.
fn operator_tin(parser: &mut Tabnas, src: &str) -> (Tin, String) {
    if let Some(tin) = parser.fixed(src) {
        let name = parser.token_name(tin);
        return (tin, name);
    }
    let name = format!("#E{src}");
    let tin = parser.token_with_source(name.clone(), src);
    (tin, name)
}

/// An operator definition's binding power, or the fallback when it is
/// unset.
///
/// The canonical `makeOpMap` writes `opdef.left || Number.MIN_SAFE_INTEGER`
/// and `opdef.right || Number.MAX_SAFE_INTEGER`. JavaScript's `||` is
/// falsy-based, so a power of ZERO is not a power of zero there: it falls
/// through to the fallback exactly as an absent one does. Keeping the zero
/// changes the tree a zero-power operator builds against one with a
/// negative power, which `../test/spec/binding-power-zero.tsv` pins.
fn binding_power(power: Option<i64>, unset: i64) -> i64 {
    match power {
        Some(0) | None => unset,
        Some(power) => power,
    }
}

/// Build one token-led operator map, the port of the canonical
/// `makeOpMap`.
fn make_op_map(
    parser: &mut Tabnas,
    ops: &IndexMap<String, Option<OpDef>>,
    anyfix: Anyfix,
) -> IndexMap<Tin, Arc<Op>> {
    let mut map: IndexMap<Tin, Arc<Op>> = IndexMap::new();
    for (name, def) in ops {
        let Some(def) = def else { continue };
        if !anyfix.selects(def) {
            continue;
        }
        let src = def.src.first().cloned().unwrap_or_default();
        let (tin, tkn) = operator_tin(parser, &src);

        let suffix = format!("-{}", anyfix.name());
        let op = Op {
            src: src.clone(),
            left: binding_power(def.left, MIN_SAFE_INTEGER),
            right: binding_power(def.right, MAX_SAFE_INTEGER),
            name: if name.ends_with(&suffix) {
                name.clone()
            } else {
                format!("{name}{suffix}")
            },
            infix: Anyfix::Infix == anyfix,
            prefix: Anyfix::Prefix == anyfix,
            suffix: Anyfix::Suffix == anyfix,
            ternary: Anyfix::Ternary == anyfix,
            tkn,
            tin,
            terms: anyfix.terms(),
            use_data: def.use_data.clone(),
            ..Default::default()
        };

        if Anyfix::Ternary == anyfix {
            // A ternary has two source strings, and both resolve to the
            // same description apart from their token and their index.
            let second = def.src.get(1).cloned().unwrap_or_default();
            let (second_tin, second_tkn) = operator_tin(parser, &second);

            let mut first = op.clone();
            first.ternary_index = Some(0);
            map.insert(tin, Arc::new(first));

            let mut last = op;
            last.src = second;
            last.ternary_index = Some(1);
            last.tkn = second_tkn;
            last.tin = second_tin;
            map.insert(second_tin, Arc::new(last));
        } else {
            map.insert(tin, Arc::new(op));
        }
    }
    map
}

/// Build the paren operator map, the port of the canonical
/// `makeParenMap`.
fn make_paren_map(
    parser: &mut Tabnas,
    ops: &IndexMap<String, Option<OpDef>>,
) -> IndexMap<Tin, Arc<Op>> {
    let mut map: IndexMap<Tin, Arc<Op>> = IndexMap::new();
    for (name, def) in ops {
        let Some(def) = def else { continue };
        if !def.paren {
            continue;
        }
        let (otin, otkn) = operator_tin(parser, &def.osrc);
        let (ctin, ctkn) = operator_tin(parser, &def.csrc);
        let op = Op {
            name: format!("{name}-paren"),
            osrc: def.osrc.clone(),
            csrc: def.csrc.clone(),
            otkn,
            otin,
            ctkn,
            ctin,
            preval: def.preval.clone().unwrap_or_default(),
            paren: true,
            src: def.osrc.clone(),
            terms: 1,
            use_data: def.use_data.clone(),
            ..Default::default()
        };
        map.insert(otin, Arc::new(op));
    }
    map
}

/// Everything the rule alternates read: the operator maps, the token
/// identities they match on, and the evaluator.
struct Grammar {
    prefix: IndexMap<Tin, Arc<Op>>,
    suffix: IndexMap<Tin, Arc<Op>>,
    infix: IndexMap<Tin, Arc<Op>>,
    ternary: IndexMap<Tin, Arc<Op>>,
    paren_open: IndexMap<Tin, Arc<Op>>,
    paren_close: IndexMap<Tin, Arc<Op>>,
    tin_prefix: Vec<Tin>,
    tin_suffix: Vec<Tin>,
    tin_infix: Vec<Tin>,
    tin_tern0: Vec<Tin>,
    tin_tern1: Vec<Tin>,
    tin_open: Vec<Tin>,
    tin_close: Vec<Tin>,
    evaluate: Option<Evaluate>,
}

/// The value tokens an expression term can start with, the canonical
/// `VAL = [TX, NR, ST, VL]`.
const VAL_TINS: [Tin; 4] = [TIN_TX, TIN_NR, TIN_ST, TIN_VL];

impl Grammar {
    fn has_prefix(&self) -> bool {
        !self.tin_prefix.is_empty()
    }

    fn has_infix(&self) -> bool {
        !self.tin_infix.is_empty()
    }

    fn has_suffix(&self) -> bool {
        !self.tin_suffix.is_empty()
    }

    fn has_ternary(&self) -> bool {
        !self.tin_tern0.is_empty() && !self.tin_tern1.is_empty()
    }

    fn has_paren(&self) -> bool {
        !self.tin_open.is_empty() && !self.tin_close.is_empty()
    }

    /// The operator occurrence a matched token introduces, the port of the
    /// canonical `makeOp`.
    fn op_use(map: &IndexMap<Tin, Arc<Op>>, token: Option<&Token>) -> Option<OpRef> {
        let token = token?;
        map.get(&token.tin)
            .map(|op| OpRef::at(op, Some(token.clone())))
    }
}

// ---------------------------------------------------------------------------
// Rule helpers
// ---------------------------------------------------------------------------

/// A rule counter, absent reading as zero.
fn counter(rule: &Rule, name: &str) -> i32 {
    rule.n.get(name).copied().unwrap_or(0)
}

/// A rule-local number, absent reading as zero.
fn local_number(rule: &Rule, name: &str) -> i64 {
    match rule.u.get(name) {
        Some(Value::Number(number)) => *number as i64,
        _ => 0,
    }
}

fn set_local_number(rule: &mut Rule, name: &str, value: i64) {
    rule.u_mut()
        .insert(name.to_string(), Value::Number(value as f64));
}

/// Assign a rule's node: the Rust spelling of the canonical `r.node = v`.
///
/// A pushed or replaced rule SHARES its parent's node cell, so writing
/// through `rule.node.borrow_mut()` would overwrite the parent's node too.
/// Assigning installs a fresh cell instead.
fn set_node(rule: &mut Rule, value: Value) {
    rule.node = Rc::new(RefCell::new(value));
}

fn node_of(rule: &Rule) -> Value {
    rule.node.borrow().clone()
}

/// The `u` key an expr rule keeps its attachment point under, so that a
/// later rule reads the same node the canonical port would read off
/// `rule.node` even after the after-close has put the root back there.
const ATTACH: &str = "expr_attach";

/// Publish an expression from an expr rule.
///
/// `rule.node` takes the ATTACHMENT POINT, where the operator's next term
/// belongs, exactly as the canonical port leaves it: the rules pushed from
/// here are seeded from it, and a chained prefix nests into it.
///
/// The ROOT goes to the rule's `u` bag, and the rule's after-close puts it
/// back on the node. The canonical port has no need for that, because it
/// reads the parse result off the FIRST rule of a replacement chain, which
/// the root reaches through `prior_expr`. This engine reads it off the
/// LAST, so a rule that kept only its attachment point would hand a
/// sub-expression back as the whole parse: `1+2*3` came back as
/// `["*",2,3]`.
fn publish_expr(rule: &mut Rule, attach: Value) {
    set_node(rule, attach.clone());
    rule.u_mut().insert(ATTACH.to_string(), attach);
}

/// Write a value through a rule's shared node cell.
fn publish(rule: &mut Rule, value: Value) {
    *rule.node.borrow_mut() = value;
}

/// The root of the expression this rule is building.
///
/// The rule the first term belongs to holds it: `prior_expr` writes the
/// new expression there, and the Pratt core rewrites that same node in
/// place however the tree grows, so its identity never moves. A rule with
/// no such prior holds the root itself.
fn expression_root(rule: &Rule) -> Value {
    root_node(&node_of(rule))
}

/// A linked rule's node as the canonical port would read it.
///
/// An expr rule keeps its ATTACHMENT POINT on `rule.node` there, and this
/// port puts the root back on the node once the rule has closed, so an
/// expr rule is read through its attachment point instead: `0!-1!*2!`
/// needs the trailing suffix to bind to `2`, which it only does when the
/// Pratt core is handed the sub-expression the last operator attached to.
fn linked_node(link: Option<&Rc<RuleSnapshot>>) -> Value {
    let Some(link) = link else {
        return Value::Undefined;
    };
    if let Some(attach) = link.u.get(ATTACH) {
        if is_op(attach) {
            return attach.clone();
        }
    }
    link.node.borrow().clone()
}

fn write_linked_node(link: Option<&Rc<RuleSnapshot>>, value: Value) {
    if let Some(link) = link {
        *link.node.borrow_mut() = value;
    }
}

fn counters(pairs: &[(&str, i32)]) -> HashMap<String, i32> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_string(), *value))
        .collect()
}

fn locals(pairs: &[(&str, Value)]) -> HashMap<String, Value> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_string(), value.clone()))
        .collect()
}

/// An alternate matching one token from the given set.
fn on(tins: &[Tin]) -> AltSpec {
    AltSpec {
        s: vec![tins.to_vec()],
        ..Default::default()
    }
}

/// An alternate matching two tokens in sequence.
fn on2(first: &[Tin], second: &[Tin]) -> AltSpec {
    AltSpec {
        s: vec![first.to_vec(), second.to_vec()],
        ..Default::default()
    }
}

/// An alternate with no token filter at all.
fn always() -> AltSpec {
    AltSpec::default()
}

fn group_has(tags: &str, tag: &str) -> bool {
    tags.split(',').any(|entry| entry.trim() == tag)
}

/// Add alternates the way the canonical `RuleSpec.open`/`close` do: the
/// whole list goes in FRONT of the alternates already there, keeping its
/// own order.
fn prepend_open_all(spec: &mut RuleSpec, alts: Vec<AltSpec>) {
    for alt in alts.into_iter().rev() {
        spec.prepend_open(alt);
    }
}

fn prepend_close_all(spec: &mut RuleSpec, alts: Vec<AltSpec>) {
    for alt in alts.into_iter().rev() {
        spec.prepend_close(alt);
    }
}

// ---------------------------------------------------------------------------
// The plugin
// ---------------------------------------------------------------------------

/// Install the expression grammar on a parser that already carries the
/// jsonic base rules (`val`, `map`, `list`, `pair`, `elem`).
///
/// This is the plugin body; most callers want [`plugin`], [`apply`] or
/// [`make`].
pub fn expr(parser: &mut Tabnas, options: &ExprOptions) -> Result<(), PluginError> {
    let rules = parser.rule_names();

    // A re-run on an instance that already carries the grammar must not
    // give it a second copy of every alternate. A derived instance starts
    // from an empty rule set and so installs normally.
    if rules.iter().any(|name| "expr" == name) {
        return Ok(());
    }

    // The plugin hangs its operator alternates on the base grammar's
    // `val` rule. On a bare engine there is nothing to hang them on, and
    // the parse would fail later with a message about the document.
    if !rules.iter().any(|name| "val" == name) {
        return Err(PluginError(
            "tabnas-expr: the instance carries no `val` rule. This plugin layers on the \
             jsonic base grammar; install that first (tabnas_jsonic::jsonic, or \
             tabnas_expr::make)."
                .into(),
        ));
    }

    let ops = resolve_options(options);

    let prefix = make_op_map(parser, &ops, Anyfix::Prefix);
    let suffix = make_op_map(parser, &ops, Anyfix::Suffix);
    let infix = make_op_map(parser, &ops, Anyfix::Infix);
    let ternary = make_op_map(parser, &ops, Anyfix::Ternary);
    let paren_open = make_paren_map(parser, &ops);
    let paren_close: IndexMap<Tin, Arc<Op>> = paren_open
        .values()
        .map(|op| (op.ctin, op.clone()))
        .collect();

    let grammar = Arc::new(Grammar {
        tin_prefix: prefix.keys().copied().collect(),
        tin_suffix: suffix.keys().copied().collect(),
        tin_infix: infix.keys().copied().collect(),
        tin_tern0: ternary
            .values()
            .filter(|op| Some(0) == op.ternary_index)
            .map(|op| op.tin)
            .collect(),
        tin_tern1: ternary
            .values()
            .filter(|op| Some(1) == op.ternary_index)
            .map(|op| op.tin)
            .collect(),
        tin_open: paren_open.values().map(|op| op.otin).collect(),
        tin_close: paren_close.values().map(|op| op.ctin).collect(),
        prefix,
        suffix,
        infix,
        ternary,
        paren_open,
        paren_close,
        evaluate: options.evaluate.clone(),
    });

    comment_before_fixed(parser, &grammar);

    // Release the previous parse's expression nodes when no parse this
    // crate drives is in flight. A caller driving the engine directly
    // therefore holds its result's nodes until the next parse on the
    // thread, which is when to call `realize`.
    parser.parse_prepare(|_context: &mut Context| release_arena());

    bound_expression_size(parser);

    install_val(parser, &grammar);
    install_list(parser, &grammar);
    install_map(parser, &grammar);
    install_elem(parser, &grammar);
    install_pair(parser, &grammar);
    install_expr(parser, &grammar);
    if grammar.has_paren() {
        install_paren(parser, &grammar);
    }
    if grammar.has_ternary() {
        install_ternary(parser, &grammar);
    }

    Ok(())
}

/// Keep a comment opening marker out of the fixed-token matcher's reach.
///
/// The canonical plugin moves the comment matcher in front of the fixed
/// one, because an operator such as `/` would otherwise cut `//` into two
/// operator tokens and a comment would never be recognised. This engine's
/// matcher families sit in fixed priority bands, so the same effect comes
/// from a `check` on the fixed family: where a comment starts, the fixed
/// matcher stands aside and the comment matcher takes the run.
///
/// It is installed whenever some operator source really is a prefix of a
/// comment marker, INCLUDING on a host that already configured
/// `options.fixed.check`. Omitting it there left the host's check in place
/// and the `/` operator ahead of `//` and `/*`, so a valid comment lexed
/// as operators and the document failed; the canonical runs a host check
/// and lexes comments correctly at the same time, because reordering the
/// matchers means the fixed family, its check included, is never reached
/// at a comment opener. Standing aside there is the same behaviour.
///
/// A host check is DISPLACED rather than chained. `LexCheck` holds its
/// callback privately and the engine offers no way to run one from
/// outside, so the check installed here cannot call through to the one it
/// replaces; everywhere but a contested comment opener it returns
/// `Continue`, which is what the fixed family does with no check at all. A
/// host that needs both installs its own check AFTER this plugin and skips
/// the comment openers itself.
fn comment_before_fixed(parser: &mut Tabnas, grammar: &Grammar) {
    let options = parser.config();
    let markers: Vec<String> = options
        .comment
        .definitions
        .values()
        .filter(|def| def.lex)
        .map(|def| def.start.clone())
        .filter(|start| !start.is_empty())
        .collect();
    if markers.is_empty() {
        return;
    }

    let sources: Vec<String> = grammar
        .prefix
        .values()
        .chain(grammar.suffix.values())
        .chain(grammar.infix.values())
        .chain(grammar.ternary.values())
        .map(|op| op.src.clone())
        .chain(
            grammar
                .paren_open
                .values()
                .flat_map(|op| [op.osrc.clone(), op.csrc.clone()]),
        )
        .filter(|src| !src.is_empty())
        .collect();

    let contested: Vec<String> = markers
        .into_iter()
        .filter(|marker| {
            sources
                .iter()
                .any(|src| src.len() < marker.len() && marker.starts_with(src.as_str()))
        })
        .collect();
    if contested.is_empty() {
        return;
    }

    parser.lex_check_ref(COMMENT_FIRST, move |remaining: &str| {
        if contested
            .iter()
            .any(|marker| remaining.starts_with(marker.as_str()))
        {
            tabnas::LexCheckResult::Skip
        } else {
            tabnas::LexCheckResult::Continue
        }
    });
    let _ = parser.grammar_json(COMMENT_FIRST_DOCUMENT);
}

/// Refuse an expression that has outgrown [`NODE_LIMIT`].
///
/// The check rides on the engine's parse budget, and composes with a
/// budget already in force rather than replacing it: the jsonic base
/// grammar installs one to bound container nesting, and both limits exist
/// for the same reason (see [`NODE_LIMIT`]).
fn bound_expression_size(parser: &mut Tabnas) {
    let budget = parser.config().parse.budget.clone();
    let existing = budget.on_check.clone();
    let every = match budget.check_every_n {
        0 => 32,
        set => set.min(32),
    };
    parser.parse_budget(every, move |context: &Context| {
        if OVER_LIMIT.with(Cell::get) {
            return false;
        }
        if context.rule_stack.len() > RULE_LIMIT {
            return false;
        }
        existing.as_ref().is_none_or(|check| check(context))
    });
}

const COMMENT_FIRST: &str = "@expr-comment-before-fixed";
const COMMENT_FIRST_DOCUMENT: &str =
    r#"{"options":{"fixed":{"check":"@expr-comment-before-fixed"}}}"#;

// ---------------------------------------------------------------------------
// val
// ---------------------------------------------------------------------------

fn install_val(parser: &mut Tabnas, grammar: &Arc<Grammar>) {
    let grammar = grammar.clone();
    parser.define_rule("val", move |spec| {
        // Implicit pair not allowed inside a ternary: the `2:` of `1?2:3`
        // is the ternary's second operator, not a pair key.
        if grammar.has_ternary() && grammar.tin_tern1.contains(&TIN_CL) {
            for alt in spec.open.iter_mut().filter(|alt| group_has(&alt.g, "pair")) {
                let previous = alt.c_fn.clone();
                alt.c_fn = Some(Arc::new(move |rule: &mut Rule, context: &mut Context| {
                    previous
                        .as_ref()
                        .is_none_or(|condition| condition(rule, context))
                        && 0 == counter(rule, "expr_ternary")
                }));
            }
        }

        let mut open: Vec<AltSpec> = Vec::new();

        // The prefix operator of the first term of an expression.
        if grammar.has_prefix() {
            let mut alt = on(&grammar.tin_prefix);
            alt.b = 1;
            alt.n = counters(&[("expr_prefix", 1), ("expr_suffix", 0)]);
            alt.p = Some("expr".into());
            alt.g = "expr,expr-prefix".into();
            alt.add_action(detach_node);
            open.push(alt);
        }

        // A value followed by an opening paren: `foo(1)`, `a[1]`.
        if grammar.has_paren() {
            let mut alt = on2(&VAL_TINS, &grammar.tin_open);
            alt.b = 1;
            alt.p = Some("expr".into());
            let condition = grammar.clone();
            alt.c_fn = Some(Arc::new(move |rule: &mut Rule, context: &mut Context| {
                let Some(pdef) = rule
                    .o1()
                    .and_then(|token| condition.paren_open.get(&token.tin))
                    .cloned()
                else {
                    return false;
                };
                if !pdef.preval.active {
                    return false;
                }
                let Some(allow) = &pdef.preval.allow else {
                    return true;
                };
                let preval = rule.resolve_open_value(0, context);
                allow.contains(&value_string(Some(&preval)))
            }));
            alt.u = locals(&[("paren_preval", Value::Bool(true))]);
            alt.g = "expr,expr-paren,expr-paren-preval".into();
            alt.add_action(|rule: &mut Rule, context: &mut Context| {
                let preval = rule.resolve_open_value(0, context);
                set_node(rule, preval);
            });
            open.push(alt);
        }

        // An opening parenthesis. NOTE: this can happen outside an
        // expression.
        if grammar.has_paren() {
            let mut alt = on(&grammar.tin_open);
            alt.b = 1;
            alt.p = Some("expr".into());
            let condition = grammar.clone();
            alt.c_fn = Some(Arc::new(move |rule: &mut Rule, _context: &mut Context| {
                rule.o0()
                    .and_then(|token| condition.paren_open.get(&token.tin))
                    .is_some_and(|pdef| !pdef.preval.required)
            }));
            alt.g = "expr,expr-paren".into();
            alt.add_action(detach_node);
            open.push(alt);
        }

        prepend_open_all(spec, open);

        let mut close: Vec<AltSpec> = Vec::new();

        // Comma-op suppression. When a parent rule (an embedding
        // grammar's wrapper rule, say) sets n.no_comma_op, bail at `,`
        // without treating it as the comma operator: the parent then
        // consumes the `,` itself as a separator. Match by `src` on the
        // next infix token, so this works whichever token the embedding
        // grammar uses for `,`.
        if grammar.has_infix() {
            let mut alt = on(&grammar.tin_infix);
            alt.c_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
                0 < counter(rule, "no_comma_op")
                    && rule.c0().is_some_and(|token| "," == token.src.as_str())
            }));
            alt.b = 1;
            alt.g = "expr,no-comma-op-bail".into();
            close.push(alt);
        }

        if grammar.has_ternary() {
            let mut alt = on(&grammar.tin_tern0);
            alt.c_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
                0 == counter(rule, "expr")
            }));
            alt.b = 1;
            alt.r = Some("ternary".into());
            alt.g = "expr,expr-ternary".into();
            close.push(alt);
        }

        // The infix operator following the first term of an expression.
        if grammar.has_infix() {
            let mut alt = on(&grammar.tin_infix);
            alt.b = 1;
            alt.n = counters(&[("expr_prefix", 0), ("expr_suffix", 0)]);
            alt.r_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
                (0 == counter(rule, "expr")).then(|| "expr".to_string())
            }));
            alt.g = "expr,expr-infix".into();
            close.push(alt);
        }

        // The suffix operator following the first term of an expression.
        if grammar.has_suffix() {
            let mut alt = on(&grammar.tin_suffix);
            alt.b = 1;
            alt.n = counters(&[("expr_prefix", 0), ("expr_suffix", 1)]);
            alt.r_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
                (0 == counter(rule, "expr")).then(|| "expr".to_string())
            }));
            alt.g = "expr,expr-suffix".into();
            close.push(alt);
        }

        // The closing parenthesis of an expression.
        if grammar.has_paren() {
            let mut alt = on(&grammar.tin_close);
            alt.c_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
                0 != counter(rule, "expr_paren")
            }));
            alt.b = 1;
            alt.g = "expr,expr-paren".into();
            close.push(alt);
        }

        // Chain for postfix paren forms. When a val has just produced a
        // value (`a[0]`, `f(0)`, or a parenthesised expression like
        // `(*p)`) and the next token is another preval-active paren-open,
        // push expr, which descends into paren, so the new paren-form
        // picks up this val's node as the preval. Pushing rather than
        // replacing keeps the current val rule alive, so the chained
        // result is still what the parse returns. `paren_preval` is set
        // so the paren close finds it on the grandparent and pushes this
        // node into the new paren expression. This complements the
        // open-time preval alternate above, which only fires on the first
        // preval-paren of an expression: chain-time detection is needed
        // for later parens, because by then the leading value is a
        // produced node rather than a token in the lex buffer.
        if grammar.has_paren() {
            let mut alt = on(&grammar.tin_open);
            alt.b = 1;
            let condition = grammar.clone();
            alt.c_fn = Some(Arc::new(move |rule: &mut Rule, _context: &mut Context| {
                let Some(pdef) = rule
                    .c0()
                    .and_then(|token| condition.paren_open.get(&token.tin))
                    .cloned()
                else {
                    return false;
                };
                let node = node_of(rule);
                if !pdef.preval.active || node.is_undefined() {
                    return false;
                }
                let Some(allow) = &pdef.preval.allow else {
                    return true;
                };
                allow.contains(&value_string(Some(&node)))
            }));
            alt.p = Some("expr".into());
            alt.u = locals(&[("paren_preval", Value::Bool(true))]);
            alt.g = "expr,expr-paren,expr-paren-preval-chain".into();
            close.push(alt);
        }

        if grammar.has_ternary() {
            let mut alt = on(&grammar.tin_tern1);
            alt.c_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
                0 != counter(rule, "expr_ternary")
            }));
            alt.b = 1;
            alt.g = "expr,expr-ternary".into();
            close.push(alt);
        }

        // Do not create an implicit list inside an expression (comma
        // separator).
        let mut comma = on(&[TIN_CA]);
        comma.c_fn = Some(Arc::new(no_implicit_in_expr));
        comma.b = 1;
        comma.g = "expr,list,val,imp,comma,top".into();
        close.push(comma);

        // Do not create an implicit list inside an expression (space
        // separator).
        let mut space = on(&VAL_TINS);
        space.c_fn = Some(Arc::new(no_implicit_in_expr));
        space.b = 1;
        space.g = "expr,list,val,imp,space,top".into();
        close.push(space);

        prepend_close_all(spec, close);
    });
}

/// Give this rule its own node cell, keeping the value it was seeded with.
///
/// A rule pushed by the engine shares its parent's cell, so a later write
/// to this rule's node would reach the parent too. The canonical engine
/// gives every rule its own `node` field and seeds it by VALUE, and the
/// base grammar's own val alternates reach the same state by resetting the
/// node outright. The alternates this plugin adds to `val` need the seeded
/// value, a partly built expression the operator alternates read back, so
/// they detach instead of resetting.
fn detach_node(rule: &mut Rule, _context: &mut Context) {
    let node = node_of(rule);
    set_node(rule, node);
}

fn no_implicit_in_expr(rule: &mut Rule, _context: &mut Context) -> bool {
    (1 == rule.d && (1 <= counter(rule, "expr") || 1 <= counter(rule, "expr_ternary")))
        || (1 <= counter(rule, "expr_ternary") && 1 <= counter(rule, "expr_paren"))
}

// ---------------------------------------------------------------------------
// list, map, elem, pair
// ---------------------------------------------------------------------------

fn clear_expr_counters(rule: &mut Rule) {
    let counters = rule.n_mut();
    counters.insert("expr".into(), 0);
    counters.insert("expr_prefix".into(), 0);
    counters.insert("expr_suffix".into(), 0);
    counters.insert("expr_paren".into(), 0);
    counters.insert("expr_ternary".into(), 0);
}

fn install_list(parser: &mut Tabnas, grammar: &Arc<Grammar>) {
    let grammar = grammar.clone();
    parser.define_rule("list", move |spec| {
        spec.prepend_bo(|rule: &mut Rule, _context: &mut Context| {
            // List elements are new expressions, unless this is an
            // implicit list.
            let implicit = rule
                .prev_rule
                .as_ref()
                .is_some_and(|prev| matches!(prev.u.get("implist"), Some(Value::Bool(true))));
            if !implicit {
                clear_expr_counters(rule);
            }
        });

        if grammar.has_paren() {
            let mut alt = on(&grammar.tin_close);
            // If this is the end of a normal list, consume `]`: it is not
            // a close paren.
            alt.b_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
                let square = rule.c0().is_some_and(|token| TIN_CS == token.tin);
                if square && 0 == counter(rule, "expr_paren") {
                    0
                } else {
                    1
                }
            }));
            alt.g = "expr".into();
            prepend_close_all(spec, vec![alt]);

            spec.add_ac(|rule: &mut Rule, _context: &mut Context| {
                propagate_to_paren(rule);
            });
        }
    });
}

fn install_map(parser: &mut Tabnas, grammar: &Arc<Grammar>) {
    let grammar = grammar.clone();
    parser.define_rule("map", move |spec| {
        // Map values are new expressions.
        spec.prepend_bo(|rule: &mut Rule, _context: &mut Context| clear_expr_counters(rule));

        if grammar.has_paren() {
            let mut alt = on(&grammar.tin_close);
            // If this is the end of a normal map, consume `}`: it is not
            // a close paren.
            alt.b_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
                let brace = rule.c0().is_some_and(|token| TIN_CB == token.tin);
                if brace && 0 == counter(rule, "expr_paren") {
                    0
                } else {
                    1
                }
            }));
            alt.g = "expr".into();
            prepend_close_all(spec, vec![alt]);
        }
    });
}

fn install_elem(parser: &mut Tabnas, grammar: &Arc<Grammar>) {
    let grammar = grammar.clone();
    parser.define_rule("elem", move |spec| {
        if !grammar.has_paren() {
            return;
        }
        let mut close: Vec<AltSpec> = Vec::new();

        // Close an implicit list within parens.
        let mut alt = on(&grammar.tin_close);
        alt.b = 1;
        alt.c_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
            0 != counter(rule, "expr_paren")
        }));
        alt.g = "expr,expr-paren,imp,close,list".into();
        close.push(alt);

        // The following element is a paren expression.
        let mut alt = on(&grammar.tin_open);
        alt.b = 1;
        alt.r = Some("elem".into());
        alt.g = "expr,expr-paren,imp,open,list".into();
        close.push(alt);

        prepend_close_all(spec, close);

        // Propagate the collected list to the enclosing paren. A
        // container is copied on write here, so the paren cannot hold the
        // same array the `elem` rules are appending to; it takes the
        // finished one instead. The Go port does the same, for the same
        // reason.
        spec.add_ac(|rule: &mut Rule, _context: &mut Context| {
            propagate_to_paren(rule);
        });
    });
}

/// Write a rule's node onto the paren rule that encloses it.
fn propagate_to_paren(rule: &mut Rule) {
    if 0 == counter(rule, "expr_paren") {
        return;
    }
    let node = node_of(rule);
    let mut cursor = rule.parent_rule.clone();
    while let Some(current) = cursor {
        if "paren" == current.name.as_ref() {
            *current.node.borrow_mut() = node;
            return;
        }
        cursor = current.parent_rule.clone();
    }
}

fn install_pair(parser: &mut Tabnas, grammar: &Arc<Grammar>) {
    let grammar = grammar.clone();
    parser.define_rule("pair", move |spec| {
        if !grammar.has_paren() {
            return;
        }
        // Close an implicit map within parens.
        let mut alt = on(&grammar.tin_close);
        alt.b = 1;
        alt.c_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
            0 != counter(rule, "expr_paren") || 0 < counter(rule, "pk")
        }));
        alt.g = "expr,expr-paren,imp,map".into();
        prepend_close_all(spec, vec![alt]);
    });
}

// ---------------------------------------------------------------------------
// expr
// ---------------------------------------------------------------------------

/// The completed expression a top-level implicit list should start with.
///
/// The rule that meets the separator is the last one the Pratt core built,
/// which for a right-leaning tree is a SUB-expression: `1+2*3` leaves it
/// holding `[*,2,3]` while the finished `[+,1,[*,2,3]]` sits on the val
/// this expression publishes to, and on the outermost expr in the
/// replacement chain. Seeding the list from the rule's own node therefore
/// dropped the `1+`, a wrong answer with nothing to signal it.
fn expr_root(rule: &Rule) -> Value {
    let parent = linked_node(rule.parent_rule.as_ref());
    if rule.parent_rule.is_some() && is_op(&parent) {
        return parent;
    }

    // No usable parent. The outermost expr in the chain holds the root;
    // later entries are the sub-expressions built under it.
    let mut root: Option<Value> = None;
    if "expr" == rule.name.as_ref() {
        let node = node_of(rule);
        if is_op(&node) {
            root = Some(node);
        }
    }
    let mut cursor = rule.prev_rule.clone();
    while let Some(current) = cursor {
        if "expr" == current.name.as_ref() {
            let node = current.node.borrow().clone();
            if is_op(&node) {
                root = Some(node);
            }
        }
        cursor = current.prev_rule.clone();
    }

    root.unwrap_or_else(|| node_of(rule))
}

/// Start a top-level implicit list, publishing it to the rule's parent
/// unless there is none.
///
/// A rule seeds its node from its parent, so a list written to a missing
/// parent would be handed straight back out as the starting node of the
/// rules that follow. The rule's own node is what the parse returns at
/// this depth, so skipping the write costs nothing.
fn top_list(rule: &mut Rule) -> Value {
    let list = Value::array(vec![expr_root(rule)]);

    if rule.parent_rule.is_some() {
        write_linked_node(rule.parent_rule.as_ref(), list.clone());
        return list;
    }

    // No parent to publish to. The list is still built correctly by the
    // `elem` rules that follow, but nothing would ever read it: the
    // result would come back as the first member alone. Write it back
    // along the replacement chain instead, so whichever rule the parse
    // returns is holding this list.
    set_node(rule, list.clone());
    let mut cursor = rule.prev_rule.clone();
    while let Some(current) = cursor {
        *current.node.borrow_mut() = list.clone();
        cursor = current.prev_rule.clone();
    }

    list
}

/// Collect an expression into the implicit list its enclosing paren is
/// building, the port of the canonical `implicitList`.
fn implicit_list(mut matched: AltMatch, rule: &mut Rule, context: &mut Context) -> AltMatch {
    // Find the paren rule that contains this implicit list. A map or list
    // rule between the expression and the paren means the expression is
    // inside a contained value, not a direct paren child, so no implicit
    // list is created.
    let mut paren: Option<Rc<RuleSnapshot>> = None;
    let mut paren_index = 0usize;
    for index in (0..context.rule_stack.len()).rev() {
        let frame = &context.rule_stack[index];
        let name = frame.name.as_ref();
        if "paren" == name {
            paren = Some(frame.clone());
            paren_index = index;
            break;
        }
        if "map" == name || "list" == name {
            return matched;
        }
    }

    let Some(paren) = paren else {
        return matched;
    };

    // The node this expression contributes, captured before the write
    // below replaces it with the list. The ROOT, not the attachment
    // point: the canonical port takes it off the rule the paren pushed,
    // which is where `prior_expr` writes the whole expression, so a
    // right-leaning member such as `f(1+2*3, 4)` keeps its `1+`.
    let expr_node = node_of(rule);
    let expr_node = if is_op(&expr_node) {
        expression_root(rule)
    } else {
        expr_node
    };

    // Create the list value for the paren rule, unless one is already
    // being collected, in which case the `elem` rules do the collecting.
    //
    // The canonical port reads and writes this through the paren's own
    // child rule, because a JavaScript array is shared by reference: the
    // rule collecting the list and the paren then hold the same array.
    // This engine copies a container on write, so the list is kept on the
    // paren itself and the `elem` after-close hook puts the finished one
    // back (as the Go port does, for the same reason).
    let paren_node = paren.node.borrow().clone();
    if matches!(&paren_node, Value::Array(members) if !members.is_empty()) {
        // An `elem` rule is already collecting the list, and this
        // expression is one of its members: leave the node alone so that
        // the member, and not the list, is what the collector reads.
        return matched;
    }

    *paren.node.borrow_mut() = Value::array(vec![expr_node.clone()]);
    matched.r = Some("elem".into());
    matched.b = 0;

    let list = paren.node.borrow().clone();
    set_node(rule, list.clone());

    // The rule the first term belongs to holds the list too. The
    // canonical port gets that for nothing, because the rule it writes
    // the list through IS that rule; here the list is kept on the paren,
    // so this write is what the expression's own after-close reads when
    // it reduces the members with an evaluator.
    write_linked_node(rule.parent_rule.as_ref(), list.clone());

    // The rule that sees the comma is not always the paren's own child. A
    // first argument that nests operators, a prefix whose operand is
    // itself a prefix, `f(- -1, 2)`, leaves further rules on the stack
    // between the paren and this rule, all holding the same expression
    // node. Each writes that node back over the paren's when it closes,
    // discarding every element collected after the first. Point them at
    // the implicit list so the close-back is a no-op instead of a
    // truncation.
    //
    // Only the frames that carry this expression: the `expr` rules
    // themselves, and the `val` rules they were pushed from, which hold
    // either nothing yet or the expression node. A rule a host plugin
    // stacked in between has a node of its own to close with, and
    // overwriting it would destroy a value this plugin knows nothing
    // about.
    for index in (paren_index + 1)..context.rule_stack.len() {
        let frame = context.rule_stack[index].clone();
        let name = frame.name.as_ref().to_string();
        let node = frame.node.borrow().clone();
        let carries = "expr" == name
            || ("val" == name
                && (node.is_undefined()
                    || matches!(node, Value::Null)
                    || same_node(&node, &expr_node)
                    || is_op(&node)));
        if carries {
            *frame.node.borrow_mut() = list.clone();
        }
    }

    matched
}

/// Node identity, the canonical `===` on two expression nodes.
fn same_node(left: &Value, right: &Value) -> bool {
    match (node_id(left), node_id(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn install_expr(parser: &mut Tabnas, grammar: &Arc<Grammar>) {
    let grammar = grammar.clone();
    parser.define_rule("expr", move |spec| {
        let mut open: Vec<AltSpec> = Vec::new();

        // An opening parenthesis of an expression.
        if grammar.has_paren() {
            let mut alt = on(&grammar.tin_open);
            alt.p = Some("paren".into());
            alt.b = 1;
            alt.g = "expr,expr-paren,expr-start".into();
            open.push(alt);
        }

        if grammar.has_prefix() {
            let mut alt = on(&grammar.tin_prefix);
            alt.c_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
                0 != counter(rule, "expr_prefix")
            }));
            alt.n = counters(&[("expr", 1), ("dlist", 1), ("dmap", 1)]);
            alt.p = Some("val".into());
            alt.g = "expr,expr-prefix".into();
            let ops = grammar.clone();
            alt.add_action(move |rule: &mut Rule, _context: &mut Context| {
                let Some(op) = Grammar::op_use(&ops.prefix, rule.o0()) else {
                    return;
                };
                let parent = linked_node(rule.parent_rule.as_ref());
                // Only fold into the parent's node when it is an
                // expression THIS prefix chain is building (the outer `-`
                // of `--1`). A ternary or paren node on the parent is a
                // foreign seed the engine threaded down, and folding into
                // it would corrupt the wrapper with a stray hole and a
                // duplicated operand. Start a fresh operand expression
                // for those, mirroring the infix guard.
                if is_op(&parent) && !is_ternary_op(&parent) && !is_paren_op(&parent) {
                    let attach = prattify(&parent, &op);
                    publish_expr(rule, attach);
                } else {
                    let prior = rule.parent_rule.clone();
                    let node = prior_expr(rule, prior.as_ref(), &op);
                    publish_expr(rule, node);
                }
            });
            open.push(alt);
        }

        if grammar.has_infix() {
            let mut alt = on(&grammar.tin_infix);
            alt.p = Some("val".into());
            alt.n = counters(&[("expr", 1), ("expr_prefix", 0), ("dlist", 1), ("dmap", 1)]);
            alt.g = "expr,expr-infix".into();
            let ops = grammar.clone();
            alt.add_action(move |rule: &mut Rule, _context: &mut Context| {
                let Some(op) = Grammar::op_use(&ops.infix, rule.o0()) else {
                    return;
                };
                let parent = linked_node(rule.parent_rule.as_ref());
                let previous = linked_node(rule.prev_rule.as_ref());

                // Second and further operators.
                if is_op(&parent) && !is_ternary_op(&parent) {
                    let attach = prattify(&parent, &op);
                    publish_expr(rule, attach);
                }
                // The first term was a unary expression.
                else if is_op(&previous) {
                    let attach = prattify(&previous, &op);
                    if let Some(prev) = rule.prev_rule.clone() {
                        rule.parent_node = Some(prev.node.clone());
                        rule.parent_rule = Some(prev);
                    }
                    publish_expr(rule, attach);
                }
                // The first term was a plain value or a ternary part.
                else {
                    let prior = rule.prev_rule.clone();
                    let node = prior_expr(rule, prior.as_ref(), &op);
                    publish_expr(rule, node);
                }
            });
            open.push(alt);
        }

        if grammar.has_suffix() {
            let mut alt = on(&grammar.tin_suffix);
            alt.n = counters(&[("expr", 1), ("expr_prefix", 0), ("dlist", 1), ("dmap", 1)]);
            alt.g = "expr,expr-suffix".into();
            let ops = grammar.clone();
            alt.add_action(move |rule: &mut Rule, _context: &mut Context| {
                let Some(op) = Grammar::op_use(&ops.suffix, rule.o0()) else {
                    return;
                };
                let previous = linked_node(rule.prev_rule.as_ref());
                if is_op(&previous) {
                    let attach = prattify(&previous, &op);
                    publish_expr(rule, attach);
                } else {
                    let prior = rule.prev_rule.clone();
                    let node = prior_expr(rule, prior.as_ref(), &op);
                    publish_expr(rule, node);
                }
            });
            open.push(alt);
        }

        prepend_open_all(spec, open);

        // Append the final term to the expression.
        spec.add_bc(|rule: &mut Rule, _context: &mut Context| {
            let node = node_of(rule);
            let Some(op) = op_of(&node) else {
                return;
            };
            let child = rule.child_node.clone();
            if term_count(&node) < op.terms && !same_node(&node, &child) {
                push_term(&node, child);
            }
        });

        let mut close: Vec<AltSpec> = Vec::new();

        let mut alt = always();
        alt.c_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
            rule.child_rule
                .as_ref()
                .is_some_and(|child| "paren" == child.name.as_ref())
        }));
        alt.n = counters(&[("expr", 0)]);
        alt.g = "expr,expr-end,expr-paren-end".into();
        close.push(alt);

        // Comma-op suppression. When n.no_comma_op is set by a parent
        // rule, terminate the expression at `,` without treating it as
        // the comma operator. Closing the expression frame lets the
        // parent rule regain control before any comma-op alternate below
        // fires.
        if grammar.has_infix() {
            let mut alt = on(&grammar.tin_infix);
            alt.c_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
                0 < counter(rule, "no_comma_op")
                    && rule.c0().is_some_and(|token| "," == token.src.as_str())
            }));
            alt.b = 1;
            alt.n = counters(&[("expr", 0)]);
            alt.g = "expr,no-comma-op-bail".into();
            close.push(alt);
        }

        if grammar.has_infix() {
            // Complete the prefix first.
            let mut alt = on(&grammar.tin_infix);
            alt.c_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
                0 == counter(rule, "expr_prefix")
            }));
            alt.b = 1;
            alt.r = Some("expr".into());
            alt.g = "expr,expr-infix,expr-prefix".into();
            close.push(alt);

            let mut alt = on(&grammar.tin_infix);
            alt.c_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
                0 != counter(rule, "expr_prefix")
            }));
            alt.b = 1;
            alt.g = "expr,expr-infix".into();
            close.push(alt);
        }

        if grammar.has_suffix() {
            let mut alt = on(&grammar.tin_suffix);
            alt.c_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
                0 == counter(rule, "expr_prefix")
            }));
            alt.b = 1;
            alt.r = Some("expr".into());
            alt.g = "expr,expr-suffix,expr-prefix".into();
            close.push(alt);
        }

        if grammar.has_paren() {
            let mut alt = on(&grammar.tin_close);
            alt.c_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
                0 != counter(rule, "expr_paren")
            }));
            alt.b = 1;
            alt.g = "expr".into();
            close.push(alt);
        }

        if grammar.has_ternary() {
            let mut alt = on(&grammar.tin_tern0);
            alt.c_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
                0 == counter(rule, "expr_prefix")
            }));
            alt.b = 1;
            alt.r = Some("ternary".into());
            alt.g = "expr,expr-ternary".into();
            close.push(alt);
        }

        // Implicit list at the top level, comma separated.
        let mut alt = on(&[TIN_CA]);
        alt.c_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
            0 == rule.d
        }));
        alt.n = counters(&[("expr", 0)]);
        alt.r = Some("elem".into());
        alt.add_action(|rule: &mut Rule, _context: &mut Context| {
            let list = top_list(rule);
            set_node(rule, list);
        });
        alt.g = "expr,comma,list,top".into();
        close.push(alt);

        // Implicit list at the top level, space separated.
        let mut alt = on(&VAL_TINS);
        alt.c_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
            0 == rule.d
        }));
        alt.n = counters(&[("expr", 0)]);
        alt.b = 1;
        alt.r = Some("elem".into());
        alt.add_action(|rule: &mut Rule, _context: &mut Context| {
            let list = top_list(rule);
            set_node(rule, list);
        });
        alt.g = "expr,space,list,top".into();
        close.push(alt);

        // Implicit list indicated by a comma.
        let mut alt = on(&[TIN_CA]);
        alt.c_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
            rule.lte("pk", 0)
        }));
        alt.n = counters(&[("expr", 0)]);
        alt.b = 1;
        alt.h_match = Some(Arc::new(|matched, rule, context, _next| {
            implicit_list(matched, rule, context)
        }));
        alt.g = "expr,list,val,imp,comma".into();
        close.push(alt);

        // Implicit list indicated by a space separated value.
        //
        // This alternate carries no token filter, so it also has to be
        // kept off a pending suffix operator. A suffix belongs to the
        // expression that just closed, not to a new element: `f(@x!)`
        // reaches here on the `!` of a completed prefix, opens `elem`,
        // and offers `!` as the start of a value, which nothing matches,
        // so a valid expression fails as unexpected. Leaving the token
        // alone lets the val-close suffix alternate take it, which is how
        // the same expression already parses at the top level.
        let mut alt = always();
        let suffix_tins = grammar.tin_suffix.clone();
        alt.c_fn = Some(Arc::new(move |rule: &mut Rule, context: &mut Context| {
            rule.lte("pk", 0)
                && rule.lte("expr_suffix", 0)
                && !context
                    .t0()
                    .is_some_and(|token| suffix_tins.contains(&token.tin))
        }));
        alt.n = counters(&[("expr", 0)]);
        alt.h_match = Some(Arc::new(|matched, rule, context, _next| {
            implicit_list(matched, rule, context)
        }));
        alt.g = "expr,list,val,imp,space".into();
        close.push(alt);

        // The expression ends on a non-expression token.
        let mut alt = always();
        alt.n = counters(&[("expr", 0)]);
        alt.g = "expr,expr-end".into();
        close.push(alt);

        prepend_close_all(spec, close);

        // Put the ROOT of the expression back on the node, so the last
        // rule of a replacement chain hands the whole expression to the
        // rule that collects it, rather than the sub-expression the last
        // operator attached to. Only while the node still IS the
        // attachment point: a close alternate that produced an implicit
        // list has replaced it deliberately.
        spec.add_ac(|rule: &mut Rule, _context: &mut Context| {
            if is_op(&node_of(rule)) {
                let root = expression_root(rule);
                set_node(rule, root);
            }
        });

        // Evaluate at the root of the expression, where n.expr is zero.
        if let Some(evaluate) = grammar.evaluate.clone() {
            spec.add_ac(move |rule: &mut Rule, context: &mut Context| {
                if 0 != counter(rule, "expr") {
                    return;
                }
                let Some(parent) = rule.parent_rule.clone() else {
                    return;
                };
                let parent_node = parent.node.borrow().clone();

                // The parent holds an implicit list (`f(1+2, 3)`,
                // `1+2, 3`) rather than this expression's own node. A
                // list is a plain array, not an expression node, so its
                // members would otherwise reach the caller as raw
                // S-expressions the evaluator was never called on.
                //
                // Reduce them in place, keeping the array: the list is
                // still being collected, and the paren rule and the expr
                // rules between it and here all hold a reference to this
                // exact array, so replacing it would strand the members
                // still to come. A member already reduced is returned
                // untouched rather than reduced again.
                if let Value::Array(members) = &parent_node {
                    let mut site = EvalSite::new(Some(parent.clone()), context);
                    let reduced: Vec<Value> = members
                        .iter()
                        .map(|member| evaluation(&mut site.reborrow(), member, &evaluate))
                        .collect();
                    let reduced = Value::array(reduced);
                    *parent.node.borrow_mut() = reduced.clone();
                    // Through the shared cell, not into a fresh one: the
                    // rule that collects the rest of the list was already
                    // seeded from this cell, and the canonical port keeps
                    // the one array for the same reason.
                    publish(rule, reduced);
                    return;
                }

                // The parent node holds the root of the expression tree.
                let out = {
                    let mut site = EvalSite::new(Some(parent.clone()), context);
                    evaluation(&mut site, &parent_node, &evaluate)
                };
                *parent.node.borrow_mut() = out.clone();

                // Also write the evaluated result onto this rule's own
                // node. When the expr rule was PUSHED from a still-open
                // val (the prefix and paren forms, `+1`, `(a)`), that
                // val's close coalescing reads its child's node, which is
                // this rule, and would otherwise restore the raw
                // S-expression and discard the evaluated value.
                publish(rule, out);
            });
        }
    });
}

// ---------------------------------------------------------------------------
// paren
// ---------------------------------------------------------------------------

fn paren_depth(name: &str) -> String {
    format!("expr_paren_depth_{name}")
}

fn install_paren(parser: &mut Tabnas, grammar: &Arc<Grammar>) {
    let grammar = grammar.clone();
    parser.define_rule("paren", move |spec| {
        // Allow implicits inside parens.
        spec.add_bo(|rule: &mut Rule, _context: &mut Context| {
            let counters = rule.n_mut();
            counters.insert("dmap".into(), 0);
            counters.insert("dlist".into(), 0);
            counters.insert("pk".into(), 0);
        });

        let mut open: Vec<AltSpec> = Vec::new();

        // Empty parens: `()`.
        let mut alt = on2(&grammar.tin_open, &grammar.tin_close);
        alt.b = 1;
        alt.g = "expr,expr-paren,empty".into();
        let condition = grammar.clone();
        alt.c_fn = Some(Arc::new(move |rule: &mut Rule, _context: &mut Context| {
            let open = rule
                .o0()
                .and_then(|token| condition.paren_open.get(&token.tin));
            let close = rule
                .o1()
                .and_then(|token| condition.paren_close.get(&token.tin));
            match (open, close) {
                (Some(open), Some(close)) => open.name == close.name,
                _ => false,
            }
        }));
        let ops = grammar.clone();
        alt.add_action(move |rule: &mut Rule, _context: &mut Context| {
            open_paren(&ops, rule);
        });
        open.push(alt);

        // A normal paren open: consume the opener and push to val.
        let mut alt = on(&grammar.tin_open);
        alt.p = Some("val".into());
        alt.n = counters(&[
            ("expr_paren", 1),
            ("expr", 0),
            ("expr_prefix", 0),
            ("expr_suffix", 0),
        ]);
        alt.g = "expr,expr-paren,open".into();
        let ops = grammar.clone();
        alt.add_action(move |rule: &mut Rule, _context: &mut Context| {
            open_paren(&ops, rule);
        });
        open.push(alt);

        prepend_open_all(spec, open);

        let mut alt = on(&grammar.tin_close);
        let condition = grammar.clone();
        alt.c_fn = Some(Arc::new(move |rule: &mut Rule, _context: &mut Context| {
            let Some(pdef) = rule
                .c0()
                .and_then(|token| condition.paren_close.get(&token.tin))
                .cloned()
            else {
                return false;
            };
            0 != counter(rule, &paren_depth(&pdef.name))
        }));
        let ops = grammar.clone();
        alt.add_action(move |rule: &mut Rule, _context: &mut Context| {
            close_paren(&ops, rule);
        });
        alt.g = "expr,expr-paren,close".into();
        prepend_close_all(spec, vec![alt]);

        spec.add_ac(|rule: &mut Rule, _context: &mut Context| {
            let node = node_of(rule);
            let parent = rule.parent_rule.clone();
            write_linked_node(parent.as_ref(), node.clone());
            let grandparent = parent.and_then(|parent| parent.parent_rule.clone());
            write_linked_node(grandparent.as_ref(), node);
        });
    });
}

fn open_paren(grammar: &Grammar, rule: &mut Rule) {
    let Some(op) = Grammar::op_use(&grammar.paren_open, rule.o0()) else {
        return;
    };
    let depth = paren_depth(&op.op.name);
    rule.u_mut().insert(depth.clone(), Value::Number(1.0));
    rule.n_mut().insert(depth, 1);
    set_node(rule, Value::Undefined);
}

fn close_paren(grammar: &Grammar, rule: &mut Rule) {
    let child = rule.child_node.clone();
    let mut node = node_of(rule);
    // The child's value, when this paren holds nothing of its own yet.
    //
    // The canonical port also takes the child whenever it is an operator,
    // and can, because it collects an implicit list IN the child's node
    // (`paren.child.node = [...]`): there the child IS the list. This
    // engine copies a container on write, so the list is collected on the
    // paren itself instead, while the child link stays on the rule the
    // paren PUSHED rather than on whichever rule of a replacement chain
    // popped last (tabnas/parser a801621, matching TS and Go). That rule
    // can still hold the list's first member, an operator. A node already
    // on the paren is the collected list, and the child must not overwrite
    // it: `(1?2:3 b)` would come back as `(1?2:3)`.
    if node.is_undefined() {
        node = child;
    }

    let Some(op) = Grammar::op_use(&grammar.paren_close, rule.c0()) else {
        set_node(rule, node);
        return;
    };
    let depth = paren_depth(&op.op.name);

    // Construct the completed paren expression.
    if local_number(rule, &depth) == i64::from(counter(rule, &depth)) {
        let mut terms: Vec<Value> = Vec::new();

        let grandparent = rule
            .parent_rule
            .as_ref()
            .and_then(|parent| parent.parent_rule.clone());
        if let Some(grandparent) = &grandparent {
            let preval = grandparent.node.borrow().clone();
            if matches!(grandparent.u.get("paren_preval"), Some(Value::Bool(true)))
                && !preval.is_undefined()
            {
                terms.push(preval);
            }
        }

        if !node.is_undefined() {
            terms.push(node);
        }

        set_node(rule, op.node(terms));
    } else {
        set_node(rule, node);
    }
}

// ---------------------------------------------------------------------------
// ternary
// ---------------------------------------------------------------------------

fn install_ternary(parser: &mut Tabnas, grammar: &Arc<Grammar>) {
    let grammar = grammar.clone();
    parser.define_rule("ternary", move |spec| {
        let mut open: Vec<AltSpec> = Vec::new();

        let mut alt = on(&grammar.tin_tern0);
        alt.p = Some("val".into());
        alt.n = counters(&[
            ("expr_ternary", 1),
            ("expr", 0),
            ("expr_prefix", 0),
            ("expr_suffix", 0),
        ]);
        alt.u = locals(&[("expr_ternary_step", Value::Number(1.0))]);
        alt.g = "expr,expr-ternary,open".into();
        let ops = grammar.clone();
        alt.add_action(move |rule: &mut Rule, _context: &mut Context| {
            let Some(op) = Grammar::op_use(&ops.ternary, rule.o0()) else {
                return;
            };
            rule.u_mut().insert(
                "expr_ternary_name".into(),
                Value::String(op.op.name.clone()),
            );

            let previous = linked_node(rule.prev_rule.as_ref());
            let node = if is_op(&previous) {
                let duplicate = dup_node(&previous);
                update_expr_node(
                    &previous,
                    Some(op.op.clone()),
                    op.token.clone(),
                    vec![duplicate],
                );
                previous
            } else {
                let node = op.node(vec![previous]);
                write_linked_node(rule.prev_rule.as_ref(), node.clone());
                node
            };
            set_node(rule, node);

            let paren = if 0 != counter(rule, "expr_paren") {
                i64::from(counter(rule, "expr_paren"))
            } else {
                rule.prev_rule
                    .as_ref()
                    .map_or(0, |prev| snapshot_number(prev, "expr_ternary_paren"))
            };
            rule.u_mut()
                .insert("expr_ternary_paren".into(), Value::Number(paren as f64));
            rule.n_mut().insert("expr_paren".into(), 0);
        });
        open.push(alt);

        let mut alt = always();
        alt.p = Some("val".into());
        alt.c_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
            rule.prev_rule
                .as_ref()
                .is_some_and(|prev| 2 == snapshot_number(prev, "expr_ternary_step"))
        }));
        alt.add_action(|rule: &mut Rule, _context: &mut Context| {
            let (step, paren) = rule.prev_rule.as_ref().map_or((0, 0), |prev| {
                (
                    snapshot_number(prev, "expr_ternary_step"),
                    snapshot_number(prev, "expr_ternary_paren"),
                )
            });
            set_local_number(rule, "expr_ternary_step", step);
            set_local_number(rule, "expr_ternary_paren", paren);
            rule.n_mut().insert("expr_paren".into(), paren as i32);
        });
        alt.g = "expr,expr-ternary,step".into();
        open.push(alt);

        prepend_open_all(spec, open);

        let mut close: Vec<AltSpec> = Vec::new();

        let mut alt = on(&grammar.tin_tern1);
        let condition = grammar.clone();
        alt.c_fn = Some(Arc::new(move |rule: &mut Rule, _context: &mut Context| {
            if 1 != local_number(rule, "expr_ternary_step") {
                return false;
            }
            let Some(op) = rule
                .c0()
                .and_then(|token| condition.ternary.get(&token.tin))
                .cloned()
            else {
                return false;
            };
            matches!(rule.u.get("expr_ternary_name"), Some(Value::String(name)) if name == &op.name)
        }));
        alt.r = Some("ternary".into());
        alt.add_action(|rule: &mut Rule, _context: &mut Context| {
            let step = local_number(rule, "expr_ternary_step");
            set_local_number(rule, "expr_ternary_step", step + 1);
            let node = node_of(rule);
            push_term(&node, rule.child_node.clone());
        });
        alt.g = "expr,expr-ternary,step".into();
        close.push(alt);

        // End of ternary at the top level, implicit list indicated by a
        // comma. Handle a ternary as the first item of an implicit list
        // inside parens.
        let mut tins = vec![TIN_CA];
        tins.extend(grammar.tin_close.iter().copied());
        let mut alt = on(&tins);
        alt.c_fn = Some(Arc::new(implicit_ternary_cond));
        let closers = grammar.tin_close.clone();
        alt.b_fn = Some(Arc::new(move |_rule: &mut Rule, context: &mut Context| {
            usize::from(next_is_close(context, &closers))
        }));
        let closers = grammar.tin_close.clone();
        alt.r_fn = Some(Arc::new(move |rule: &mut Rule, context: &mut Context| {
            let to_elem = !next_is_close(context, &closers)
                && (0 == rule.d
                    || (rule
                        .prev_rule
                        .as_ref()
                        .is_some_and(|prev| 0 != snapshot_number(prev, "expr_ternary_paren"))
                        && 0 == node_length(&linked_node(rule.parent_rule.as_ref()))));
            to_elem.then(|| "elem".to_string())
        }));
        alt.add_action_with_match(|rule: &mut Rule, _context: &mut Context, matched| {
            implicit_ternary_action(rule, matched);
            None
        });
        alt.g = "expr,expr-ternary,list,val,imp,comma".into();
        close.push(alt);

        // End of ternary at the top level, implicit list indicated by a
        // space separated value.
        let mut alt = always();
        alt.c_fn = Some(Arc::new(implicit_ternary_cond));
        let closers = grammar.tin_close.clone();
        alt.r_fn = Some(Arc::new(move |rule: &mut Rule, context: &mut Context| {
            let ended = context.t0().is_some_and(|token| TIN_ZZ == token.tin);
            let to_elem = (0 == rule.d
                || !next_is_close(context, &closers)
                || rule
                    .prev_rule
                    .as_ref()
                    .is_some_and(|prev| 0 != snapshot_number(prev, "expr_ternary_paren")))
                && 0 == node_length(&linked_node(rule.parent_rule.as_ref()))
                && !ended;
            to_elem.then(|| "elem".to_string())
        }));
        alt.add_action_with_match(|rule: &mut Rule, _context: &mut Context, matched| {
            implicit_ternary_action(rule, matched);
            None
        });
        alt.g = "expr,expr-ternary,list,val,imp,space".into();
        close.push(alt);

        // End of ternary.
        let mut alt = always();
        alt.c_fn = Some(Arc::new(|rule: &mut Rule, _context: &mut Context| {
            0 < rule.d && 2 == local_number(rule, "expr_ternary_step")
        }));
        alt.add_action(|rule: &mut Rule, _context: &mut Context| {
            let node = node_of(rule);
            push_term(&node, rule.child_node.clone());
        });
        alt.g = "expr,expr-ternary,close".into();
        close.push(alt);

        prepend_close_all(spec, close);

        // Ensure ternary results get evaluated. Without this, a ternary
        // that is not wrapped in an expr leaves its result as a raw
        // S-expression. Fire on every ternary instance's after-close, but
        // act only when the chain has reached its final step: the node
        // has accumulated all three operands and the next rule is not
        // another ternary. Write the evaluated value back along the
        // replacement chain so whichever node the parse returns reflects
        // the evaluated form.
        if let Some(evaluate) = grammar.evaluate.clone() {
            spec.add_ac(move |rule: &mut Rule, context: &mut Context| {
                if rule
                    .next_rule_name
                    .as_ref()
                    .is_some_and(|name| "ternary" == name.as_ref())
                {
                    return;
                }
                let node = node_of(rule);
                if !is_op(&node) {
                    return;
                }
                // The node is not fully populated until all three
                // operands sit under it; earlier steps carry fewer.
                if node_length(&node) < 4 {
                    return;
                }
                let out = {
                    // The canonical port hands the ternary rule itself to
                    // the evaluator here, where the expr rule hands its
                    // parent.
                    let site_rule = rule.snapshot();
                    let mut site = EvalSite::new(Some(site_rule), context);
                    evaluation(&mut site, &node, &evaluate)
                };
                publish(rule, out.clone());
                let mut cursor = rule.prev_rule.clone();
                while let Some(current) = cursor {
                    *current.node.borrow_mut() = out.clone();
                    cursor = current.prev_rule.clone();
                }
                write_linked_node(rule.parent_rule.as_ref(), out);
            });
        }
    });
}

fn snapshot_number(snapshot: &RuleSnapshot, name: &str) -> i64 {
    match snapshot.u.get(name) {
        Some(Value::Number(number)) => *number as i64,
        _ => 0,
    }
}

fn next_is_close(context: &mut Context, closers: &[Tin]) -> bool {
    context
        .t0()
        .is_some_and(|token| closers.contains(&token.tin))
}

fn implicit_ternary_cond(rule: &mut Rule, _context: &mut Context) -> bool {
    (0 == rule.d || 1 <= counter(rule, "expr_paren"))
        && 0 == counter(rule, "pk")
        && 2 == local_number(rule, "expr_ternary_step")
}

fn implicit_ternary_action(rule: &mut Rule, matched: &AltMatch) {
    let paren = rule
        .prev_rule
        .as_ref()
        .map_or(0, |prev| snapshot_number(prev, "expr_ternary_paren"));
    rule.n_mut().insert("expr_paren".into(), paren as i32);

    let node = node_of(rule);
    push_term(&node, rule.child_node.clone());

    if matched.r.as_deref() != Some("elem") {
        return;
    }

    // The completed ternary becomes the first element of an implicit
    // list. The canonical port rewrites the node into that list in place,
    // so that every holder sees it; here the list is written through the
    // shared node cell instead, which is what the replacing `elem` rule
    // is seeded from, and onto the enclosing paren, because a container
    // is copied on write and the two cannot share one array.
    let list = Value::array(vec![node]);
    publish(rule, list.clone());
    if 1 <= counter(rule, "expr_paren") {
        let mut cursor = rule.parent_rule.clone();
        while let Some(current) = cursor {
            if "paren" == current.name.as_ref() {
                *current.node.borrow_mut() = list;
                break;
            }
            cursor = current.parent_rule.clone();
        }
    }
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// The plugin, for [`Tabnas::use_plugin`], carrying the default operator
/// table as its declared defaults.
///
/// Call-site options are deep-merged over those defaults by the engine, so
/// an entry naming a default operator extends it and a `null` entry
/// deletes it, exactly as in the canonical TypeScript. An evaluator cannot
/// travel in a serialized option bag: pass one with [`plugin_with`].
///
/// ```
/// use tabnas::Value;
///
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let mut parser = tabnas_jsonic::make();
///     parser.use_plugin(tabnas_expr::plugin(), None)?;
///     let value = tabnas_expr::realize(&parser.parse("1+2")?);
///     assert_eq!(tabnas_expr::simplify(&value).to_string(), "[\"+\",1,2]");
///     Ok::<(), Box<dyn std::error::Error>>(())
/// }
/// ```
pub fn plugin() -> Plugin {
    Plugin::new(PLUGIN_NAME, |parser: &mut Tabnas, options: &Value| {
        expr(parser, &ExprOptions::from_value(options))
    })
    .with_defaults(defaults())
}

/// The plugin with typed options, which is how an evaluator and any
/// operator data that no `Value` can carry reach the grammar.
///
/// The typed options resolve the defaults themselves, so the serialized
/// option bag is not consulted: an operator given here replaces the
/// default of the same name outright rather than extending it.
pub fn plugin_with(options: ExprOptions) -> Plugin {
    Plugin::new(PLUGIN_NAME, move |parser: &mut Tabnas, _options: &Value| {
        expr(parser, &options)
    })
}

/// Install the expression grammar on a parser through
/// [`Tabnas::use_plugin`], so that a derived instance rebuilds it.
pub fn apply(parser: &mut Tabnas, options: ExprOptions) -> Result<(), PluginError> {
    parser.use_plugin(plugin_with(options), None).map(|_| ())
}

/// Build a parser with the jsonic base grammar and the default operator
/// table.
///
/// ```
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let parser = tabnas_expr::make();
///     let value = tabnas_expr::parse_with(&parser, "(1+2)*3")?;
///     assert_eq!(tabnas_expr::simplify(&value).to_string(),
///                r#"["*",["(",["+",1,2]],3]"#);
///     Ok(())
/// }
/// ```
pub fn make() -> Tabnas {
    make_with(ExprOptions::new())
}

/// Build a parser with the jsonic base grammar and the given options.
pub fn make_with(options: ExprOptions) -> Tabnas {
    let mut parser = tabnas_jsonic::make();
    parser
        .use_plugin(plugin_with(options), None)
        .expect("the expression grammar installs on the jsonic base");
    parser
}

/// The lazily built instance the no-options [`parse`] reuses.
///
/// Building the grammar dominates a parse, so a fresh instance per call is
/// many times slower (see `tests/perf_test.rs`). Parsing builds a fresh
/// context per call and only reads instance state, so one shared instance
/// is safe to use from several threads.
static DEFAULT_PARSER: OnceLock<Tabnas> = OnceLock::new();

/// Parse one document with the default operator table.
///
/// ```
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let value = tabnas_expr::parse("a:1+2")?;
///     assert_eq!(tabnas_expr::simplify(&value).to_string(),
///                r#"{"a":["+",1,2]}"#);
///     Ok(())
/// }
/// ```
pub fn parse(src: &str) -> Result<Value, TabnasError> {
    parse_with(DEFAULT_PARSER.get_or_init(make), src)
}

/// Parse one document with a parser this crate configured, and realize the
/// expression nodes in the result.
///
/// A parser driven directly through [`Tabnas::parse`] returns a value whose
/// expression nodes are still handles into the per-parse arena; pass that
/// value to [`realize`] before the next parse on the same thread.
pub fn parse_with(parser: &Tabnas, src: &str) -> Result<Value, TabnasError> {
    let _guard = ArenaGuard::enter();
    let value = parser.parse(src)?;
    Ok(realize(&value))
}

/// Parse one document and hand the unrealized value to a closure, with
/// the expression arena held open for as long as it runs.
///
/// This is the parse-once, evaluate-many workflow: inside the closure the
/// expression nodes are live, so [`evaluation`] can reduce them, and
/// whatever the closure returns outlives them.
///
/// The arena is released when this returns, so the closure must not let an
/// expression HANDLE escape in what it returns. Return a reduced value, as
/// the example below does, or call [`realize`] on the tree and return
/// that; [`parse_with`] is the same parse with the realizing already done.
/// A handle that escapes names a node that no longer exists, and
/// [`realize`] and [`simplify`] then PANIC rather than report the lost
/// expression as an empty array. The closure return type is a type
/// parameter, so nothing in the signature can realize it for a caller, and
/// nothing in it can stop a handle from being returned.
///
/// ```
/// use tabnas::Value;
/// use tabnas_expr::{evaluation, EvalSite, Evaluate, Op};
///
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let parser = tabnas_expr::make();
///     let add: Evaluate = std::sync::Arc::new(
///         |_site: &mut EvalSite<'_>, _op: &Op, terms: &[Value]| match terms {
///             [Value::Number(left), Value::Number(right)] => Value::Number(left + right),
///             _ => Value::Null,
///         },
///     );
///     let total = tabnas_expr::parse_scope(&parser, "1+2", |value| {
///         evaluation(&mut EvalSite::detached(), &value, &add)
///     })?;
///     assert_eq!(total, Value::Number(3.0));
///     Ok(())
/// }
/// ```
pub fn parse_scope<R>(
    parser: &Tabnas,
    src: &str,
    use_value: impl FnOnce(Value) -> R,
) -> Result<R, TabnasError> {
    let _guard = ArenaGuard::enter();
    let value = parser.parse(src)?;
    Ok(use_value(value))
}

/// Parse one document and reduce it to the S-expression form the shared
/// fixtures compare.
pub fn parse_simplified(parser: &Tabnas, src: &str) -> Result<Value, TabnasError> {
    let _guard = ArenaGuard::enter();
    let value = parser.parse(src)?;
    Ok(simplify(&value))
}
