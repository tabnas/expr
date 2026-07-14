/* Copyright (c) 2021-2025 Richard Rodger and other contributors, MIT License */

// Ports of TS test scenarios from ts/test/expr.test.ts that exercise the
// Pratt algorithm directly (via the exported Prattify/Opify, mirroring the
// TS `testing` export) and parse-level behaviours not covered by the TSV
// spec files: prattify-basic, prattify-assoc, no-comma-op-suppression,
// mini-config and ternary-evaluate-ac.

package tabnasexpr

import (
	"encoding/json"
	"math"
	"testing"

	jsonic "github.com/tabnas/jsonic/go"
)

// checkSx compares a node (simplified + JSON-normalized) against an
// expected JSON literal, mirroring the TS tests' C(S(x)) comparisons.
func checkSx(t *testing.T, label string, node interface{}, wantJSON string) {
	t.Helper()
	gotB, err := json.Marshal(simplifyAndNormalize(node))
	if err != nil {
		t.Fatalf("%s: marshal got: %v", label, err)
	}
	var want interface{}
	if err := json.Unmarshal([]byte(wantJSON), &want); err != nil {
		t.Fatalf("%s: bad want JSON %q: %v", label, wantJSON, err)
	}
	wantB, _ := json.Marshal(want)
	if string(gotB) != string(wantB) {
		t.Errorf("%s:\n  got:  %s\n  want: %s", label, gotB, wantB)
	}
}

// parseSx parses src and compares the simplified result against an
// expected JSON literal (mirrors the TS tests' mj(je) helper).
func parseSx(t *testing.T, j *jsonic.Jsonic, src string, wantJSON string) {
	t.Helper()
	got, err := j.Parse(src)
	if err != nil {
		t.Errorf("parse %q error: %v", src, err)
		return
	}
	checkSx(t, src, got, wantJSON)
}

// moT mirrors the TS test's makeOp helper: zero base, name from src,
// derived terms, then opify.
func moT(spec Op) *Op {
	o := spec
	if o.Name == "" {
		o.Name = o.Src
	}
	return Opify(&o)
}

// asNum reports v as a float64 when it is numeric.
func asNum(v interface{}) (float64, bool) {
	switch n := v.(type) {
	case float64:
		return n, true
	case int:
		return float64(n), true
	case int64:
		return float64(n), true
	}
	return 0, false
}

func numAt(a []interface{}, i int) float64 {
	if i >= len(a) {
		return 0
	}
	n, _ := asNum(a[i])
	return n
}

// TestPrattifyBasic ports the TS 'prattify-basic' test: direct unit tests
// of the Pratt algorithm via the exported Prattify/Opify. Each case checks
// the returned attachment point (T) and the mutated expression tree (E).
func TestPrattifyBasic(t *testing.T) {
	ME := makeExpr

	PLUS_LA := moT(Op{Infix: true, Src: "+", Left: 140, Right: 150})
	PLUS_RA := moT(Op{Infix: true, Src: "+", Left: 150, Right: 140})

	MUL_LA := moT(Op{Infix: true, Src: "*", Left: 160, Right: 170})
	PIPE_LA := moT(Op{Infix: true, Src: "|", Left: 18000, Right: 17000})

	AT_P := moT(Op{Prefix: true, Src: "@", Right: 1500})
	PER_P := moT(Op{Prefix: true, Src: "%", Right: 1300})

	BANG_S := moT(Op{Suffix: true, Src: "!", Left: 1600})
	QUEST_S := moT(Op{Suffix: true, Src: "?", Left: 1400})

	// 1+2+N => (1+2)+N
	E := ME(PLUS_LA, 1, 2)
	checkSx(t, "1+2+N T", Prattify(E, PLUS_LA), `["+",["+",1,2]]`)
	checkSx(t, "1+2+N E", E, `["+",["+",1,2]]`)

	// 1+2+N => 1+(2+N)
	E = ME(PLUS_RA, 1, 2)
	checkSx(t, "1+2+N ra T", Prattify(E, PLUS_RA), `["+",2]`)
	checkSx(t, "1+2+N ra E", E, `["+",1,["+",2]]`)

	// 1+2*N => 1+(2*N)
	E = ME(PLUS_LA, 1, 2)
	checkSx(t, "1+2*N T", Prattify(E, MUL_LA), `["*",2]`)
	checkSx(t, "1+2*N E", E, `["+",1,["*",2]]`)

	// 1*2+N => (1*2)+N
	E = ME(MUL_LA, 1, 2)
	checkSx(t, "1*2+N T", Prattify(E, PLUS_LA), `["+",["*",1,2]]`)
	checkSx(t, "1*2+N E", E, `["+",["*",1,2]]`)

	// @1+N => (@1)+N
	E = ME(AT_P, 1)
	checkSx(t, "@1+N T", Prattify(E, PLUS_LA), `["+",["@",1]]`)
	checkSx(t, "@1+N E", E, `["+",["@",1]]`)

	// 1!+N => (1!)+N
	E = ME(BANG_S, 1)
	checkSx(t, "1!+N T", Prattify(E, PLUS_LA), `["+",["!",1]]`)
	checkSx(t, "1!+N E", E, `["+",["!",1]]`)

	// @1|N => @(1|N)
	E = ME(AT_P, 1)
	checkSx(t, "@1|N T", Prattify(E, PIPE_LA), `["|",1]`)
	checkSx(t, "@1|N E", E, `["@",["|",1]]`)

	// 1|@N => 1|(@N)
	E = ME(PIPE_LA, 1)
	checkSx(t, "1|@N T", Prattify(E, AT_P), `["@"]`)
	checkSx(t, "1|@N E", E, `["|",1,["@"]]`)

	// 1!|N => (1!)|N
	E = ME(BANG_S, 1)
	checkSx(t, "1!|N T", Prattify(E, PIPE_LA), `["|",["!",1]]`)
	checkSx(t, "1!|N E", E, `["|",["!",1]]`)

	// 1+@N => 1+(@N)
	E = ME(PLUS_LA, 1)
	checkSx(t, "1+@N T", Prattify(E, AT_P), `["@"]`)
	checkSx(t, "1+@N E", E, `["+",1,["@"]]`)

	// @@N => @(@N)
	E = ME(AT_P)
	checkSx(t, "@@N T", Prattify(E, AT_P), `["@"]`)
	checkSx(t, "@@N E", E, `["@",["@"]]`)

	// %@N => %(@N)
	E = ME(PER_P)
	checkSx(t, "%@N T", Prattify(E, AT_P), `["@"]`)
	checkSx(t, "%@N E", E, `["%",["@"]]`)

	// @%N => @(%N)
	E = ME(AT_P)
	checkSx(t, "@%N T", Prattify(E, PER_P), `["%"]`)
	checkSx(t, "@%N E", E, `["@",["%"]]`)

	// 1+2! => 1+(2!)
	E = ME(PLUS_LA, 1, 2)
	checkSx(t, "1+2! T", Prattify(E, BANG_S), `["+",1,["!",2]]`)
	checkSx(t, "1+2! E", E, `["+",1,["!",2]]`)

	// 1|2! => (1|2)!
	E = ME(PIPE_LA, 1, 2)
	checkSx(t, "1|2! T", Prattify(E, BANG_S), `["!",["|",1,2]]`)
	checkSx(t, "1|2! E", E, `["!",["|",1,2]]`)

	// 1!! => (1!)!
	E = ME(BANG_S, 1)
	checkSx(t, "1!! T", Prattify(E, BANG_S), `["!",["!",1]]`)
	checkSx(t, "1!! E", E, `["!",["!",1]]`)

	// 1!? => (1!)?
	E = ME(BANG_S, 1)
	checkSx(t, "1!? T", Prattify(E, QUEST_S), `["?",["!",1]]`)
	checkSx(t, "1!? E", E, `["?",["!",1]]`)

	// 1?! => (1?)!
	E = ME(QUEST_S, 1)
	checkSx(t, "1?! T", Prattify(E, BANG_S), `["!",["?",1]]`)
	checkSx(t, "1?! E", E, `["!",["?",1]]`)

	// @1! => @(1!)
	E = ME(AT_P, 1)
	checkSx(t, "@1! T", Prattify(E, BANG_S), `["@",["!",1]]`)
	checkSx(t, "@1! E", E, `["@",["!",1]]`)

	// @1? => (@1)?
	E = ME(AT_P, 1)
	checkSx(t, "@1? T", Prattify(E, QUEST_S), `["?",["@",1]]`)
	checkSx(t, "@1? E", E, `["?",["@",1]]`)

	// @@1! => @(@(1!))
	E = ME(AT_P, ME(AT_P, 1))
	checkSx(t, "@@1! T", Prattify(E, BANG_S), `["@",["@",["!",1]]]`)
	checkSx(t, "@@1! E", E, `["@",["@",["!",1]]]`)

	// @@1? => (@(@1))?
	E = ME(AT_P, ME(AT_P, 1))
	checkSx(t, "@@1? T", Prattify(E, QUEST_S), `["?",["@",["@",1]]]`)
	checkSx(t, "@@1? E", E, `["?",["@",["@",1]]]`)
}

// TestPrattifyAssoc ports the TS 'prattify-assoc' test: associativity of
// left- and right-associative infix chains via the exported Prattify.
func TestPrattifyAssoc(t *testing.T) {
	ME := makeExpr

	AT_LA := moT(Op{Infix: true, Src: "@", Left: 14, Right: 15})
	PER_RA := moT(Op{Infix: true, Src: "%", Left: 17, Right: 16})

	// 1@2@N
	E := ME(AT_LA, 1, 2)
	checkSx(t, "1@2@N T", Prattify(E, AT_LA), `["@",["@",1,2]]`)
	checkSx(t, "1@2@N E", E, `["@",["@",1,2]]`)

	// 1@2@3@N
	E = ME(AT_LA, ME(AT_LA, 1, 2), 3)
	checkSx(t, "1@2@3@N T", Prattify(E, AT_LA), `["@",["@",["@",1,2],3]]`)
	checkSx(t, "1@2@3@N E", E, `["@",["@",["@",1,2],3]]`)

	// 1@2@3@4@N
	E = ME(AT_LA, ME(AT_LA, ME(AT_LA, 1, 2), 3), 4)
	checkSx(t, "1@2@3@4@N T", Prattify(E, AT_LA), `["@",["@",["@",["@",1,2],3],4]]`)
	checkSx(t, "1@2@3@4@N E", E, `["@",["@",["@",["@",1,2],3],4]]`)

	// 1@2@3@4@5@N
	E = ME(AT_LA, ME(AT_LA, ME(AT_LA, ME(AT_LA, 1, 2), 3), 4), 5)
	checkSx(t, "1@2@3@4@5@N T", Prattify(E, AT_LA), `["@",["@",["@",["@",["@",1,2],3],4],5]]`)
	checkSx(t, "1@2@3@4@5@N E", E, `["@",["@",["@",["@",["@",1,2],3],4],5]]`)

	// 1%2%N
	E = ME(PER_RA, 1, 2)
	checkSx(t, "1%2%N T", Prattify(E, PER_RA), `["%",2]`)
	checkSx(t, "1%2%N E", E, `["%",1,["%",2]]`)

	// 1%2%3%N
	E = ME(PER_RA, 1, ME(PER_RA, 2, 3))
	checkSx(t, "1%2%3%N T", Prattify(E, PER_RA), `["%",3]`)
	checkSx(t, "1%2%3%N E", E, `["%",1,["%",2,["%",3]]]`)

	// 1%2%3%4%N
	E = ME(PER_RA, 1, ME(PER_RA, 2, ME(PER_RA, 3, 4)))
	checkSx(t, "1%2%3%4%N T", Prattify(E, PER_RA), `["%",4]`)
	checkSx(t, "1%2%3%4%N E", E, `["%",1,["%",2,["%",3,["%",4]]]]`)

	// 1%2%3%4%5%N
	E = ME(PER_RA, 1, ME(PER_RA, 2, ME(PER_RA, 3, ME(PER_RA, 4, 5))))
	checkSx(t, "1%2%3%4%5%N T", Prattify(E, PER_RA), `["%",5]`)
	checkSx(t, "1%2%3%4%5%N E", E, `["%",1,["%",2,["%",3,["%",4,["%",5]]]]]`)
}

// TestNoCommaOpSuppression ports the TS 'no-comma-op-suppression' test.
// With `,` defined as an infix operator it is absorbed as the comma op
// everywhere; a host rule setting n.no_comma_op suppresses it (the bail
// alts leave `,` for the enclosing rule to consume as a separator).
func TestNoCommaOpSuppression(t *testing.T) {
	opts := func() map[string]interface{} {
		return map[string]interface{}{
			"op": map[string]interface{}{
				"comma_op": map[string]interface{}{
					"infix": true, "src": ",", "left": 1000000, "right": 1100000,
				},
			},
		}
	}

	// Baseline: with comma-op defined, `,` is absorbed as the comma
	// operator everywhere — including inside list/paren contexts where
	// it would otherwise have been a separator.
	jBase := makeExprJsonic(opts())
	parseSx(t, jBase, "1,2", `[",",1,2]`)
	parseSx(t, jBase, "[1,2]", `[[",",1,2]]`)
	parseSx(t, jBase, "(1,2)", `["(",[",",1,2]]`)

	// Suppression plugin: hook the built-in list rule's bo() so any
	// expressions parsed *inside* `[...]` see n.no_comma_op set. This
	// mirrors how a host grammar (e.g. C around `static_assert(cond,
	// msg)`) uses n.no_comma_op around boundary expressions to keep `,`
	// out of the comma-op infix alt's reach.
	je := makeExprJsonic(opts())
	je.Rule("list", func(rs *jsonic.RuleSpec, _ *jsonic.Parser) {
		rs.AddBO(func(r *jsonic.Rule, ctx *jsonic.Context) {
			r.N["no_comma_op"] = r.N["no_comma_op"] + 1
		})
	})

	// Inside `[...]`, `,` is a separator again, not the comma op.
	parseSx(t, je, "[1,2]", `[1,2]`)
	parseSx(t, je, "[1,2,3]", `[1,2,3]`)
	// Other operators inside the list still work.
	parseSx(t, je, "[1+2,3+4]", `[["+",1,2],["+",3,4]]`)
	// Outside the list, comma-op still applies — n.no_comma_op is scoped
	// to the list rule and its child val/expr rules.
	parseSx(t, je, "1,2", `[",",1,2]`)
}

// TestTernaryEvaluateAC ports the TS 'ternary-evaluate-ac' test. The
// ternary rule's after-close fires evaluation when options.evaluate is
// set, even for ternaries that aren't wrapped in an expr. Without it, the
// result would leak as a raw [op, ...] op-array.
func TestTernaryEvaluateAC(t *testing.T) {
	evalTernary := func(r *jsonic.Rule, ctx *jsonic.Context, op *Op, terms []interface{}) interface{} {
		if op.Name == "q-ternary" || op.Name == "q" {
			if toFloat(terms[0]) != 0 {
				return terms[1]
			}
			return terms[2]
		}
		return math.NaN()
	}

	je := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"q": map[string]interface{}{
				"ternary": true, "src": []interface{}{"?", ":"},
			},
		},
		"evaluate": evalTernary,
	})

	cases := []struct {
		src  string
		want float64
	}{
		// Direct ternary at top level evaluates through the after-close.
		{"1?2:3", 2},
		{"0?2:3", 3},
		// Right-associative chains evaluate fully.
		{"1?2: 0?4:5", 2},
		{"0?2: 1?4:5", 4},
		{"0?2: 0?4:5", 5},
	}

	for _, c := range cases {
		result, err := je.Parse(c.src)
		if err != nil {
			t.Errorf("parse %q error: %v", c.src, err)
			continue
		}
		if n, ok := asNum(result); !ok || n != c.want {
			t.Errorf("parse %q = %#v, want %v", c.src, result, c.want)
		}
	}
}

// TestMiniConfig ports the TS 'mini-config' test: a small evaluated
// expression language exercising preval func-parens (custom `<...>`
// delimiters and overloaded `(...)`), plain parens, implicit lists and
// map/list embedding, all through options.evaluate.
func TestMiniConfig(t *testing.T) {
	funcMap := map[string]func(args ...interface{}) interface{}{
		"floor": func(args ...interface{}) interface{} {
			if len(args) == 0 {
				return nil // TS: Math.floor(undefined) -> isNaN -> undefined
			}
			v, ok := asNum(args[0])
			if !ok {
				return nil // TS: isNaN(v) ? undefined
			}
			return math.Floor(v)
		},
	}

	MF := map[string]func(a ...interface{}) interface{}{
		"addition-infix": func(a ...interface{}) interface{} {
			return numAt(a, 0) + numAt(a, 1)
		},
		"subtraction-infix": func(a ...interface{}) interface{} {
			return numAt(a, 0) - numAt(a, 1)
		},
		"plain-paren": func(a ...interface{}) interface{} {
			if len(a) > 0 {
				return a[0]
			}
			return nil
		},
		"func-paren": func(a ...interface{}) interface{} {
			var out interface{}
			if len(a) > 1 {
				out = a[1]
			}
			fname, _ := a[0].(string)
			if fname != "" {
				fn := funcMap[fname]
				if fn == nil {
					out = nil
				} else {
					out = fn(a[1:]...)
				}
			}
			// TS: out = null == out ? null : out (Go nil is already null)
			return out
		},
	}

	// hasPreval mirrors the TS evaluate's `!r.u.paren_preval` check.
	hasPreval := func(r *jsonic.Rule) bool {
		return r != nil && r.U != nil && r.U["paren_preval"] == true
	}

	j0 := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"func": map[string]interface{}{
				"paren": true, "preval": true, "osrc": "<", "csrc": ">",
			},
		},
		"evaluate": func(r *jsonic.Rule, ctx *jsonic.Context, op *Op, terms []interface{}) interface{} {
			mf := MF[op.Name]
			if op.Name == "func-paren" && !hasPreval(r) {
				terms = append([]interface{}{""}, terms...)
			}
			var out interface{}
			if mf != nil {
				out = mf(terms...)
			}
			return out
		},
	})

	parseSx(t, j0, "11+22", `33`)
	parseSx(t, j0, "44-33", `11`)
	parseSx(t, j0, "(44-33)+11", `22`)
	parseSx(t, j0, "44-(33+11)", `0`)
	parseSx(t, j0, "44-33+11", `22`)

	parseSx(t, j0, "(1.1)", `1.1`)
	parseSx(t, j0, "[0,(1)]", `[0,1]`)
	parseSx(t, j0, "[0 (1)]", `[0,1]`)

	parseSx(t, j0, "floor<1.5>", `1`)
	parseSx(t, j0, "a:floor<2.5>", `{"a":2}`)
	parseSx(t, j0, "{b:floor<3.5>}", `{"b":3}`)
	parseSx(t, j0, "[floor<4.5>]", `[4]`)
	parseSx(t, j0, "[0 floor<5.5>]", `[0,5]`)

	parseSx(t, j0, "1+floor<1.5>", `2`)
	parseSx(t, j0, "1+floor<1.5>+3", `5`)
	parseSx(t, j0, "floor<1.5>+4", `5`)
	parseSx(t, j0, "a:floor<1.5>+4", `{"a":5}`)

	parseSx(t, j0, "a:(1+2) b:floor<1.9>", `{"a":3,"b":1}`)

	parseSx(t, j0, "()", `null`)
	parseSx(t, j0, "<>", `null`)
	parseSx(t, j0, "<1>", `1`)
	parseSx(t, j0, "c:<2>", `{"c":2}`)

	parseSx(t, j0, "a:floor<>", `{"a":null}`)
	parseSx(t, j0, "floor<>", `null`)
	parseSx(t, j0, "[floor<>]", `[null]`)
	parseSx(t, j0, `floor<"a">`, `null`)
	parseSx(t, j0, `a:floor<"a">`, `{"a":null}`)

	parseSx(t, j0, "[1 (2) (2+1) floor<4.5>]", `[1,2,3,4]`)
	parseSx(t, j0, "1 (2) (2+1) floor<4.5>", `[1,2,3,4]`)

	parseSx(t, j0, "bad<9>", `null`)

	j1 := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"plain": nil,
			"func": map[string]interface{}{
				"paren": true,
				"preval": map[string]interface{}{
					"active": true,
					"allow":  []interface{}{"floor"},
				},
				"osrc": "(", "csrc": ")",
			},
		},
		"evaluate": func(r *jsonic.Rule, ctx *jsonic.Context, op *Op, terms []interface{}) interface{} {
			mf := MF[op.Name]
			if op.Name == "func-paren" && !hasPreval(r) {
				terms = append([]interface{}{""}, terms...)
			}
			var out interface{}
			if mf != nil {
				out = mf(terms...)
			} else {
				out = math.NaN()
			}
			return out
		},
	})

	parseSx(t, j1, "()", `null`)
	parseSx(t, j1, "(0)", `0`)
	parseSx(t, j1, "(0+1)", `1`)
	parseSx(t, j1, "[(0) 1]", `[0,1]`)

	parseSx(t, j1, "[0,(1),2]", `[0,1,2]`)
	parseSx(t, j1, "[0,(1)]", `[0,1]`)

	parseSx(t, j1, "[(1)]", `[1]`)
	parseSx(t, j1, "[(0),(1)]", `[0,1]`)
	parseSx(t, j1, "(0),(1)", `[0,1]`)

	parseSx(t, j1, "floor(1.1)", `1`)
	parseSx(t, j1, "floor (1.1)", `1`)

	parseSx(t, j1, "floor(0.5)", `0`)
	parseSx(t, j1, "a:floor(2.5)", `{"a":2}`)

	parseSx(t, j1, "{b:floor(3.5)}", `{"b":3}`)
	parseSx(t, j1, "[floor(4.5)]", `[4]`)
	parseSx(t, j1, "[0 floor(5.5)]", `[0,5]`)
	parseSx(t, j1, "[(0) 1 floor(5.5)]", `[0,1,5]`)
	parseSx(t, j1, "[(0) floor(5.5)]", `[0,5]`)
	parseSx(t, j1, "[0,(1),floor(5.5)]", `[0,1,5]`)

	parseSx(t, j1, "[1,(2),(2+1)]", `[1,2,3]`)
	parseSx(t, j1, "[1,(2),(2+1),floor(4.5)]", `[1,2,3,4]`)

	parseSx(t, j1, "a:floor(1.5)", `{"a":1}`)

	parseSx(t, j1, "[3+2]", `[5]`)
	parseSx(t, j1, "[3+(2)]", `[5]`)
	parseSx(t, j1, "[(3)+2]", `[5]`)
	parseSx(t, j1, "[(3)+(2)]", `[5]`)
	parseSx(t, j1, "[(3+2)]", `[5]`)
	parseSx(t, j1, "[(3+(2))]", `[5]`)
	parseSx(t, j1, "[((3)+2)]", `[5]`)
	parseSx(t, j1, "[((3)+(2))]", `[5]`)

	parseSx(t, j1, "[1,3+2]", `[1,5]`)
	parseSx(t, j1, "[1,3+(2)]", `[1,5]`)
	parseSx(t, j1, "[1,(3)+2]", `[1,5]`)
	parseSx(t, j1, "[1,(3)+(2)]", `[1,5]`)
	parseSx(t, j1, "[1,(3+2)]", `[1,5]`)
	parseSx(t, j1, "[1,(3+(2))]", `[1,5]`)
	parseSx(t, j1, "[1,((3)+2)]", `[1,5]`)
	parseSx(t, j1, "[1,((3)+(2))]", `[1,5]`)

	parseSx(t, j1, "[3+2,4]", `[5,4]`)
	parseSx(t, j1, "[3+(2),4]", `[5,4]`)
	parseSx(t, j1, "[(3)+2,4]", `[5,4]`)
	parseSx(t, j1, "[(3)+(2),4]", `[5,4]`)
	parseSx(t, j1, "[(3+2),4]", `[5,4]`)
	parseSx(t, j1, "[(3+(2)),4]", `[5,4]`)
	parseSx(t, j1, "[((3)+2),4]", `[5,4]`)
	parseSx(t, j1, "[((3)+(2)),4]", `[5,4]`)

	parseSx(t, j1, "[1,3+2,4]", `[1,5,4]`)
	parseSx(t, j1, "[1,3+(2),4]", `[1,5,4]`)
	parseSx(t, j1, "[1,(3)+2,4]", `[1,5,4]`)
	parseSx(t, j1, "[1,(3)+(2),4]", `[1,5,4]`)
	parseSx(t, j1, "[1,(3+2),4]", `[1,5,4]`)
	parseSx(t, j1, "[1,(3+(2)),4]", `[1,5,4]`)
	parseSx(t, j1, "[1,((3)+2),4]", `[1,5,4]`)
	parseSx(t, j1, "[1,((3)+(2)),4]", `[1,5,4]`)

	parseSx(t, j1, "1+floor(1.1)", `2`)
	parseSx(t, j1, "floor(1.1)+1", `2`)
	parseSx(t, j1, "1+floor(1.1)+1", `3`)

	parseSx(t, j1, "a:(2)+1", `{"a":3}`)

	parseSx(t, j1, "a:1+floor(1.1)", `{"a":2}`)
	parseSx(t, j1, "a:(1.1)+1", `{"a":2.1}`)
	parseSx(t, j1, "a:floor(1.1)+1", `{"a":2}`)
	parseSx(t, j1, "a:1+floor(1.1)+1", `{"a":3}`)

	parseSx(t, j1, "[1+floor(1.1)]", `[2]`)
	parseSx(t, j1, "[floor(1.1)+2]", `[3]`)
	parseSx(t, j1, "[3+floor(1.1)+2]", `[6]`)

	parseSx(t, j1, "b:1.1+1,c:C0", `{"b":2.1,"c":"C0"}`)
	parseSx(t, j1, "b:(1.1+1),c:C0a", `{"b":2.1,"c":"C0a"}`)

	parseSx(t, j1, "b:(1.1)+1,c:C1", `{"b":2.1,"c":"C1"}`)
	parseSx(t, j1, "b:((1.1)+1),c:C1a", `{"b":2.1,"c":"C1a"}`)

	parseSx(t, j1, "b:1+floor(1.1),c:C2c", `{"b":2,"c":"C2c"}`)
	parseSx(t, j1, "b:floor(1.1)+1,c:C2d", `{"b":2,"c":"C2d"}`)

	parseSx(t, j1, "b:(floor(1.1)),c:C2a", `{"b":1,"c":"C2a"}`)
	parseSx(t, j1, "b:(1+floor(1.1)),c:C2b", `{"b":2,"c":"C2b"}`)

	parseSx(t, j1, "1+(floor(1.1))", `2`)

	parseSx(t, j1, "(11,22)", `[11,22]`)
	parseSx(t, j1, "21+31", `52`)
	parseSx(t, j1, "(21)+31", `52`)
	parseSx(t, j1, "(21+31)", `52`)
	parseSx(t, j1, "(floor(2.2))", `2`)
	parseSx(t, j1, "((floor(2.2)))", `2`)
	parseSx(t, j1, "(floor(2.2))+1", `3`)
	parseSx(t, j1, "floor(2.2)+3", `5`)
	parseSx(t, j1, "(floor(1.1)+2)", `3`)
	parseSx(t, j1, "b:(floor(1.1)+2),c:C2c", `{"b":3,"c":"C2c"}`)
}
