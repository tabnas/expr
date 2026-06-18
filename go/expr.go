/* Copyright (c) 2021-2025 Richard Rodger and other contributors, MIT License */

// Package expr provides a Pratt-parser expression plugin for the jsonic
// JSON parser. It supports infix, prefix, suffix, ternary and paren
// operators with configurable precedence.
//
// Expressions are encoded as LISP-style S-expressions using arrays/slices.
// The operator source string is the first element of the slice.
package expr

import (
	"strings"

	jsonic "github.com/tabnas/jsonic/go"
)

// Version is the Go module version of this plugin.
const Version = "0.1.3"

// OpDef defines an operator for the expression parser.
type OpDef struct {
	Src     interface{} // string or []string (for ternary)
	OSrc    string
	CSrc    string
	Left    int
	Right   int
	Prefix  bool
	Suffix  bool
	Infix   bool
	Ternary bool
	Paren   bool
	Preval  interface{}
	Use     interface{}
}

// Op is the full operator description available during parsing and evaluation.
type Op struct {
	Name    string
	Src     string
	Left    int
	Right   int
	Prefix  bool
	Suffix  bool
	Infix   bool
	Ternary bool
	Paren   bool
	Terms   int
	Tkn     string
	Tin     int
	OSrc    string
	CSrc    string
	OTkn    string
	OTin    int
	CTkn    string
	CTin    int
	Preval  PrevalDef
	Use     interface{}
}

// PrevalDef specifies paren-preval options.
type PrevalDef struct {
	Active   bool
	Required bool
	Allow    []string
}

// ExprOptions configures the Expr plugin.
type ExprOptions struct {
	Op       map[string]*OpDef
	Evaluate func(rule *jsonic.Rule, ctx *jsonic.Context, op *Op, terms []interface{}) interface{}
}

// _unfilled is a sentinel value for pre-allocated but unfilled expression slots.
// Expression nodes are wrapped in *jsonic.ListRef so the slice header lives
// inside a struct that all references share by pointer. Re-pointing a
// ListRef.Val in one rule's action is then visible to every other rule
// that captured the same *ListRef — the property TS arrays get for free
// via in-place mutation.
var _unfilled interface{} = &struct{ x int }{-1}

func isUnfilled(v interface{}) bool { return v == _unfilled }

// unwrapExpr returns the underlying op-array from a *ListRef wrapper,
// or the value as-is if it's already a plain slice (or anything else).
// Returns (slice, ok) where ok is true if the value is an expression
// slice (wrapped or unwrapped).
func unwrapExpr(node interface{}) ([]interface{}, bool) {
	if lr, ok := node.(*jsonic.ListRef); ok {
		return lr.Val, lr.Val != nil
	}
	if sl, ok := node.([]interface{}); ok {
		return sl, true
	}
	return nil, false
}

// isOp checks if a node is an expression (slice starting with *Op).
// Accepts *jsonic.ListRef wrappers and plain []interface{}.
func isOp(node interface{}) bool {
	sl, ok := unwrapExpr(node)
	if !ok || len(sl) == 0 {
		return false
	}
	_, isOpV := sl[0].(*Op)
	return isOpV
}

// isExprOp checks if a node's op is an infix/prefix/suffix expression
// (not a ternary or paren, which are structural and shouldn't be drilled into).
func isExprOp(node interface{}) bool {
	sl, ok := unwrapExpr(node)
	if !ok || len(sl) == 0 {
		return false
	}
	if op, ok := sl[0].(*Op); ok {
		return !op.Ternary && !op.Paren
	}
	return false
}

// fillNextSlot walks the expression tree depth-first and fills the deepest
// unfilled (_unfilled sentinel) slot with val. The node parameter must be
// a *jsonic.ListRef (or nil/non-expr — returns false). Mutates ListRef.Val
// in place via index assignment so every rule holding the same pointer
// observes the fill. Returns true if a slot was filled.
func fillNextSlot(node interface{}, val interface{}) bool {
	return fillNextSlotSeen(node, val, nil)
}

func fillNextSlotSeen(node interface{}, val interface{}, seen map[*jsonic.ListRef]bool) bool {
	box, _ := node.(*jsonic.ListRef)
	if box == nil || len(box.Val) == 0 {
		return false
	}
	// Guard against cyclic expression graphs: a ternary/prefix rewrite
	// that re-points a shared *ListRef can leave a node reachable from
	// itself. Without this the depth-first walk recurses forever.
	if seen == nil {
		seen = map[*jsonic.ListRef]bool{}
	}
	if seen[box] {
		return false
	}
	seen[box] = true
	op, ok := box.Val[0].(*Op)
	if !ok {
		return false
	}
	// Check children first (depth-first) to fill innermost incomplete expr.
	for i := 1; i <= op.Terms && i < len(box.Val); i++ {
		if sub, ok := box.Val[i].(*jsonic.ListRef); ok {
			if fillNextSlotSeen(sub, val, seen) {
				return true
			}
		}
	}
	// Then check this node's own slots.
	for i := 1; i <= op.Terms && i < len(box.Val); i++ {
		if isUnfilled(box.Val[i]) {
			box.Val[i] = val
			return true
		}
	}
	return false
}

// makeExpr creates a pre-allocated expression wrapped in *jsonic.ListRef.
// The wrapper means later rule actions can re-point ListRef.Val (e.g. when
// a ternary opens after a prefix/suffix expr) and every rule holding the
// same pointer sees the update — Go slices don't share that property.
func makeExpr(op *Op, terms ...interface{}) *jsonic.ListRef {
	n := op.Terms + 1
	val := make([]interface{}, n)
	val[0] = op
	for i := 1; i < n; i++ {
		if i-1 < len(terms) {
			val[i] = terms[i-1]
		} else {
			val[i] = _unfilled
		}
	}
	return &jsonic.ListRef{Val: val, Meta: map[string]any{"expr": true}}
}

// asListRef returns node as *jsonic.ListRef if it already is one, or wraps
// a plain []interface{} op-array in a fresh ListRef. Used to box values
// that arrived from outside this plugin so subsequent rebinding works.
func asListRef(node interface{}) *jsonic.ListRef {
	if lr, ok := node.(*jsonic.ListRef); ok {
		return lr
	}
	if sl, ok := node.([]interface{}); ok {
		return &jsonic.ListRef{Val: sl, Meta: map[string]any{"expr": true}}
	}
	return nil
}

// Expr is the expression parser plugin for jsonic.
func Expr(j *jsonic.Jsonic, opts map[string]interface{}) error {
	eopts := resolveOptions(opts)
	allOps := makeAllOps(j, eopts)

	// Build lookup maps.
	infixByTin := make(map[int]*Op)
	prefixByTin := make(map[int]*Op)
	suffixByTin := make(map[int]*Op)
	parenOpenByTin := make(map[int]*Op)
	parenCloseByTin := make(map[int]*Op)
	ternaryByTin := make(map[int]*Op)
	ternaryCloseByTin := make(map[int]*Op)

	for _, op := range allOps {
		if op.Infix {
			infixByTin[op.Tin] = op
		}
		if op.Prefix {
			prefixByTin[op.Tin] = op
		}
		if op.Suffix {
			suffixByTin[op.Tin] = op
		}
		if op.Paren {
			parenOpenByTin[op.OTin] = op
			parenCloseByTin[op.CTin] = op
		}
		if op.Ternary {
			ternaryByTin[op.Tin] = op
			ternaryCloseByTin[op.CTin] = op
		}
	}

	collectTins := func(m map[int]*Op) []int {
		var tins []int
		for t := range m {
			tins = append(tins, t)
		}
		return tins
	}

	PREFIX := collectTins(prefixByTin)
	INFIX := collectTins(infixByTin)
	SUFFIX := collectTins(suffixByTin)
	OP := collectTins(parenOpenByTin)
	CP := collectTins(parenCloseByTin)
	TERN0 := collectTins(ternaryByTin)
	TERN1 := collectTins(ternaryCloseByTin)

	hasPrefix := len(PREFIX) > 0
	hasInfix := len(INFIX) > 0
	hasSuffix := len(SUFFIX) > 0
	hasParen := len(OP) > 0
	hasTernary := len(TERN0) > 0

	// Check if any paren op has preval active.
	hasPreval := false
	for _, op := range allOps {
		if op.Paren && op.Preval.Active {
			hasPreval = true
			break
		}
	}

	mkS := func(tins []int) [][]int { return [][]int{tins} }

	// appendExprTag appends "expr" to the alt's G (group tag), mirroring
	// the TS plugin's tagExpr helper — which in turn mirrors the jsonic
	// grammar(...) setting {rule:{alt:{g:'expr'}}}. Applied manually
	// because the plugin uses j.Rule() (not j.Grammar()).
	appendExprTag := func(a *jsonic.AltSpec) {
		if a == nil {
			return
		}
		if a.G == "" {
			a.G = "expr"
		} else {
			a.G = a.G + ",expr"
		}
	}

	// modifyRule wraps j.Rule(): snapshot the existing alt pointers on
	// rs.Open/rs.Close, run the modifier, then tag only the alts the
	// modifier added (by identity) with "expr".
	modifyRule := func(name string, fn func(rs *jsonic.RuleSpec)) {
		j.Rule(name, func(rs *jsonic.RuleSpec, _ *jsonic.Parser) {
			preOpen := make(map[*jsonic.AltSpec]bool, len(rs.OpenAlts()))
			for _, a := range rs.OpenAlts() {
				preOpen[a] = true
			}
			preClose := make(map[*jsonic.AltSpec]bool, len(rs.CloseAlts()))
			for _, a := range rs.CloseAlts() {
				preClose[a] = true
			}
			fn(rs)
			for _, a := range rs.OpenAlts() {
				if !preOpen[a] {
					appendExprTag(a)
				}
			}
			for _, a := range rs.CloseAlts() {
				if !preClose[a] {
					appendExprTag(a)
				}
			}
		})
	}

	// tagAllAlts tags every alt on the given rule spec with "expr".
	// Used for plugin-created rules (expr, paren, ternary) where every
	// alt is plugin-added.
	tagAllAlts := func(rs *jsonic.RuleSpec) {
		for _, a := range rs.OpenAlts() {
			appendExprTag(a)
		}
		for _, a := range rs.CloseAlts() {
			appendExprTag(a)
		}
	}

	// === VAL rule modifications ===
	modifyRule("val", func(rs *jsonic.RuleSpec) {
		// Prefix operator: backtrack and push to 'expr'.
		if hasPrefix {
			rs.PrependOpen(&jsonic.AltSpec{
				S: mkS(PREFIX),
				B: 1,
				P: "expr",
				N: map[string]int{"expr_prefix": 1, "expr_suffix": 0},
				G: "expr,prefix",
			})
		}

		// Preval: value followed by paren open (e.g., foo(1,2)).
		if hasPreval {
			valTinsLocal := j.TokenSet("VAL")
			rs.PrependOpen(&jsonic.AltSpec{
				S: [][]int{valTinsLocal, OP},
				B: 1,
				P: "expr",
				C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
					pdef := parenOpenByTin[r.O1.Tin]
					if pdef == nil || !pdef.Preval.Active {
						return false
					}
					if len(pdef.Preval.Allow) > 0 {
						val, _ := r.O0.ResolveVal(r, ctx).(string)
						for _, a := range pdef.Preval.Allow {
							if a == val {
								return true
							}
						}
						return false
					}
					return true
				},
				U: map[string]interface{}{"paren_preval": true},
				A: func(r *jsonic.Rule, ctx *jsonic.Context) {
					r.Node = r.O0.ResolveVal(r, ctx)
				},
				G: "expr,paren,preval",
			})
		}

		// Block pair detection when inside ternary and the colon
		// is a ternary close token (e.g., `1?2:3` — the `2:` should
		// NOT be treated as a key-value pair).
		if hasTernary {
			rs.PrependOpen(&jsonic.AltSpec{
				S: [][]int{j.TokenSet("VAL"), TERN1},
				B: 1,
				C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
					return r.N["expr_ternary"] > 0
				},
				// Clear the parent-seeded node (mirrors json's #VAL @reset$).
				// This alt consumes the value token (r.O0, e.g. the `2` of
				// `2:3`) without going through json's #VAL alt, so its
				// @reset$ never runs. The engine seeds a pushed val from its
				// parent (here the ternary's *ListRef op-array). Because
				// @val-bc's primitive-vs-container test treats the plugin's
				// *ListRef box as a primitive (it is not a recognised
				// container type), the stale box would survive coalescing
				// instead of the resolved scalar — and the ternary then-slot
				// would fill with the ternary node itself (a self-cycle).
				// Resetting forces @val-bc to resolve the matched token.
				A: func(r *jsonic.Rule, ctx *jsonic.Context) {
					r.Node = jsonic.Undefined
				},
				G: "expr,ternary,block-pair",
			})
		}

		// Paren open: backtrack and push to 'expr'.
		if hasParen {
			rs.PrependOpen(&jsonic.AltSpec{
				S: mkS(OP),
				B: 1,
				P: "expr",
				C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
					pdef := parenOpenByTin[r.O0.Tin]
					return !pdef.Preval.Required
				},
				G: "expr,paren",
			})
		}

		// Infix after value: backtrack, replace with 'expr' (only when NOT inside an expr).
		if hasInfix {
			rs.PrependClose(&jsonic.AltSpec{
				S: mkS(INFIX),
				B: 1,
				N: map[string]int{"expr_prefix": 0, "expr_suffix": 0},
				RF: func(r *jsonic.Rule, ctx *jsonic.Context) string {
					if r.N["expr"] < 1 {
						return "expr"
					}
					return ""
				},
				G: "expr,infix",
			})
		}

		// Suffix after value: backtrack, replace with 'expr' (only when NOT inside an expr).
		if hasSuffix {
			rs.PrependClose(&jsonic.AltSpec{
				S: mkS(SUFFIX),
				B: 1,
				N: map[string]int{"expr_prefix": 0, "expr_suffix": 1},
				RF: func(r *jsonic.Rule, ctx *jsonic.Context) string {
					if r.N["expr"] < 1 {
						return "expr"
					}
					return ""
				},
				G: "expr,suffix",
			})
		}

		// Ternary first separator.
		if hasTernary {
			rs.PrependClose(&jsonic.AltSpec{
				S: mkS(TERN0),
				B: 1,
				C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
					return r.N["expr"] < 1
				},
				R: "ternary",
				G: "expr,ternary",
			})

			// Ternary close: backtrack so ternary rule can consume it.
			rs.PrependClose(&jsonic.AltSpec{
				S: mkS(TERN1),
				B: 1,
				C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
					return r.N["expr_ternary"] > 0
				},
				G: "expr,ternary,close",
			})
		}

		// Paren close propagation.
		if hasParen {
			rs.PrependClose(&jsonic.AltSpec{
				S: mkS(CP),
				B: 1,
				C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
					return r.N["expr_paren"] > 0
				},
				G: "expr,paren-close",
			})
		}

		// Prevent implicit list inside expression (comma).
		rs.PrependClose(&jsonic.AltSpec{
			S: mkS([]int{jsonic.TinCA}),
			B: 1,
			C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
				return (r.D == 1 && (r.N["expr"] >= 1 || r.N["expr_ternary"] >= 1)) ||
					(r.N["expr_ternary"] >= 1 && r.N["expr_paren"] >= 1)
			},
			G: "expr,imp,comma",
		})

		// Prevent implicit list inside expression (space).
		valTins := j.TokenSet("VAL")
		rs.PrependClose(&jsonic.AltSpec{
			S: mkS(valTins),
			B: 1,
			C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
				return (r.D == 1 && (r.N["expr"] >= 1 || r.N["expr_ternary"] >= 1)) ||
					(r.N["expr_ternary"] >= 1 && r.N["expr_paren"] >= 1)
			},
			G: "expr,imp,space",
		})

		// Chain-time preval: when val has produced a node and the next
		// token is a preval-active paren-open, push to 'expr' so the new
		// paren-form picks up this val's node as the preval term. This
		// complements the open-time [VAL,OP] preval alt for the leading
		// preval-paren of an expression — chain-time detection handles
		// subsequent parens because the leading "value" is by then a
		// produced node, not a token in the lex buffer.
		// Examples: a[0][1], f(x)(y), f(x)[i], (1+2)(3).
		if hasParen && hasPreval {
			rs.PrependClose(&jsonic.AltSpec{
				S: mkS(OP),
				B: 1,
				P: "expr",
				C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
					pdef := parenOpenByTin[r.C0.Tin]
					if pdef == nil || !pdef.Preval.Active {
						return false
					}
					if r.Node == nil || jsonic.IsUndefined(r.Node) {
						return false
					}
					if len(pdef.Preval.Allow) > 0 {
						s, _ := r.Node.(string)
						for _, a := range pdef.Preval.Allow {
							if a == s {
								return true
							}
						}
						return false
					}
					return true
				},
				U: map[string]interface{}{"paren_preval": true},
				G: "expr,paren,preval,chain",
			})
		}

		// Comma-op suppression: when an enclosing rule (e.g. an embedding
		// grammar's wrapper) sets n.no_comma_op, bail at `,` without
		// treating it as the comma operator — the parent then consumes
		// the `,` itself as a separator.
		if hasInfix {
			rs.PrependClose(&jsonic.AltSpec{
				S: mkS(INFIX),
				B: 1,
				C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
					return r.N["no_comma_op"] > 0 && r.C0 != nil && r.C0.Src == ","
				},
				G: "expr,no-comma-op-bail",
			})
		}
	})

	// === LIST rule modifications ===
	modifyRule("list", func(rs *jsonic.RuleSpec) {
		rs.AddBO(func(r *jsonic.Rule, ctx *jsonic.Context) {
			if r.Prev == nil || r.Prev == jsonic.NoRule || r.Prev.U["implist"] == nil {
				r.N["expr"] = 0
				r.N["expr_prefix"] = 0
				r.N["expr_suffix"] = 0
				r.N["expr_paren"] = 0
				r.N["expr_ternary"] = 0
			}
		})
		if hasParen {
			rs.PrependClose(&jsonic.AltSpec{
				S: mkS(CP),
				BF: func(r *jsonic.Rule, ctx *jsonic.Context) int {
					if r.C0.Tin == jsonic.TinCS && r.N["expr_paren"] < 1 {
						return 0
					}
					return 1
				},
				G: "expr,paren,list",
			})
			// Propagate implicit list node to enclosing paren.
			// Go slice append may reallocate, making paren.Child.Node
			// (which points to the original val) stale.
			rs.AddAC(func(r *jsonic.Rule, ctx *jsonic.Context) {
				if r.N["expr_paren"] > 0 && r.Parent != nil && r.Parent != jsonic.NoRule && r.Parent.Name == "paren" {
					r.Parent.Node = r.Node
				}
			})
		}
	})

	// === MAP rule modifications ===
	modifyRule("map", func(rs *jsonic.RuleSpec) {
		rs.AddBO(func(r *jsonic.Rule, ctx *jsonic.Context) {
			r.N["expr"] = 0
			r.N["expr_prefix"] = 0
			r.N["expr_suffix"] = 0
			r.N["expr_paren"] = 0
			r.N["expr_ternary"] = 0
		})
		if hasParen {
			rs.PrependClose(&jsonic.AltSpec{
				S: mkS(CP),
				BF: func(r *jsonic.Rule, ctx *jsonic.Context) int {
					if r.C0.Tin == jsonic.TinCB && r.N["expr_paren"] < 1 {
						return 0
					}
					return 1
				},
				G: "expr,paren,map",
			})
		}
	})

	// === PAIR rule modifications ===
	modifyRule("pair", func(rs *jsonic.RuleSpec) {
		if hasParen {
			rs.PrependClose(&jsonic.AltSpec{
				S: mkS(CP),
				B: 1,
				C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
					return r.N["expr_paren"] > 0 || r.N["pk"] > 0
				},
				G: "expr,paren,pair",
			})
		}
	})

	// === ELEM rule modifications ===
	modifyRule("elem", func(rs *jsonic.RuleSpec) {
		if hasParen {
			// Close implicit list within parens when ')' is seen.
			rs.PrependClose([]*jsonic.AltSpec{
				{
					S: mkS(CP),
					B: 1,
					C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
						return r.N["expr_paren"] > 0
					},
					G: "expr,paren,elem,close",
				},
				// Following elem is a paren expression.
				{
					S: mkS(OP),
					B: 1,
					R: "elem",
					G: "expr,paren,elem,open",
				},
			}...)
			// Propagate elem node to enclosing paren after close.
			// Go slice append may reallocate, making earlier
			// references to the list stale.
			rs.AddAC(func(r *jsonic.Rule, ctx *jsonic.Context) {
				if r.N["expr_paren"] > 0 {
					// Walk parent chain to find paren rule.
					for p := r.Parent; p != nil && p != jsonic.NoRule; p = p.Parent {
						if p.Name == "paren" {
							p.Node = r.Node
							break
						}
					}
				}
			})
		}
	})

	// === EXPR rule ===
	exprSpec := &jsonic.RuleSpec{Name: "expr"}

	exprOpen := make([]*jsonic.AltSpec, 0)

	// Paren open inside expression: push to 'paren' rule (not 'val').
	// The 'paren' rule consumes '(' and pushes to 'val', breaking the
	// val→expr→val backtrack loop.
	if hasParen {
		exprOpen = append(exprOpen, &jsonic.AltSpec{
			S: mkS(OP),
			P: "paren",
			B: 1,
			G: "expr,paren,open",
		})
	}

	// Prefix operator.
	if hasPrefix {
		exprOpen = append(exprOpen, &jsonic.AltSpec{
			S: mkS(PREFIX),
			P: "val",
			N: map[string]int{"expr": 1, "dlist": 1, "dmap": 1},
			C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
				return r.N["expr_prefix"] > 0
			},
			A: func(r *jsonic.Rule, ctx *jsonic.Context) {
				op := prefixByTin[r.O0.Tin]
				if isOp(r.Parent.Node) && isExprOp(r.Parent.Node) {
					r.Node = prattify(r.Parent.Node, op)
					r.Parent.Node = r.Node // sync after potential reallocation
				} else {
					r.Node = prior(r, r.Parent, op)
				}
			},
			G: "expr,prefix",
		})
	}

	// Infix operator.
	if hasInfix {
		exprOpen = append(exprOpen, &jsonic.AltSpec{
			S: mkS(INFIX),
			P: "val",
			N: map[string]int{"expr": 1, "expr_prefix": 0, "dlist": 1, "dmap": 1},
			A: func(r *jsonic.Rule, ctx *jsonic.Context) {
				op := infixByTin[r.O0.Tin]
				prev := r.Prev
				parent := r.Parent

				if isOp(parent.Node) && isExprOp(parent.Node) {
					r.Node = prattify(parent.Node, op)
					parent.Node = r.Node // sync after potential reallocation
				} else if isOp(prev.Node) {
					r.Node = prattify(prev.Node, op)
					r.Parent = prev
					prev.Node = r.Node // sync after potential reallocation
				} else {
					r.Node = prior(r, prev, op)
				}
			},
			G: "expr,infix",
		})
	}

	// Suffix operator.
	if hasSuffix {
		exprOpen = append(exprOpen, &jsonic.AltSpec{
			S: mkS(SUFFIX),
			N: map[string]int{"expr": 1, "expr_prefix": 0, "dlist": 1, "dmap": 1},
			A: func(r *jsonic.Rule, ctx *jsonic.Context) {
				op := suffixByTin[r.O0.Tin]
				prev := r.Prev
				if isOp(prev.Node) {
					r.Node = prattifySuffix(prev.Node, op)
				} else {
					r.Node = prior(r, prev, op)
				}
			},
			G: "expr,suffix",
		})
	}

	exprSpec.AddOpen(exprOpen...)

	// expr.BC: attach child result to incomplete expression.
	// Uses fillNextSlot to find the deepest unfilled slot and fill it.
	// This avoids Go slice append issues and works with the Go parser's
	// replacement-chain result extraction.
	exprSpec.AddBC(func(r *jsonic.Rule, ctx *jsonic.Context) {
		if r.Child == nil || r.Child == jsonic.NoRule {
			return
		}
		// Paren child: paren.AC already propagated the result.
		if r.Child.Name == "paren" {
			return
		}
		childNode := r.Child.Node
		if jsonic.IsUndefined(childNode) {
			childNode = nil
		}

		if box, ok := r.Node.(*jsonic.ListRef); ok && len(box.Val) > 0 {
			if _, isOpV := box.Val[0].(*Op); isOpV {
				fillNextSlot(box, childNode)
			}
		}
	})

	// expr.Close alternates.
	exprClose := make([]*jsonic.AltSpec, 0)

	// After paren child (paren rule completed).
	if hasParen {
		exprClose = append(exprClose, &jsonic.AltSpec{
			C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
				return r.Child != nil && r.Child != jsonic.NoRule && r.Child.Name == "paren"
			},
			N: map[string]int{"expr": 0},
			G: "expr,paren,end",
		})
	}

	// Comma-op suppression: bail at `,` and close the expr frame so the
	// parent rule (e.g. an embedding grammar's wrapper) consumes the
	// comma as a separator instead of as the comma operator.
	if hasInfix {
		exprClose = append(exprClose, &jsonic.AltSpec{
			S: mkS(INFIX),
			C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
				return r.N["no_comma_op"] > 0 && r.C0 != nil && r.C0.Src == ","
			},
			B: 1,
			N: map[string]int{"expr": 0},
			G: "expr,no-comma-op-bail",
		})
	}

	// More infix (not during prefix).
	if hasInfix {
		exprClose = append(exprClose, &jsonic.AltSpec{
			S: mkS(INFIX),
			C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
				return r.N["expr_prefix"] < 1
			},
			B: 1,
			R: "expr",
			G: "expr,infix,more",
		})
		// Infix seen during prefix: just end and backtrack.
		exprClose = append(exprClose, &jsonic.AltSpec{
			S: mkS(INFIX),
			C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
				return r.N["expr_prefix"] > 0
			},
			B: 1,
			G: "expr,infix,prefix-end",
		})
	}

	// More suffix (not during prefix).
	if hasSuffix {
		exprClose = append(exprClose, &jsonic.AltSpec{
			S: mkS(SUFFIX),
			C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
				return r.N["expr_prefix"] < 1
			},
			B: 1,
			R: "expr",
			G: "expr,suffix,more",
		})
	}

	// Paren close inside expression.
	if hasParen {
		exprClose = append(exprClose, &jsonic.AltSpec{
			S: mkS(CP),
			C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
				return r.N["expr_paren"] > 0
			},
			B: 1,
			G: "expr,paren,close",
		})
	}

	// Ternary start.
	if hasTernary {
		exprClose = append(exprClose, &jsonic.AltSpec{
			S: mkS(TERN0),
			C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
				return r.N["expr_prefix"] < 1
			},
			B: 1,
			R: "ternary",
			G: "expr,ternary",
		})
	}

	// Implicit list at top level (comma).
	valTins := j.TokenSet("VAL")
	exprClose = append(exprClose, &jsonic.AltSpec{
		S: mkS([]int{jsonic.TinCA}),
		C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
			return r.D <= 0
		},
		N: map[string]int{"expr": 0},
		R: "elem",
		A: func(r *jsonic.Rule, ctx *jsonic.Context) {
			node := r.Node
			if isOp(node) {
				node = cleanExpr(node)
			}
			r.Parent.Node = []interface{}{node}
			r.Node = r.Parent.Node
		},
		G: "expr,comma,list,top",
	})

	// Implicit list at top level (space).
	exprClose = append(exprClose, &jsonic.AltSpec{
		S: mkS(valTins),
		C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
			return r.D <= 0
		},
		N: map[string]int{"expr": 0},
		B: 1,
		R: "elem",
		A: func(r *jsonic.Rule, ctx *jsonic.Context) {
			node := r.Node
			if isOp(node) {
				node = cleanExpr(node)
			}
			r.Parent.Node = []interface{}{node}
			r.Node = r.Parent.Node
		},
		G: "expr,space,list,top",
	})

	// Implicit list inside paren (comma).
	// When expr finishes inside a paren (expr_paren > 0) and sees a
	// comma, wrap the expression in a list on the paren node and
	// replace with elem to process subsequent items.
	implicitListAction := func(r *jsonic.Rule, ctx *jsonic.Context) {
		// Find enclosing paren rule in the stack.
		// If a map or list rule sits between the expression and the paren,
		// the expression is inside a contained value — not a direct paren
		// child — so don't create an implicit list.
		var paren *jsonic.Rule
		for rI := ctx.RSI - 1; rI >= 0; rI-- {
			if ctx.RS[rI].Name == "paren" {
				paren = ctx.RS[rI]
				break
			}
			if ctx.RS[rI].Name == "map" || ctx.RS[rI].Name == "list" {
				return
			}
		}
		if paren == nil {
			return
		}
		node := r.Node
		if isOp(node) {
			node = cleanExpr(node)
		}
		// If paren already has a plain list (not an op-array), append.
		// Otherwise create a new list.
		if sl, ok := paren.Node.([]interface{}); ok && len(sl) > 0 {
			if _, isOpV := sl[0].(*Op); !isOpV {
				// Plain list, append.
				paren.Node = append(sl, node)
				r.Node = paren.Node
				return
			}
		}
		paren.Node = []interface{}{node}
		r.Node = paren.Node
	}
	if hasParen {
		// Only fire when there's no existing list/elem handling
		// the implicit list. Walk the parent chain to check if
		// there's an elem/list between this expr and the paren.
		isFirstImplicitInParen := func(r *jsonic.Rule) bool {
			if r.N["expr_paren"] < 1 || r.N["pk"] >= 1 {
				return false
			}
			for p := r.Parent; p != nil && p != jsonic.NoRule; p = p.Parent {
				if p.Name == "elem" || p.Name == "list" {
					return false // existing list machinery handles it
				}
				if p.Name == "paren" {
					return true // reached paren without finding elem/list
				}
			}
			return true
		}
		exprClose = append(exprClose, &jsonic.AltSpec{
			S: mkS([]int{jsonic.TinCA}),
			C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
				return isFirstImplicitInParen(r)
			},
			N: map[string]int{"expr": 0, "expr_prefix": 0, "expr_suffix": 0},
			R: "elem",
			A: implicitListAction,
			G: "expr,paren,imp,comma",
		})
		exprClose = append(exprClose, &jsonic.AltSpec{
			S: mkS(valTins),
			C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
				return isFirstImplicitInParen(r) && r.N["expr_suffix"] < 1
			},
			N: map[string]int{"expr": 0, "expr_prefix": 0, "expr_suffix": 0},
			B: 1,
			R: "elem",
			A: implicitListAction,
			G: "expr,paren,imp,space",
		})
	}

	// Implicit list (comma, not top).
	exprClose = append(exprClose, &jsonic.AltSpec{
		S: mkS([]int{jsonic.TinCA}),
		C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
			return r.N["pk"] < 1
		},
		N: map[string]int{"expr": 0},
		B: 1,
		G: "expr,list,imp,comma",
	})

	// Implicit list (space, not top).
	exprClose = append(exprClose, &jsonic.AltSpec{
		C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
			return r.N["pk"] < 1 && r.N["expr_suffix"] < 1
		},
		N: map[string]int{"expr": 0},
		G: "expr,list,imp,space",
	})

	// Expression ends on non-expression token (catch-all).
	// Required so ParseAlts finds a match when the expr rule has consumed
	// its tokens but the next token isn't one that extends the expression
	// (e.g. ZZ after a suffix like "1!"). Without this, jsonic/go >= v0.1.13
	// raises jsonic/unexpected.
	exprClose = append(exprClose, &jsonic.AltSpec{
		N: map[string]int{"expr": 0},
		G: "expr,expr-end",
	})

	exprSpec.AddClose(exprClose...)

	// AC: evaluate at root of expression, matching TS:
	//   if (options.evaluate && 0 === r.n.expr) {
	//     out = evaluation(r.parent, ctx, r.parent.node, options.evaluate)
	//     r.parent.node = out
	//     r.node = out
	//   }
	exprSpec.AddAC(func(r *jsonic.Rule, ctx *jsonic.Context) {
		if eopts.Evaluate != nil && r.N["expr"] < 1 {
			parent := r.Parent
			if parent != nil && parent != jsonic.NoRule {
				out := evaluation(parent, ctx, parent.Node, eopts.Evaluate)
				parent.Node = out
				// Also write the evaluated result onto this expr rule's own
				// node. When expr was PUSHED from a still-open value val (the
				// prefix/paren/suffix forms `-1`, `(1+2)`, `(2+1)!`), that
				// val's @val-bc coalescing reads its child (this expr) and
				// would otherwise keep the raw op-array. Writing the result
				// here makes the coalesced value the evaluated scalar. The
				// infix form replaces the val (r:) instead, so this is
				// harmless there.
				r.Node = out
			}
		}
	})

	tagAllAlts(exprSpec)
	j.RSM()["expr"] = exprSpec

	// === PAREN rule ===
	// Intermediary rule that consumes '(' and pushes to val.
	// This breaks the val→expr→val backtrack loop.
	if hasParen {
		parenSpec := &jsonic.RuleSpec{Name: "paren"}

		parenSpec.AddBO(func(r *jsonic.Rule, ctx *jsonic.Context) {
			// Allow implicits inside parens.
			r.N["dmap"] = 0
			r.N["dlist"] = 0
			r.N["pk"] = 0
		})

		parenSpec.AddOpen([]*jsonic.AltSpec{
			// Empty parens: ()
			{
				S: func() [][]int { return [][]int{OP, CP} }(),
				B: 1,
				G: "expr,paren,empty",
				C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
					oOp := parenOpenByTin[r.O0.Tin]
					cOp := parenCloseByTin[r.O1.Tin]
					return oOp != nil && cOp != nil && oOp.Name == cOp.Name
				},
				A: func(r *jsonic.Rule, ctx *jsonic.Context) {
					pop := parenOpenByTin[r.O0.Tin]
					pd := "expr_paren_depth_" + pop.Name
					r.U[pd] = 1
					r.N[pd] = 1
					r.Node = jsonic.Undefined
				},
			},
			// Normal paren open: consumes '(' and pushes to val.
			{
				S: mkS(OP),
				P: "val",
				N: map[string]int{
					"expr_paren":  1,
					"expr":        0,
					"expr_prefix": 0,
					"expr_suffix": 0,
				},
				G: "expr,paren,open",
				A: func(r *jsonic.Rule, ctx *jsonic.Context) {
					pop := parenOpenByTin[r.O0.Tin]
					pd := "expr_paren_depth_" + pop.Name
					r.U[pd] = 1
					r.N[pd] = 1
					r.Node = jsonic.Undefined
				},
			},
		}...)

		parenSpec.AddClose([]*jsonic.AltSpec{
			{
				S: mkS(CP),
				C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
					cop := parenCloseByTin[r.C0.Tin]
					if cop == nil {
						return false
					}
					pd := "expr_paren_depth_" + cop.Name
					_, ok := r.N[pd]
					return ok && r.N[pd] > 0
				},
				A: func(r *jsonic.Rule, ctx *jsonic.Context) {
					// Construct completed paren expression.
					cop := parenCloseByTin[r.C0.Tin]
					pop := parenOpenByTin[cop.OTin]
					if pop == nil {
						// Lookup by matching name.
						for _, op := range allOps {
							if op.Paren && op.Name == cop.Name {
								pop = op
								break
							}
						}
					}
					if pop == nil {
						return
					}

					val := r.Node

					// Build paren expression node as a *ListRef so it shares
					// the same shared-mutation contract as other op-arrays
					// (e.g., the chain-preval case where a chained paren
					// wraps a previously-built one).
					resultVal := []interface{}{pop}

					// Inject function name if preval is active.
					if r.Parent != nil && r.Parent != jsonic.NoRule &&
						r.Parent.Parent != nil && r.Parent.Parent != jsonic.NoRule &&
						r.Parent.Parent.U["paren_preval"] == true &&
						r.Parent.Parent.Node != nil {
						resultVal = append(resultVal, r.Parent.Parent.Node)
					}

					if !jsonic.IsUndefined(val) {
						resultVal = append(resultVal, val)
					}

					r.Node = &jsonic.ListRef{Val: resultVal, Meta: map[string]any{"expr": true}}
				},
				G: "expr,paren,close",
			},
		}...)

		parenSpec.AddBC(func(r *jsonic.Rule, ctx *jsonic.Context) {
			if r.Child == nil || r.Child == jsonic.NoRule {
				return
			}
			childNode := r.Child.Node
			if jsonic.IsUndefined(childNode) {
				return
			}
			if jsonic.IsUndefined(r.Node) {
				r.Node = childNode
			} else if isOp(childNode) {
				// Don't overwrite if paren.Node is already a plain list
				// (set by implicit list handling in elem/ternary).
				if !isOp(r.Node) {
					if sl, ok := r.Node.([]interface{}); ok && len(sl) > 0 {
						return // keep the implicit list
					}
				}
				r.Node = childNode
			}
		})

		parenSpec.AddAC(func(r *jsonic.Rule, ctx *jsonic.Context) {
			// Propagate paren result to parent.
			r.Parent.Node = r.Node
			if r.Parent.Parent != nil && r.Parent.Parent != jsonic.NoRule {
				r.Parent.Parent.Node = r.Node
			}
		})

		tagAllAlts(parenSpec)
		j.RSM()["paren"] = parenSpec
	}

	// === TERNARY rule ===
	if hasTernary {
		ternarySpec := &jsonic.RuleSpec{Name: "ternary"}

		ternarySpec.AddOpen([]*jsonic.AltSpec{
			{
				S: mkS(TERN0),
				P: "val",
				N: map[string]int{"expr_ternary": 1, "dlist": 1, "dmap": 1, "expr": 0, "expr_prefix": 0, "expr_suffix": 0},
				A: func(r *jsonic.Rule, ctx *jsonic.Context) {
					op := ternaryByTin[r.O0.Tin]
					prev := r.Prev
					prevNode := prev.Node
					// If prev.Node is already a *ListRef expression box, REUSE
					// the same box pointer and rebind its Val to the new
					// ternary expression. Any other rule that captured the
					// pointer (e.g. an outer ternary's r.Child.Node, or a
					// prefix expr's slot) sees the new ternary on next read.
					// Without this indirection, Go's slice reassignment leaves
					// the outer rules pointing at stale pre-rewrap slices.
					if box, ok := prevNode.(*jsonic.ListRef); ok && len(box.Val) > 0 {
						if _, isOpV := box.Val[0].(*Op); isOpV {
							priorCopy := dupExpr(box)
							n := op.Terms + 1
							newVal := make([]interface{}, n)
							newVal[0] = op
							newVal[1] = priorCopy
							for i := 2; i < n; i++ {
								newVal[i] = _unfilled
							}
							box.Val = newVal
							r.Node = box
							return
						}
					}
					// Scalar (or unwrapped) cond — first ternary level.
					r.Node = makeExpr(op, prevNode)
					prev.Node = r.Node
				},
				G: "expr,ternary,open",
			},
		}...)

		ternarySpec.AddBC(func(r *jsonic.Rule, ctx *jsonic.Context) {
			if r.Child == nil || r.Child == jsonic.NoRule {
				return
			}
			childNode := r.Child.Node
			if jsonic.IsUndefined(childNode) {
				childNode = nil
			}
			if box, ok := r.Node.(*jsonic.ListRef); ok {
				step, _ := r.U["ternary_step"].(int)
				if step == 0 {
					fillNextSlot(box, childNode)
					r.U["ternary_step"] = 1
				} else if step == 1 {
					fillNextSlot(box, childNode)
					r.U["ternary_step"] = 2
				} else if step == 2 {
					// Final slot filled when ternary ends
					// (e.g., inside an existing elem/list).
					fillNextSlot(box, childNode)
				}
			}
		})

		// Condition for implicit list after ternary completes.
		// Only fire when ternary is the FIRST expression — i.e., not already
		// inside an elem/list that handles implicit list continuation.
		implicitTernaryCond := func(r *jsonic.Rule) bool {
			step, _ := r.U["ternary_step"].(int)
			if step != 2 || r.N["pk"] >= 1 {
				return false
			}
			if r.D == 0 {
				// Top-level: check no elem/list parent exists.
				for p := r.Parent; p != nil && p != jsonic.NoRule; p = p.Parent {
					if p.Name == "elem" || p.Name == "list" {
						return false
					}
				}
				return true
			}
			if r.N["expr_paren"] >= 1 {
				// Inside paren: check no elem/list between ternary and paren.
				for p := r.Parent; p != nil && p != jsonic.NoRule; p = p.Parent {
					if p.Name == "elem" || p.Name == "list" {
						return false
					}
					if p.Name == "paren" {
						return true
					}
				}
				return true
			}
			return false
		}

		// Action to wrap ternary result as first element of implicit list.
		implicitTernaryAction := func(r *jsonic.Rule, ctx *jsonic.Context) {
			// Fill the last slot with child node.
			if r.Child != nil && r.Child != jsonic.NoRule {
				childNode := r.Child.Node
				if jsonic.IsUndefined(childNode) {
					childNode = nil
				}
				if box, ok := r.Node.(*jsonic.ListRef); ok {
					fillNextSlot(box, childNode)
				}
			}
			// Wrap the completed ternary node as the first element of a list.
			ternaryNode := r.Node
			if isOp(ternaryNode) {
				ternaryNode = cleanExpr(ternaryNode)
			}
			listNode := []interface{}{ternaryNode}

			// If inside a paren, store the list on paren.Node directly
			// (same approach as implicitListAction for expr).
			if r.N["expr_paren"] >= 1 {
				for rI := ctx.RSI - 1; rI >= 0; rI-- {
					if ctx.RS[rI].Name == "paren" {
						ctx.RS[rI].Node = listNode
						break
					}
				}
			}
			r.Node = listNode
		}

		ternarySpec.AddClose([]*jsonic.AltSpec{
			// Second separator (e.g. ':').
			{
				S: mkS(TERN1),
				P: "val",
				N: map[string]int{"expr": 0, "expr_prefix": 0, "expr_suffix": 0},
				C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
					step, _ := r.U["ternary_step"].(int)
					return step == 1
				},
				G: "expr,ternary,sep2",
			},

			// Implicit list after ternary (comma): 1?2:3,b → [[?,1,2,3],"b"]
			{
				S: mkS([]int{jsonic.TinCA}),
				C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
					return implicitTernaryCond(r)
				},
				R: "elem",
				A: implicitTernaryAction,
				G: "expr,ternary,list,imp,comma",
			},

			// Paren close after ternary: backtrack so paren can consume it.
			{
				S: mkS(CP),
				C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
					step, _ := r.U["ternary_step"].(int)
					return step == 2 && r.N["expr_paren"] >= 1
				},
				B: 1,
				G: "expr,ternary,paren,close",
			},

			// Implicit list after ternary (space): 1?2:3 b → [[?,1,2,3],"b"]
			{
				C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
					return implicitTernaryCond(r) && ctx.T0.Tin != jsonic.TinZZ
				},
				R: "elem",
				A: implicitTernaryAction,
				G: "expr,ternary,list,imp,space",
			},

			// End of ternary (deeper depth, or no more tokens).
			{
				C: func(r *jsonic.Rule, ctx *jsonic.Context) bool {
					step, _ := r.U["ternary_step"].(int)
					return step == 2
				},
				G: "expr,ternary,end",
			},
		}...)

		ternarySpec.AddAC(func(r *jsonic.Rule, ctx *jsonic.Context) {
			if eopts.Evaluate != nil {
				if isOp(r.Node) {
					r.Node = evaluation(r, ctx, r.Node, eopts.Evaluate)
				}
			}
		})

		tagAllAlts(ternarySpec)
		j.RSM()["ternary"] = ternarySpec
	}

	return nil
}

// prior converts a prior rule's node into the start of a new expression.
// All expression nodes are returned as *jsonic.ListRef so subsequent rule
// actions can re-point ListRef.Val and have every reference (including the
// outer rule's r.Child.Node) observe the update.
func prior(rule *jsonic.Rule, priorRule *jsonic.Rule, op *Op) *jsonic.ListRef {
	priorNode := priorRule.Node
	if isOp(priorNode) {
		priorNode = dupExpr(priorNode)
	}

	var expr *jsonic.ListRef
	if op.Prefix {
		expr = makeExpr(op)
	} else {
		expr = makeExpr(op, priorNode)
	}
	priorRule.Node = expr
	rule.Parent = priorRule
	return expr
}

// prattify integrates a new operator into the expression tree according to
// operator precedence (Pratt algorithm). Operates on the *jsonic.ListRef
// wrapper so any rebinding of expr.Val is visible to all holders of the
// pointer. Returns the outermost expression *ListRef.
func prattify(exprNode interface{}, op *Op) *jsonic.ListRef {
	box := asListRef(exprNode)
	if box == nil || len(box.Val) == 0 {
		return makeExpr(op, exprNode)
	}

	exprOp, isOpV := box.Val[0].(*Op)
	if !isOpV {
		return makeExpr(op, exprNode)
	}

	// Paren expressions are complete units — never drill into them.
	if exprOp.Paren {
		return makeExpr(op, dupExpr(box))
	}

	if op.Infix {
		// op is lower or equal precedence: wrap entire expression in place.
		if exprOp.Suffix || op.Left <= exprOp.Right {
			wrapExpr(box, op)
			return box
		}

		// op is higher: drill into last term. Create inner expression.
		end := exprOp.Terms
		if end < len(box.Val) {
			if isOp(box.Val[end]) {
				subBox := asListRef(box.Val[end])
				subOp := subBox.Val[0].(*Op)
				if subOp.Right < op.Left {
					box.Val[end] = prattify(subBox, op)
					return box
				}
			}
			box.Val[end] = makeExpr(op, box.Val[end])
			return box
		}
		return box
	}

	if op.Prefix {
		// Chained prefixes nest: a new prefix must go into the DEEPEST
		// unfilled slot of the existing prefix chain, not overwrite the
		// last term. Overwriting collapsed `---1` to `[-,[-,1]]` (the
		// middle prefix was lost). fillNextSlot walks the chain
		// depth-first and drops the new (empty) prefix expr into the
		// innermost open slot, giving [-,[-,[-,_]]].
		newExpr := makeExpr(op)
		if fillNextSlot(box, newExpr) {
			return box
		}
		end := exprOp.Terms
		if end < len(box.Val) {
			box.Val[end] = newExpr
			return box
		}
		return box
	}

	if op.Suffix {
		return prattifySuffix(box, op)
	}

	return box
}

// wrapExpr rewraps an existing expression with a new operator IN PLACE on
// the ListRef. The same *ListRef pointer continues to be used; only its
// Val slice is replaced. Other rules holding the pointer see the new Val
// on next read.
func wrapExpr(box *jsonic.ListRef, op *Op) {
	oldCopy := dupExpr(box)
	needed := op.Terms + 1
	newVal := make([]interface{}, needed)
	newVal[0] = op
	newVal[1] = oldCopy
	for i := 2; i < needed; i++ {
		newVal[i] = _unfilled
	}
	box.Val = newVal
}

// prattifySuffix integrates a suffix operator into the expression tree.
func prattifySuffix(node interface{}, op *Op) *jsonic.ListRef {
	box := asListRef(node)
	if box == nil || len(box.Val) == 0 {
		return makeExpr(op, node)
	}

	exprOp, isOpV := box.Val[0].(*Op)
	if !isOpV {
		return makeExpr(op, node)
	}

	if !exprOp.Suffix && exprOp.Right <= op.Left {
		end := exprOp.Terms
		if end < len(box.Val) {
			lastTerm := box.Val[end]
			// Drill into prefix.
			if subBox := asListRef(lastTerm); subBox != nil && len(subBox.Val) > 0 {
				if subOp, isSub := subBox.Val[0].(*Op); isSub && subOp.Prefix && subOp.Right < op.Left {
					prattifySuffix(subBox, op)
					return box
				}
			}
			box.Val[end] = makeExpr(op, lastTerm)
			return box
		}
	}

	// Wrap entire expression.
	wrapExpr(box, op)
	return box
}

// cleanExpr removes _unfilled sentinels from an expression tree, returning
// a plain []interface{}. Used at the boundary where a partially-built
// expression escapes into an enclosing list/elem (and so loses its slot
// allocation contract).
func cleanExpr(node interface{}) []interface{} {
	sl, ok := unwrapExpr(node)
	if !ok {
		return nil
	}
	out := make([]interface{}, 0, len(sl))
	for _, el := range sl {
		if isUnfilled(el) {
			continue
		}
		if isOp(el) {
			out = append(out, cleanExpr(el))
		} else {
			out = append(out, el)
		}
	}
	return out
}

// dupExpr produces a shallow copy of an expression as a fresh *jsonic.ListRef.
// Children that are themselves *ListRef remain shared (they each have their
// own pointer-mutation contract); top-level slots are copied so the new
// ListRef is independent of the original.
func dupExpr(node interface{}) *jsonic.ListRef {
	sl, ok := unwrapExpr(node)
	if !ok {
		return nil
	}
	out := make([]interface{}, len(sl))
	copy(out, sl)
	return &jsonic.ListRef{Val: out, Meta: map[string]any{"expr": true}}
}

// Parse is a convenience function.
func Parse(src string, opts ...map[string]interface{}) (interface{}, error) {
	j := MakeJsonic(opts...)
	return j.Parse(src)
}

// MakeJsonic creates a jsonic instance configured with the Expr plugin.
func MakeJsonic(opts ...map[string]interface{}) *jsonic.Jsonic {
	j := jsonic.Make()
	var pluginOpts map[string]interface{}
	if len(opts) > 0 {
		pluginOpts = opts[0]
	}
	_ = j.Use(Expr, pluginOpts)
	return j
}

func resolveOptions(opts map[string]interface{}) *ExprOptions {
	eopts := &ExprOptions{Op: make(map[string]*OpDef)}
	if opts == nil {
		addDefaultOps(eopts)
		return eopts
	}
	if opRaw, ok := opts["op"]; ok {
		if opMap, ok := opRaw.(map[string]interface{}); ok {
			for name, defRaw := range opMap {
				if defRaw == nil {
					eopts.Op[name] = nil
					continue
				}
				if defMap, ok := defRaw.(map[string]interface{}); ok {
					od := &OpDef{}
					if v, ok := defMap["src"]; ok {
						od.Src = v
					}
					if v, ok := defMap["osrc"].(string); ok {
						od.OSrc = v
					}
					if v, ok := defMap["csrc"].(string); ok {
						od.CSrc = v
					}
					if v, ok := defMap["left"].(float64); ok {
						od.Left = int(v)
					} else if v, ok := defMap["left"].(int); ok {
						od.Left = v
					}
					if v, ok := defMap["right"].(float64); ok {
						od.Right = int(v)
					} else if v, ok := defMap["right"].(int); ok {
						od.Right = v
					}
					if v, ok := defMap["prefix"].(bool); ok {
						od.Prefix = v
					}
					if v, ok := defMap["suffix"].(bool); ok {
						od.Suffix = v
					}
					if v, ok := defMap["infix"].(bool); ok {
						od.Infix = v
					}
					if v, ok := defMap["ternary"].(bool); ok {
						od.Ternary = v
					}
					if v, ok := defMap["paren"].(bool); ok {
						od.Paren = v
					}
					if v, ok := defMap["preval"]; ok {
						od.Preval = v
					}
					if v, ok := defMap["use"]; ok {
						od.Use = v
					}
					eopts.Op[name] = od
				}
			}
		}
	}
	if evalRaw, ok := opts["evaluate"]; ok {
		if evalFn, ok := evalRaw.(func(*jsonic.Rule, *jsonic.Context, *Op, []interface{}) interface{}); ok {
			eopts.Evaluate = evalFn
		}
	}
	addDefaultOps(eopts)
	return eopts
}

func addDefaultOps(eopts *ExprOptions) {
	defaults := map[string]*OpDef{
		"positive":       {Prefix: true, Right: 4000000, Src: "+"},
		"negative":       {Prefix: true, Right: 4000000, Src: "-"},
		"addition":       {Infix: true, Left: 2000000, Right: 2100000, Src: "+"},
		"subtraction":    {Infix: true, Left: 2000000, Right: 2100000, Src: "-"},
		"multiplication": {Infix: true, Left: 3000000, Right: 3100000, Src: "*"},
		"division":       {Infix: true, Left: 3000000, Right: 3100000, Src: "/"},
		"remainder":      {Infix: true, Left: 3000000, Right: 3100000, Src: "%"},
		"plain":          {Paren: true, OSrc: "(", CSrc: ")"},
	}
	for name, def := range defaults {
		if _, exists := eopts.Op[name]; exists {
			continue
		}
		// Skip default paren ops whose open/close source is already claimed
		// by a user-provided paren op. Otherwise both ops share the same
		// token tin and the non-deterministic map iteration in makeAllOps
		// would let either win in parenOpenByTin (e.g. user's "func" with
		// preval vs default "plain" without). This mirrors the TS plugin,
		// where insertion-order iteration lets the user op win last-write.
		if def.Paren {
			conflict := false
			for _, existing := range eopts.Op {
				if existing != nil && existing.Paren &&
					existing.OSrc == def.OSrc && existing.CSrc == def.CSrc {
					conflict = true
					break
				}
			}
			if conflict {
				continue
			}
		}
		eopts.Op[name] = def
	}
}

func makeAllOps(j *jsonic.Jsonic, eopts *ExprOptions) []*Op {
	// Track registered tins by source string to share between operators
	// (e.g., "+" is both prefix "positive" and infix "addition").
	// FixedTokens is a map[string]Tin, so only one tin per source string.
	srcTins := make(map[string]int) // src → tin

	getOrCreateTin := func(name, src string) int {
		if src == "" {
			return j.Token(name)
		}
		if tin, ok := srcTins[src]; ok {
			return tin
		}
		// Reuse existing fixed token tin if src matches a built-in token
		// (e.g., ":" is TinCL, "[" is TinOS). This prevents overriding
		// jsonic's built-in token types when operators share syntax.
		if existingTin, ok := jsonic.FixedTokens[src]; ok {
			srcTins[src] = int(existingTin)
			return int(existingTin)
		}
		tin := j.Token(name, src)
		srcTins[src] = tin
		return tin
	}

	var ops []*Op
	for name, def := range eopts.Op {
		if def == nil {
			continue
		}
		op := &Op{
			Name: name, Left: def.Left, Right: def.Right,
			Prefix: def.Prefix, Suffix: def.Suffix, Infix: def.Infix,
			Ternary: def.Ternary, Paren: def.Paren, Use: def.Use,
		}
		if def.Infix {
			op.Terms = 2
		} else if def.Ternary {
			op.Terms = 3
		} else {
			op.Terms = 1
		}
		if def.Paren {
			op.OSrc = def.OSrc
			op.CSrc = def.CSrc
			op.Name = name + "-paren"
			// Match TS: #E + src (e.g. "#E(" and "#E)")
			op.OTkn = "#E" + def.OSrc
			op.CTkn = "#E" + def.CSrc
			op.OTin = getOrCreateTin(op.OTkn, op.OSrc)
			op.CTin = getOrCreateTin(op.CTkn, op.CSrc)
			if def.Preval != nil {
				switch pv := def.Preval.(type) {
				case bool:
					op.Preval.Active = pv
				case map[string]interface{}:
					if v, ok := pv["active"].(bool); ok {
						op.Preval.Active = v
					} else {
						// Default: active=true when preval object is specified
						op.Preval.Active = true
					}
					if v, ok := pv["required"].(bool); ok {
						op.Preval.Required = v
					}
					if v, ok := pv["allow"].([]interface{}); ok {
						for _, a := range v {
							if s, ok := a.(string); ok {
								op.Preval.Allow = append(op.Preval.Allow, s)
							}
						}
					}
					if v, ok := pv["allow"].([]string); ok {
						op.Preval.Allow = v
					}
				case PrevalDef:
					op.Preval = pv
				}
			}
		} else if def.Ternary {
			op.Name = name + "-ternary"
			if src, ok := def.Src.([]interface{}); ok && len(src) >= 2 {
				op.Src = src[0].(string)
				op.CSrc = src[1].(string)
			}
			// Match TS: #E + src
			op.Tkn = "#E" + op.Src
			op.Tin = getOrCreateTin(op.Tkn, op.Src)
			op.CTkn = "#E" + op.CSrc
			op.CTin = getOrCreateTin(op.CTkn, op.CSrc)
		} else {
			srcStr := ""
			if s, ok := def.Src.(string); ok {
				srcStr = s
			}
			op.Src = srcStr
			kind := "infix"
			if def.Prefix {
				kind = "prefix"
			} else if def.Suffix {
				kind = "suffix"
			}
			// Match TS: only append "-kind" if name doesn't already end with it.
			suffix := "-" + kind
			if strings.HasSuffix(name, suffix) {
				op.Name = name
			} else {
				op.Name = name + suffix
			}
			// Match TS token naming: #E + src (e.g. "#E&", "#E.", "#E$")
			// TS: tin = fixed(src) || token('#E' + src)
			op.Tkn = "#E" + srcStr
			op.Tin = getOrCreateTin(op.Tkn, srcStr)
		}
		ops = append(ops, op)
	}
	return ops
}

// Evaluation recursively evaluates an expression tree.
func Evaluation(
	rule *jsonic.Rule, ctx *jsonic.Context, node interface{},
	resolve func(*jsonic.Rule, *jsonic.Context, *Op, []interface{}) interface{},
) interface{} {
	return evaluation(rule, ctx, node, resolve)
}

func evaluation(
	rule *jsonic.Rule, ctx *jsonic.Context, node interface{},
	resolve func(*jsonic.Rule, *jsonic.Context, *Op, []interface{}) interface{},
) interface{} {
	expr, isSlice := unwrapExpr(node)
	if !isSlice || len(expr) == 0 {
		return node
	}
	op, isOpV := expr[0].(*Op)
	if !isOpV {
		result := make([]interface{}, len(expr))
		for i, el := range expr {
			result[i] = evaluation(rule, ctx, el, resolve)
		}
		return result
	}
	terms := make([]interface{}, 0, len(expr)-1)
	for _, sub := range expr[1:] {
		if isUnfilled(sub) {
			continue
		}
		terms = append(terms, evaluation(rule, ctx, sub, resolve))
	}
	return resolve(rule, ctx, op, terms)
}

// Simplify converts an expression tree with *Op nodes into plain
// arrays/maps with string operator names. Handles both bare slices and
// *jsonic.ListRef wrappers (the internal representation).
func Simplify(node interface{}) interface{} {
	switch v := node.(type) {
	case *jsonic.ListRef:
		if v == nil {
			return nil
		}
		return Simplify(v.Val)
	case []interface{}:
		if len(v) == 0 {
			return v
		}
		if op, isOpV := v[0].(*Op); isOpV {
			result := make([]interface{}, 0, len(v))
			src := op.Src
			if op.Paren {
				src = op.OSrc
			}
			result = append(result, src)
			for _, el := range v[1:] {
				if isUnfilled(el) {
					continue
				}
				s := Simplify(el)
				if s != nil {
					result = append(result, s)
				}
			}
			return result
		}
		result := make([]interface{}, len(v))
		for i, el := range v {
			result[i] = Simplify(el)
		}
		return result
	case map[string]interface{}:
		result := make(map[string]interface{})
		for k, val := range v {
			result[k] = Simplify(val)
		}
		return result
	default:
		return node
	}
}
