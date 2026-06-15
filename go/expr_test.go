/* Copyright (c) 2021-2025 Richard Rodger and other contributors, MIT License */

package expr

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"testing"

	jsonic "github.com/tabnas/jsonic/go"
)

// specEntry holds one line from a TSV spec file.
type specEntry struct {
	input    string
	expected interface{}
}

// loadSpec reads a TSV spec file and returns parsed entries.
func loadSpec(t *testing.T, name string) []specEntry {
	t.Helper()

	// Find spec dir relative to this test file.
	_, filename, _, _ := runtime.Caller(0)
	specDir := filepath.Join(filepath.Dir(filename), "..", "test", "spec")
	specPath := filepath.Join(specDir, name)

	f, err := os.Open(specPath)
	if err != nil {
		t.Fatalf("failed to open spec file %s: %v", specPath, err)
	}
	defer f.Close()

	var entries []specEntry
	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		parts := strings.SplitN(line, "\t", 2)
		if len(parts) != 2 {
			continue
		}
		var expected interface{}
		if err := json.Unmarshal([]byte(parts[1]), &expected); err != nil {
			t.Fatalf("failed to parse expected JSON in %s: %q: %v", name, parts[1], err)
		}
		entries = append(entries, specEntry{input: parts[0], expected: expected})
	}
	if err := scanner.Err(); err != nil {
		t.Fatalf("error reading spec file %s: %v", name, err)
	}
	return entries
}

// simplifyAndNormalize converts the parse result to simplified form
// and normalizes it to match JSON-parsed expected values.
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

// runSpec runs all entries from a TSV spec file against a jsonic instance.
func runSpec(t *testing.T, specName string, j *jsonic.Jsonic) {
	t.Helper()
	entries := loadSpec(t, specName)
	for _, e := range entries {
		t.Run(e.input, func(t *testing.T) {
			result, err := j.Parse(e.input)
			if err != nil {
				t.Fatalf("parse error for %q: %v", e.input, err)
			}
			got := simplifyAndNormalize(result)
			if !reflect.DeepEqual(got, e.expected) {
				gotJSON, _ := json.Marshal(got)
				expJSON, _ := json.Marshal(e.expected)
				t.Errorf("input: %q\n  got:  %s\n  want: %s", e.input, gotJSON, expJSON)
			}
		})
	}
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
				"prefix": true, "right": 15000, "src": "@",
			},
			"tight": map[string]interface{}{
				"infix": true, "left": 120000, "right": 130000, "src": "~",
			},
		},
	})
	runSpec(t, "unary-prefix-edge.tsv", j)
}

func TestSpecUnarySuffixBasic(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"factorial": map[string]interface{}{
				"suffix": true, "left": 15000, "src": "!",
			},
			"question": map[string]interface{}{
				"suffix": true, "left": 13000, "src": "?",
			},
		},
	})
	runSpec(t, "unary-suffix-basic.tsv", j)
}

func TestSpecUnarySuffixEdge(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"factorial": map[string]interface{}{
				"suffix": true, "left": 15000, "src": "!",
			},
			"question": map[string]interface{}{
				"suffix": true, "left": 13000, "src": "?",
			},
			"tight": map[string]interface{}{
				"infix": true, "left": 120000, "right": 130000, "src": "~",
			},
		},
	})
	runSpec(t, "unary-suffix-edge.tsv", j)
}

func TestSpecUnarySuffixStructure(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"factorial": map[string]interface{}{
				"suffix": true, "left": 15000, "src": "!",
			},
			"question": map[string]interface{}{
				"suffix": true, "left": 13000, "src": "?",
			},
		},
	})
	runSpec(t, "unary-suffix-structure.tsv", j)
}

func TestSpecUnarySuffixPrefix(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"factorial": map[string]interface{}{
				"suffix": true, "left": 15000, "src": "!",
			},
			"question": map[string]interface{}{
				"suffix": true, "left": 13000, "src": "?",
			},
		},
	})
	runSpec(t, "unary-suffix-prefix.tsv", j)
}

func TestSpecUnarySuffixParen(t *testing.T) {
	j := makeExprJsonic(map[string]interface{}{
		"op": map[string]interface{}{
			"factorial": map[string]interface{}{
				"suffix": true, "left": 15000, "src": "!",
			},
			"question": map[string]interface{}{
				"suffix": true, "left": 13000, "src": "?",
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
				"suffix": true, "src": "!", "left": 15000,
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
				"suffix": true, "src": "!", "left": 15000,
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
				"infix": true, "left": 180, "right": 190, "src": "foo",
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
				"suffix": true, "left": 15000, "src": "!",
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

	m, ok := result.(map[string]interface{})
	if !ok {
		t.Fatalf("result type = %T, want map", result)
	}

	got := m["x"]
	if got != "a.b.c" {
		t.Errorf("x = %v, want %q", got, "a.b.c")
		t.Logf("evaluate calls: %v", calls)
	}

	// Also test simple single infix
	calls = nil
	result, _ = j.Parse("p:a.b")
	m = result.(map[string]interface{})
	if m["p"] != "a.b" {
		t.Errorf("p = %v, want %q", m["p"], "a.b")
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
					switch fname {
					case "min":
						x := toNum(args, 0)
						y := toNum(args, 1)
						if x < y {
							return x
						}
						return y
					case "max":
						x := toNum(args, 0)
						y := toNum(args, 1)
						if x > y {
							return x
						}
						return y
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
