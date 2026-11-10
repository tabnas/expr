/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

package tabnasexpr

// fold_test.go: the tree layer in the language, the Go half of
// ts/test/tree.test.ts (identity, cycles, documents, the inherit
// discipline, errors, the deferred mode and positions), and what only the
// Go port has to settle: the deferred filter measured to fold once per
// parse with and without a start-rule replace, a fold failure surfacing
// from Parse with its code, and the maps that have no member order.

import (
	"errors"
	"fmt"
	"math"
	"path/filepath"
	"reflect"
	"sort"
	"strings"
	"testing"
	"time"

	jsonic "github.com/tabnas/jsonic/go"
	tabnas "github.com/tabnas/parser/go"
	support "github.com/tabnas/support/go"
)

// om builds an ordered map from name, value pairs.
func om(pairs ...any) *tabnas.OrderedMap {
	out := tabnas.NewOrderedMap()
	for i := 0; i+1 < len(pairs); i += 2 {
		out.Set(pairs[i].(string), pairs[i+1])
	}
	return out
}

// cj is a value's canonical JSON.
func cj(t *testing.T, v any) string {
	t.Helper()
	s, err := canonicalJSON(v)
	if err != nil {
		t.Fatal(err)
	}
	return s
}

// refParser is jsonic with only the $ref key operator, as the
// TypeScript parser().
func refParser(t *testing.T) *jsonic.Jsonic {
	t.Helper()
	return treeParser(t, map[string]interface{}{"op": noDefaults(refOp())})
}

func parsed(t *testing.T, j *jsonic.Jsonic, src string) any {
	t.Helper()
	raw, err := j.Parse(src)
	if err != nil {
		t.Fatalf("%s: %v", src, err)
	}
	return raw
}

func mustFold(t *testing.T, raw any, opts FoldOptions) any {
	t.Helper()
	out, err := Fold(raw, opts)
	if err != nil {
		t.Fatal(err)
	}
	return out
}

func foldCode(err error) string {
	var fe *FoldError
	if errors.As(err, &fe) {
		return fe.Code
	}
	var te *tabnas.TabnasError
	if errors.As(err, &te) {
		return te.Code
	}
	return ""
}

func member(v any, path ...string) any {
	for _, name := range path {
		v, _ = recordGet(v, name)
	}
	return v
}

func cycleRows(report FoldReport) []string {
	rows := []string{}
	for _, c := range report.Cycles {
		rows = append(rows, c.At+">"+c.Target)
	}
	return rows
}

func TestFoldLeavesTheRawTreeAndReturnsANewOne(t *testing.T) {
	raw := parsed(t, refParser(t), `{a:{$ref:"#/b"},b:{c:1}}`)
	text := cj(t, raw)
	out := mustFold(t, raw, FoldOptions{Op: refDef, Evaluate: resolving})
	if cj(t, raw) != text {
		t.Errorf("the raw tree changed: %s", cj(t, raw))
	}
	if out == raw {
		t.Error("the fold returned its input")
	}
	if got := cj(t, out); got != `{"a":{"c":1},"b":{"c":1}}` {
		t.Errorf("folded: %s", got)
	}
	if _, kept := out.(*tabnas.OrderedMap); !kept {
		t.Errorf("the engine's map kind is not kept: %T", out)
	}
}

func TestFoldOneTargetOneFoldedObject(t *testing.T) {
	raw := parsed(t, refParser(t), `{a:{$ref:"#/c"},b:{$ref:"#/c"},c:{d:1}}`)
	out := mustFold(t, raw, FoldOptions{Op: refDef, Evaluate: func(site *Site, _ *Op, terms []any) (any, error) {
		hit, err := site.At(fragPointer(terms[0].(string)))
		return hit.Value, err
	}})
	c := member(out, "c")
	if member(out, "a") != c || member(out, "b") != c {
		t.Error("the sites naming one target do not share one folded object")
	}
}

func TestFoldAPointerThatEndsAtASiteAndOneThatPassesThroughIt(t *testing.T) {
	raw := parsed(t, refParser(t), `{a:{$ref:"#/b"},b:{$ref:"#/c"},c:{d:{e:1}},f:{$ref:"#/b/d"}}`)
	got := cj(t, mustFold(t, raw, FoldOptions{Op: refDef, Evaluate: resolving}))
	if got != `{"a":{"d":{"e":1}},"b":{"d":{"e":1}},"c":{"d":{"e":1}},"f":{"e":1}}` {
		t.Errorf("folded: %s", got)
	}
}

func TestFoldCyclesAreReportedAndKeptVerbatimOrRaised(t *testing.T) {
	j := refParser(t)
	src := `{a:{$ref:"#/a"},b:{c:{$ref:"#/b"}},x:{$ref:"#/y"},y:{$ref:"#/x"},r:{$ref:"#"}}`
	var report FoldReport
	out := mustFold(t, parsed(t, j, src), FoldOptions{Op: refDef, Evaluate: resolving,
		Report: func(r FoldReport) { report = r }})
	if cj(t, out) != cj(t, parsed(t, j, src)) {
		t.Errorf("the cycles did not stay verbatim: %s", cj(t, out))
	}
	// A reference INTO a cycle is one: x names y, whose own resolution met
	// the cycle back to x, so both ring members stay verbatim.
	rows := cycleRows(report)
	sort.Strings(rows)
	if want := []string{"/a>/a", "/b/c>/b", "/r>", "/x>/y", "/y>/x"}; !reflect.DeepEqual(rows, want) {
		t.Errorf("cycles %v, want %v", rows, want)
	}

	_, err := Fold(parsed(t, j, `{a:{$ref:"#/a"}}`), FoldOptions{Op: refDef, Evaluate: resolving, Cycle: "error"})
	var fe *FoldError
	if !errors.As(err, &fe) || fe.Code != "fold_cycle" || fe.At != "/a" ||
		!strings.Contains(fe.Error(), `cycle at "/a"`) {
		t.Errorf("Cycle error: %v", err)
	}
}

func TestFoldATargetThatNamesNothingIsReportedAndTheSiteKept(t *testing.T) {
	var report FoldReport
	out := mustFold(t, parsed(t, refParser(t), `{a:{$ref:"#/nope/x"}}`),
		FoldOptions{Op: refDef, Evaluate: resolving, Report: func(r FoldReport) { report = r }})
	if got := cj(t, out); got != `{"a":{"$ref":"#/nope/x"}}` {
		t.Errorf("folded: %s", got)
	}
	if want := []FoldEntry{{Doc: "", At: "/a", TargetDoc: "", Target: "/nope/x"}}; !reflect.DeepEqual(report.Unresolved, want) {
		t.Errorf("unresolved %v", report.Unresolved)
	}
}

func TestFoldADynamicKeyIsReportedAndFoldedAsAPlainMap(t *testing.T) {
	j := treeParser(t, map[string]interface{}{"op": atPointerOptions()})
	var report FoldReport
	out := mustFold(t, parsed(t, j, `{a:{$dynamicRef:"#/b",c:{$ref:"#/b"}},b:1}`),
		FoldOptions{Op: atPointerOp, Evaluate: resolving, Report: func(r FoldReport) { report = r }})
	if got := cj(t, out); got != `{"a":{"$dynamicRef":"#/b","c":1},"b":1}` {
		t.Errorf("folded: %s", got)
	}
	if want := []FoldDynamic{{Doc: "", At: "/a", Src: "$dynamicRef"}}; !reflect.DeepEqual(report.Dynamic, want) {
		t.Errorf("dynamic %v", report.Dynamic)
	}
}

// d is folded first and reaches /b/c through a reference; the bag at /b/c
// is computed from /b, not from d's ancestors.
func TestFoldTheInheritedBagComesFromTheTargetsOwnAncestors(t *testing.T) {
	raw := parsed(t, refParser(t), `{d:{$ref:"#/b/c"},base:x,b:{base:y,c:{$ref:r}}}`)
	var seen []string
	mustFold(t, raw, FoldOptions{Op: refDef, Inherit: baseInherit,
		Evaluate: func(site *Site, _ *Op, terms []any) (any, error) {
			if ref, ok := terms[0].(string); ok && strings.HasPrefix(ref, "#/") {
				hit, err := site.At(fragPointer(ref))
				return hit.Value, err
			}
			k, err := site.K()
			if err != nil {
				return nil, err
			}
			seen = append(seen, fmt.Sprintf("%v=%v", site.Path(), k["base"]))
			return terms[0], nil
		}})
	if want := []string{"[b c]=y"}; !reflect.DeepEqual(seen, want) {
		t.Errorf("seen %v, want %v", seen, want)
	}
}

func TestFoldDeferredFoldsTheFinishedRootWithPositions(t *testing.T) {
	var sites []string
	j := treeParser(t, map[string]interface{}{
		"op": noDefaults(refOp()),
		"fold": FoldOptions{
			Positions: true,
			Evaluate: func(site *Site, op *Op, terms []any) (any, error) {
				where := "none"
				if site.Token != nil {
					where = fmt.Sprintf("%d:%d", site.Token.RI, site.Token.CI)
				}
				sites = append(sites, fmt.Sprintf("%v@%s", site.Path(), where))
				return resolving(site, op, terms)
			},
		},
	})
	if got := cj(t, parsed(t, j, "{\n  a: { $ref: \"#/b\" },\n  b: 1\n}")); got != `{"a":1,"b":1}` {
		t.Errorf("deferred: %s", got)
	}
	if want := []string{"[a]@2:6"}; !reflect.DeepEqual(sites, want) {
		t.Errorf("sites %v, want %v", sites, want)
	}
	// A second parse on the same instance folds again, from scratch.
	if got := cj(t, parsed(t, j, `{c:{$ref:"#/d"},d:2}`)); got != `{"c":2,"d":2}` {
		t.Errorf("second parse: %s", got)
	}
}

func TestFoldEagerEvaluationAndTheDeferredFoldCompose(t *testing.T) {
	j := treeParser(t, map[string]interface{}{
		"op": refOp(),
		"evaluate": func(_ *tabnas.Rule, _ *tabnas.Context, op *Op, terms []interface{}) interface{} {
			if op.Src == "+" {
				return terms[0].(float64) + terms[1].(float64)
			}
			return math.NaN()
		},
		"fold": FoldOptions{Evaluate: resolving},
	})
	if got := cj(t, parsed(t, j, `{a:1+2,b:{$ref:"#/a"}}`)); got != `{"a":3,"b":3}` {
		t.Errorf("composed: %s", got)
	}
}

// A resolver's own error surfaces from Parse, its text kept: the engine
// labels a panic that is not a TabnasError "internal", where TypeScript
// rethrows the error as it is. The instance keeps working.
func TestFoldAResolverErrorSurfacesFromParseAndTheInstanceSurvives(t *testing.T) {
	j := treeParser(t, map[string]interface{}{
		"op": noDefaults(refOp()),
		"fold": FoldOptions{Evaluate: func(*Site, *Op, []any) (any, error) {
			return nil, errors.New("resolver: boom")
		}},
	})
	_, err := j.Parse(`{a:{$ref:"#/b"},b:1}`)
	if err == nil || !strings.Contains(err.Error(), "resolver: boom") || foldCode(err) != "internal" {
		t.Errorf("a resolver error: %v (code %q)", err, foldCode(err))
	}
	if got := cj(t, parsed(t, j, `{b:1}`)); got != `{"b":1}` {
		t.Errorf("after the error: %s", got)
	}
}

// A fold failure in deferred mode surfaces from Parse with the fold's code:
// the plugin panics it as a TabnasError, whose code the engine keeps.
func TestFoldAFoldErrorSurfacesFromParseWithItsCode(t *testing.T) {
	for _, c := range []struct {
		fold FoldOptions
		src  string
		code string
	}{
		{FoldOptions{Evaluate: resolving, Cycle: "error"}, `{a:{$ref:"#/a"}}`, "fold_cycle"},
		{FoldOptions{Evaluate: resolving, MaxDepth: 2}, `{a:{b:{c:1}}}`, "fold_depth"},
		// Inherit runs only for a bag something reads, so this resolver reads
		// one.
		{FoldOptions{
			Inherit: func(*Site, any, map[string]any) (map[string]any, error) { return nil, nil },
			Evaluate: func(site *Site, _ *Op, _ []any) (any, error) {
				k, err := site.K()
				return k, err
			},
		}, `{a:{$ref:"#/b"},b:1}`, "fold_inherit"},
	} {
		j := treeParser(t, map[string]interface{}{"op": noDefaults(refOp()), "fold": c.fold})
		value, err := j.Parse(c.src)
		var te *tabnas.TabnasError
		if !errors.As(err, &te) || te.Code != c.code || !strings.Contains(te.Detail, "expr fold: ") || value != nil {
			t.Errorf("%s: %v, %#v; want a TabnasError coded %s", c.src, value, err, c.code)
		}
		if te != nil && strings.Contains(te.Hint, "Unknown error code") {
			t.Errorf("%s: the hint is the engine's for an unknown code: %q", c.src, te.Hint)
		}
		if got := cj(t, parsed(t, j, `{b:1}`)); got != `{"b":1}` {
			t.Errorf("%s: after the error: %s", c.src, got)
		}
	}
}

// Go has no prototype, so a member named __proto__ is data as any other.
func TestFoldAMemberNamedProtoIsData(t *testing.T) {
	raw := parsed(t, refParser(t), `{"__proto__":{x:1},a:{$ref:"#/__proto__"}}`)
	out := mustFold(t, raw, FoldOptions{Op: refDef, Evaluate: resolving})
	if keys := out.(*tabnas.OrderedMap).Keys; !reflect.DeepEqual(keys, []string{"__proto__", "a"}) {
		t.Errorf("keys %v", keys)
	}
	if got := cj(t, member(out, "a")); got != `{"x":1}` {
		t.Errorf("a: %s", got)
	}
	var calls [][2]any
	evaluate := func(site *Site, op *Op, terms []any) (any, error) {
		calls = append(calls, [2]any{op.Src, terms[0]})
		return site.Node, nil
	}
	proto := map[string]*OpDef{"p": {Key: true, Src: "__proto__"}}
	mustFold(t, om("name", "ordinary", "src", "data"), FoldOptions{Op: proto, Evaluate: evaluate})
	if len(calls) != 0 {
		t.Errorf("a map owning no __proto__ member was a site: %v", calls)
	}
	mustFold(t, om("__proto__", 42.0, "k", 1.0), FoldOptions{Op: proto, Evaluate: evaluate})
	if want := [][2]any{{"__proto__", 42.0}}; !reflect.DeepEqual(calls, want) {
		t.Errorf("calls %v", calls)
	}
}

// Neither declaration order nor map order decides: static before dynamic,
// then code points.
func TestFoldAMapOwningTwoKeyMembersStaticFirstThenByCodePoint(t *testing.T) {
	keys, err := MakeKeyMap(map[string]*OpDef{
		"z":      {Key: true, Src: "$z"},
		"dyn":    {Key: true, Src: "$a", Dynamic: true},
		"two":    {Key: true, Src: "2"},
		"astral": {Key: true, Src: "\U0001F600"},
		"bmp":    {Key: true, Src: "｡"},
	})
	if err != nil {
		t.Fatal(err)
	}
	var order []string
	for _, op := range keys {
		order = append(order, op.Src)
	}
	if want := []string{"$z", "2", "｡", "\U0001F600", "$a"}; !reflect.DeepEqual(order, want) {
		t.Errorf("order %q, want %q", order, want)
	}
	if op := KeyOpOf(keys, om("$a", 1, "$z", 2, "2", 3)); op == nil || op.Src != "$z" {
		t.Errorf("static first: %v", op)
	}
	if op := KeyOpOf(keys, map[string]any{"$a": 1, "2": 3}); op == nil || op.Src != "2" {
		t.Errorf("then by code point: %v", op)
	}
	if op := KeyOpOf(keys, tabnas.MapRef{Val: map[string]any{"$a": 1}}); op == nil || op.Name != "dyn-key" || !op.Key || !op.Dynamic || op.Terms != 2 {
		t.Errorf("a dynamic one: %+v", op)
	}
	for _, leaf := range []any{[]any{om("$z", 1)}, "x", map[string]int{"$z": 1}, nil} {
		if op := KeyOpOf(keys, leaf); op != nil {
			t.Errorf("%T is not a map the fold walks, but is a site of %s", leaf, op.Name)
		}
	}
}

func TestFoldTwoKeyOperatorsOnOneMemberNameAreRefused(t *testing.T) {
	_, err := MakeKeyMap(map[string]*OpDef{"ref": {Key: true, Src: "$ref"}, "again": {Key: true, Src: "$ref"}})
	var fe *FoldError
	if !errors.As(err, &fe) || fe.Code != "fold_key" ||
		fe.Error() != `expr fold: key operators "again" and "ref" both name the member "$ref"` {
		t.Errorf("MakeKeyMap: %v", err)
	}
	if _, err := MakeKeyMap(map[string]*OpDef{"none": {Key: true}}); foldCode(err) != "fold_key" {
		t.Errorf("a key operator naming no member: %v", err)
	}
	twice := map[string]interface{}{"op": map[string]interface{}{
		"a": map[string]interface{}{"key": true, "src": "x"},
		"b": map[string]interface{}{"key": true, "src": "x"},
	}}
	if err := jsonic.Make().Use(Expr, twice); foldCode(err) != "fold_key" {
		t.Errorf("Use: %v", err)
	}
	if _, err := Parse("1", twice); foldCode(err) != "fold_key" {
		t.Errorf("Parse: %v", err)
	}
}

func TestFoldAHandBuiltOperatorNodeIsRecognised(t *testing.T) {
	op := Opify(&Op{Name: "ref-key", Src: "$ref", Key: true})
	if op.Terms != 2 {
		t.Errorf("Opify gives a key operator %d terms", op.Terms)
	}
	node := []any{op, "x", map[string]any{}}
	if !IsOp(node) || IsOp([]any{"$ref", "x", map[string]any{}}) {
		t.Error("IsOp")
	}
	got := Evaluation(nil, nil, node, func(_ *tabnas.Rule, _ *tabnas.Context, o *Op, terms []interface{}) interface{} {
		return append([]any{o.Src}, terms...)
	})
	if !reflect.DeepEqual(got, []any{"$ref", "x", map[string]any{}}) {
		t.Errorf("Evaluation: %v", got)
	}
}

func TestFoldASiteInsideATokenExpressionIsFoldedAtItsOwnPath(t *testing.T) {
	j := treeParser(t, map[string]interface{}{"op": refOp()})
	raw := parsed(t, j, `{a:5,e:1+2*{$ref:"#/a"}}`)
	if !IsOp(member(raw, "e")) {
		t.Fatalf("e is %T", member(raw, "e"))
	}
	var paths []Path
	out := mustFold(t, raw, FoldOptions{Op: refDef, Evaluate: func(site *Site, op *Op, terms []any) (any, error) {
		if op.Key {
			paths = append(paths, site.Path())
			hit, err := site.At(fragPointer(terms[0].(string)))
			return hit.Value, err
		}
		if op.Src == "+" {
			return terms[0].(float64) + terms[1].(float64), nil
		}
		return terms[0].(float64) * terms[1].(float64), nil
	}})
	if want := []Path{{"e", 2, 2}}; !reflect.DeepEqual(paths, want) {
		t.Errorf("paths %v, want %v", paths, want)
	}
	if got := cj(t, out); got != `{"a":5,"e":11}` {
		t.Errorf("folded: %s", got)
	}
}

func TestFoldAPointerIntoARingIsACycleInEveryMemberOrder(t *testing.T) {
	for _, src := range []string{
		`{a:{$ref:"#/b/x"},b:{$ref:"#/c",x:7},c:{$ref:"#/b"}}`,
		`{b:{$ref:"#/c",x:7},c:{$ref:"#/b"},a:{$ref:"#/b/x"}}`,
	} {
		var report FoldReport
		out := mustFold(t, parsed(t, refParser(t), src),
			FoldOptions{Op: refDef, Evaluate: resolving, Report: func(r FoldReport) { report = r }})
		if got := cj(t, member(out, "a")); got != `{"$ref":"#/b/x"}` {
			t.Errorf("%s: a is %s", src, got)
		}
		found := false
		for _, c := range report.Cycles {
			found = found || (c.At == "/a" && c.Target == "/b")
		}
		if !found {
			t.Errorf("%s: no cycle /a>/b in %v", src, report.Cycles)
		}
	}
}

// failingSources are parses that fail, which the deferred mode must not
// fold: a closer the alternate raises on, one more than the map has, and a
// lexer error the rule loop leaves latched.
var failingSources = []string{`{a:{$ref:"#/b"},b:1} ]`, `{a:{$ref:"#/b"},b:1}}`, `{a:{$ref:"#/b"},b:1} "x`}

func TestFoldAParseThatFailsIsNotFolded(t *testing.T) {
	calls := 0
	j := treeParser(t, map[string]interface{}{
		"op": noDefaults(refOp()),
		"fold": FoldOptions{Evaluate: func(site *Site, op *Op, terms []any) (any, error) {
			calls++
			return resolving(site, op, terms)
		}},
	})
	for _, src := range failingSources {
		if _, err := j.Parse(src); err == nil || strings.HasPrefix(foldCode(err), "fold_") {
			t.Errorf("%s: %v", src, err)
		}
	}
	if calls != 0 {
		t.Errorf("a parse that fails was folded: %d evaluate calls", calls)
	}

	// A start rule whose last close backtracks over a token leaves it for
	// the engine's trailing-content check, which fails the parse after the
	// rule loop: that pass is not folded either.
	k := treeParser(t, map[string]interface{}{"op": noDefaults(refOp()), "fold": FoldOptions{
		Evaluate: func(*Site, *Op, []any) (any, error) { calls++; return nil, errors.New("folded") },
	}})
	k.Rule("top", func(rs *tabnas.RuleSpec, _ *tabnas.Parser) {
		rs.AddOpen(&tabnas.AltSpec{P: "val"})
		rs.AddClose(&tabnas.AltSpec{S: [][]int{{tabnas.TinZZ}}}, &tabnas.AltSpec{S: [][]int{{tabnas.TinCS}}, B: 1})
		rs.AddBC(func(r *tabnas.Rule, _ *tabnas.Context) { r.Node = r.Child.Node })
	})
	k.SetOptions(tabnas.Options{Rule: &tabnas.RuleOptions{Start: "top"}})
	if _, err := k.Parse(`{a:{$ref:"#/b"},b:1} ]`); err == nil || foldCode(err) != "unexpected" || calls != 0 {
		t.Errorf("a token left after the last pass: %v, %d calls", err, calls)
	}
}

// countingFold is a deferred fold that counts its runs (through Report)
// and its sites.
func countingFold(runs, sites *int) FoldOptions {
	return FoldOptions{
		Report: func(FoldReport) { *runs++ },
		Evaluate: func(site *Site, op *Op, terms []any) (any, error) {
			*sites++
			if op.Key {
				return resolving(site, op, terms)
			}
			a, _ := terms[0].(float64)
			b, _ := terms[1].(float64)
			return a + b, nil
		},
	}
}

// The deferred filter matches exactly one pass per parse, without a
// start-rule replace and with one: the expr grammar replaces its start val
// with an expr rule on a top-level infix (1+2), jsonic with a list on a
// top-level comma, and a custom start rule here replaces itself. The fold
// writes the node of the rule the last pass belongs to, which is the node
// Parse returns.
func TestFoldDeferredFoldsOncePerParse(t *testing.T) {
	var runs, sites int
	j := treeParser(t, map[string]interface{}{"op": refOp(), "fold": countingFold(&runs, &sites)})
	for _, c := range []struct{ src, want string }{
		{`{a:{$ref:"#/b"},b:1}`, `{"a":1,"b":1}`}, // no replace
		{`1+2`, `3`},                                // val replaced by expr
		{`{$ref:"#/1"},2`, `[2,2]`},                 // val replaced by list
		{`1+{$ref:"#/1"},5`, `[6,5]`},               // val, expr, then elem
		{`{a:[1+2,{$ref:"#/a/0"}]}`, `{"a":[3,3]}`}, // no replace
	} {
		runs, sites = 0, 0
		if got := cj(t, parsed(t, j, c.src)); got != c.want {
			t.Errorf("%s: %s, want %s", c.src, got, c.want)
		}
		if runs != 1 {
			t.Errorf("%s: folded %d times", c.src, runs)
		}
	}

	// A start rule that replaces itself, with no alternates of its own
	// after the replace: the last pass is the replacement's close.
	runs, sites = 0, 0
	k := treeParser(t, map[string]interface{}{"op": noDefaults(refOp()), "fold": countingFold(&runs, &sites)})
	k.Rule("top", func(rs *tabnas.RuleSpec, _ *tabnas.Parser) {
		rs.AddOpen(&tabnas.AltSpec{P: "val"})
		rs.AddClose(&tabnas.AltSpec{R: "tail"})
		rs.AddBC(func(r *tabnas.Rule, _ *tabnas.Context) { r.Node = r.Child.Node })
	})
	k.Rule("tail", func(rs *tabnas.RuleSpec, _ *tabnas.Parser) {})
	k.SetOptions(tabnas.Options{Rule: &tabnas.RuleOptions{Start: "top"}})
	if got := cj(t, parsed(t, k, `{a:{$ref:"#/b"},b:1}`)); got != `{"a":1,"b":1}` {
		t.Errorf("a replaced start rule: %s", got)
	}
	if runs != 1 || sites != 1 {
		t.Errorf("a replaced start rule: %d folds, %d sites", runs, sites)
	}
}

// The filter measured over every input the fixtures hold, test/spec and
// test/tree, on the default operators, and over the parses that fail
// above: with recovery off, exactly one last pass for a parse that
// succeeds and none for one that fails; with recovery on, exactly one for a
// clean parse and never more than one. A recovered parse whose root never
// closes (recovery stops inside a structure) has no last pass, in the
// canonical as here, and is not folded.
func TestFoldTheDeferredFilterMatchesOnePassPerParse(t *testing.T) {
	inputs := append([]string{}, failingSources...)
	for _, dir := range []string{"", "tree"} {
		path, err := support.FindSpecDir("")
		if err != nil {
			t.Fatal(err)
		}
		if dir != "" {
			path = filepath.Join(path, "..", dir)
		}
		files, err := support.LoadSpecDir(path, nil)
		if err != nil {
			t.Fatal(err)
		}
		for _, file := range files {
			for _, row := range file.Rows {
				inputs = append(inputs, row.Unesc(0))
			}
		}
	}
	if len(inputs) < 1188 {
		t.Fatalf("%d inputs, fewer than the fixtures hold", len(inputs))
	}

	for _, recovering := range []bool{false, true} {
		passes := 0
		j := treeParser(t, nil)
		if recovering {
			j.SetOptions(tabnas.Options{Parse: &tabnas.ParseOptions{Recover: &tabnas.RecoverOptions{Enabled: true}}})
		}
		j.SubRuleDone(func(r *tabnas.Rule, ctx *tabnas.Context, done tabnas.RuleDone) {
			if lastPass(r, ctx, done) {
				passes++
			}
		})
		for _, src := range inputs {
			passes = 0
			_, errs, err := j.ParseRecover(src)
			clean := err == nil && len(errs) == 0
			switch {
			case !recovering && err == nil && passes != 1,
				!recovering && err != nil && passes != 0,
				recovering && clean && passes != 1,
				passes > 1:
				t.Errorf("recovery %v, %q: %d last passes (error %v, %d recovered)", recovering, src, passes, err, len(errs))
			}
		}
	}
}

func TestFoldAStartRuleWithNoCloseAlternatesIsStillFolded(t *testing.T) {
	j := treeParser(t, map[string]interface{}{"op": noDefaults(refOp()), "fold": FoldOptions{Evaluate: resolving}})
	j.Rule("top", func(rs *tabnas.RuleSpec, _ *tabnas.Parser) {
		rs.AddOpen(&tabnas.AltSpec{P: "val"})
		rs.AddBC(func(r *tabnas.Rule, _ *tabnas.Context) { r.Node = r.Child.Node })
	})
	j.SetOptions(tabnas.Options{Rule: &tabnas.RuleOptions{Start: "top"}})
	if got := cj(t, parsed(t, j, `{a:{$ref:"#/b"},b:1}`)); got != `{"a":1,"b":1}` {
		t.Errorf("folded: %s", got)
	}
}

type point struct{ X int }

// Values that are not maps or lists the fold walks are leaves, kept by
// identity: a struct, a time, a map of another type.
func TestFoldOtherValuesAreLeavesKeptByIdentity(t *testing.T) {
	when := time.Unix(0, 0).UTC()
	counts := map[string]int{"k": 1}
	at := &point{X: 3}
	raw := map[string]any{
		"when": when, "counts": counts, "point": at,
		"a": map[string]any{"$ref": "#/when"},
		"b": map[string]any{"$ref": "#/point/X"},
	}
	direct := func(site *Site, _ *Op, terms []any) (any, error) {
		hit, err := site.At(fragPointer(terms[0].(string)))
		if err != nil || !hit.Found || hit.Cycle {
			return site.Node, err
		}
		return hit.Value, nil
	}
	out := mustFold(t, raw, FoldOptions{Op: refDef, Evaluate: direct}).(map[string]any)
	if out["when"] != when || out["point"] != at || out["a"] != when {
		t.Errorf("not kept: %v", out)
	}
	if reflect.ValueOf(out["counts"]).Pointer() != reflect.ValueOf(counts).Pointer() {
		t.Error("a map of another type was rebuilt")
	}
	if !reflect.DeepEqual(out["b"], map[string]any{"$ref": "#/point/X"}) {
		t.Errorf("a struct was stepped into: %v", out["b"])
	}
}

func TestFoldArrayIndicesAreRFC6901(t *testing.T) {
	hit := func(seg any) any {
		var got FoldHit
		mustFold(t, om("b", []any{"zero", "one", "two"}, "a", om("$ref", "x")), FoldOptions{Op: refDef,
			Evaluate: func(site *Site, _ *Op, _ []any) (any, error) {
				var err error
				got, err = site.At(Path{"b", seg})
				return 1.0, err
			}})
		if !got.Found {
			return "-"
		}
		return got.Value
	}
	for _, c := range []struct {
		seg  any
		want any
	}{{"0", "zero"}, {"1", "one"}, {"2", "two"}, {1, "one"}, {2.0, "two"}} {
		if got := hit(c.seg); got != c.want {
			t.Errorf("%#v: %v, want %v", c.seg, got, c.want)
		}
	}
	for _, seg := range []any{"", " ", "01", "1.0", "1e0", "0x1", "+1", "-0", "-", "3", "length", 1.5, -1, 3} {
		if got := hit(seg); got != "-" {
			t.Errorf("%#v names %v", seg, got)
		}
	}
	var located FoldHit
	mustFold(t, om("b", []any{"zero", "one"}, "a", om("$ref", "x")), FoldOptions{Op: refDef,
		Evaluate: func(site *Site, _ *Op, _ []any) (any, error) {
			var err error
			located, err = site.Locate(Path{"b", "01"})
			return 1.0, err
		}})
	if located != (FoldHit{}) {
		t.Errorf("Locate 01: %+v", located)
	}
}

func TestFoldACycleHitNeverCarriesAValue(t *testing.T) {
	var hits []FoldHit
	record := func(site *Site, _ *Op, terms []any) (any, error) {
		h, err := site.At(fragPointer(terms[0].(string)))
		hits = append(hits, h)
		if err != nil || !h.Found || h.Cycle {
			return site.Node, err
		}
		return h.Value, nil
	}
	mustFold(t, parsed(t, refParser(t), `{a:{$ref:"#/a"},b:{$ref:"#/a"},x:{$ref:"#/y"},y:{$ref:"#/x"},z:{$ref:"#/x/q"}}`),
		FoldOptions{Op: refDef, Evaluate: record})
	if len(hits) < 5 {
		t.Errorf("%d hits", len(hits))
	}
	for _, h := range hits {
		if !h.Cycle || h.Value != nil {
			t.Errorf("hit %+v", h)
		}
	}
}

func TestFoldTheBagAtASiteIncludesItsOwnMapAndInheritMustReturnOne(t *testing.T) {
	j := refParser(t)
	out := mustFold(t, parsed(t, j, `{base:x,a:{$ref:r,base:z}}`), FoldOptions{Op: refDef, Inherit: baseInherit,
		Evaluate: func(site *Site, _ *Op, _ []any) (any, error) {
			k, err := site.K()
			return k["base"], err
		}})
	if member(out, "a") != "z" {
		t.Errorf("a is %v", member(out, "a"))
	}
	bag := func(site *Site, _ *Op, _ []any) (any, error) {
		k, err := site.K()
		return k, err
	}
	for i, bad := range []Inherit{
		func(*Site, any, map[string]any) (map[string]any, error) { return nil, nil },
		func(_ *Site, _ any, k map[string]any) (map[string]any, error) { k["base"] = 1; return nil, nil },
	} {
		_, err := Fold(parsed(t, j, `{a:{b:{$ref:x}}}`), FoldOptions{Op: refDef, Inherit: bad, Evaluate: bag})
		if foldCode(err) != "fold_inherit" {
			t.Errorf("bad inherit %d: %v", i, err)
		}
	}
	// The root's inherit folds q, whose bag needs the root's: refused.
	_, err := Fold(parsed(t, j, `{a:{b:{$ref:x}},q:{$ref:y}}`), FoldOptions{Op: refDef,
		Inherit: func(site *Site, _ any, k map[string]any) (map[string]any, error) {
			if len(site.Path()) == 0 {
				if _, err := site.At(Path{"q"}); err != nil {
					return nil, err
				}
			}
			return k, nil
		},
		Evaluate: bag})
	if foldCode(err) != "fold_inherit" {
		t.Errorf("an inherit that folds its own subtree: %v", err)
	}
}

func TestFoldReportRowsAreKeptOnceEach(t *testing.T) {
	var report FoldReport
	mustFold(t, om("a", om("$ref", "#/nope"), "c", om("$ref", "#/c")), FoldOptions{Op: refDef,
		Evaluate: func(site *Site, _ *Op, terms []any) (any, error) {
			for i := 0; i < 2; i++ {
				if _, err := site.At(fragPointer(terms[0].(string))); err != nil {
					return nil, err
				}
			}
			return site.Node, nil
		},
		Report: func(r FoldReport) { report = r }})
	if want := []FoldEntry{{At: "/a", Target: "/nope"}}; !reflect.DeepEqual(report.Unresolved, want) {
		t.Errorf("unresolved %v", report.Unresolved)
	}
	if want := []FoldEntry{{At: "/c", Target: "/c"}}; !reflect.DeepEqual(report.Cycles, want) {
		t.Errorf("cycles %v", report.Cycles)
	}
}

// Node -> Tree, Tree/properties/child -> Node: a cycle through a container
// is cut once, at the site whose target was being folded, in either member
// order; only the place of the cut follows the walk.
func TestFoldACycleThroughAContainerIsCutOnce(t *testing.T) {
	tree := `{properties:{child:{$ref:"#/Node"}}}`
	for _, c := range []struct{ src, cut string }{
		{`{Node:{$ref:"#/Tree"},Tree:` + tree + `}`, "/Tree/properties/child>/Node"},
		{`{Tree:` + tree + `,Node:{$ref:"#/Tree"}}`, "/Node>/Tree"},
	} {
		var report FoldReport
		out := mustFold(t, parsed(t, refParser(t), c.src),
			FoldOptions{Op: refDef, Evaluate: resolving, Report: func(r FoldReport) { report = r }})
		if rows := cycleRows(report); !reflect.DeepEqual(rows, []string{c.cut}) {
			t.Errorf("%s: cuts %v, want %s", c.src, rows, c.cut)
		}
		if !strings.Contains(cj(t, out), "properties") {
			t.Errorf("%s: %s", c.src, cj(t, out))
		}
	}
}

func TestFoldAPointerThroughACutSiteIsACycleNotAMiss(t *testing.T) {
	var report FoldReport
	out := mustFold(t, parsed(t, refParser(t),
		`{Tree:{properties:{child:{$ref:"#/Node"}}},Node:{$ref:"#/Tree"},Q:{$ref:"#/Node/properties"}}`),
		FoldOptions{Op: refDef, Evaluate: resolving, Report: func(r FoldReport) { report = r }})
	if got := cj(t, member(out, "Q")); got != `{"$ref":"#/Node/properties"}` {
		t.Errorf("Q is %s", got)
	}
	if len(report.Unresolved) != 0 {
		t.Errorf("unresolved %v", report.Unresolved)
	}
	found := false
	for _, c := range report.Cycles {
		found = found || (c.At == "/Q" && c.Target == "/Node")
	}
	if !found {
		t.Errorf("no cycle /Q>/Node in %v", report.Cycles)
	}
}

func TestFoldAPathThroughADynamicMapOrARefusedOneReadsOn(t *testing.T) {
	var report FoldReport
	keep := func(r FoldReport) { report = r }
	out := mustFold(t, om("a", om("$dynamicRef", "#/b", "c", om("$ref", "#/a/d"), "d", 1.0), "b", 2.0),
		FoldOptions{Op: atPointerOp, Evaluate: resolving, Report: keep})
	if member(out, "a", "c") != 1.0 || len(report.Cycles) != 0 {
		t.Errorf("through a dynamic map: %s, cycles %v", cj(t, out), report.Cycles)
	}
	asked := 0
	out = mustFold(t, om("P", om("$ref", om("type", "string"), "other", om("$ref", "#/P/third"), "third", om("t", 3.0))),
		FoldOptions{Op: refDef, Evaluate: resolving, Report: keep,
			Recognise: func(site *Site, _ *Op) (bool, error) {
				path := site.Path()
				if len(path) == 1 && path[0] == "P" {
					asked++
				}
				return len(path) == 0 || path[len(path)-1] != "P", nil
			}})
	if got := cj(t, member(out, "P", "other")); got != `{"t":3}` {
		t.Errorf("through a refused map: %s", got)
	}
	if len(report.Cycles) != 0 || asked != 1 {
		t.Errorf("cycles %v, recognise asked %d times for /P", report.Cycles, asked)
	}
}

func TestFoldSeveralDocumentsLoadedOnceSharedTargetsCyclesAcrossThem(t *testing.T) {
	docs := map[string]any{"b.json": om("Z", om("z", 1.0), "X", om("$ref", "a.json#/Y"))}
	var loads []string
	split := func(ref string) (string, Path) {
		doc, frag, _ := strings.Cut(ref, "#")
		return doc, fragPointer("#" + frag)
	}
	var report FoldReport
	out := mustFold(t, om("P", om("$ref", "b.json#/Z"), "Q", om("$ref", "b.json#/Z"),
		"Y", om("$ref", "b.json#/X"), "R", om("$ref", "c.json#/W")), FoldOptions{
		Op:  refDef,
		Doc: "a.json",
		Load: func(key string, _ *Site) (any, bool, error) {
			loads = append(loads, key)
			root, ok := docs[key]
			return root, ok, nil
		},
		Evaluate: func(site *Site, _ *Op, terms []any) (any, error) {
			doc, path := split(terms[0].(string))
			var hit FoldHit
			var err error
			if doc == "" {
				hit, err = site.At(path)
			} else {
				hit, err = site.At(path, doc)
			}
			if err != nil || !hit.Found || hit.Cycle {
				return site.Node, err
			}
			return hit.Value, nil
		},
		Report: func(r FoldReport) { report = r },
	})
	if !reflect.DeepEqual(loads, []string{"b.json", "c.json"}) {
		t.Errorf("loads %v", loads)
	}
	if member(out, "P") != member(out, "Q") {
		t.Error("one folded object per target, across documents")
	}
	if got := cj(t, member(out, "Y")); got != `{"$ref":"b.json#/X"}` {
		t.Errorf("Y is %s", got)
	}
	var rows []string
	for _, c := range report.Cycles {
		rows = append(rows, strings.Join([]string{c.Doc, c.At, c.TargetDoc, c.Target}, " "))
	}
	sort.Strings(rows)
	if want := []string{"a.json /Y b.json /X", "b.json /X a.json /Y"}; !reflect.DeepEqual(rows, want) {
		t.Errorf("cycles %v", rows)
	}
	if want := []FoldEntry{{Doc: "a.json", At: "/R", TargetDoc: "c.json", Target: "/W"}}; !reflect.DeepEqual(report.Unresolved, want) {
		t.Errorf("unresolved %v", report.Unresolved)
	}
}

func TestFoldRecogniseAndPlain(t *testing.T) {
	j := refParser(t)
	var calls []string
	out := mustFold(t, parsed(t, j, `{S:{properties:{$ref:{type:string},other:{$ref:"#/T"}}},T:{t:1},Q:{$ref:"#/S/properties/other"}}`),
		FoldOptions{Op: refDef,
			Recognise: func(site *Site, _ *Op) (bool, error) {
				path := site.Path()
				return path[len(path)-1] != "properties", nil
			},
			Evaluate: func(site *Site, op *Op, terms []any) (any, error) {
				var parts []string
				for _, seg := range site.Path() {
					parts = append(parts, fmt.Sprint(seg))
				}
				calls = append(calls, strings.Join(parts, "/"))
				return resolving(site, op, terms)
			}})
	if got := cj(t, member(out, "S", "properties")); got != `{"$ref":{"type":"string"},"other":{"t":1}}` {
		t.Errorf("S/properties is %s", got)
	}
	if got := cj(t, member(out, "Q")); got != `{"t":1}` {
		t.Errorf("Q is %s", got)
	}
	sort.Strings(calls)
	if !reflect.DeepEqual(calls, []string{"Q", "S/properties/other"}) {
		t.Errorf("calls %v", calls)
	}

	// A resolver that finds no operation returns Plain(): the map, folded,
	// members in their own order.
	plain := mustFold(t, parsed(t, j, `{a:{x:{$ref:"#/b"},$ref:42,y:1},b:2}`), FoldOptions{Op: refDef,
		Evaluate: func(site *Site, op *Op, terms []any) (any, error) {
			if _, isText := terms[0].(string); isText {
				return resolving(site, op, terms)
			}
			return site.Plain()
		}})
	if keys := member(plain, "a").(*tabnas.OrderedMap).Keys; !reflect.DeepEqual(keys, []string{"x", "$ref", "y"}) {
		t.Errorf("keys %v", keys)
	}
	if got := cj(t, member(plain, "a")); got != `{"x":2,"$ref":42,"y":1}` {
		t.Errorf("a is %s", got)
	}
}

func TestFoldPositionsALookupInTheFunctionFormTrueOnlyWhenDeferred(t *testing.T) {
	j := refParser(t)
	positions := RecordPositions(j)
	raw := parsed(t, j, "{\n  a: { $ref: \"#/b\" },\n  b: 1\n}")
	var at []string
	mustFold(t, raw, FoldOptions{Op: refDef, Positions: positions,
		Evaluate: func(site *Site, op *Op, terms []any) (any, error) {
			if site.Token == nil {
				t.Fatal("no token")
			}
			at = append(at, fmt.Sprintf("%d:%d", site.Token.RI, site.Token.CI))
			return resolving(site, op, terms)
		}})
	if !reflect.DeepEqual(at, []string{"2:6"}) {
		t.Errorf("at %v", at)
	}
	for _, bad := range []any{true, 7} {
		if _, err := Fold(raw, FoldOptions{Op: refDef, Positions: bad, Evaluate: resolving}); foldCode(err) != "fold_option" {
			t.Errorf("Positions %v: %v", bad, err)
		}
	}
	for _, fold := range []any{"yes", FoldOptions{}, FoldOptions{Evaluate: resolving, Positions: 7}} {
		if err := jsonic.Make().Use(Expr, map[string]interface{}{"fold": fold}); foldCode(err) != "fold_option" {
			t.Errorf("the fold option %#v: %v", fold, err)
		}
	}
}

func TestFoldTokensOffKeepsAnOperatorNodeAnOperatorNode(t *testing.T) {
	raw := parsed(t, treeParser(t, map[string]interface{}{"op": refOp()}), `{a:1+{$ref:"#/b"},b:2}`)
	out := mustFold(t, raw, FoldOptions{Op: refDef, Tokens: tabnas.Bool(false), Evaluate: resolving})
	node := member(out, "a")
	if !IsOp(node) {
		t.Fatalf("a is %T", node)
	}
	got, _ := unwrapExpr(node)
	want, _ := unwrapExpr(member(raw, "a"))
	if got[0] != want[0] || !reflect.DeepEqual(got[1:], []any{1.0, 2.0}) {
		t.Errorf("a is %v", got)
	}
	math := func(_ *tabnas.Rule, _ *tabnas.Context, op *Op, terms []interface{}) interface{} {
		return terms[0].(float64) + terms[1].(float64)
	}
	if v := Evaluation(nil, nil, node, math); v != 3.0 {
		t.Errorf("Evaluation: %v", v)
	}
}

func TestFoldDepthRaisesFoldDepth(t *testing.T) {
	chain := func(n int) *tabnas.OrderedMap {
		o := tabnas.NewOrderedMap()
		for i := 0; i < n; i++ {
			if i < n-1 {
				o.Set(fmt.Sprintf("a%d", i), om("$ref", fmt.Sprintf("#/a%d", i+1)))
			} else {
				o.Set(fmt.Sprintf("a%d", i), om("v", 1.0))
			}
		}
		return o
	}
	if got := cj(t, member(mustFold(t, chain(500), FoldOptions{Op: refDef, Evaluate: resolving}), "a0")); got != `{"v":1}` {
		t.Errorf("a chain of 500: %s", got)
	}
	for _, n := range []int{1200, 20000} {
		if _, err := Fold(chain(n), FoldOptions{Op: refDef, Evaluate: resolving}); foldCode(err) != "fold_depth" {
			t.Errorf("a chain of %d: %v", n, err)
		}
	}
	var deep any = om("x", 1.0)
	for i := 0; i < 1200; i++ {
		deep = om("c", deep)
	}
	if _, err := Fold(deep, FoldOptions{Op: refDef, Evaluate: resolving}); foldCode(err) != "fold_depth" {
		t.Errorf("nesting 1200 deep: %v", err)
	}
	mustFold(t, deep, FoldOptions{Op: refDef, Evaluate: resolving, MaxDepth: 1300})
	cyclic := map[string]any{"a": map[string]any{}}
	cyclic["a"].(map[string]any)["self"] = cyclic["a"]
	if _, err := Fold(cyclic, FoldOptions{Op: refDef, Evaluate: resolving}); foldCode(err) != "fold_depth" {
		t.Errorf("a Go value that contains itself: %v", err)
	}
}

func TestFoldACycleErrorEscapesMemberNamesAndBoundsTheMessage(t *testing.T) {
	name := "l1\nl2\u001b[31m" + strings.Repeat("K", 300)
	_, err := Fold(om(name, om("$ref", "self")), FoldOptions{Op: refDef, Cycle: "error",
		Evaluate: func(site *Site, _ *Op, _ []any) (any, error) {
			_, err := site.At(Path{name})
			return nil, err
		}})
	var fe *FoldError
	if !errors.As(err, &fe) || fe.Code != "fold_cycle" || fe.At != "/"+name {
		t.Fatalf("%v", err)
	}
	if strings.ContainsAny(fe.Error(), "\n\u001b") || len(fe.Error()) >= 220 {
		t.Errorf("message %q (%d)", fe.Error(), len(fe.Error()))
	}
}

// A Go map has no member order, so map[string]any and MapRef are walked in
// code-point order (Go's byte order on UTF-8), each folds into a new map
// of its own kind, and the siblings of a site are an OrderedMap in that
// order. An OrderedMap keeps its own order, and its Sorted flag.
func TestFoldMapsWithNoOrderGoInCodePointOrder(t *testing.T) {
	var order []string
	note := func(site *Site, _ *Op, terms []any) (any, error) {
		order = append(order, fmt.Sprint(site.Path()))
		return terms[1], nil
	}
	plain := map[string]any{
		"z": map[string]any{"$ref": 1.0}, "a": map[string]any{"$ref": 2.0},
		"é": map[string]any{"$ref": 3.0}, "\U0001F600": map[string]any{"$ref": 4.0},
		"s": map[string]any{"$ref": 5.0, "y": 1.0, "b": 2.0},
	}
	out := mustFold(t, plain, FoldOptions{Op: refDef, Evaluate: note})
	if want := []string{"[a]", "[s]", "[z]", "[é]", "[\U0001F600]"}; !reflect.DeepEqual(order, want) {
		t.Errorf("order %q, want %q", order, want)
	}
	folded, isPlain := out.(map[string]any)
	if !isPlain {
		t.Fatalf("a map[string]any folds into %T", out)
	}
	if siblings := folded["s"].(*tabnas.OrderedMap); !reflect.DeepEqual(siblings.Keys, []string{"b", "y"}) {
		t.Errorf("siblings %v", siblings.Keys)
	}

	ref := tabnas.MapRef{Val: map[string]any{"b": 1.0, "a": tabnas.MapRef{Val: map[string]any{"$ref": "#/b"}}}, Implicit: true, Meta: map[string]any{"m": 1}}
	got := mustFold(t, ref, FoldOptions{Op: refDef, Evaluate: resolving})
	mr, isRef := got.(tabnas.MapRef)
	if !isRef || !mr.Implicit || mr.Val["a"] != 1.0 || mr.Val["b"] != 1.0 || mr.Meta["m"] != 1 {
		t.Errorf("a MapRef folds into %#v", got)
	}
	if reflect.ValueOf(mr.Val).Pointer() == reflect.ValueOf(ref.Val).Pointer() {
		t.Error("the MapRef's map is the input's")
	}

	sorted := tabnas.NewSortedMap()
	sorted.Set("b", 1.0)
	sorted.Set("a", om("$ref", "#/b"))
	gotSorted := mustFold(t, sorted, FoldOptions{Op: refDef, Evaluate: resolving}).(*tabnas.OrderedMap)
	if !gotSorted.Sorted || cj(t, gotSorted) != `{"a":1,"b":1}` {
		t.Errorf("a sorted map folds into %s (sorted %v)", cj(t, gotSorted), gotSorted.Sorted)
	}
}

// One instance folds parses from several goroutines at once: each parse
// keeps its positions in its own context, and the fold's state is its own.
// Run with -race to check the claim rather than the values.
func TestFoldDeferredParsesConcurrently(t *testing.T) {
	j := treeParser(t, map[string]interface{}{
		"op": noDefaults(refOp()),
		"fold": FoldOptions{Positions: true, Evaluate: func(site *Site, op *Op, terms []any) (any, error) {
			if site.Token == nil {
				return nil, errors.New("no token")
			}
			return resolving(site, op, terms)
		}},
	})
	recorded := RecordPositions(j)
	done := make(chan error)
	for g := 0; g < 8; g++ {
		go func(g int) {
			for i := 0; i < 40; i++ {
				src := fmt.Sprintf("{a:{$ref:\"#/b\"},b:{n:%d}}", g*1000+i)
				value, err := j.Parse(src)
				if err == nil {
					want := fmt.Sprintf(`{"a":{"n":%d},"b":{"n":%d}}`, g*1000+i, g*1000+i)
					if got, cerr := canonicalJSON(value); cerr != nil || got != want {
						err = fmt.Errorf("%s: %s", src, got)
					}
					_ = recorded(value)
				}
				if err != nil {
					done <- err
					return
				}
			}
			done <- nil
		}(g)
	}
	for g := 0; g < 8; g++ {
		if err := <-done; err != nil {
			t.Error(err)
		}
	}
}

// What the fold reads off a parse: a key operator registers no token, so
// $ref stays a key, and an operator node is what IsOp recognises.
func TestFoldReadsTheParse(t *testing.T) {
	j := treeParser(t, map[string]interface{}{"op": refOp()})
	raw := parsed(t, j, `{$ref:1+2}`)
	if !IsOp(member(raw, "$ref")) {
		t.Errorf("$ref holds %T", member(raw, "$ref"))
	}
	if j.FixedSrc("$ref") != 0 {
		t.Error("a key operator registered a fixed token")
	}
}
