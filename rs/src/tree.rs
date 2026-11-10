/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// The tree layer: key operators and `fold`, the port of the canonical
// layer in `ts/src/expr.ts` (`makeKeyMap`, `keyOpOf`, `FoldError`, `fold`
// and `foldTree`). The contract is the one the repository's `AGENTS.md`
// states under "The tree layer"; what is particular to this port is said
// where it is done, and summed up in `rs/AGENTS.md`.
//
// Three things shape it that the canonical does not have to think about.
//
// 1. The memory is a trie of locations held in one `Vec`, addressed by
//    index, never by a `Value`'s identity: an engine value is copied on
//    write, so identity says nothing about location.
// 2. Every callback can re-enter the fold (`site.at`, `site.plain`,
//    `site.k`, `site.locate`) while the fold is still inside an outer
//    callback. All state sits in one `RefCell`, and no borrow of it is
//    ever held across a callback.
// 3. A stack overflow is an abort here, not an exception. The walk over
//    nested containers is iterative, so nesting costs heap, not stack;
//    the recursion that remains is the re-entry through callbacks, one
//    level per reference hop, and `guard` turns a stack running low into
//    `fold_depth` before it can abort the process.

use std::borrow::Cow;
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use indexmap::IndexMap;
use tabnas::{TabnasError, Token, Value};

use crate::{
    new_node, node_id, released_handle, with_node, Op, OpDef, MAX_SAFE_INTEGER, MIN_SAFE_INTEGER,
};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A cycle met while [`FoldOptions::cycle`] is [`FoldCycle::Error`].
pub const FOLD_CYCLE: &str = "fold_cycle";
/// Nesting and reference hops past [`FoldOptions::max_depth`], or the
/// thread's stack running low before that.
pub const FOLD_DEPTH: &str = "fold_depth";
/// An `inherit` that returned no bag, or folded its own subtree.
pub const FOLD_INHERIT: &str = "fold_inherit";
/// An operator table with two key operators on one member name, or one
/// on none.
pub const FOLD_KEY: &str = "fold_key";
/// TypeScript's refusal of `positions: true` outside its deferred mode.
/// The Rust options cannot say that, so a Rust fold never raises it; the
/// code is declared so every runtime names the same set.
pub const FOLD_OPTION: &str = "fold_option";

/// The error [`fold`] returns: one of the layer's own failures, with its
/// code and location, or the error a callback returned, unchanged.
///
/// `code` is what a fixture pins: [`FOLD_CYCLE`], [`FOLD_DEPTH`],
/// [`FOLD_INHERIT`] or [`FOLD_KEY`] for the layer's own failures, and the
/// callback's own code for a callback's. The document key and the
/// pointers are fields; the message quotes them JSON-escaped and cut
/// short, since member names are document content.
///
/// `TabnasError::from` gives the callback's error back as it was, and
/// for the layer's own failures a new error carrying the code and the
/// message, so `?` works inside a callback, which returns a
/// `TabnasError`: the fold recognises its own failure coming back out of
/// a callback and returns it with its location.
#[derive(Debug, Clone, PartialEq)]
pub struct FoldError(Box<FoldErrorData>);

#[derive(Debug, Clone, PartialEq)]
struct FoldErrorData {
    code: String,
    message: String,
    doc: Option<String>,
    at: Option<String>,
    target_doc: Option<String>,
    target: Option<String>,
    cause: Option<TabnasError>,
}

impl FoldError {
    fn new(code: &str, message: impl fmt::Display) -> Self {
        FoldError(Box::new(FoldErrorData {
            code: code.to_string(),
            message: format!("expr fold: {message}"),
            doc: None,
            at: None,
            target_doc: None,
            target: None,
            cause: None,
        }))
    }

    fn callback(cause: TabnasError) -> Self {
        FoldError(Box::new(FoldErrorData {
            code: cause.code.clone(),
            message: cause.detail.clone(),
            doc: None,
            at: None,
            target_doc: None,
            target: None,
            cause: Some(cause),
        }))
    }

    fn located(mut self, doc: &str, at: Option<&str>) -> Self {
        self.0.doc = Some(doc.to_string());
        self.0.at = at.map(str::to_string);
        self
    }

    fn targeted(mut self, target_doc: &str, target: &str) -> Self {
        self.0.target_doc = Some(target_doc.to_string());
        self.0.target = Some(target.to_string());
        self
    }

    /// The code: one of the layer's own, or the failing callback's.
    pub fn code(&self) -> &str {
        &self.0.code
    }

    /// The message, which for the layer's own failures starts
    /// `expr fold: `.
    pub fn message(&self) -> &str {
        &self.0.message
    }

    /// The key of the document the failure happened in.
    pub fn doc(&self) -> Option<&str> {
        self.0.doc.as_deref()
    }

    /// The location of the failure in that document, as an RFC 6901
    /// pointer.
    pub fn at(&self) -> Option<&str> {
        self.0.at.as_deref()
    }

    /// For a cycle, the document of the location the site named.
    pub fn target_doc(&self) -> Option<&str> {
        self.0.target_doc.as_deref()
    }

    /// For a cycle, the location the site named, as an RFC 6901 pointer.
    pub fn target(&self) -> Option<&str> {
        self.0.target.as_deref()
    }

    /// The error a callback (`evaluate`, `inherit`, `recognise` or
    /// `load`) returned, when the failure was the callback's.
    pub fn cause(&self) -> Option<&TabnasError> {
        self.0.cause.as_ref()
    }
}

impl fmt::Display for FoldError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0.message)
    }
}

impl std::error::Error for FoldError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0
            .cause
            .as_ref()
            .map(|cause| cause as &(dyn std::error::Error + 'static))
    }
}

impl From<FoldError> for TabnasError {
    fn from(error: FoldError) -> Self {
        let data = *error.0;
        if let Some(cause) = data.cause {
            return cause;
        }
        // As the engine converts its own `ActionError`: no source position,
        // since a fold runs on a finished tree. The hint is cleared rather
        // than left as the engine's "unknown error code" text.
        let mut out = TabnasError::new(data.code, "", "", 0, 1, 1);
        out.detail = data.message;
        out.hint = String::new();
        out
    }
}

// ---------------------------------------------------------------------------
// Paths, hits and the report
// ---------------------------------------------------------------------------

/// One step of a [`Path`]: a member name, or an array index.
///
/// An index into a list is RFC 6901's: [`Seg::Index`], or a name that is
/// `0` or digits with no leading zero. A name that is anything else, `-`,
/// `01` or `1.0` among them, names nothing in a list. A number steps into
/// a map as the member of that name, as in TypeScript.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Seg {
    /// A member name, or an index written as a name.
    Name(String),
    /// An array index.
    Index(usize),
}

impl Seg {
    fn text(&self) -> Cow<'_, str> {
        match self {
            Seg::Name(name) => Cow::Borrowed(name),
            Seg::Index(index) => Cow::Owned(index.to_string()),
        }
    }
}

impl From<&str> for Seg {
    fn from(name: &str) -> Self {
        Seg::Name(name.to_string())
    }
}

impl From<String> for Seg {
    fn from(name: String) -> Self {
        Seg::Name(name)
    }
}

impl From<usize> for Seg {
    fn from(index: usize) -> Self {
        Seg::Index(index)
    }
}

impl fmt::Display for Seg {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.text())
    }
}

/// A location, as steps from a document's root.
pub type Path = Vec<Seg>;

/// What [`Site::at`] gives back: whether the location names anything,
/// whether reaching it closed a cycle, and, only when it was found and is
/// not a cycle, its folded value.
#[derive(Debug, Clone, PartialEq)]
pub struct FoldHit {
    pub found: bool,
    pub cycle: bool,
    pub value: Option<Value>,
}

impl FoldHit {
    fn missing() -> Self {
        FoldHit {
            found: false,
            cycle: false,
            value: None,
        }
    }

    fn cycle() -> Self {
        FoldHit {
            found: true,
            cycle: true,
            value: None,
        }
    }

    fn found(value: Value) -> Self {
        FoldHit {
            found: true,
            cycle: false,
            value: Some(value),
        }
    }
}

/// One report row: the site (`doc`, `at`) and the location it named
/// (`target_doc`, `target`), as document keys and RFC 6901 pointers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FoldEntry {
    pub doc: String,
    pub at: String,
    pub target_doc: String,
    pub target: String,
}

/// A dynamic key operator's site: recognised, reported, never evaluated.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FoldDynamic {
    pub doc: String,
    pub at: String,
    pub src: String,
}

/// What a fold met that it could not resolve, each row once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FoldReport {
    pub cycles: Vec<FoldEntry>,
    pub unresolved: Vec<FoldEntry>,
    pub dynamic: Vec<FoldDynamic>,
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// Reduce one site: a key operator with `[value, siblings]`, both already
/// folded, or, when [`FoldOptions::tokens`] is on, a token operator met in
/// the walk with its folded terms. What it returns is placed at the site as
/// it is.
pub type FoldEvaluate<'a> =
    Box<dyn Fn(&Site<'_>, &Op, &[Value]) -> Result<Value, TabnasError> + 'a>;

/// The bag a map's subtree inherits, from the bag its parent's subtree
/// sees: called with the site, the map, and the parent's bag, once per map
/// on a path the fold needs, from the root down. It returns a map, or at
/// least a container, as the canonical's `typeof === 'object'` admits;
/// anything else fails the fold with [`FOLD_INHERIT`].
pub type Inherit<'a> = Box<dyn Fn(&Site<'_>, &Value, &Value) -> Result<Value, TabnasError> + 'a>;

/// Whether a map that owns a key operator's member is an occurrence of it
/// here. A map for which this is false folds as a plain map, with no
/// `evaluate` call and no report row.
pub type Recognise<'a> = Box<dyn Fn(&Site<'_>, &Op) -> Result<bool, TabnasError> + 'a>;

/// Another document's raw root, by key: called once per key, `None` when
/// it cannot be had.
pub type Load<'a> = Box<dyn Fn(&str, &Site<'_>) -> Result<Option<Value>, TabnasError> + 'a>;

/// The opening token of a container, when the host can say: see
/// [`Site::token`].
pub type Positions<'a> = Box<dyn Fn(&Value) -> Option<Token> + 'a>;

/// What a fold does on meeting a cycle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FoldCycle {
    /// Tell the resolver: [`Site::at`] answers with `cycle: true`.
    #[default]
    Verbatim,
    /// Fail the fold with [`FOLD_CYCLE`].
    Error,
}

/// The options of one [`fold`]. Only `evaluate` is required; build with
/// [`FoldOptions::new`] and the `with_` methods.
///
/// The callbacks are plain closures that may borrow from the caller's
/// scope, for the life `'a` of the options: a fold runs to completion
/// inside the call.
pub struct FoldOptions<'a> {
    /// The operator table; only its key operators matter to a fold. The
    /// same table the plugin was given can be passed as it is.
    pub op: IndexMap<String, Option<OpDef>>,
    pub evaluate: FoldEvaluate<'a>,
    pub inherit: Option<Inherit<'a>>,
    pub recognise: Option<Recognise<'a>>,
    /// The starting bag of every document's root. An empty map by default.
    pub k: Value,
    /// The key of the document being folded. Empty by default.
    pub doc: String,
    pub load: Option<Load<'a>>,
    pub cycle: FoldCycle,
    /// Reduce token operator nodes met in the walk through `evaluate`
    /// too, so one fold handles `+` and `$ref`. On by default. A token
    /// operator is recognised only on a LIVE expression node, inside
    /// [`crate::parse_scope`]: see [`fold`].
    pub tokens: bool,
    pub positions: Option<Positions<'a>>,
    /// How deep the walk may go, nesting and reference hops counted
    /// together. 1000 by default.
    pub max_depth: usize,
    /// How much stack, in bytes, the fold may use, measured from the
    /// outermost fold running on the thread: [`FOLD_MAX_STACK`], a
    /// megabyte, by default. Past it the fold fails with [`FOLD_DEPTH`]
    /// rather than overflow the stack, which would end the process. A
    /// level of nesting costs no stack, since the walk over containers is
    /// iterative; each reference hop does, since it re-enters the fold
    /// through the resolver. Raise it only on a thread known to have more
    /// than twice as much stack.
    pub max_stack: usize,
    /// Called once with the report when the fold succeeds.
    pub report: Option<Box<dyn Fn(FoldReport) + 'a>>,
}

impl<'a> FoldOptions<'a> {
    /// Options with the given resolver and every default.
    pub fn new(
        evaluate: impl Fn(&Site<'_>, &Op, &[Value]) -> Result<Value, TabnasError> + 'a,
    ) -> Self {
        FoldOptions {
            op: IndexMap::new(),
            evaluate: Box::new(evaluate),
            inherit: None,
            recognise: None,
            k: Value::object(IndexMap::new()),
            doc: String::new(),
            load: None,
            cycle: FoldCycle::Verbatim,
            tokens: true,
            positions: None,
            max_depth: 1000,
            max_stack: FOLD_MAX_STACK,
            report: None,
        }
    }

    /// Add or replace one operator definition.
    pub fn with_op(mut self, name: impl Into<String>, def: OpDef) -> Self {
        self.op.insert(name.into(), Some(def));
        self
    }

    /// Use a whole operator table, such as the one an `ExprOptions`
    /// carries.
    pub fn with_ops(mut self, op: IndexMap<String, Option<OpDef>>) -> Self {
        self.op = op;
        self
    }

    pub fn with_inherit(
        mut self,
        inherit: impl Fn(&Site<'_>, &Value, &Value) -> Result<Value, TabnasError> + 'a,
    ) -> Self {
        self.inherit = Some(Box::new(inherit));
        self
    }

    pub fn with_recognise(
        mut self,
        recognise: impl Fn(&Site<'_>, &Op) -> Result<bool, TabnasError> + 'a,
    ) -> Self {
        self.recognise = Some(Box::new(recognise));
        self
    }

    pub fn with_k(mut self, k: Value) -> Self {
        self.k = k;
        self
    }

    pub fn with_doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = doc.into();
        self
    }

    pub fn with_load(
        mut self,
        load: impl Fn(&str, &Site<'_>) -> Result<Option<Value>, TabnasError> + 'a,
    ) -> Self {
        self.load = Some(Box::new(load));
        self
    }

    pub fn with_cycle(mut self, cycle: FoldCycle) -> Self {
        self.cycle = cycle;
        self
    }

    pub fn with_tokens(mut self, tokens: bool) -> Self {
        self.tokens = tokens;
        self
    }

    pub fn with_positions(mut self, positions: impl Fn(&Value) -> Option<Token> + 'a) -> Self {
        self.positions = Some(Box::new(positions));
        self
    }

    pub fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }

    pub fn with_max_stack(mut self, max_stack: usize) -> Self {
        self.max_stack = max_stack;
        self
    }

    pub fn with_report(mut self, report: impl Fn(FoldReport) + 'a) -> Self {
        self.report = Some(Box::new(report));
        self
    }
}

impl fmt::Debug for FoldOptions<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let function = |present: bool| if present { "<function>" } else { "none" };
        formatter
            .debug_struct("FoldOptions")
            .field("op", &self.op)
            .field("evaluate", &"<function>")
            .field("inherit", &function(self.inherit.is_some()))
            .field("recognise", &function(self.recognise.is_some()))
            .field("k", &self.k)
            .field("doc", &self.doc)
            .field("load", &function(self.load.is_some()))
            .field("cycle", &self.cycle)
            .field("tokens", &self.tokens)
            .field("positions", &function(self.positions.is_some()))
            .field("max_depth", &self.max_depth)
            .field("max_stack", &self.max_stack)
            .field("report", &function(self.report.is_some()))
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Key operators
// ---------------------------------------------------------------------------

/// The key operators of a table, in the order a map that owns two of their
/// members is resolved by: static operators before dynamic ones, then
/// member names in code-point order. Neither declaration order nor member
/// order is used, so every runtime resolves such a map alike.
#[derive(Debug, Clone, Default)]
pub struct KeyMap {
    ops: Vec<Arc<Op>>,
}

impl KeyMap {
    /// The operators, in resolution order.
    pub fn iter(&self) -> std::slice::Iter<'_, Arc<Op>> {
        self.ops.iter()
    }

    /// The operator a member name marks.
    pub fn get(&self, src: &str) -> Option<&Arc<Op>> {
        self.ops.iter().find(|op| op.src == src)
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

/// The key operators of an operator table, the port of the canonical
/// `makeKeyMap`. A key operator carries no token, no tin and no binding
/// power; its `terms` is 2, the member's value and the sibling map. Two
/// key operators on one member name, or one naming no member, are refused
/// with [`FOLD_KEY`].
pub fn make_key_map(ops: &IndexMap<String, Option<OpDef>>) -> Result<KeyMap, FoldError> {
    let mut found: Vec<Op> = Vec::new();
    let mut owner: HashMap<&str, &str> = HashMap::new();
    for (name, def) in ops {
        let Some(def) = def else { continue };
        if !def.key {
            continue;
        }
        let src = def.src.first().map(String::as_str).unwrap_or_default();
        if src.is_empty() {
            return Err(FoldError::new(
                FOLD_KEY,
                format_args!("key operator {} names no member", shown(name)),
            ));
        }
        if let Some(prior) = owner.get(src) {
            return Err(FoldError::new(
                FOLD_KEY,
                format_args!(
                    "key operators {} and {} both name the member {}",
                    shown(prior),
                    shown(name),
                    shown(src)
                ),
            ));
        }
        owner.insert(src, name);
        found.push(Op {
            name: if name.ends_with("-key") {
                name.clone()
            } else {
                format!("{name}-key")
            },
            src: src.to_string(),
            left: MIN_SAFE_INTEGER,
            right: MAX_SAFE_INTEGER,
            use_data: def.use_data.clone(),
            terms: 2,
            key: true,
            dynamic: def.dynamic,
            ..Default::default()
        });
    }
    // `false` sorts before `true`, so static operators come first; `str`
    // compares by byte, which for UTF-8 is code-point order.
    found.sort_by(|a, b| a.dynamic.cmp(&b.dynamic).then_with(|| a.src.cmp(&b.src)));
    Ok(KeyMap {
        ops: found.into_iter().map(Arc::new).collect(),
    })
}

/// The key operator a map is an occurrence of, by shape: the first, in the
/// table's order, whose member the map owns. `None` for anything else,
/// lists and scalars included.
pub fn key_op_of(keys: &KeyMap, node: &Value) -> Option<Arc<Op>> {
    let members = map_entries(node)?;
    keys.ops
        .iter()
        .find(|op| members.contains_key(op.src.as_str()))
        .cloned()
}

// ---------------------------------------------------------------------------
// The site
// ---------------------------------------------------------------------------

/// What [`fold`] hands its callbacks for one map, or one token operator
/// node: where it is, what it is, and the ways to reach the rest of the
/// tree. `at`, `plain`, `locate` and `k` re-enter the fold, which is
/// what a resolver does to fold the target of a reference.
pub struct Site<'f> {
    fold: &'f Folder<'f>,
    pn: usize,
    parent_k: Option<Value>,
    doc: Rc<str>,
    root: Value,
    node: Value,
    path: OnceCell<Path>,
}

impl Site<'_> {
    /// The verbatim root of the site's document.
    pub fn root(&self) -> &Value {
        &self.root
    }

    /// The key of the site's document.
    pub fn doc(&self) -> &str {
        &self.doc
    }

    /// The path from the document's root to the site.
    pub fn path(&self) -> &[Seg] {
        self.path.get_or_init(|| self.fold.path_of(self.pn))
    }

    /// The verbatim node at the site: the map, or the operator node.
    pub fn node(&self) -> &Value {
        &self.node
    }

    /// The bag inherited along the path: [`FoldOptions::inherit`] applied
    /// at every map from the root down to and including this one, each
    /// map's bag kept, so a site reached early through a reference gets
    /// its own ancestors' bag, never the referrer's. Inside an `inherit`
    /// call it is the parent's bag.
    pub fn k(&self) -> Result<Value, FoldError> {
        match &self.parent_k {
            Some(k) => Ok(k.clone()),
            None => self.fold.k_of(self.pn),
        }
    }

    /// A position for the site. At a token operator site, the token the
    /// operator OCCURRENCE was matched from, as [`crate::EvalSite::token`]
    /// gives it. At a map, what [`FoldOptions::positions`] says, if it is
    /// set: this crate records no opening tokens of its own, so without a
    /// host's lookup it is `None`.
    pub fn token(&self) -> Option<Token> {
        if node_id(&self.node).is_some() {
            return with_node(&self.node, |entry| entry.token.clone()).flatten();
        }
        self.fold
            .options
            .positions
            .as_ref()
            .and_then(|lookup| lookup(&self.node))
    }

    /// The folded value at `path` in this document, or in the document
    /// `doc`, through any key operator site on the way: a pointer may pass
    /// through a `$ref` to reach a member of its target. A target still
    /// being folded closes a cycle, and so does a path through a site that
    /// was cut. A hit never carries a value when `cycle` is set.
    pub fn at(&self, path: &[Seg], doc: Option<&str>) -> Result<FoldHit, FoldError> {
        self.fold.at(self.pn, path, doc)
    }

    /// The raw value at `path`, following nothing.
    pub fn locate(&self, path: &[Seg], doc: Option<&str>) -> Result<Option<Value>, FoldError> {
        self.fold.locate(self.pn, path, doc)
    }

    /// This map folded as a plain map, members in their own order, which is
    /// what a resolver returns where the operator does not apply.
    pub fn plain(&self) -> Result<Value, FoldError> {
        self.fold.plain_of(self.pn)
    }
}

impl fmt::Debug for Site<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Site")
            .field("doc", &self.doc)
            .field("path", &self.path())
            .field("node", &self.node)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// The fold
// ---------------------------------------------------------------------------

/// Fold a finished tree: walk it, reduce every key operator site bottom-up
/// through `options.evaluate`, and return a NEW tree; the input is never
/// changed, though what a resolver returns, `site.node()` among it, is
/// placed in the output as it is.
///
/// Values flow up through `evaluate`; context flows down through
/// `inherit`, worked out per path from each map's own ancestors and kept.
/// One result per location, so every site naming one target shares one
/// folded value. Every container being folded is on an active stack, and a
/// site whose target is on it closes a cycle, which the resolver is told
/// about (or which fails the fold, under [`FoldCycle::Error`]). Dynamic key
/// operators are reported and folded as plain maps.
///
/// A TOKEN operator is recognised only on a live expression node, so a
/// fold that should reduce `+` as well as `$ref` runs inside
/// [`crate::parse_scope`], on the value the parse hands the closure. A
/// realized array, the operator description followed by its terms, is
/// plain data here and is never read as an operator: a document can spell
/// exactly that array, and reading it would let untrusted input forge one.
///
/// `max_depth` bounds nesting and reference hops together. The walk over
/// nested containers is iterative, so nesting costs no stack. Each
/// reference hop re-enters the fold through the resolver, and the fold
/// also fails with [`FOLD_DEPTH`] once the hops have used
/// [`FoldOptions::max_stack`] bytes of stack, a megabyte by default, rather
/// than let a thread with the default 2 MiB overflow, which would end the
/// process.
pub fn fold(root: &Value, options: &FoldOptions<'_>) -> Result<Value, FoldError> {
    let keys = make_key_map(&options.op)?;
    let base = StackBase::enter();
    let folder = Folder {
        options,
        keys,
        stack_base: base.at,
        main: Rc::from(options.doc.as_str()),
        state: RefCell::new(State::default()),
    };
    let doc = folder.make_doc(&options.doc, root.clone());
    let top = folder.state.borrow().docs[doc].top;
    let out = folder.walk(top, FROM_PARENT)?;
    if let Some(report) = &options.report {
        let rows = std::mem::take(&mut folder.state.borrow_mut().report);
        report(rows);
    }
    Ok(out)
}

// How a location stands: not folded yet, being folded (on the active
// stack), or folded and kept.
const FREE: u8 = 0;
const ACTIVE: u8 = 1;
const DONE: u8 = 2;

// How the walk entered a container: from its parent, or through a
// reference.
const FROM_PARENT: u8 = 0;
const FROM_REFERENCE: u8 = 1;

// The report lists, for the de-duplication key.
const CYCLES: u8 = 0;
const UNRESOLVED: u8 = 1;
const DYNAMIC: u8 = 2;

/// The default of [`FoldOptions::max_stack`]: a megabyte, which leaves the
/// other half of a default 2 MiB thread for the fold's caller and for the
/// frames of the callback that runs past the last check.
pub const FOLD_MAX_STACK: usize = 1 << 20;

/// The engine of one fold. Every method takes `&self`, so a callback can
/// re-enter it through its [`Site`]; the state is behind one `RefCell`,
/// and no borrow of it is held across a callback.
struct Folder<'f> {
    options: &'f FoldOptions<'f>,
    keys: KeyMap,
    stack_base: usize,
    main: Rc<str>,
    state: RefCell<State>,
}

#[derive(Default)]
struct State {
    /// The trie of locations, every document's, by index.
    nodes: Vec<PNode>,
    docs: Vec<Doc>,
    /// Every document key asked for, and its document when it could be
    /// had.
    doc_keys: HashMap<String, Option<usize>>,
    /// The active stack: every container being folded, outermost first.
    stack: Vec<usize>,
    report: FoldReport,
    noted: HashSet<(u8, String, String, String, String)>,
    /// The last of the layer's own failures, so it can be recognised when
    /// it comes back out of a callback as a `TabnasError`.
    failure: Option<FoldError>,
}

struct Doc {
    key: Rc<str>,
    root: Value,
    top: usize,
}

/// One location the fold has reached: a node of the trie.
struct PNode {
    doc: usize,
    parent: Option<usize>,
    seg: Seg,
    raw: Value,
    kids: Option<Box<Kids>>,
    state: u8,
    via: u8,
    value: Value,
    k: Option<Value>,
    k_busy: bool,
    /// Its resolution met a cycle, so the resolver was told `cycle: true`.
    cut: bool,
    /// A member of a loop of references with no container in it, or a
    /// reference into one: there is nothing to resolve it to.
    looped: bool,
    /// The key operator this map is an occurrence of, decided once: shape,
    /// then the host's `recognise`.
    site: Option<Option<Arc<Op>>>,
    parts: Option<Box<Parts>>,
    plain: Option<Value>,
    ptr: Option<Rc<str>>,
}

impl PNode {
    fn new(doc: usize, parent: Option<usize>, seg: Seg, raw: Value) -> Self {
        PNode {
            doc,
            parent,
            seg,
            raw,
            kids: None,
            state: FREE,
            via: FROM_PARENT,
            value: Value::Undefined,
            k: None,
            k_busy: false,
            cut: false,
            looped: false,
            site: None,
            parts: None,
            plain: None,
            ptr: None,
        }
    }
}

/// A node's children by step: a map's by member name, a list's by index.
enum Kids {
    Names(HashMap<String, usize>),
    Indices(HashMap<usize, usize>),
}

/// A site's terms, kept for `site.plain()`.
struct Parts {
    src: String,
    value: Value,
    siblings: Value,
}

/// The stack address the outermost fold on this thread started from.
struct StackBase {
    at: usize,
    outermost: bool,
}

thread_local! {
    static STACK_BASE: Cell<Option<usize>> = const { Cell::new(None) };
}

impl StackBase {
    fn enter() -> Self {
        let here = stack_here();
        STACK_BASE.with(|base| match base.get() {
            Some(at) => StackBase {
                at,
                outermost: false,
            },
            None => {
                base.set(Some(here));
                StackBase {
                    at: here,
                    outermost: true,
                }
            }
        })
    }
}

impl Drop for StackBase {
    fn drop(&mut self) {
        if self.outermost {
            STACK_BASE.with(|base| base.set(None));
        }
    }
}

/// The address of a local in a frame of its own: where the stack is now.
#[inline(never)]
fn stack_here() -> usize {
    let marker = 0u8;
    std::hint::black_box(&marker) as *const u8 as usize
}

/// A container in the middle of being walked: its members are taken one at
/// a time, and the folded ones collected.
struct Frame {
    pn: usize,
    raw: Value,
    next: usize,
    kind: Kind,
}

enum Kind {
    /// A list.
    List { out: Vec<Value> },
    /// A live expression node: its operator and occurrence token, and its
    /// terms, at path indices from 1 when it has an operator, the head
    /// taking index 0. A node with no operator is a ternary the grammar
    /// rewrote into a plain list of its terms, in place.
    Node {
        op: Option<Arc<Op>>,
        terms: Vec<Value>,
        out: Vec<Value>,
    },
    /// A map that is not a static site.
    Plain { out: IndexMap<String, Value> },
    /// A key operator site.
    Site {
        op: Arc<Op>,
        value: Value,
        siblings: IndexMap<String, Value>,
    },
}

/// A member taken from a frame: its index, or the place of its name in the
/// map.
enum Member {
    Index(usize),
    Name(usize),
}

/// How far a path reaches before the fold has to walk.
enum Found {
    /// An answer, with nothing to walk.
    Hit(FoldHit),
    /// A static site part way along: walk it, then read the rest of the
    /// path, from this step on, in its folded value.
    Through(usize, usize),
    /// The location at the end of the path: walk it.
    End(usize),
}

impl Frame {
    fn take(&mut self) -> Option<(Member, Value)> {
        let at = self.next;
        let taken = match &self.kind {
            Kind::List { .. } => list_items(&self.raw)
                .get(at)
                .map(|item| (Member::Index(at), item.clone())),
            Kind::Node { op, terms, .. } => terms
                .get(at)
                .map(|term| (Member::Index(at + usize::from(op.is_some())), term.clone())),
            Kind::Plain { .. } | Kind::Site { .. } => map_entries(&self.raw)
                .and_then(|members| members.get_index(at))
                .map(|(_, member)| (Member::Name(at), member.clone())),
        };
        if taken.is_some() {
            self.next += 1;
        }
        taken
    }

    fn accept(&mut self, value: Value) {
        let at = self.next - 1;
        let Frame { raw, kind, .. } = self;
        match kind {
            Kind::List { out } | Kind::Node { out, .. } => out.push(value),
            Kind::Plain { out } => {
                out.insert(member_name(raw, at).to_string(), value);
            }
            Kind::Site {
                op,
                value: slot,
                siblings,
            } => {
                let name = member_name(raw, at);
                if name == op.src {
                    *slot = value;
                } else {
                    siblings.insert(name.to_string(), value);
                }
            }
        }
    }
}

impl Folder<'_> {
    fn site(&self, pn: usize, parent_k: Option<Value>) -> Site<'_> {
        let state = self.state.borrow();
        let node = &state.nodes[pn];
        let doc = &state.docs[node.doc];
        Site {
            fold: self,
            pn,
            parent_k,
            doc: doc.key.clone(),
            root: doc.root.clone(),
            node: node.raw.clone(),
            path: OnceCell::new(),
        }
    }

    fn make_doc(&self, key: &str, raw: Value) -> usize {
        let mut state = self.state.borrow_mut();
        let doc = state.docs.len();
        let top = state.nodes.len();
        state
            .nodes
            .push(PNode::new(doc, None, Seg::Name(String::new()), raw.clone()));
        state.docs.push(Doc {
            key: Rc::from(key),
            root: raw,
            top,
        });
        state.doc_keys.insert(key.to_string(), Some(doc));
        doc
    }

    fn doc_key(&self, pn: usize) -> Rc<str> {
        let state = self.state.borrow();
        state.docs[state.nodes[pn].doc].key.clone()
    }

    /// Another document, by key, loaded once; `None` when it cannot be had.
    fn doc_of(&self, from: usize, key: Option<&str>) -> Result<Option<usize>, FoldError> {
        let own = self.state.borrow().nodes[from].doc;
        let Some(key) = key else {
            return Ok(Some(own));
        };
        if *self.doc_key(from) == *key {
            return Ok(Some(own));
        }
        let known = self.state.borrow().doc_keys.get(key).copied();
        if let Some(known) = known {
            return Ok(known);
        }
        let raw = match &self.options.load {
            Some(load) => {
                self.guard()?;
                load(key, &self.site(from, None)).map_err(|error| self.callback_error(&error))?
            }
            None => None,
        };
        let doc = match raw {
            Some(raw) if !raw.is_undefined() => Some(self.make_doc(key, raw)),
            _ => None,
        };
        self.state
            .borrow_mut()
            .doc_keys
            .insert(key.to_string(), doc);
        Ok(doc)
    }

    fn kid_named(&self, pn: usize, name: &str, raw: Value) -> usize {
        let mut state = self.state.borrow_mut();
        let found = match state.nodes[pn].kids.as_deref() {
            Some(Kids::Names(names)) => names.get(name).copied(),
            _ => None,
        };
        if let Some(found) = found {
            return found;
        }
        let id = state.nodes.len();
        let doc = state.nodes[pn].doc;
        state
            .nodes
            .push(PNode::new(doc, Some(pn), Seg::Name(name.to_string()), raw));
        let kids = state.nodes[pn]
            .kids
            .get_or_insert_with(|| Box::new(Kids::Names(HashMap::new())));
        if let Kids::Names(names) = &mut **kids {
            names.insert(name.to_string(), id);
        }
        id
    }

    fn kid_indexed(&self, pn: usize, index: usize, raw: Value) -> usize {
        let mut state = self.state.borrow_mut();
        let found = match state.nodes[pn].kids.as_deref() {
            Some(Kids::Indices(indices)) => indices.get(&index).copied(),
            _ => None,
        };
        if let Some(found) = found {
            return found;
        }
        let id = state.nodes.len();
        let doc = state.nodes[pn].doc;
        state
            .nodes
            .push(PNode::new(doc, Some(pn), Seg::Index(index), raw));
        let kids = state.nodes[pn]
            .kids
            .get_or_insert_with(|| Box::new(Kids::Indices(HashMap::new())));
        if let Kids::Indices(indices) = &mut **kids {
            indices.insert(index, id);
        }
        id
    }

    fn kid_of(&self, pn: usize, raw: &Value, member: Member, child: Value) -> usize {
        match member {
            Member::Index(index) => self.kid_indexed(pn, index, child),
            Member::Name(at) => self.kid_named(pn, member_name(raw, at), child),
        }
    }

    /// A location's RFC 6901 pointer, made only for report rows and errors
    /// and kept once made.
    fn ptr_of(&self, pn: usize) -> Rc<str> {
        let mut state = self.state.borrow_mut();
        let state = &mut *state;
        if let Some(done) = &state.nodes[pn].ptr {
            return done.clone();
        }
        let mut chain = Vec::new();
        let mut cursor = Some(pn);
        while let Some(at) = cursor {
            if state.nodes[at].ptr.is_some() {
                break;
            }
            chain.push(at);
            cursor = state.nodes[at].parent;
        }
        let mut text = cursor
            .and_then(|at| state.nodes[at].ptr.as_deref().map(str::to_string))
            .unwrap_or_default();
        for &at in chain.iter().rev() {
            let node = &mut state.nodes[at];
            if node.parent.is_none() {
                text.clear();
            } else {
                text.push('/');
                escape_segment(&mut text, &node.seg.text());
            }
            node.ptr = Some(Rc::from(text.as_str()));
        }
        state.nodes[pn].ptr.clone().unwrap_or_else(|| Rc::from(""))
    }

    fn path_of(&self, pn: usize) -> Path {
        let state = self.state.borrow();
        let mut path = Vec::new();
        let mut cursor = Some(pn);
        while let Some(at) = cursor {
            let node = &state.nodes[at];
            if node.parent.is_none() {
                break;
            }
            path.push(node.seg.clone());
            cursor = node.parent;
        }
        path.reverse();
        path
    }

    fn note(&self, list: u8, row: FoldEntry) {
        let mut state = self.state.borrow_mut();
        let key = (
            list,
            row.doc.clone(),
            row.at.clone(),
            row.target_doc.clone(),
            row.target.clone(),
        );
        if state.noted.insert(key) {
            if CYCLES == list {
                state.report.cycles.push(row);
            } else {
                state.report.unresolved.push(row);
            }
        }
    }

    fn note_dynamic(&self, pn: usize, src: &str) {
        let row = FoldDynamic {
            doc: self.doc_key(pn).to_string(),
            at: self.ptr_of(pn).to_string(),
            src: src.to_string(),
        };
        let mut state = self.state.borrow_mut();
        let key = (
            DYNAMIC,
            row.doc.clone(),
            row.at.clone(),
            row.src.clone(),
            String::new(),
        );
        if state.noted.insert(key) {
            state.report.dynamic.push(row);
        }
    }

    /// Record one of the layer's own failures, and return it.
    fn raise(&self, error: FoldError) -> FoldError {
        self.state.borrow_mut().failure = Some(error.clone());
        error
    }

    /// A callback's error as the fold's: the layer's own failure, with its
    /// location, when that is what came back out of the callback, and the
    /// callback's error, unchanged, otherwise.
    fn callback_error(&self, error: &TabnasError) -> FoldError {
        let own = self.state.borrow().failure.clone();
        match own {
            Some(own) if own.code() == error.code && own.message() == error.detail => own,
            _ => FoldError::callback(error.clone()),
        }
    }

    /// Fail with `fold_depth` before the re-entry through callbacks can
    /// run the thread's stack out.
    fn guard(&self) -> Result<(), FoldError> {
        if stack_here().abs_diff(self.stack_base) <= self.options.max_stack {
            return Ok(());
        }
        let error = FoldError::new(
            FOLD_DEPTH,
            format_args!(
                "the stack ran out before max_depth ({}) was reached; lower max_depth",
                self.options.max_depth
            ),
        )
        .located(&self.main, None);
        Err(self.raise(error))
    }

    /// The bag at a map: the `inherit` calls of its ancestors and of the
    /// map itself, from the root down, each kept, worked out without
    /// recursion.
    fn k_of(&self, pn: usize) -> Result<Value, FoldError> {
        let Some(inherit) = &self.options.inherit else {
            return Ok(self.options.k.clone());
        };
        self.guard()?;
        let (chain, mut bag) = {
            let state = self.state.borrow();
            if let Some(bag) = &state.nodes[pn].k {
                return Ok(bag.clone());
            }
            let mut chain = Vec::new();
            let mut cursor = Some(pn);
            let mut busy = None;
            while let Some(at) = cursor {
                let node = &state.nodes[at];
                if node.k.is_some() {
                    break;
                }
                if node.k_busy {
                    busy = Some(at);
                    break;
                }
                chain.push(at);
                cursor = node.parent;
            }
            match busy {
                Some(at) => (Err(at), Value::Undefined),
                None => (
                    Ok(chain),
                    cursor
                        .and_then(|at| state.nodes[at].k.clone())
                        .unwrap_or_else(|| self.options.k.clone()),
                ),
            }
        };
        let chain = match chain {
            Ok(chain) => chain,
            Err(at) => return Err(self.inherit_error("inherit folded its own subtree at", at)),
        };
        for &at in chain.iter().rev() {
            let raw = self.state.borrow().nodes[at].raw.clone();
            if is_record(&raw) {
                self.state.borrow_mut().nodes[at].k_busy = true;
                let next = inherit(&self.site(at, Some(bag.clone())), &raw, &bag);
                self.state.borrow_mut().nodes[at].k_busy = false;
                let next = next.map_err(|error| self.callback_error(&error))?;
                // A container, as the canonical's `typeof next === 'object'`
                // admits; anything else is no bag.
                if !is_container(&next) {
                    return Err(self.inherit_error("inherit returned no bag at", at));
                }
                bag = next;
            }
            self.state.borrow_mut().nodes[at].k = Some(bag.clone());
        }
        Ok(bag)
    }

    fn inherit_error(&self, what: &str, at: usize) -> FoldError {
        let pointer = self.ptr_of(at);
        let doc = self.doc_key(at);
        let error = FoldError::new(FOLD_INHERIT, format_args!("{what} {}", shown(&pointer)))
            .located(&doc, Some(&pointer));
        self.raise(error)
    }

    /// The key operator a map is an occurrence of here, if any: its shape,
    /// then the host's `recognise`, decided once per location.
    fn site_op_of(&self, pn: usize) -> Result<Option<Arc<Op>>, FoldError> {
        let raw = {
            let state = self.state.borrow();
            let node = &state.nodes[pn];
            if let Some(decided) = &node.site {
                return Ok(decided.clone());
            }
            node.raw.clone()
        };
        let op = key_op_of(&self.keys, &raw);
        self.state.borrow_mut().nodes[pn].site = Some(op.clone());
        if let (Some(found), Some(recognise)) = (&op, &self.options.recognise) {
            let yes = recognise(&self.site(pn, None), found)
                .map_err(|error| self.callback_error(&error))?;
            if !yes {
                self.state.borrow_mut().nodes[pn].site = Some(None);
                return Ok(None);
            }
        }
        Ok(op)
    }

    fn miss(&self, from: usize, key: Option<&str>, target: &[Seg]) -> FoldHit {
        let doc = self.doc_key(from);
        let row = FoldEntry {
            doc: doc.to_string(),
            at: self.ptr_of(from).to_string(),
            target_doc: key.unwrap_or(&doc).to_string(),
            target: pointer(target),
        };
        self.note(UNRESOLVED, row);
        FoldHit::missing()
    }

    /// A cycle: `from` named `target` while `target` was still being folded
    /// (a back edge), or `target` is in a loop of references. Every member
    /// of a loop made only of references is a cycle, and so is every
    /// reference into one, since none of them has anything to resolve to.
    /// A cycle that passes through a container is cut once, at the site
    /// whose target was being folded; a later reference to that site gets
    /// its folded value.
    fn cycle_to(&self, from: usize, target: usize) -> Result<FoldHit, FoldError> {
        let row = FoldEntry {
            doc: self.doc_key(from).to_string(),
            at: self.ptr_of(from).to_string(),
            target_doc: self.doc_key(target).to_string(),
            target: self.ptr_of(target).to_string(),
        };
        if FoldCycle::Error == self.options.cycle {
            let error = FoldError::new(
                FOLD_CYCLE,
                format_args!("cycle at {} to {}", shown(&row.at), shown(&row.target)),
            )
            .located(&row.doc, Some(&row.at))
            .targeted(&row.target_doc, &row.target);
            return Err(self.raise(error));
        }
        self.note(CYCLES, row);
        let mut state = self.state.borrow_mut();
        let state = &mut *state;
        state.nodes[from].cut = true;
        if ACTIVE == state.nodes[target].state {
            if let Some(start) = state.stack.iter().rposition(|&entry| entry == target) {
                let refs = state.stack[start + 1..]
                    .iter()
                    .all(|&entry| FROM_REFERENCE == state.nodes[entry].via);
                if refs {
                    for &entry in &state.stack[start..] {
                        state.nodes[entry].looped = true;
                    }
                    state.nodes[from].looped = true;
                }
            }
        } else if state.nodes[target].looped {
            state.nodes[from].looped = true;
        }
        Ok(FoldHit::cycle())
    }

    fn state_of(&self, pn: usize) -> (u8, bool, bool) {
        let state = self.state.borrow();
        let node = &state.nodes[pn];
        (node.state, node.looped, node.cut)
    }

    /// The folded value at `target` in a document, for the site at `from`.
    /// Raw members are followed from the root; a key operator site met on
    /// the way is folded (through the memory) and the remaining steps read
    /// its folded value, so a reference to a reference resolves. A target
    /// still being folded is a cycle, and so is a path that must pass
    /// through a site that was cut.
    ///
    /// The steps are taken in `find`, whose frame is gone before the walk
    /// this recurses into starts: this frame is on the stack once per
    /// reference hop, so it holds as little as it can.
    fn at(&self, from: usize, target: &[Seg], key: Option<&str>) -> Result<FoldHit, FoldError> {
        self.guard()?;
        match self.find(from, target, key)? {
            Found::Hit(hit) => Ok(hit),
            Found::Through(pn, next) => self.through(from, pn, target, next, key),
            Found::End(pn) => self.reach(from, pn),
        }
    }

    /// Fold the static site `pn` part way along `target`, then read the
    /// rest of the path, from step `next`, in its folded value.
    fn through(
        &self,
        from: usize,
        pn: usize,
        target: &[Seg],
        next: usize,
        key: Option<&str>,
    ) -> Result<FoldHit, FoldError> {
        let (state, looped, _) = self.state_of(pn);
        if ACTIVE == state || looped {
            return self.cycle_to(from, pn);
        }
        match self.walk(pn, FROM_REFERENCE) {
            Ok(folded) => self.read_on(from, pn, folded, target, next, key),
            Err(error) => Err(error),
        }
    }

    /// The rest of a path through a static site, read in the site's
    /// folded value: a cycle when the site was cut or is in a loop.
    fn read_on(
        &self,
        from: usize,
        pn: usize,
        folded: Value,
        target: &[Seg],
        next: usize,
        key: Option<&str>,
    ) -> Result<FoldHit, FoldError> {
        let (_, looped, cut) = self.state_of(pn);
        if looped || cut {
            return self.cycle_to(from, pn);
        }
        Ok(match read_raw(folded, &target[next..]) {
            Some(value) => FoldHit::found(value),
            None => self.miss(from, key, target),
        })
    }

    /// Fold the location at the end of a path.
    fn reach(&self, from: usize, pn: usize) -> Result<FoldHit, FoldError> {
        let (state, _, _) = self.state_of(pn);
        if ACTIVE == state {
            return self.cycle_to(from, pn);
        }
        match self.walk(pn, FROM_REFERENCE) {
            Ok(value) => self.reached(from, pn, value),
            Err(error) => Err(error),
        }
    }

    /// A location folded at the end of a path: a cycle when it is in a
    /// loop of references.
    fn reached(&self, from: usize, pn: usize, value: Value) -> Result<FoldHit, FoldError> {
        let (_, looped, _) = self.state_of(pn);
        if looped {
            return self.cycle_to(from, pn);
        }
        Ok(FoldHit::found(value))
    }

    /// Follow `target` raw from the document's root, as far as the fold has
    /// to go: to its end, to a static key operator site met on the way,
    /// or to an answer.
    fn find(&self, from: usize, target: &[Seg], key: Option<&str>) -> Result<Found, FoldError> {
        let Some(doc) = self.doc_of(from, key)? else {
            return Ok(Found::Hit(self.miss(from, key, target)));
        };
        let (mut pn, mut node) = {
            let state = self.state.borrow();
            let doc = &state.docs[doc];
            (doc.top, doc.root.clone())
        };
        for (i, seg) in target.iter().enumerate() {
            let last = i + 1 == target.len();
            let Some(stepped) = step(&node, seg) else {
                return Ok(Found::Hit(self.miss(from, key, target)));
            };
            let (index, value) = match stepped {
                // The head of an operator node, its description: a leaf to
                // the fold, as the canonical treats its operator object, so
                // the rest of the path reads it raw.
                Step::Head(head) => {
                    return Ok(Found::Hit(match read_raw(head, &target[i + 1..]) {
                        Some(value) => FoldHit::found(value),
                        None => self.miss(from, key, target),
                    }));
                }
                Step::Member(value) => (None, value),
                Step::Item(index, value) => (Some(index), value),
            };
            if last && !is_container(&value) {
                return Ok(Found::Hit(FoldHit::found(value)));
            }
            pn = match index {
                Some(index) => self.kid_indexed(pn, index, value.clone()),
                None => self.kid_named(pn, &seg.text(), value.clone()),
            };
            node = value;
            // Only a static site changes what lies below it; any other map,
            // a dynamic one or one `recognise` refused, folds member by
            // member, so the raw path below it names the same location.
            if !last && is_record(&node) {
                if let Some(op) = self.site_op_of(pn)? {
                    if !op.dynamic {
                        return Ok(Found::Through(pn, i + 1));
                    }
                }
            }
        }
        Ok(Found::End(pn))
    }

    fn locate(
        &self,
        from: usize,
        target: &[Seg],
        key: Option<&str>,
    ) -> Result<Option<Value>, FoldError> {
        self.guard()?;
        let Some(doc) = self.doc_of(from, key)? else {
            return Ok(None);
        };
        let root = self.state.borrow().docs[doc].root.clone();
        Ok(read_raw(root, target))
    }

    /// A member's folded value: a container through the memory, anything
    /// else as it is.
    fn member(&self, pn: usize, name: &str, child: &Value) -> Result<Value, FoldError> {
        if is_container(child) {
            let kid = self.kid_named(pn, name, child.clone());
            self.walk(kid, FROM_PARENT)
        } else {
            Ok(child.clone())
        }
    }

    /// A map folded as a plain map, members in its own order.
    fn plain_of(&self, pn: usize) -> Result<Value, FoldError> {
        self.guard()?;
        let (raw, parts) = {
            let state = self.state.borrow();
            let node = &state.nodes[pn];
            if let Some(plain) = &node.plain {
                return Ok(plain.clone());
            }
            let parts = node.parts.as_deref().map(|parts| {
                (
                    parts.src.clone(),
                    parts.value.clone(),
                    parts.siblings.clone(),
                )
            });
            (node.raw.clone(), parts)
        };
        let Some(members) = map_entries(&raw) else {
            return self.walk(pn, FROM_PARENT);
        };
        let mut out = IndexMap::with_capacity(members.len());
        for (name, member) in members {
            let value = match &parts {
                None => self.member(pn, name, member)?,
                Some((src, value, _)) if src == name => value.clone(),
                Some((_, _, siblings)) => map_entries(siblings)
                    .and_then(|siblings| siblings.get(name))
                    .cloned()
                    .unwrap_or(Value::Undefined),
            };
            out.insert(name.clone(), value);
        }
        let out = rebuild_map(&raw, out);
        self.state.borrow_mut().nodes[pn].plain = Some(out.clone());
        Ok(out)
    }

    fn depth_error(&self, pn: usize) -> FoldError {
        let pointer = self.ptr_of(pn);
        let doc = self.doc_key(pn);
        let error = FoldError::new(
            FOLD_DEPTH,
            format_args!(
                "nesting and references deeper than {} at {}",
                self.options.max_depth,
                shown(&pointer)
            ),
        )
        .located(&doc, Some(&pointer));
        self.raise(error)
    }

    /// Fold one location, and everything below it that is not folded yet.
    ///
    /// Iterative: the containers being walked are frames on a heap stack,
    /// so a level of nesting costs no stack. A callback this calls can
    /// re-enter the fold, and that nested walk is a new call with its own
    /// frames, which is where the stack grows: one call per reference hop.
    fn walk(&self, pn: usize, via: u8) -> Result<Value, FoldError> {
        self.guard()?;
        let mut frames = Vec::new();
        if let Some(value) = self.enter(pn, via, &mut frames)? {
            return Ok(value);
        }
        loop {
            if let Err(error) = self.descend(&mut frames) {
                self.unwind(&frames);
                return Err(error);
            }
            let finished = match frames.last_mut() {
                Some(top) => self.finish(top),
                None => return Ok(Value::Undefined),
            };
            // Dropped in place: `pop` would hand the frame back by value.
            frames.truncate(frames.len() - 1);
            match finished {
                Ok(value) => match frames.last_mut() {
                    Some(parent) => parent.accept(value),
                    None => return Ok(value),
                },
                Err(error) => {
                    self.unwind(&frames);
                    return Err(error);
                }
            }
        }
    }

    /// Take the top frame's members until it has none left: a leaf, or a
    /// container already folded or being folded, is accepted as it is, and
    /// any other container gets a frame of its own on top, which is then
    /// the one read. Returns once the top frame is complete.
    fn descend(&self, frames: &mut Vec<Frame>) -> Result<(), FoldError> {
        loop {
            let Some(top) = frames.last_mut() else {
                return Ok(());
            };
            let Some((member, child)) = top.take() else {
                return Ok(());
            };
            if !is_container(&child) {
                top.accept(child);
                continue;
            }
            let kid = self.kid_of(top.pn, &top.raw, member, child);
            if let Some(value) = self.enter(kid, FROM_PARENT, frames)? {
                if let Some(top) = frames.last_mut() {
                    top.accept(value);
                }
            }
        }
    }

    /// Start folding a location: its memorised value, the node itself when
    /// it is a leaf or already being folded, or `None` with a frame for its
    /// members pushed on `frames`.
    fn enter(
        &self,
        pn: usize,
        via: u8,
        frames: &mut Vec<Frame>,
    ) -> Result<Option<Value>, FoldError> {
        let raw = {
            let state = self.state.borrow();
            let node = &state.nodes[pn];
            if DONE == node.state {
                return Ok(Some(node.value.clone()));
            }
            if ACTIVE == node.state || !is_container(&node.raw) {
                return Ok(Some(node.raw.clone()));
            }
            node.raw.clone()
        };
        {
            let mut state = self.state.borrow_mut();
            if self.options.max_depth <= state.stack.len() {
                drop(state);
                return Err(self.depth_error(pn));
            }
            state.nodes[pn].state = ACTIVE;
            state.nodes[pn].via = via;
            state.stack.push(pn);
        }
        let kind = if is_record(&raw) {
            let op = match self.site_op_of(pn) {
                Ok(op) => op,
                Err(error) => {
                    self.leave(pn, None);
                    return Err(error);
                }
            };
            match op {
                Some(op) if !op.dynamic => Kind::Site {
                    op,
                    value: Value::Undefined,
                    siblings: IndexMap::new(),
                },
                op => {
                    if let Some(op) = op {
                        self.note_dynamic(pn, &op.src);
                    }
                    let plain = self.state.borrow().nodes[pn].plain.clone();
                    if let Some(plain) = plain {
                        self.leave(pn, Some(plain.clone()));
                        return Ok(Some(plain));
                    }
                    Kind::Plain {
                        out: IndexMap::with_capacity(map_entries(&raw).map_or(0, IndexMap::len)),
                    }
                }
            }
        } else if let Some(id) = node_id(&raw) {
            let (op, terms) = with_node(&raw, |entry| (entry.op.clone(), entry.terms.clone()))
                .unwrap_or_else(|| released_handle(id));
            Kind::Node {
                op,
                out: Vec::with_capacity(terms.len()),
                terms,
            }
        } else {
            Kind::List {
                out: Vec::with_capacity(list_items(&raw).len()),
            }
        };
        frames.push(Frame {
            pn,
            raw,
            next: 0,
            kind,
        });
        Ok(None)
    }

    /// Close a location once its members are folded: reduce it, keep the
    /// result, and take it off the active stack.
    ///
    /// This and the functions it calls through to the callback are on the
    /// stack once per reference hop, so each holds only what it needs: the
    /// frame is read in place, not moved, and the work that calls nothing
    /// back is done in frames that are gone before a callback runs.
    fn finish(&self, frame: &mut Frame) -> Result<Value, FoldError> {
        let pn = frame.pn;
        let out = match frame.kind {
            Kind::Node { op: Some(_), .. } => self.reduce_node(frame),
            Kind::Site { .. } => self.reduce_site(frame),
            _ => Ok(self.rebuild(frame)),
        };
        self.leave(pn, out.as_ref().ok().cloned());
        out
    }

    /// A list, an expression node with no operator, or a plain map: the
    /// folded members, in a new container of the same kind.
    fn rebuild(&self, frame: &mut Frame) -> Value {
        let Frame { pn, raw, kind, .. } = frame;
        match kind {
            Kind::List { out } => rebuild_list(raw, std::mem::take(out)),
            Kind::Node { out, .. } => Value::array(std::mem::take(out)),
            Kind::Plain { out } => {
                let out = rebuild_map(raw, std::mem::take(out));
                self.state.borrow_mut().nodes[*pn].plain = Some(out.clone());
                out
            }
            Kind::Site { .. } => Value::Undefined,
        }
    }

    /// A token operator node: through `evaluate` when `tokens` is on, and
    /// otherwise rebuilt around its own operator, so it stays an operator
    /// node, a new live node in the same arena.
    fn reduce_node(&self, frame: &mut Frame) -> Result<Value, FoldError> {
        let Frame { pn, raw, kind, .. } = frame;
        let Kind::Node {
            op: Some(op), out, ..
        } = kind
        else {
            return Ok(Value::Undefined);
        };
        let op = op.clone();
        let terms = std::mem::take(out);
        if self.options.tokens {
            self.call_evaluate(*pn, &op, &terms)
        } else {
            Ok(operator_node(raw, op, terms))
        }
    }

    /// A key operator site: its terms kept for `site.plain()`, then
    /// `evaluate` with `[value, siblings]`.
    fn reduce_site(&self, frame: &mut Frame) -> Result<Value, FoldError> {
        let Frame { pn, raw, kind, .. } = frame;
        let Kind::Site {
            op,
            value,
            siblings,
        } = kind
        else {
            return Ok(Value::Undefined);
        };
        let op = op.clone();
        let terms = [
            std::mem::replace(value, Value::Undefined),
            rebuild_map(raw, std::mem::take(siblings)),
        ];
        self.keep_parts(*pn, &op, &terms);
        self.call_evaluate(*pn, &op, &terms)
    }

    fn keep_parts(&self, pn: usize, op: &Op, terms: &[Value; 2]) {
        self.state.borrow_mut().nodes[pn].parts = Some(Box::new(Parts {
            src: op.src.clone(),
            value: terms[0].clone(),
            siblings: terms[1].clone(),
        }));
    }

    /// The resolver, at a site. Its result is a `TabnasError` result, which
    /// is large, so it is converted after the call, in a frame of its own.
    fn call_evaluate(&self, pn: usize, op: &Op, terms: &[Value]) -> Result<Value, FoldError> {
        let site = self.site(pn, None);
        let result = (self.options.evaluate)(&site, op, terms);
        // Read in place: moving the result into a conversion would copy it
        // into this frame again.
        match &result {
            Ok(value) => Ok(value.clone()),
            Err(error) => Err(self.callback_error(error)),
        }
    }

    /// Take a location off the active stack: kept when it has a value,
    /// free again when its fold failed.
    fn leave(&self, pn: usize, out: Option<Value>) {
        let mut state = self.state.borrow_mut();
        let top = state.stack.pop();
        debug_assert_eq!(Some(pn), top, "the active stack unwinds in order");
        let node = &mut state.nodes[pn];
        match out {
            Some(value) => {
                node.state = DONE;
                node.value = value;
            }
            None => node.state = FREE,
        }
    }

    /// A walk failed: every location it still had open is free again.
    fn unwind(&self, frames: &[Frame]) {
        for frame in frames.iter().rev() {
            self.leave(frame.pn, None);
        }
    }
}

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

/// A map: what a key operator can be recognised on.
fn is_record(value: &Value) -> bool {
    matches!(value, Value::Object(_) | Value::MapRef(_))
}

/// A map or a list: what the fold walks into. An expression node is a
/// list.
fn is_container(value: &Value) -> bool {
    matches!(
        value,
        Value::Object(_) | Value::MapRef(_) | Value::Array(_) | Value::ListRef(_)
    )
}

fn map_entries(value: &Value) -> Option<&IndexMap<String, Value>> {
    match value {
        Value::Object(members) => Some(members),
        Value::MapRef(map) => Some(&map.value),
        _ => None,
    }
}

fn list_items(value: &Value) -> &[Value] {
    match value {
        Value::Array(items) => items,
        Value::ListRef(list) => &list.value,
        _ => &[],
    }
}

fn member_name(raw: &Value, at: usize) -> &str {
    map_entries(raw)
        .and_then(|members| members.get_index(at))
        .map_or("", |(name, _)| name.as_str())
}

/// A new map of the same kind as `like`: an `Object` for an `Object`, and
/// a `MapRef` keeping its flags and metadata for a `MapRef`.
fn rebuild_map(like: &Value, members: IndexMap<String, Value>) -> Value {
    match like {
        Value::MapRef(map) => {
            let mut out = (**map).clone();
            out.value = members;
            Value::MapRef(Arc::new(out))
        }
        _ => Value::object(members),
    }
}

/// A new list of the same kind as `like`.
fn rebuild_list(like: &Value, items: Vec<Value>) -> Value {
    match like {
        Value::ListRef(list) => {
            let mut out = (**list).clone();
            out.value = items;
            Value::ListRef(Arc::new(out))
        }
        _ => Value::array(items),
    }
}

/// An operator node rebuilt around its own operator and occurrence token,
/// with folded terms: a new live node in the arena `raw` is in.
fn operator_node(raw: &Value, op: Arc<Op>, terms: Vec<Value>) -> Value {
    let token = with_node(raw, |entry| entry.token.clone()).flatten();
    new_node(Some(op), token, terms)
}

/// One step of a path.
enum Step {
    /// A map's member.
    Member(Value),
    /// A list's item, by its index.
    Item(usize, Value),
    /// The head of a live operator node: its realized description.
    Head(Value),
}

/// One step of a path: an index into a list, or an own member of a map.
/// `None` when the step names nothing.
fn step(node: &Value, seg: &Seg) -> Option<Step> {
    match node {
        Value::Object(members) => members.get(seg.text().as_ref()).cloned().map(Step::Member),
        Value::MapRef(map) => map
            .value
            .get(seg.text().as_ref())
            .cloned()
            .map(Step::Member),
        Value::Array(items) => {
            array_index(seg, items.len()).map(|index| Step::Item(index, items[index].clone()))
        }
        Value::ListRef(list) => match node_id(node) {
            None => array_index(seg, list.value.len())
                .map(|index| Step::Item(index, list.value[index].clone())),
            // A live expression node reads as the canonical array does: the
            // operator first, then the terms.
            Some(id) => with_node(node, |entry| {
                let head = usize::from(entry.op.is_some());
                let index = array_index(seg, entry.terms.len() + head)?;
                match (index, &entry.op) {
                    (0, Some(op)) => Some(Step::Head(op.to_value(entry.token.as_ref()))),
                    _ => Some(Step::Item(index, entry.terms[index - head].clone())),
                }
            })
            .unwrap_or_else(|| released_handle(id)),
        },
        _ => None,
    }
}

/// Read raw from `node` along `path`, following nothing.
fn read_raw(mut node: Value, path: &[Seg]) -> Option<Value> {
    for seg in path {
        node = match step(&node, seg)? {
            Step::Member(value) | Step::Item(_, value) | Step::Head(value) => value,
        };
    }
    Some(node)
}

/// An RFC 6901 array index: `0`, or digits with no leading zero, given as
/// a name, or an index; `None` for anything else, `-` and out-of-range
/// indices included.
fn array_index(seg: &Seg, length: usize) -> Option<usize> {
    let index = match seg {
        Seg::Index(index) => *index,
        Seg::Name(text) => {
            let canonical = "0" == text
                || (!text.is_empty()
                    && !text.starts_with('0')
                    && text.bytes().all(|byte| byte.is_ascii_digit()));
            if !canonical {
                return None;
            }
            text.parse::<usize>().ok()?
        }
    };
    (index < length).then_some(index)
}

/// RFC 6901: a pointer from path steps.
fn pointer(path: &[Seg]) -> String {
    let mut out = String::new();
    for seg in path {
        out.push('/');
        escape_segment(&mut out, &seg.text());
    }
    out
}

fn escape_segment(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '~' => out.push_str("~0"),
            '/' => out.push_str("~1"),
            c => out.push(c),
        }
    }
}

/// A key or pointer for a message: JSON-escaped, as `JSON.stringify`
/// writes a string, so a newline or an escape sequence in a member name
/// stays visible, and at most about 80 characters long, counted as
/// JavaScript counts them, in UTF-16 units.
fn shown(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            '\u{8}' => quoted.push_str("\\b"),
            '\u{c}' => quoted.push_str("\\f"),
            c if (c as u32) < 0x20 => quoted.push_str(&format!("\\u{:04x}", c as u32)),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    if quoted.encode_utf16().count() <= 80 {
        return quoted;
    }
    let mut out = String::new();
    let mut units = 0;
    for c in quoted.chars() {
        units += c.len_utf16();
        if 76 < units {
            break;
        }
        out.push(c);
    }
    out.push_str("...\"");
    out
}
