/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

package tabnasexpr

// fold.go is the tree layer: key operators and Fold, the Go port of the
// layer beside `evaluation` in ts/src/expr.ts. TypeScript is canonical, and
// the fixtures in test/tree/ hold the two to one behaviour.
//
// A key operator is recognised after the base grammar has built a map, by
// the map's shape: it owns the member named by the operator's Src. Fold
// walks a finished tree, reduces every key-operator site bottom-up through
// FoldOptions.Evaluate, and returns a new tree; the tree it is given is
// never changed. Values flow up through Evaluate, context flows down
// through Inherit, and every location has one result, kept in a trie of
// locations per document.

import (
	"errors"
	"fmt"
	"math"
	"reflect"
	"sort"
	"strconv"
	"strings"
	"sync"

	tabnas "github.com/tabnas/parser/go"
)

// Path is a location, as segments from a document's root: member names as
// strings, and array indices as ints or as RFC 6901 index strings ("0",
// "12"). A float64 with an integral value is an index too, as a
// JavaScript number is.
type Path []any

// FoldHit is what Site.At and Site.Locate give back: whether the location
// names anything, whether reaching it closed a cycle (At only), and the
// value, which is set only when the location was found and is not a cycle.
type FoldHit struct {
	Found bool
	Cycle bool
	Value any
}

// FoldEvaluate reduces one site. For a key operator, terms is
// []any{value, siblings}: the member's value and a new *tabnas.OrderedMap
// of the other members, both already folded. For a token operator met in
// the walk (FoldOptions.Tokens), terms are the operator's folded terms.
// What it returns stands at the site; an error stops the fold and is
// returned by Fold as it is.
type FoldEvaluate func(site *Site, op *Op, terms []any) (any, error)

// Inherit gives the bag a map's subtree inherits, from the bag its
// parent's subtree sees. It is called once per map on a path the fold
// needs, from the root down, so a site reached early through a reference
// gets its own ancestors' bag, never the referrer's. Inside the call,
// site.K() is the parent's bag. It must return a bag, and must not fold
// below its own map; either mistake is a FoldError coded fold_inherit.
type Inherit func(site *Site, node any, k map[string]any) (map[string]any, error)

// Recognise reports whether a map that owns a key operator's member is an
// occurrence of it there. A map for which it reports false folds as a
// plain map, with no Evaluate call and no report row.
type Recognise func(site *Site, op *Op) (bool, error)

// Positions looks up the opening token of a container, by the container's
// identity; RecordPositions builds one. It returns nil for a container it
// has no token for.
type Positions func(node any) *tabnas.Token

// FoldEntry is one report row: the site (Doc, At) and the location it named
// (TargetDoc, Target), as document keys and RFC 6901 pointers.
type FoldEntry struct {
	Doc       string `json:"doc"`
	At        string `json:"at"`
	TargetDoc string `json:"targetDoc"`
	Target    string `json:"target"`
}

// FoldDynamic is one row of FoldReport.Dynamic: a dynamic key operator's
// site and the member that marks it.
type FoldDynamic struct {
	Doc string `json:"doc"`
	At  string `json:"at"`
	Src string `json:"src"`
}

// FoldReport lists what a fold met, each row once, in the order the walk
// met it. It goes to FoldOptions.Report when the fold succeeds.
type FoldReport struct {
	Cycles     []FoldEntry   `json:"cycles"`
	Unresolved []FoldEntry   `json:"unresolved"`
	Dynamic    []FoldDynamic `json:"dynamic"`
}

// FoldOptions configures Fold, and the plugin's deferred mode
// (ExprOptions.Fold).
type FoldOptions struct {
	// Op is the operator table; only its Key entries matter to a fold. In
	// deferred mode the plugin's own table is used instead.
	Op map[string]*OpDef

	// Evaluate reduces each site. Required.
	Evaluate FoldEvaluate

	// Inherit computes the bag each map passes down. Optional.
	Inherit Inherit

	// Recognise can refuse a site. Optional.
	Recognise Recognise

	// K is the starting bag of every document's root. Nil is an empty bag.
	K map[string]any

	// Doc is the key of the document being folded. Default "".
	Doc string

	// Load returns another document's raw root, by key, for Site.At and
	// Site.Locate: called once per key. It reports ok false for a document
	// that cannot be had, which the fold then reports as unresolved; an
	// error stops the fold. Deciding what a key may name, and loading it,
	// is the host's.
	Load func(doc string, site *Site) (root any, ok bool, err error)

	// Cycle is "verbatim" (the default: the resolver is told, through
	// FoldHit.Cycle) or "error" (a cycle is a FoldError coded fold_cycle).
	// Any other value reads as "verbatim".
	Cycle string

	// Tokens reduces the token operator nodes met in the walk through
	// Evaluate too, so one fold handles `+` and `$ref`. Nil is true;
	// tabnas.Bool(false) keeps an operator node an operator node.
	Tokens *bool

	// Positions gives each site its opening token: a Positions lookup (a
	// func(any) *tabnas.Token is accepted too), which RecordPositions
	// builds, or true, which records the tokens during the parse and so
	// is accepted in deferred mode only (Fold refuses it, fold_option).
	// Nil or false: no tokens.
	Positions any

	// MaxDepth bounds how deep the walk goes, nesting and reference hops
	// counted together. Zero is the default, 1000.
	MaxDepth int

	// Report receives the report of a fold that succeeds.
	Report func(report FoldReport)
}

// FoldError is the fold's own error. Code is what a fixture pins:
// fold_cycle (a cycle under Cycle "error"), fold_depth (nesting and
// reference hops past MaxDepth), fold_inherit (an Inherit that returned no
// bag, or folded its own subtree), fold_key (an operator table with two
// key operators on one member name, or one on none) and fold_option (an
// option the fold cannot take, Positions true outside deferred mode among
// them). Doc, At, TargetDoc and Target name the location where the error
// has one; a document's root is the pointer "" and the default document's
// key is "", so an empty field is not on its own a missing one. The
// message quotes member names JSON-escaped and cut short, since they are
// document content.
type FoldError struct {
	Code      string
	Doc       string
	At        string
	TargetDoc string
	Target    string

	message string
}

// Error returns the message, prefixed "expr fold: " as TypeScript's is.
func (e *FoldError) Error() string {
	return "expr fold: " + e.message
}

const (
	codeFoldCycle   = "fold_cycle"
	codeFoldDepth   = "fold_depth"
	codeFoldInherit = "fold_inherit"
	codeFoldKey     = "fold_key"
	codeFoldOption  = "fold_option"
)

// defaultMaxDepth is how deep a fold walks when MaxDepth is zero. Go's
// goroutine stack grows to a gigabyte by default, and a stack overflow is
// fatal rather than recoverable, so unlike TypeScript the bound is the
// only guard: keep MaxDepth within what a goroutine stack can hold.
const defaultMaxDepth = 1000

// KeyMap is the key-operator table: the key operators in the order a map
// that owns two of their members is resolved by. Static operators come
// before dynamic ones, then member names in code-point order (Go compares
// strings by byte, which for UTF-8 is code-point order). Neither
// declaration order nor map order is used.
type KeyMap []*Op

// MakeKeyMap builds the key-operator table from an operator table, reading
// only its Key entries. A key operator's Op has no token and no binding
// power: its Name ends "-key", Src is the member name, Terms is 2 (the
// member's value, then the sibling map). Two key operators on one member
// name, or one that names no member, are refused with a FoldError coded
// fold_key.
func MakeKeyMap(op map[string]*OpDef) (KeyMap, error) {
	names := make([]string, 0, len(op))
	for name := range op {
		names = append(names, name)
	}
	sort.Strings(names)

	var keys KeyMap
	owner := map[string]string{}
	for _, name := range names {
		def := op[name]
		if def == nil || !def.Key {
			continue
		}
		src := keySource(def.Src)
		if src == "" {
			return nil, &FoldError{Code: codeFoldKey,
				message: "key operator " + shown(name) + " names no member"}
		}
		if prior, taken := owner[src]; taken {
			return nil, &FoldError{Code: codeFoldKey,
				message: "key operators " + shown(prior) + " and " + shown(name) +
					" both name the member " + shown(src)}
		}
		owner[src] = name
		opName := name
		if !strings.HasSuffix(name, "-key") {
			opName = name + "-key"
		}
		keys = append(keys, &Op{
			Name:    opName,
			Src:     src,
			Left:    MinSafeInteger,
			Right:   MaxSafeInteger,
			Terms:   2,
			Tin:     -1,
			OTin:    -1,
			CTin:    -1,
			Use:     def.Use,
			Key:     true,
			Dynamic: def.Dynamic,
		})
	}
	sort.SliceStable(keys, func(i, j int) bool {
		a, b := keys[i], keys[j]
		if a.Dynamic != b.Dynamic {
			return !a.Dynamic
		}
		return a.Src < b.Src
	})
	return keys, nil
}

// keySource is the member name a key operator's Src names: the string, or
// the first element of a list of them.
func keySource(src any) string {
	switch s := src.(type) {
	case string:
		return s
	case []string:
		if len(s) > 0 {
			return s[0]
		}
	case []any:
		if len(s) > 0 {
			if first, ok := s[0].(string); ok {
				return first
			}
		}
	}
	return ""
}

// KeyOpOf is the key operator a map is an occurrence of, by shape: the
// first, in the table's order, whose member the map owns. Nil for anything
// else, a value that is not a map the fold walks included.
func KeyOpOf(keys KeyMap, node any) *Op {
	if shapeOf(node) != shapeRecord {
		return nil
	}
	for _, op := range keys {
		if _, has := recordGet(node, op.Src); has {
			return op
		}
	}
	return nil
}

// IsOp reports whether a value is a token operator node: a *ListRef or a
// []any whose first element is an *Op, which is how a parse result carries
// an operation (the TypeScript isOp, whose mark the *Op type is here).
func IsOp(node any) bool {
	return isOp(node)
}

// Fold folds a finished tree: it walks root, reduces every key-operator
// site bottom-up through opts.Evaluate, and returns a new tree. The input
// is never changed, though a value Evaluate returns (Site.Node among them)
// is placed in the output as it is. One result per location, so every
// site naming one target shares one folded value. A site whose target is
// still being folded closes a cycle, which the resolver is told about
// (FoldHit.Cycle), or which is an error under Cycle "error". Dynamic key
// operators are reported and folded as plain maps. Token operator nodes
// met in the walk are reduced through the same Evaluate unless Tokens is
// false.
//
// The maps walked are *tabnas.OrderedMap, whose members go in their own
// order, and map[string]any and tabnas.MapRef, whose members go in
// code-point order since a Go map has none; the lists walked are []any,
// tabnas.ListRef and *tabnas.ListRef. Anything else is a leaf, kept by
// identity. A folded map is a new map of the same kind; the siblings an
// Evaluate call gets are always a new *tabnas.OrderedMap.
func Fold(root any, opts FoldOptions) (any, error) {
	keys, err := MakeKeyMap(opts.Op)
	if err != nil {
		return nil, err
	}
	lookup, record, err := positionsOption(opts.Positions)
	if err != nil {
		return nil, err
	}
	if record {
		return nil, &FoldError{Code: codeFoldOption,
			message: "Positions: true records tokens during a parse, so it works in the" +
				" deferred mode only; pass a lookup, such as RecordPositions(j), to Fold"}
	}
	return foldTree(root, &opts, keys, lookup)
}

// positionsOption reads FoldOptions.Positions: a lookup, or whether the
// tokens are to be recorded during the parse.
func positionsOption(v any) (Positions, bool, error) {
	switch p := v.(type) {
	case nil:
		return nil, false, nil
	case bool:
		return nil, p, nil
	case Positions:
		return p, false, nil
	case func(any) *tabnas.Token:
		return Positions(p), false, nil
	}
	return nil, false, &FoldError{Code: codeFoldOption,
		message: fmt.Sprintf("Positions takes true or a lookup, not %T", v)}
}

// foldTree is Fold with the key table built and the positions resolved to
// a lookup, which is what the deferred mode calls.
func foldTree(root any, opts *FoldOptions, keys KeyMap, lookup Positions) (any, error) {
	if opts.Evaluate == nil {
		return nil, &FoldError{Code: codeFoldOption, message: "Evaluate is required"}
	}
	f := &folder{
		opts:     opts,
		keys:     keys,
		tokens:   opts.Tokens == nil || *opts.Tokens,
		lookup:   lookup,
		maxDepth: opts.MaxDepth,
		base:     opts.K,
		cycleErr: opts.Cycle == "error",
		report: FoldReport{
			Cycles:     []FoldEntry{},
			Unresolved: []FoldEntry{},
			Dynamic:    []FoldDynamic{},
		},
		noted: map[noteKey]bool{},
		docs:  map[string]*foldDoc{},
	}
	if f.maxDepth == 0 {
		f.maxDepth = defaultMaxDepth
	}
	if f.base == nil {
		f.base = map[string]any{}
	}
	main := f.makeDoc(opts.Doc, root)
	out, err := f.walk(main.top, 0)
	if err != nil {
		return nil, err
	}
	if opts.Report != nil {
		opts.Report(f.report)
	}
	return out, nil
}

// Site is what Fold hands its callbacks for one map, or one token operator
// node: the verbatim root of the map's document and that document's key,
// the verbatim map, and its opening token when Positions gives one, with
// methods for the path to it, the bag inherited along that path, and the
// rest of the tree. A Site is made by the fold; a zero Site has no fold to
// reach.
type Site struct {
	// Root is the verbatim root of the site's document.
	Root any
	// Doc is that document's key.
	Doc string
	// Node is the verbatim map, or token operator node.
	Node any
	// Token is the map's opening token, when FoldOptions.Positions gives
	// one; nil otherwise.
	Token *tabnas.Token

	f         *folder
	pn        *pnode
	inInherit bool
	parentK   map[string]any
	path      Path
}

// Path is the location of the site, as segments from its document's root:
// member names as strings, array and term indices as ints. The root's path
// is empty.
func (s *Site) Path() Path {
	if s.path == nil && s.pn != nil {
		s.path = pathOf(s.pn)
	}
	return s.path
}

// K is the bag inherited along the path, Inherit applied at every map from
// the root down to and including this one, and kept per location. Inside
// an Inherit call it is the parent's bag. With no Inherit it is
// FoldOptions.K.
func (s *Site) K() (map[string]any, error) {
	if s.inInherit {
		return s.parentK, nil
	}
	if err := s.check(); err != nil {
		return nil, err
	}
	return s.f.kOf(s.pn)
}

// At folds the location path names, in this document or in the one doc
// names, and returns its folded value. Raw members are followed from the
// root; a key-operator site met on the way is folded and the rest of the
// path reads its folded value, so a reference to a reference resolves. A
// target still being folded is a cycle, and so is a path that has to pass
// through a site that was cut.
func (s *Site) At(path Path, doc ...string) (FoldHit, error) {
	if err := s.check(); err != nil {
		return FoldHit{}, err
	}
	key, named := docArg(doc)
	return s.f.at(s.pn, path, key, named)
}

// Locate reads the location path names, in this document or in the one doc
// names, raw: it follows nothing and folds nothing. Cycle is always false.
func (s *Site) Locate(path Path, doc ...string) (FoldHit, error) {
	if err := s.check(); err != nil {
		return FoldHit{}, err
	}
	key, named := docArg(doc)
	return s.f.locate(s.pn, path, key, named)
}

// Plain is this map folded as a plain map, member by member in its own
// order: what a resolver returns where the operator does not apply.
func (s *Site) Plain() (any, error) {
	if err := s.check(); err != nil {
		return nil, err
	}
	return s.f.plainOf(s.pn)
}

func (s *Site) check() error {
	if s == nil || s.f == nil || s.pn == nil {
		return &FoldError{Code: codeFoldOption, message: "a Site is made by a fold"}
	}
	return nil
}

// docArg reads the optional document key of At and Locate.
func docArg(doc []string) (string, bool) {
	if len(doc) == 0 {
		return "", false
	}
	return doc[0], true
}

// The shapes of value a fold tells apart.
type shape uint8

const (
	shapeLeaf   shape = iota // anything the fold does not walk
	shapeList                // []any, tabnas.ListRef, *tabnas.ListRef
	shapeOp                  // a token operator node
	shapeRecord              // *tabnas.OrderedMap, map[string]any, tabnas.MapRef
)

// shapeOf is the shape of a value. A nil map or list, which JSON writes as
// null, is a leaf.
func shapeOf(v any) shape {
	switch t := v.(type) {
	case *tabnas.OrderedMap:
		if t != nil {
			return shapeRecord
		}
	case map[string]any:
		if t != nil {
			return shapeRecord
		}
	case tabnas.MapRef:
		if t.Val != nil {
			return shapeRecord
		}
	case []any:
		if t != nil {
			if len(t) > 0 {
				if _, op := t[0].(*Op); op {
					return shapeOp
				}
			}
			return shapeList
		}
	case *tabnas.ListRef:
		if t != nil {
			if len(t.Val) > 0 {
				if _, op := t.Val[0].(*Op); op {
					return shapeOp
				}
			}
			return shapeList
		}
	case tabnas.ListRef:
		return shapeList
	}
	return shapeLeaf
}

// itemsOf is the members of a list or operator node.
func itemsOf(v any) []any {
	switch t := v.(type) {
	case []any:
		return t
	case *tabnas.ListRef:
		return t.Val
	case tabnas.ListRef:
		return t.Val
	}
	return nil
}

// recordGet is a map's own member.
func recordGet(v any, name string) (any, bool) {
	switch t := v.(type) {
	case *tabnas.OrderedMap:
		val, has := t.Vals[name]
		return val, has
	case map[string]any:
		val, has := t[name]
		return val, has
	case tabnas.MapRef:
		val, has := t.Val[name]
		return val, has
	}
	return nil, false
}

// recordNames is a map's member names in its own order: an OrderedMap's
// keys as it holds them, any other map's in code-point order.
func recordNames(v any) []string {
	var m map[string]any
	switch t := v.(type) {
	case *tabnas.OrderedMap:
		return t.Keys
	case map[string]any:
		m = t
	case tabnas.MapRef:
		m = t.Val
	default:
		return nil
	}
	names := make([]string, 0, len(m))
	for name := range m {
		names = append(names, name)
	}
	sort.Strings(names)
	return names
}

// newRecordLike is a new, empty map of the same kind as like.
func newRecordLike(like any, size int) any {
	switch t := like.(type) {
	case *tabnas.OrderedMap:
		return &tabnas.OrderedMap{
			Keys: make([]string, 0, size), Vals: make(map[string]any, size), Sorted: t.Sorted}
	case tabnas.MapRef:
		return tabnas.MapRef{Val: make(map[string]any, size), Implicit: t.Implicit, Meta: copyMeta(t.Meta)}
	}
	return make(map[string]any, size)
}

// recordSet adds a member to a map newRecordLike made, members arriving
// in order.
func recordSet(rec any, name string, v any) {
	switch t := rec.(type) {
	case *tabnas.OrderedMap:
		if _, has := t.Vals[name]; !has {
			t.Keys = append(t.Keys, name)
		}
		t.Vals[name] = v
	case map[string]any:
		t[name] = v
	case tabnas.MapRef:
		t.Val[name] = v
	}
}

// newSiblings is the map a site's other members go into: a new
// OrderedMap, sorted when the site is a sorted one.
func newSiblings(like any, size int) *tabnas.OrderedMap {
	sorted := false
	if om, ok := like.(*tabnas.OrderedMap); ok {
		sorted = om.Sorted
	}
	return &tabnas.OrderedMap{Keys: make([]string, 0, size), Vals: make(map[string]any, size), Sorted: sorted}
}

// newListLike is a list of the same kind as like, holding items.
func newListLike(like any, items []any) any {
	switch t := like.(type) {
	case *tabnas.ListRef:
		return &tabnas.ListRef{Val: items, Implicit: t.Implicit, Child: t.Child, Meta: copyMeta(t.Meta)}
	case tabnas.ListRef:
		return tabnas.ListRef{Val: items, Implicit: t.Implicit, Child: t.Child, Meta: copyMeta(t.Meta)}
	}
	return items
}

func copyMeta(meta map[string]any) map[string]any {
	if meta == nil {
		return nil
	}
	out := make(map[string]any, len(meta))
	for k, v := range meta {
		out[k] = v
	}
	return out
}

// arrayIndex reads an RFC 6901 array index: "0", or digits with no leading
// zero, given as a string, or a non-negative integer; -1 for anything
// else, "-" and an index out of range included.
func arrayIndex(seg any, length int) int {
	i := -1
	switch t := seg.(type) {
	case int:
		i = t
	case int64:
		if t >= 0 && t < int64(length) {
			i = int(t)
		}
	case int32:
		i = int(t)
	case float64:
		if t == math.Trunc(t) && t >= 0 && t < float64(length) {
			i = int(t)
		}
	case string:
		if isIndexText(t) {
			if n, err := strconv.Atoi(t); err == nil {
				i = n
			}
		}
	}
	if 0 <= i && i < length {
		return i
	}
	return -1
}

func isIndexText(s string) bool {
	if s == "0" {
		return true
	}
	if s == "" || s[0] < '1' || '9' < s[0] {
		return false
	}
	for i := 1; i < len(s); i++ {
		if s[i] < '0' || '9' < s[i] {
			return false
		}
	}
	return true
}

// segName is a segment as a member name, as JavaScript's String(seg)
// writes it.
func segName(seg any) (string, bool) {
	switch t := seg.(type) {
	case string:
		return t, true
	case int:
		return strconv.Itoa(t), true
	case int64:
		return strconv.FormatInt(t, 10), true
	case int32:
		return strconv.FormatInt(int64(t), 10), true
	case float64:
		return jsNumber(t), true
	}
	return "", false
}

// jsNumber is ECMA-262 Number::toString.
func jsNumber(f float64) string {
	switch {
	case math.IsNaN(f):
		return "NaN"
	case math.IsInf(f, 1):
		return "Infinity"
	case math.IsInf(f, -1):
		return "-Infinity"
	case f == 0:
		return "0"
	}
	if abs := math.Abs(f); abs < 1e-6 || abs >= 1e21 {
		mantissa, exponent, _ := strings.Cut(strconv.FormatFloat(f, 'e', -1, 64), "e")
		return mantissa + "e" + exponent[:1] + strings.TrimLeft(exponent[1:], "0")
	}
	return strconv.FormatFloat(f, 'f', -1, 64)
}

// step takes one step of a path: an index into a list or operator node,
// or an own member of a map. ok is false when the step names nothing.
func step(node any, seg any) (any, bool) {
	switch shapeOf(node) {
	case shapeList, shapeOp:
		items := itemsOf(node)
		i := arrayIndex(seg, len(items))
		if i < 0 || isUnfilled(items[i]) {
			return nil, false
		}
		return items[i], true
	case shapeRecord:
		name, ok := segName(seg)
		if !ok {
			return nil, false
		}
		return recordGet(node, name)
	}
	return nil, false
}

// pointer is the RFC 6901 pointer of path segments.
func pointer(path Path) string {
	var b strings.Builder
	for _, seg := range path {
		name, ok := segName(seg)
		if !ok {
			name = fmt.Sprint(seg)
		}
		b.WriteByte('/')
		b.WriteString(escapeSegment(name))
	}
	return b.String()
}

var segmentEscaper = strings.NewReplacer("~", "~0", "/", "~1")

func escapeSegment(name string) string {
	return segmentEscaper.Replace(name)
}

// shown is a member name or pointer for a message: JSON-escaped, so a
// newline or an escape sequence in a member name stays visible, and at
// most about 80 characters long.
func shown(s string) string {
	q := []rune(jsonQuote(s))
	if len(q) <= 80 {
		return string(q)
	}
	return string(q[:76]) + `..."`
}

// jsonQuote quotes a string as JSON.stringify does: the quote, the
// backslash and the control characters escaped, nothing else.
func jsonQuote(s string) string {
	var b strings.Builder
	b.WriteByte('"')
	for _, r := range s {
		switch r {
		case '"':
			b.WriteString(`\"`)
		case '\\':
			b.WriteString(`\\`)
		case '\b':
			b.WriteString(`\b`)
		case '\f':
			b.WriteString(`\f`)
		case '\n':
			b.WriteString(`\n`)
		case '\r':
			b.WriteString(`\r`)
		case '\t':
			b.WriteString(`\t`)
		default:
			if r < 0x20 {
				fmt.Fprintf(&b, `\u%04x`, r)
			} else {
				b.WriteRune(r)
			}
		}
	}
	b.WriteByte('"')
	return b.String()
}

// The fold's memory: one node per container location the fold has
// reached, in a trie that follows the document, so a location's memo,
// state and inherited bag cost a constant amount however deep it is.
// Pointers are rendered only for report rows and errors.
type foldDoc struct {
	key  string
	root any
	top  *pnode
}

type pnode struct {
	doc    *foldDoc
	parent *pnode
	// The segment from the parent: a member name when named, else an
	// index into a list or an operator node.
	name  string
	index int
	named bool
	raw   any

	names map[string]*pnode
	items []*pnode

	// 0 not yet folded, 1 being folded (on the active stack), 2 folded.
	state int8
	// How the walk entered it: 0 from its parent, 1 through a reference.
	via   int8
	value any

	// The bag at this location: nil until worked out.
	k     map[string]any
	kBusy bool

	// Its resolution met a cycle, so the resolver was told Cycle.
	cut bool
	// A member of a loop of references with no container in it, or a
	// reference into one: there is nothing to resolve it to.
	loop bool

	// The key operator this map is an occurrence of, decided once (shape,
	// then the host's Recognise): site is nil for none.
	siteDone bool
	site     *Op
	parts    *siteParts

	plainDone  bool
	plainValue any

	ptr    string
	ptrSet bool
}

// siteParts are a site's folded terms, kept so that Plain can rebuild the
// map without folding its members again.
type siteParts struct {
	op       *Op
	value    any
	siblings *tabnas.OrderedMap
}

// The report lists a row can go in.
const (
	listCycles int8 = iota
	listUnresolved
	listDynamic
)

type noteKey struct {
	list                            int8
	doc, at, targetDoc, target, src string
}

type folder struct {
	opts     *FoldOptions
	keys     KeyMap
	tokens   bool
	lookup   Positions
	maxDepth int
	base     map[string]any
	cycleErr bool
	report   FoldReport
	noted    map[noteKey]bool
	// The active stack: every container being folded, innermost last.
	stack []*pnode
	// The documents by key; a nil entry is one that cannot be had.
	docs map[string]*foldDoc
}

func (f *folder) makeDoc(key string, raw any) *foldDoc {
	doc := &foldDoc{key: key, root: raw}
	doc.top = &pnode{doc: doc, raw: raw}
	f.docs[key] = doc
	return doc
}

// docOf is another document, by key, loaded once; nil when it cannot be
// had.
func (f *folder) docOf(from *pnode, key string, named bool) (*foldDoc, error) {
	if !named || key == from.doc.key {
		return from.doc, nil
	}
	if doc, seen := f.docs[key]; seen {
		return doc, nil
	}
	if f.opts.Load != nil {
		raw, ok, err := f.opts.Load(key, f.siteOf(from))
		if err != nil {
			return nil, err
		}
		if ok {
			return f.makeDoc(key, raw), nil
		}
	}
	f.docs[key] = nil
	return nil, nil
}

func (f *folder) kidAt(pn *pnode, i int, raw any) *pnode {
	if pn.items == nil {
		pn.items = make([]*pnode, len(itemsOf(pn.raw)))
	}
	for len(pn.items) <= i {
		pn.items = append(pn.items, nil)
	}
	kid := pn.items[i]
	if kid == nil {
		kid = &pnode{doc: pn.doc, parent: pn, index: i, raw: raw}
		pn.items[i] = kid
	}
	return kid
}

func (f *folder) kidNamed(pn *pnode, name string, raw any) *pnode {
	if pn.names == nil {
		pn.names = map[string]*pnode{}
	}
	kid := pn.names[name]
	if kid == nil {
		kid = &pnode{doc: pn.doc, parent: pn, name: name, named: true, raw: raw}
		pn.names[name] = kid
	}
	return kid
}

// kidOf is the location one step of a path from pn reaches, the step taken
// through parent, pn's raw value.
func (f *folder) kidOf(pn *pnode, parent any, seg any, raw any) *pnode {
	if sh := shapeOf(parent); sh == shapeList || sh == shapeOp {
		return f.kidAt(pn, arrayIndex(seg, len(itemsOf(parent))), raw)
	}
	name, _ := segName(seg)
	return f.kidNamed(pn, name, raw)
}

func (f *folder) ptrOf(pn *pnode) string {
	if pn.ptrSet {
		return pn.ptr
	}
	var chain []*pnode
	p := pn
	for p != nil && !p.ptrSet {
		chain = append(chain, p)
		p = p.parent
	}
	s := ""
	if p != nil {
		s = p.ptr
	}
	for i := len(chain) - 1; 0 <= i; i-- {
		c := chain[i]
		if c.parent == nil {
			s = ""
		} else if c.named {
			s = s + "/" + escapeSegment(c.name)
		} else {
			s = s + "/" + strconv.Itoa(c.index)
		}
		c.ptr = s
		c.ptrSet = true
	}
	return s
}

func pathOf(pn *pnode) Path {
	path := Path{}
	for p := pn; p != nil && p.parent != nil; p = p.parent {
		if p.named {
			path = append(path, p.name)
		} else {
			path = append(path, p.index)
		}
	}
	for i, j := 0, len(path)-1; i < j; i, j = i+1, j-1 {
		path[i], path[j] = path[j], path[i]
	}
	return path
}

func (f *folder) note(key noteKey) bool {
	if f.noted[key] {
		return false
	}
	f.noted[key] = true
	return true
}

func (f *folder) noteEntry(list int8, row FoldEntry) {
	if !f.note(noteKey{list: list, doc: row.Doc, at: row.At, targetDoc: row.TargetDoc, target: row.Target}) {
		return
	}
	if list == listCycles {
		f.report.Cycles = append(f.report.Cycles, row)
	} else {
		f.report.Unresolved = append(f.report.Unresolved, row)
	}
}

func (f *folder) noteDynamic(row FoldDynamic) {
	if f.note(noteKey{list: listDynamic, doc: row.Doc, at: row.At, src: row.Src}) {
		f.report.Dynamic = append(f.report.Dynamic, row)
	}
}

// kOf is the bag at a map: the Inherit calls of its ancestors and of the
// map itself, from the root down, each kept, worked out without recursion.
func (f *folder) kOf(pn *pnode) (map[string]any, error) {
	if f.opts.Inherit == nil {
		return f.base, nil
	}
	if pn.k != nil {
		return pn.k, nil
	}
	var chain []*pnode
	p := pn
	for p != nil && p.k == nil {
		if p.kBusy {
			ptr := f.ptrOf(p)
			return nil, &FoldError{Code: codeFoldInherit, Doc: p.doc.key, At: ptr,
				message: "inherit folded its own subtree at " + shown(ptr)}
		}
		chain = append(chain, p)
		p = p.parent
	}
	k := f.base
	if p != nil {
		k = p.k
	}
	for i := len(chain) - 1; 0 <= i; i-- {
		c := chain[i]
		if shapeOf(c.raw) == shapeRecord {
			site := f.siteOf(c)
			site.inInherit = true
			site.parentK = k
			c.kBusy = true
			next, err := f.opts.Inherit(site, c.raw, k)
			c.kBusy = false
			if err != nil {
				return nil, err
			}
			if next == nil {
				ptr := f.ptrOf(c)
				return nil, &FoldError{Code: codeFoldInherit, Doc: c.doc.key, At: ptr,
					message: "inherit returned no bag at " + shown(ptr)}
			}
			k = next
		}
		c.k = k
	}
	return k, nil
}

func (f *folder) siteOf(pn *pnode) *Site {
	site := &Site{Root: pn.doc.root, Doc: pn.doc.key, Node: pn.raw, f: f, pn: pn}
	if f.lookup != nil && shapeOf(pn.raw) != shapeLeaf {
		site.Token = f.lookup(pn.raw)
	}
	return site
}

func (f *folder) locate(from *pnode, target Path, key string, named bool) (FoldHit, error) {
	doc, err := f.docOf(from, key, named)
	if err != nil || doc == nil {
		return FoldHit{}, err
	}
	node := doc.root
	for _, seg := range target {
		next, ok := step(node, seg)
		if !ok {
			return FoldHit{}, nil
		}
		node = next
	}
	return FoldHit{Found: true, Value: node}, nil
}

func (f *folder) miss(from *pnode, targetDoc string, target Path) FoldHit {
	f.noteEntry(listUnresolved, FoldEntry{
		Doc: from.doc.key, At: f.ptrOf(from), TargetDoc: targetDoc, Target: pointer(target)})
	return FoldHit{}
}

// cycleTo answers a cycle: from named target while target was still being
// folded (a back edge), or target is in a loop of references. Every member
// of a loop made only of references is a cycle, and so is every reference
// into one, since none of them has anything to resolve to. A cycle that
// passes through a container is cut once, at the site whose target was
// being folded; a later reference to that site gets its folded value.
func (f *folder) cycleTo(from, target *pnode) (FoldHit, error) {
	row := FoldEntry{Doc: from.doc.key, At: f.ptrOf(from), TargetDoc: target.doc.key, Target: f.ptrOf(target)}
	if f.cycleErr {
		return FoldHit{}, &FoldError{Code: codeFoldCycle,
			Doc: row.Doc, At: row.At, TargetDoc: row.TargetDoc, Target: row.Target,
			message: "cycle at " + shown(row.At) + " to " + shown(row.Target)}
	}
	f.noteEntry(listCycles, row)
	from.cut = true
	if target.state == 1 {
		i := len(f.stack) - 1
		for ; 0 <= i && f.stack[i] != target; i-- {
		}
		refs := 0 <= i
		for j := i + 1; refs && j < len(f.stack); j++ {
			refs = f.stack[j].via == 1
		}
		if refs {
			for j := i; j < len(f.stack); j++ {
				f.stack[j].loop = true
			}
			from.loop = true
		}
	} else if target.loop {
		from.loop = true
	}
	return FoldHit{Found: true, Cycle: true}, nil
}

// at is the folded value at target in a document, for the site at from.
func (f *folder) at(from *pnode, target Path, key string, named bool) (FoldHit, error) {
	targetDoc := from.doc.key
	if named {
		targetDoc = key
	}
	doc, err := f.docOf(from, key, named)
	if err != nil {
		return FoldHit{}, err
	}
	if doc == nil {
		return f.miss(from, targetDoc, target), nil
	}
	pn := doc.top
	node := doc.root
	for i := 0; i < len(target); i++ {
		v, ok := step(node, target[i])
		if !ok {
			return f.miss(from, targetDoc, target), nil
		}
		vs := shapeOf(v)
		if i == len(target)-1 && vs == shapeLeaf {
			return FoldHit{Found: true, Value: v}, nil
		}
		pn = f.kidOf(pn, node, target[i], v)
		node = v
		// Only a static site changes what lies below it; any other map,
		// a dynamic one or one Recognise refused, folds member by member,
		// so the raw path below it names the same location.
		if i < len(target)-1 && vs == shapeRecord {
			op, err := f.siteOpOf(pn)
			if err != nil {
				return FoldHit{}, err
			}
			if op != nil && !op.Dynamic {
				if pn.state == 1 || pn.loop {
					return f.cycleTo(from, pn)
				}
				folded, err := f.walk(pn, 1)
				if err != nil {
					return FoldHit{}, err
				}
				if pn.loop || pn.cut {
					return f.cycleTo(from, pn)
				}
				for i++; i < len(target); i++ {
					if folded, ok = step(folded, target[i]); !ok {
						return f.miss(from, targetDoc, target), nil
					}
				}
				return FoldHit{Found: true, Value: folded}, nil
			}
		}
	}
	if pn.state == 1 {
		return f.cycleTo(from, pn)
	}
	value, err := f.walk(pn, 1)
	if err != nil {
		return FoldHit{}, err
	}
	if pn.loop {
		return f.cycleTo(from, pn)
	}
	return FoldHit{Found: true, Value: value}, nil
}

// siteOpOf is the key operator a map is an occurrence of here, if any: its
// shape, then the host's Recognise, decided once per location.
func (f *folder) siteOpOf(pn *pnode) (*Op, error) {
	if pn.siteDone {
		return pn.site, nil
	}
	op := KeyOpOf(f.keys, pn.raw)
	pn.siteDone = true
	pn.site = op
	if op != nil && f.opts.Recognise != nil {
		ok, err := f.opts.Recognise(f.siteOf(pn), op)
		if err != nil {
			return nil, err
		}
		if !ok {
			pn.site = nil
			op = nil
		}
	}
	return op, nil
}

// memberAt and memberNamed are a member's folded value: a container
// through the memory, anything else as it is.
func (f *folder) memberAt(pn *pnode, i int, child any) (any, error) {
	if shapeOf(child) == shapeLeaf {
		return child, nil
	}
	return f.walk(f.kidAt(pn, i, child), 0)
}

func (f *folder) memberNamed(pn *pnode, name string, child any) (any, error) {
	if shapeOf(child) == shapeLeaf {
		return child, nil
	}
	return f.walk(f.kidNamed(pn, name, child), 0)
}

// plainOf is a map folded as a plain map, members in its own order.
func (f *folder) plainOf(pn *pnode) (any, error) {
	if pn.plainDone {
		return pn.plainValue, nil
	}
	node := pn.raw
	if shapeOf(node) != shapeRecord {
		return f.walk(pn, 0)
	}
	parts := pn.parts
	names := recordNames(node)
	out := newRecordLike(node, len(names))
	for _, name := range names {
		var v any
		switch {
		case parts == nil:
			child, _ := recordGet(node, name)
			folded, err := f.memberNamed(pn, name, child)
			if err != nil {
				return nil, err
			}
			v = folded
		case name == parts.op.Src:
			v = parts.value
		default:
			v = parts.siblings.Vals[name]
		}
		recordSet(out, name, v)
	}
	pn.plainDone = true
	pn.plainValue = out
	return out, nil
}

func (f *folder) walk(pn *pnode, via int8) (any, error) {
	if pn.state == 2 {
		return pn.value, nil
	}
	node := pn.raw
	sh := shapeOf(node)
	if sh == shapeLeaf || pn.state == 1 {
		return node, nil
	}
	if f.maxDepth <= len(f.stack) {
		ptr := f.ptrOf(pn)
		return nil, &FoldError{Code: codeFoldDepth, Doc: pn.doc.key, At: ptr,
			message: "nesting and references deeper than " + strconv.Itoa(f.maxDepth) + " at " + shown(ptr)}
	}
	pn.state = 1
	pn.via = via
	f.stack = append(f.stack, pn)
	out, err := f.walkInto(pn, node, sh)
	f.stack = f.stack[:len(f.stack)-1]
	pn.state = 0
	if err != nil {
		return nil, err
	}
	pn.state = 2
	pn.value = out
	return out, nil
}

// walkInto folds a container that walk has put on the active stack.
func (f *folder) walkInto(pn *pnode, node any, sh shape) (any, error) {
	switch sh {
	case shapeOp:
		// Each term is folded at its own path first, so a key-operator
		// site inside a token expression resolves, and a nested expression
		// is kept by path. With Tokens off the operator node is rebuilt
		// around its own head, so it stays an operator node.
		items := itemsOf(node)
		op := items[0].(*Op)
		terms := make([]any, 0, len(items)-1)
		for i := 1; i < len(items); i++ {
			if isUnfilled(items[i]) {
				continue
			}
			term, err := f.memberAt(pn, i, items[i])
			if err != nil {
				return nil, err
			}
			terms = append(terms, term)
		}
		if f.tokens {
			return f.opts.Evaluate(f.siteOf(pn), op, terms)
		}
		return newListLike(node, append([]any{op}, terms...)), nil

	case shapeList:
		items := itemsOf(node)
		out := make([]any, len(items))
		for i, c := range items {
			if shapeOf(c) == shapeLeaf {
				out[i] = c
				continue
			}
			folded, err := f.walk(f.kidAt(pn, i, c), 0)
			if err != nil {
				return nil, err
			}
			out[i] = folded
		}
		return newListLike(node, out), nil
	}

	op, err := f.siteOpOf(pn)
	if err != nil {
		return nil, err
	}
	if op == nil || op.Dynamic {
		if op != nil {
			f.noteDynamic(FoldDynamic{Doc: pn.doc.key, At: f.ptrOf(pn), Src: op.Src})
		}
		if pn.plainDone {
			return pn.plainValue, nil
		}
		names := recordNames(node)
		out := newRecordLike(node, len(names))
		for _, name := range names {
			c, _ := recordGet(node, name)
			if shapeOf(c) != shapeLeaf {
				folded, err := f.walk(f.kidNamed(pn, name, c), 0)
				if err != nil {
					return nil, err
				}
				c = folded
			}
			recordSet(out, name, c)
		}
		pn.plainDone = true
		pn.plainValue = out
		return out, nil
	}

	names := recordNames(node)
	siblings := newSiblings(node, len(names))
	var value any
	for _, name := range names {
		c, _ := recordGet(node, name)
		folded, err := f.memberNamed(pn, name, c)
		if err != nil {
			return nil, err
		}
		if name == op.Src {
			value = folded
		} else {
			recordSet(siblings, name, folded)
		}
	}
	pn.parts = &siteParts{op: op, value: value, siblings: siblings}
	site := f.siteOf(pn)
	// A copy, as TypeScript hands the resolver a new description per site,
	// so a resolver cannot change the table.
	described := *op
	return f.opts.Evaluate(site, &described, []any{value, siblings})
}

// RecordPositions records the opening token of every map, and of every
// token operator node, a parse on j builds, by the container's identity,
// and returns the lookup Fold takes as Positions. It subscribes to the
// engine's post-process rule event on j, so it works in any grammar.
//
// Go has no weak map, so each parse records into a table of its own, kept
// in the parse's context, and the lookup answers from the table of the
// last parse on j that finished; that table, and through it the
// containers it names, is kept until the next parse finishes. In deferred
// mode, pass Positions true instead: it reads the table of the parse
// being folded. A list other than an operator node has no identity a
// table can key (a Go slice can move as it grows), so only maps and
// operator nodes have a position.
func RecordPositions(j *tabnas.Tabnas) Positions {
	rec := newPositionRecorder(j, true)
	return rec.lookup
}

// positionKey identifies a container: an *OrderedMap or *ListRef by its
// pointer, a map[string]any or a MapRef by its map's.
type positionKey struct {
	kind uint8
	ptr  uintptr
}

func positionKeyOf(node any) (positionKey, bool) {
	switch t := node.(type) {
	case *tabnas.OrderedMap:
		if t != nil {
			return positionKey{kind: 1, ptr: reflect.ValueOf(t).Pointer()}, true
		}
	case map[string]any:
		if t != nil {
			return positionKey{kind: 2, ptr: reflect.ValueOf(t).Pointer()}, true
		}
	case tabnas.MapRef:
		if t.Val != nil {
			return positionKey{kind: 2, ptr: reflect.ValueOf(t.Val).Pointer()}, true
		}
	case *tabnas.ListRef:
		if t != nil {
			return positionKey{kind: 3, ptr: reflect.ValueOf(t).Pointer()}, true
		}
	}
	return positionKey{}, false
}

// positionTable is one parse's tokens. Each entry holds the container as
// well as its token: a strong reference keeps the address the key is made
// from from being reused while the entry lives.
type positionTable struct {
	mu     sync.RWMutex
	tokens map[positionKey]positionEntry
}

type positionEntry struct {
	node  any
	token *tabnas.Token
}

func (t *positionTable) get(node any) *tabnas.Token {
	if t == nil {
		return nil
	}
	key, ok := positionKeyOf(node)
	if !ok {
		return nil
	}
	t.mu.RLock()
	entry, found := t.tokens[key]
	t.mu.RUnlock()
	if !found {
		return nil
	}
	return entry.token
}

type positionRecorder struct {
	slot string // the context's U key of a parse's table
	mu   sync.Mutex
	last *positionTable
}

// newPositionRecorder subscribes to j's post-process rule event: on an
// open pass it records the rule's node and opening token, the first token
// for a container winning; on the last pass of a parse that succeeds, when
// publish is set, it makes that parse's table the one lookup reads.
func newPositionRecorder(j *tabnas.Tabnas, publish bool) *positionRecorder {
	rec := &positionRecorder{}
	rec.slot = fmt.Sprintf("expr_positions_%p", rec)
	j.SubRuleDone(func(r *tabnas.Rule, ctx *tabnas.Context, done tabnas.RuleDone) {
		if done.State == tabnas.OPEN {
			key, ok := positionKeyOf(r.Node)
			if !ok || r.O0 == nil || ctx.U == nil {
				return
			}
			table := rec.tableIn(ctx, true)
			table.mu.Lock()
			if _, seen := table.tokens[key]; !seen {
				table.tokens[key] = positionEntry{node: r.Node, token: r.O0}
			}
			table.mu.Unlock()
			return
		}
		if publish && lastPass(r, ctx, done) {
			table := rec.tableIn(ctx, false)
			rec.mu.Lock()
			rec.last = table
			rec.mu.Unlock()
		}
	})
	return rec
}

// tableIn is the parse's table, made on first use when create is set.
func (rec *positionRecorder) tableIn(ctx *tabnas.Context, create bool) *positionTable {
	if ctx == nil || ctx.U == nil {
		return nil
	}
	if table, ok := ctx.U[rec.slot].(*positionTable); ok {
		return table
	}
	if !create {
		return nil
	}
	table := &positionTable{tokens: map[positionKey]positionEntry{}}
	ctx.U[rec.slot] = table
	return table
}

func (rec *positionRecorder) lookup(node any) *tabnas.Token {
	rec.mu.Lock()
	table := rec.last
	rec.mu.Unlock()
	return table.get(node)
}

// lastPass reports whether a post-process rule event is the last pass of
// a parse that will succeed, which is where the deferred mode folds: a
// close at depth 0 whose alternate is nil (the rule has no close
// alternates) or neither pushes, replaces nor failed, outside any probe
// and not synthesised by recovery. Go reports a failure two ways the
// TypeScript event folds into alt.err: an alternate that matched and then
// raised (its E) leaves the error on the context rather than on Alt, and,
// with recovery off, a lexer error latched by the pass or a token left
// after it fails the parse once the rule loop has ended. A parse that
// fails is therefore never folded.
func lastPass(r *tabnas.Rule, ctx *tabnas.Context, done tabnas.RuleDone) bool {
	if r == nil || ctx == nil || r.D != 0 || done.State != tabnas.CLOSE || done.Probe || done.Forced {
		return false
	}
	if alt := done.Alt; alt != nil && (alt.P != "" || alt.R != "" || alt.Err != nil) {
		return false
	}
	if ctx.ParseErr != nil {
		return false
	}
	if ctx.Cfg == nil || !ctx.Cfg.Recover.Enabled {
		if ctx.Lex != nil && ctx.Lex.Err != nil {
			return false
		}
		if t := ctx.T0; t != nil && !t.IsNoToken() && t.Tin != tabnas.TinZZ {
			return false
		}
	}
	return true
}

// foldHint is the hint a deferred fold's error carries: the fold codes
// are the layer's own, not in any instance's catalogue, so the engine
// would otherwise show its hint for an unknown code.
const foldHint = "The source parsed, and the expr plugin's deferred fold of it failed.\n" +
	"The code names the failure: see FoldError in the expr reference."

// deferredFailure is what a deferred fold panics with, for the engine to
// return from the parse. A FoldError becomes a *tabnas.TabnasError with
// the fold code, since the engine keeps a TabnasError's code and labels any
// other panic "internal"; a TabnasError goes as it is; any other error
// goes as it is too, and so arrives as "internal", its text kept.
func deferredFailure(err error) any {
	var fe *FoldError
	if errors.As(err, &fe) {
		return &tabnas.TabnasError{Code: fe.Code, Detail: err.Error(), Hint: foldHint}
	}
	var te *tabnas.TabnasError
	if errors.As(err, &te) && te != nil {
		return te
	}
	return err
}

// deferFold wires the deferred mode onto an instance: the fold runs on the
// last pass of every parse and writes the node of the rule that pass
// belongs to, which is the rule Parse returns the node of (a start rule
// replaced on the way hands it on).
func deferFold(j *tabnas.Tabnas, fold FoldOptions, keys KeyMap) error {
	if fold.Evaluate == nil {
		return &FoldError{Code: codeFoldOption, message: "Evaluate is required"}
	}
	lookup, record, err := positionsOption(fold.Positions)
	if err != nil {
		return err
	}
	var rec *positionRecorder
	if record {
		rec = newPositionRecorder(j, false)
	}
	j.SubRuleDone(func(r *tabnas.Rule, ctx *tabnas.Context, done tabnas.RuleDone) {
		if !lastPass(r, ctx, done) {
			return
		}
		positions := lookup
		if rec != nil {
			table := rec.tableIn(ctx, false)
			positions = table.get
		}
		out, err := foldTree(r.Node, &fold, keys, positions)
		if err != nil {
			panic(deferredFailure(err))
		}
		r.Node = out
	})
	return nil
}
