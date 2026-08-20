/* Copyright (c) 2021-2025 Richard Rodger and other contributors, MIT License */

package tabnasexpr

import (
	"encoding/json"
	"fmt"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	jsonic "github.com/tabnas/jsonic/go"
	support "github.com/tabnas/support/go"
)

// The fixtures live at the repo root in `test/spec/*.tsv` and are read by
// github.com/tabnas/support/go, whose TypeScript half ts/test/spec.test.ts
// uses to run the SAME files — so the two implementations cannot drift
// without one going red, and neither can the two loaders.
//
// What varies per case is the CONFIGURED PARSER, which cannot live in an
// opts column: several fixtures need operators defined in the plugin's
// options. So each test builds its own and hands it here, and each ROW
// becomes its own subtest.

func runSpec(t *testing.T, specName string, j *jsonic.Jsonic) {
	t.Helper()

	dir, err := support.FindSpecDir("")
	if err != nil {
		t.Fatal(err)
	}

	support.Runner{
		Parse:     j.Parse,
		Normalize: simplifyAndNormalize,
	}.File(t, filepath.Join(dir, specName))
}

func simplifyAndNormalize(node interface{}) interface{} {
	simplified := Simplify(node)
	// Round-trip through JSON to normalize types (float64 for numbers, etc.)
	b, err := json.Marshal(simplified)
	if err != nil {
		return simplified
	}
	var normalized interface{}
	if err := json.Unmarshal(b, &normalized); err != nil {
		return simplified
	}
	return normalized
}

func makeExprJsonic(opOpts ...map[string]interface{}) *jsonic.Jsonic {
	j := jsonic.Make()
	var opts map[string]interface{}
	if len(opOpts) > 0 {
		opts = opOpts[0]
	}
	_ = j.Use(Expr, opts)
	return j
}

func TestSpecHappy(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "happy.tsv", j)
}

func TestSpecBinary(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "binary.tsv", j)
}

func TestSpecArithmeticMixed(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "arithmetic-mixed.tsv", j)
}

func TestSpecPrefixInfixMixed(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "prefix-infix-mixed.tsv", j)
}

func TestSpecParenDeepNest(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "paren-deep-nest.tsv", j)
}

func TestSpecStructureArith(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "structure-arith.tsv", j)
}

func TestSpecStructure(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "structure.tsv", j)
}

func TestSpecUnaryPrefixBasic(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "unary-prefix-basic.tsv", j)
}

func TestSpecUnaryPrefixEdge(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"at": map[string]interface{}{
				"prefix": true, "right": 5000000, "src": "@",
			},
			"tight": map[string]interface{}{
				"infix": true, "left": 7000000, "right": 7100000, "src": "~",
			},
		},
	})
	runSpec(t, "unary-prefix-edge.tsv", j)
}

func TestSpecUnarySuffixBasic(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"factorial": map[string]interface{}{
				"suffix": true, "left": 6000000, "src": "!",
			},
			"question": map[string]interface{}{
				"suffix": true, "left": 3500000, "src": "?",
			},
		},
	})
	runSpec(t, "unary-suffix-basic.tsv", j)
}

func TestSpecUnarySuffixArith(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"factorial": map[string]interface{}{
				"suffix": true, "left": 6000000, "src": "!",
			},
			"question": map[string]interface{}{
				"suffix": true, "left": 3500000, "src": "?",
			},
		},
	})
	runSpec(t, "unary-suffix-arith.tsv", j)
}

func TestSpecUnarySuffixEdge(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"factorial": map[string]interface{}{
				"suffix": true, "left": 6000000, "src": "!",
			},
			"question": map[string]interface{}{
				"suffix": true, "left": 3500000, "src": "?",
			},
			"tight": map[string]interface{}{
				"infix": true, "left": 7000000, "right": 7100000, "src": "~",
			},
		},
	})
	runSpec(t, "unary-suffix-edge.tsv", j)
}

func TestSpecUnarySuffixStructure(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"factorial": map[string]interface{}{
				"suffix": true, "left": 6000000, "src": "!",
			},
			"question": map[string]interface{}{
				"suffix": true, "left": 3500000, "src": "?",
			},
		},
	})
	runSpec(t, "unary-suffix-structure.tsv", j)
}

func TestSpecUnarySuffixPrefix(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"factorial": map[string]interface{}{
				"suffix": true, "left": 6000000, "src": "!",
			},
			"question": map[string]interface{}{
				"suffix": true, "left": 3500000, "src": "?",
			},
		},
	})
	runSpec(t, "unary-suffix-prefix.tsv", j)
}

func TestSpecUnarySuffixParen(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"factorial": map[string]interface{}{
				"suffix": true, "left": 6000000, "src": "!",
			},
			"question": map[string]interface{}{
				"suffix": true, "left": 3500000, "src": "?",
			},
		},
	})
	runSpec(t, "unary-suffix-paren.tsv", j)
}

func TestSpecParenBasic(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "paren-basic.tsv", j)
}

func TestSpecImplicitListTopBasic(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "implicit-list-top-basic.tsv", j)
}

func TestSpecTernaryBasic(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"factorial": map[string]interface{}{
				"suffix": true, "src": "!", "left": 6000000,
			},
			"ternary": map[string]interface{}{
				"ternary": true, "src": []interface{}{"?", ":"},
			},
		},
	})
	runSpec(t, "ternary-basic.tsv", j)
}

func TestSpecTernaryImplicitList(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"factorial": map[string]interface{}{
				"suffix": true, "src": "!", "left": 6000000,
			},
			"ternary": map[string]interface{}{
				"ternary": true, "src": []interface{}{"?", ":"},
			},
		},
	})
	runSpec(t, "ternary-implicit-list.tsv", j)
}

func TestSpecJSONBase(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "json-base.tsv", j)
}

func TestSpecParenImplicitMap(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "paren-implicit-map.tsv", j)
}

func TestSpecJsonicBase(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "jsonic-base.tsv", j)
}

func TestSpecImplicitListTopParen(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "implicit-list-top-paren.tsv", j)
}

func TestSpecParenImplicitList(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "paren-implicit-list.tsv", j)
}

func TestSpecMapImplicitListParen(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "map-implicit-list-paren.tsv", j)
}

func TestSpecParenListImplicitStructureComma(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "paren-list-implicit-structure-comma.tsv", j)
}

func TestSpecParenListImplicitStructureSpace(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "paren-list-implicit-structure-space.tsv", j)
}

func TestSpecParenMapImplicitStructureComma(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "paren-map-implicit-structure-comma.tsv", j)
}

func TestSpecParenMapImplicitStructureSpace(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "paren-map-implicit-structure-space.tsv", j)
}

func TestSpecAddInfix(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"foo": map[string]interface{}{
				"infix": true, "left": 3500000, "right": 3600000, "src": "foo",
			},
		},
	})
	runSpec(t, "add-infix.tsv", j)
}

// TestSimplify verifies the Simplify function.
func TestSimplify(t *testing.T) {
	op := &Op{Name: "addition-infix", Src: "+", Infix: true}
	expr := []interface{}{op, 1.0, 2.0}
	got := Simplify(expr)

	expected := []interface{}{"+", 1.0, 2.0}
	if !reflect.DeepEqual(got, expected) {
		t.Errorf("Simplify: got %v, want %v", got, expected)
	}
}

// TestEvaluation verifies basic evaluation.
func TestEvaluation(t *testing.T) {
	mathResolve := func(r *jsonic.Rule, ctx *jsonic.Context, op *Op, terms []interface{}) interface{} {
		switch op.Name {
		case "addition-infix":
			return toFloat(terms[0]) + toFloat(terms[1])
		case "subtraction-infix":
			return toFloat(terms[0]) - toFloat(terms[1])
		case "multiplication-infix":
			return toFloat(terms[0]) * toFloat(terms[1])
		case "negative-prefix":
			return -1 * toFloat(terms[0])
		case "positive-prefix":
			return toFloat(terms[0])
		case "plain-paren":
			if len(terms) > 0 {
				return terms[0]
			}
			return nil
		default:
			return nil
		}
	}

	j := jsonic.Make()
	_ = j.Use(Expr, nil)

	tests := []struct {
		input    string
		expected float64
	}{
		{"1+2", 3},
		{"1+2+3", 6},
		{"1*2+3", 5},
		{"1+2*3", 7},
		{"(1+2)*3", 9},
		{"3*(1+2)", 9},
		{"(1)", 1},
		{"(1+2)", 3},
		{"3+(1+2)", 6},
		{"(1+2)+3", 6},
		{"111+222", 333},
		{"(111+222)", 333},
		{"111+(222)", 333},
		{"(111)+222", 333},
		{"(111)+(222)", 333},
		{"(1+2)*4", 12},
		{"1+(2*4)", 9},
		{"((1+2)*4)", 12},
		{"(1+(2*4))", 9},
		{"((114))", 114},
		{"(((115)))", 115},
		{"1-3", -2},
		{"-1", -1},
		{"+1", 1},
		{"1+(-3)", -2},
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			result, err := j.Parse(tt.input)
			if err != nil {
				t.Fatalf("parse error: %v", err)
			}
			val := Evaluation(nil, nil, result, mathResolve)
			if got := toFloat(val); got != tt.expected {
				t.Errorf("got %v, want %v", got, tt.expected)
			}
		})
	}
}

func toFloat(v interface{}) float64 {
	switch n := v.(type) {
	case float64:
		return n
	case int:
		return float64(n)
	case int64:
		return float64(n)
	default:
		return 0
	}
}

// TestParseConvenience tests the Parse convenience function.
func TestParseConvenience(t *testing.T) {
	result, err := Parse("1+2")
	if err != nil {
		t.Fatalf("Parse error: %v", err)
	}
	got := simplifyAndNormalize(result)
	expected := []interface{}{"+", float64(1), float64(2)}
	expectedJSON, _ := json.Marshal(expected)
	gotJSON, _ := json.Marshal(got)
	if string(gotJSON) != string(expectedJSON) {
		t.Errorf("got %s, want %s", gotJSON, expectedJSON)
	}
	_ = fmt.Sprintf("") // use fmt
}

// TestEvaluateSets verifies set union/intersection evaluation with custom operators.
func TestEvaluateSets(t *testing.T) {
	setResolve := func(r *jsonic.Rule, ctx *jsonic.Context, op *Op, terms []interface{}) interface{} {
		switch op.Name {
		case "plain-paren":
			if len(terms) > 0 {
				return terms[0]
			}
			return nil
		case "union-infix":
			a := toIntSlice(terms[0])
			b := toIntSlice(terms[1])
			seen := make(map[int]bool)
			var result []int
			for _, v := range a {
				if !seen[v] {
					seen[v] = true
					result = append(result, v)
				}
			}
			for _, v := range b {
				if !seen[v] {
					seen[v] = true
					result = append(result, v)
				}
			}
			sortInts(result)
			return intsToInterface(result)
		case "intersection-infix":
			a := toIntSlice(terms[0])
			b := toIntSlice(terms[1])
			setA := make(map[int]bool)
			for _, v := range a {
				setA[v] = true
			}
			var result []int
			seen := make(map[int]bool)
			for _, v := range b {
				if setA[v] && !seen[v] {
					seen[v] = true
					result = append(result, v)
				}
			}
			sortInts(result)
			return intsToInterface(result)
		default:
			return []interface{}{}
		}
	}

	j := jsonic.Make()
	j.Use(Expr, map[string]interface{}{
		"op": map[string]interface{}{
			"union": map[string]interface{}{
				"infix": true, "src": "U", "left": 140, "right": 150,
			},
			"intersection": map[string]interface{}{
				"infix": true, "src": "N", "left": 140, "right": 150,
			},
		},
	})

	tests := []struct {
		input    string
		expected []int
	}{
		{"[1]U[2]", []int{1, 2}},
		{"[1,3]U[1,2]", []int{1, 2, 3}},
		{"[1,3]N[1,2]", []int{1}},
		{"[1,3]N[2]", []int{}},
		{"[1,3]N[2,1]", []int{1}},
		{"[1,3]N[2]U[1,2]", []int{1, 2}},
		{"[1,3]N([2]U[1,2])", []int{1}},
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			result, err := j.Parse(tt.input)
			if err != nil {
				t.Fatalf("parse error for %q: %v", tt.input, err)
			}
			val := Evaluation(nil, nil, result, setResolve)
			got := toIntSlice(val)
			if !reflect.DeepEqual(got, tt.expected) {
				t.Errorf("got %v, want %v", got, tt.expected)
			}
		})
	}
}

func toIntSlice(v interface{}) []int {
	switch s := v.(type) {
	case []interface{}:
		result := make([]int, 0, len(s))
		for _, el := range s {
			result = append(result, int(toFloat(el)))
		}
		return result
	case []int:
		return s
	default:
		return []int{}
	}
}

func intsToInterface(nums []int) []interface{} {
	result := make([]interface{}, len(nums))
	for i, n := range nums {
		result[i] = float64(n)
	}
	return result
}

func sortInts(a []int) {
	for i := 0; i < len(a); i++ {
		for j := i + 1; j < len(a); j++ {
			if a[j] < a[i] {
				a[i], a[j] = a[j], a[i]
			}
		}
	}
}

// TestExampleDotpath verifies custom dot-path operator with evaluation.
func TestExampleDotpath(t *testing.T) {
	// Go's makeAllOps appends "-infix"/"-prefix" to the user-provided name,
	// so "dot" becomes "dot-infix" and "dot-prefix" respectively.
	dotResolve := func(r *jsonic.Rule, ctx *jsonic.Context, op *Op, terms []interface{}) interface{} {
		switch op.Name {
		case "dot-infix":
			parts := make([]string, len(terms))
			for i, term := range terms {
				parts[i] = fmt.Sprintf("%v", term)
			}
			return strings.Join(parts, "/")
		case "dotpre-prefix":
			return "/" + fmt.Sprintf("%v", terms[0])
		case "plain-paren":
			if len(terms) > 0 {
				return terms[0]
			}
			return nil
		case "positive-prefix":
			return terms[0]
		case "addition-infix":
			return toFloat(terms[0]) + toFloat(terms[1])
		default:
			return nil
		}
	}

	j := jsonic.Make()
	j.Use(Expr, map[string]interface{}{
		"op": map[string]interface{}{
			"dot": map[string]interface{}{
				"src": ".", "infix": true, "left": 15000000, "right": 14000000,
			},
			"dotpre": map[string]interface{}{
				"src": ".", "prefix": true, "right": 14000000,
			},
		},
	})

	tests := []struct {
		input    string
		expected interface{}
	}{
		{"a.b", "a/b"},
		{"a.b.c", "a/b/c"},
		{"a.b.c.d", "a/b/c/d"},
		{".a", "/a"},
		{".a.b", "/a/b"},
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			result, err := j.Parse(tt.input)
			if err != nil {
				t.Fatalf("parse error: %v", err)
			}
			val := Evaluation(nil, nil, result, dotResolve)
			if val != tt.expected {
				t.Errorf("got %v, want %v", val, tt.expected)
			}
		})
	}
}

func TestSpecPrevalBasic(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"angle": map[string]interface{}{
				"osrc": "<", "csrc": ">", "paren": true,
				"preval": map[string]interface{}{"active": true},
			},
		},
	})
	runSpec(t, "paren-preval-basic.tsv", j)
}

func TestSpecPrevalOverload(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"factorial": map[string]interface{}{
				"suffix": true, "left": 6000000, "src": "!",
			},
			"square": map[string]interface{}{
				"osrc": "[", "csrc": "]", "paren": true,
				"preval": map[string]interface{}{"required": true},
			},
			"brace": map[string]interface{}{
				"osrc": "{", "csrc": "}", "paren": true,
				"preval": map[string]interface{}{"required": true},
			},
		},
	})
	runSpec(t, "paren-preval-overload.tsv", j)
}

func TestSpecPrevalImplicit(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"plain": map[string]interface{}{
				"paren": true, "osrc": "(", "csrc": ")",
				"preval": map[string]interface{}{"active": true},
			},
		},
	})
	runSpec(t, "paren-preval-implicit.tsv", j)
}

func TestSpecAddParen(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"angle": map[string]interface{}{
				"paren": true, "osrc": "<", "csrc": ">",
			},
		},
	})
	runSpec(t, "add-paren.tsv", j)
}

// TestEvaluateNestedInfix verifies that a left-associative chain like a.b.c
// evaluates correctly — the evaluate callback should receive the fully-built
// result of inner expressions, and only the outermost result should appear
// in the final parse output (not intermediate results).
func TestEvaluateNestedInfix(t *testing.T) {
	// Track evaluate calls
	var calls []string

	j := jsonic.Make()
	j.Use(Expr, map[string]interface{}{
		"op": map[string]interface{}{
			"dot": map[string]interface{}{
				"infix": true, "src": ".", "left": 250, "right": 240,
			},
			"plain": nil, "addition": nil, "subtraction": nil,
			"multiplication": nil, "division": nil, "remainder": nil,
		},
		"evaluate": func(r *jsonic.Rule, ctx *jsonic.Context, op *Op, terms []interface{}) interface{} {
			// Concatenate all terms with dots
			parts := make([]string, len(terms))
			for i, t := range terms {
				parts[i] = fmt.Sprintf("%v", t)
			}
			result := strings.Join(parts, ".")
			calls = append(calls, result)
			return result
		},
	})

	// a.b.c is left-associative: (a.b).c
	// evaluate should be called twice:
	//   1. dot("a", "b") → "a.b"
	//   2. dot("a.b", "c") → "a.b.c"
	// The final result should contain "a.b.c", NOT "a.b"
	result, err := j.Parse("x:a.b.c")
	if err != nil {
		t.Fatalf("parse error: %v", err)
	}

	m, ok := result.(*jsonic.OrderedMap)
	if !ok {
		t.Fatalf("result type = %T, want *jsonic.OrderedMap", result)
	}

	got, _ := m.Get("x")
	if got != "a.b.c" {
		t.Errorf("x = %v, want %q", got, "a.b.c")
		t.Logf("evaluate calls: %v", calls)
	}

	// Also test simple single infix
	calls = nil
	result, _ = j.Parse("p:a.b")
	m = result.(*jsonic.OrderedMap)
	if p, _ := m.Get("p"); p != "a.b" {
		t.Errorf("p = %v, want %q", p, "a.b")
	}
}

// TestSpecEvaluateMath tests the evaluate callback with a math expression
// grammar. This exercises the full pipeline: parse → S-expression → evaluate
// → result. It catches bugs where nested/chained expressions produce
// intermediate results instead of the final computed value.
func TestSpecEvaluateMath(t *testing.T) {
	factorial := func(n float64) float64 {
		if n <= 1 {
			return 1
		}
		r := 1.0
		for i := 2.0; i <= n; i++ {
			r *= i
		}
		return r
	}

	j := jsonic.Make()
	j.Use(Expr, map[string]interface{}{
		"op": map[string]interface{}{
			"addition":       map[string]interface{}{"infix": true, "src": "+", "left": 140, "right": 150},
			"subtraction":    map[string]interface{}{"infix": true, "src": "-", "left": 140, "right": 150},
			"multiplication": map[string]interface{}{"infix": true, "src": "*", "left": 160, "right": 170},
			"division":       map[string]interface{}{"infix": true, "src": "/", "left": 160, "right": 170},
			"negative":       map[string]interface{}{"prefix": true, "src": "-", "right": 200},
			"positive":       map[string]interface{}{"prefix": true, "src": "+", "right": 200},
			"factorial":      map[string]interface{}{"suffix": true, "src": "!", "left": 300},
			"func":           map[string]interface{}{"paren": true, "preval": map[string]interface{}{"active": true}, "osrc": "(", "csrc": ")"},
		},
		"evaluate": func(r *jsonic.Rule, ctx *jsonic.Context, op *Op, terms []interface{}) interface{} {
			a := toNum(terms, 0)
			b := toNum(terms, 1)
			switch op.Name {
			case "addition-infix":
				return a + b
			case "subtraction-infix":
				return a - b
			case "multiplication-infix":
				return a * b
			case "division-infix":
				if b == 0 {
					return 0.0
				}
				return a / b
			case "negative-prefix":
				return -a
			case "positive-prefix":
				return a
			case "factorial-suffix":
				return factorial(a)
			case "func-paren":
				fname, isStr := terms[0].(string)
				if isStr {
					// Preval function call: terms = [fname, [arg1, arg2]] or [fname, arg1]
					rawArgs := terms[1:]
					// Flatten: args may be wrapped in an array (implicit list from comma)
					var args []interface{}
					if len(rawArgs) == 1 {
						if sl, ok := rawArgs[0].([]interface{}); ok {
							args = sl
						} else {
							args = rawArgs
						}
					} else {
						args = rawArgs
					}
					// Variadic, as the TypeScript side's `Math.min(...args)`
					// is. Fixed at two arguments, a three-argument row
					// agrees with TS only when the third happens not to be
					// the extremum, so the fixture would silently test less
					// on this runtime than on the other.
					switch fname {
					case "min", "max":
						best := toNum(args, 0)
						for argI := 1; argI < len(args); argI++ {
							n := toNum(args, argI)
							if ("min" == fname) == (n < best) && n != best {
								best = n
							}
						}
						return best
					default:
						return toNum(args, 0)
					}
				}
				// Plain parens (no preval) — return inner value
				return a
			case "plain-paren":
				return a
			default:
				return a
			}
		},
	})
	runSpec(t, "evaluate-math.tsv", j)
}

func toNum(terms []interface{}, idx int) float64 {
	if idx >= len(terms) {
		return 0
	}
	switch v := terms[idx].(type) {
	case float64:
		return v
	case int:
		return float64(v)
	case int64:
		return float64(v)
	default:
		return 0
	}
}

func TestSpecInfixInParenMap(t *testing.T) {
	j := makeExprJsonic()
	runSpec(t, "infix-in-paren-map.tsv", j)
}

func TestSpecTernaryMany2(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"foo": map[string]interface{}{
				"ternary": true,
				"src":     []interface{}{"?", ":"},
			},
			"bar": map[string]interface{}{
				"ternary": true,
				"src":     []interface{}{"QQ", "CC"},
			},
		},
	})
	runSpec(t, "ternary-many-2.tsv", j)
}

func TestSpecTernaryMany3(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"foo": map[string]interface{}{
				"ternary": true,
				"src":     []interface{}{"?", ":"},
			},
			"bar": map[string]interface{}{
				"ternary": true,
				"src":     []interface{}{"QQ", "CC"},
			},
			"zed": map[string]interface{}{
				"ternary": true,
				"src":     []interface{}{"%%", "@@"},
			},
		},
	})
	runSpec(t, "ternary-many-3.tsv", j)
}

func TestSpecTernaryParenPreval(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"ternary": map[string]interface{}{
				"ternary": true,
				"src":     []interface{}{"?", ":"},
			},
			"plain": map[string]interface{}{
				"paren": true, "osrc": "(", "csrc": ")",
				"preval": map[string]interface{}{"active": true},
			},
		},
	})
	runSpec(t, "ternary-paren-preval.tsv", j)
}

func TestSpecParenPrevalChain(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"index": map[string]interface{}{
				"paren": true, "osrc": "[", "csrc": "]",
				"preval": map[string]interface{}{"required": true},
			},
			"call": map[string]interface{}{
				"paren": true, "osrc": "(", "csrc": ")",
				"preval": map[string]interface{}{"active": true},
			},
			"plain": nil,
		},
	})
	runSpec(t, "paren-preval-chain.tsv", j)
}

// --- Go-port parity regression tests ---

// TestInstanceFixedTokenBinding locks in the fix for the instance-vs-global
// tin defect. A host grammar (e.g. the @tabnas/c lexer) registers its own
// fixed token for an operator's source string on the instance BEFORE the
// expr plugin runs. The plugin must bind the operator to that instance tin
// (mirroring the TS plugin's `tabnas.fixed(src)`), not consult the global
// fixed-token table and mint a fresh "#E"+src tin that the host lexer never
// emits.
func TestInstanceFixedTokenBinding(t *testing.T) {
	j := jsonic.Make()
	hostTin := j.Token("#AT", "@") // host-registered, instance-level only

	if int(j.FixedSrc("@")) != hostTin {
		t.Fatalf("precondition: FixedSrc(@)=%d, want host tin %d", j.FixedSrc("@"), hostTin)
	}

	eopts := &ExprOptions{Op: map[string]*OpDef{
		"at": {Infix: true, Left: 2000000, Right: 2100000, Src: "@"},
	}}
	ops := makeAllOps(j, eopts)

	var atOp *Op
	for _, op := range ops {
		if op.Src == "@" {
			atOp = op
		}
	}
	if atOp == nil {
		t.Fatal("expected an operator built for src @")
	}
	if atOp.Tin != hostTin {
		t.Errorf("operator bound to tin %d, want host instance tin %d", atOp.Tin, hostTin)
	}
	if atOp.Tin != int(j.FixedSrc("@")) {
		t.Errorf("operator tin %d != instance FixedSrc(@) %d", atOp.Tin, j.FixedSrc("@"))
	}
}

// TestOperatorOrderDeterministic locks in the fix for map-iteration
// nondeterminism. makeAllOps must build operators in a stable, sorted order
// regardless of Go's per-run map-iteration randomization, so tin assignment
// and last-write-wins tin resolution never vary between runs (which made
// `1 + 2 * 3` parse flakily as `1+(2*3)` vs `(1+2)*3`).
func TestOperatorOrderDeterministic(t *testing.T) {
	build := func() []string {
		j := jsonic.Make()
		ops := makeAllOps(j, resolveOptions(nil))
		names := make([]string, len(ops))
		for i, op := range ops {
			names[i] = op.Name
		}
		return names
	}
	first := build()
	for i := 0; i < 50; i++ {
		got := build()
		if !reflect.DeepEqual(got, first) {
			t.Fatalf("operator order not deterministic across builds:\n first: %v\n got:   %v", first, got)
		}
	}
}

// TestPrecedenceStable parses a precedence-sensitive expression repeatedly on
// fresh instances; with deterministic operator setup every run must agree on
// `1 + 2 * 3 == 1 + (2*3)`.
func TestPrecedenceStable(t *testing.T) {
	want := []interface{}{"+", float64(1), []interface{}{"*", float64(2), float64(3)}}
	for i := 0; i < 50; i++ {
		j := makeExprJsonic()
		result, err := j.Parse("1 + 2 * 3")
		if err != nil {
			t.Fatalf("parse error: %v", err)
		}
		got := simplifyAndNormalize(result)
		wantN := simplifyAndNormalize(want)
		if !reflect.DeepEqual(got, wantN) {
			gj, _ := json.Marshal(got)
			wj, _ := json.Marshal(wantN)
			t.Fatalf("run %d: got %s, want %s", i, gj, wj)
		}
	}
}

// TestNoRuleSentinelStaysEmpty locks in the fix for process-wide state
// corruption. NoRule is a package-level sentinel shared by every parser
// instance, and it carries a Node field like any other rule. A rule action
// that assigns to r.Parent.Node without checking leaves that node on the
// sentinel for the life of the process, and every later parse — on any
// instance — reads it back as a value of its own.
//
// `@x!, 3` was the trigger: a top-level implicit list whose first member is
// a suffix applied to a prefix closes with NoRule as its parent. One such
// parse, and an unrelated `a,b` afterwards returned
// `["a", [["!", ["@", "x"]]]]`.
//
// Asserting on the sentinel rather than on any one call site means a new
// unguarded assignment anywhere in the plugin fails this test.
func TestNoRuleSentinelStaysEmpty(t *testing.T) {
	clean, err := json.Marshal(simplifyAndNormalize(jsonic.NoRule.Node))
	if err != nil {
		t.Fatal(err)
	}

	check := func(where string) {
		t.Helper()
		got, err := json.Marshal(simplifyAndNormalize(jsonic.NoRule.Node))
		if err != nil {
			t.Fatal(err)
		}
		if string(got) != string(clean) {
			t.Fatalf("NoRule.Node written after %s:\n  got:  %s\n  want: %s",
				where, got, clean)
		}
	}

	check("startup")

	build := func() *jsonic.Jsonic {
		return makeExprJsonic(map[string]interface{}{
			"op": map[string]interface{}{
				"pre-a": map[string]interface{}{
					"prefix": true, "src": "@", "right": 31000000},
				"suf-a": map[string]interface{}{
					"suffix": true, "src": "!", "left": 27000000},
				"func": map[string]interface{}{
					"paren": true, "osrc": "(", "csrc": ")",
					"preval": map[string]interface{}{"active": true}},
			},
		})
	}

	// The known trigger, plus the shapes around it: a top-level implicit
	// list is the case where a rule's parent is the sentinel.
	for _, src := range []string{
		"@x!, 3", "@x!", "x!, 3", "@x, 3", "1+2,3", "a,b", "a b",
		"@x! 3", "f(@x!, 3)", "[@x!, 3]", "{a: @x!}", "@x!, @y!, 3",
	} {
		build().Parse(src)
		check("parse of " + src)
	}

	// A parse after the trigger must be unaffected by it.
	build().Parse("@x!, 3")

	got, err := build().Parse("a,b")
	if err != nil {
		t.Fatalf("a,b after @x!, 3: %v", err)
	}

	b, err := json.Marshal(simplifyAndNormalize(got))
	if err != nil {
		t.Fatal(err)
	}
	if `["a","b"]` != string(b) {
		t.Errorf("a,b after @x!, 3 returned %s, want [\"a\",\"b\"]", b)
	}
}

// Mirror of the TypeScript suite's `evaluate-called-once-per-operator`.
//
// An evaluate callback is free to return another marked S-expression — an
// identity evaluator, a symbolic rewriter — and is free to have effects.
// A member of an implicit list is reduced when it closes and the list is
// reduced again as a whole, so a reduction that did not land its result
// where the list can see it would hand the same operator to the callback
// a second time, once per member except the last.
func TestEvaluateCalledOncePerOperator(t *testing.T) {
	for _, src := range []string{
		"f(1+2,3+4,5+6)", "f(1+2,3+4)", "1+2,3+4,5+6", "f(1+2,3)",
		"f(-1+2,3+4)", "f(g(1+2,3+4),5+6)", "f(1+2,(3+4,5+6))",
		"(1+2,3+4)", "[f(1+2,3+4)]", "{k:f(1+2,3+4)}",
		"f(1+2,3+4),f(5+6,7+8)",
	} {
		counts := map[string]int{}

		j := jsonic.Make()
		_ = j.Use(Expr, map[string]interface{}{
			"op": map[string]interface{}{
				"func": map[string]interface{}{
					"paren": true, "osrc": "(", "csrc": ")",
					"preval": map[string]interface{}{"active": true}},
			},
			"evaluate": func(
				_ *jsonic.Rule, _ *jsonic.Context, op *Op, terms []interface{},
			) interface{} {
				b, err := json.Marshal(Simplify(terms))
				if err != nil {
					t.Fatal(err)
				}
				counts[op.Name+string(b)]++
				return append([]interface{}{op}, terms...)
			},
		})

		if _, err := j.Parse(src); err != nil {
			t.Fatalf("%s: %v", src, err)
		}

		for key, n := range counts {
			if 1 < n {
				t.Errorf("%s: evaluate called %d times for %s", src, n, key)
			}
		}
	}
}

// --- AST shape divergence (see DIVERGENCE.md) ------------------------------
//
// TWO pins, deliberately separate. The two halves of the divergence can be
// repaired independently: adding json tags closes the naming half and
// leaves the Val wrapper. A single test covering both would fail on that
// partial repair and, following its own instruction, take the record for a
// still-live wrapper divergence with it.
//
// Each pin therefore names only its own half, and DIVERGENCE.md says which
// paragraph goes with which.
//
// The TypeScript twins are in ts/test/ast-shape.test.ts. Both sides measure
// the SERIALISED shape — the boundary a consumer sees, and the one the
// entry is about — so the TS side round-trips through JSON rather than
// inspecting the live object.

// astShapeDoc parses `{a:1+2}` and returns the serialised form of `a`, the
// simplest expression that shows both halves of the divergence.
func astShapeDoc(t *testing.T) map[string]any {
	t.Helper()
	v, err := Parse("{a:1+2}")
	if err != nil {
		t.Fatalf("parse failed: %v", err)
	}
	raw, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("marshal failed: %v", err)
	}
	var doc map[string]any
	if err := json.Unmarshal(raw, &doc); err != nil {
		t.Fatalf("unmarshal failed: %v", err)
	}
	a, ok := doc["a"]
	if !ok {
		t.Fatal("no `a` key, so these tests prove nothing")
	}
	obj, isObj := a.(map[string]any)
	if !isObj {
		t.Fatalf("`a` serialises as %T, not an object — the wrapper is "+
			"gone. See TestASTShapeWrapperDiverges", a)
	}
	return obj
}

// TestASTShapeWrapperDiverges pins the FIRST half: this port wraps the term
// list under Val, TypeScript yields it directly.
func TestASTShapeWrapperDiverges(t *testing.T) {
	obj := astShapeDoc(t)
	if _, hasVal := obj["Val"]; !hasVal {
		t.Error("`a` no longer carries Val. If the wrapper has been " +
			"removed to match TypeScript, delete THIS test, its twin in " +
			"ts/test/ast-shape.test.ts, and the wrapper paragraph of the " +
			"DIVERGENCE.md entry — leave the naming pin and paragraph " +
			"alone unless that half was repaired too")
	}
}

// TestASTShapeNamingDiverges pins the SECOND half: this port's AST structs
// carry no json tags, so encoding/json emits the exported Go names where
// TypeScript emits lower-case ones.
func TestASTShapeNamingDiverges(t *testing.T) {
	obj := astShapeDoc(t)
	terms, _ := obj["Val"].([]any)
	if 0 == len(terms) {
		t.Fatalf("Val is %T with no terms, so the checks below would pass "+
			"vacuously", obj["Val"])
	}
	op, isOp := terms[0].(map[string]any)
	if !isOp {
		t.Fatalf("first term is %T, not an object", terms[0])
	}
	if _, pascal := op["Name"]; !pascal {
		t.Error("the first term has no `Name` key")
	}
	if _, lower := op["name"]; lower {
		t.Error("the first term has a lower-case `name` key, so this port " +
			"now agrees with TypeScript. If json tags have been added, " +
			"delete THIS test, its twin, and the naming paragraph of the " +
			"DIVERGENCE.md entry — leave the wrapper pin and paragraph " +
			"alone unless that half was repaired too")
	}
}
