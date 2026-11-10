/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

package tabnasexpr

// tree_spec_test.go runs the tree layer's fixtures, test/tree/*.tsv, as
// ts/test/tree-spec.test.ts does: each file folds the parse of its input
// with its own operator table and resolver, ported here exactly, and every
// cell is the folded value's canonical JSON, byte for byte. The files sit
// beside test/spec/, not in it, until the Rust port runs them too.

import (
	"fmt"
	"math"
	"os"
	"path/filepath"
	"strings"
	"testing"

	jsonic "github.com/tabnas/jsonic/go"
	tabnas "github.com/tabnas/parser/go"
	support "github.com/tabnas/support/go"
)

// treeDir is test/tree, beside the shared fixtures.
func treeDir(t *testing.T) string {
	t.Helper()
	dir, err := support.FindSpecDir("")
	if err != nil {
		t.Fatal(err)
	}
	return filepath.Join(dir, "..", "tree")
}

// runTree runs one tree fixture through run: structurally, through the
// shared runner, and then every value row against its cell, byte for byte.
func runTree(t *testing.T, specName string, run func(string) (any, error)) {
	t.Helper()
	path := filepath.Join(treeDir(t), specName)

	support.Runner{Parse: run, Normalize: tabnas.Plainify}.File(t, path)

	spec, err := support.LoadSpec(path, nil)
	if err != nil {
		t.Fatal(err)
	}
	for _, row := range spec.Rows {
		if strings.HasPrefix(row.Col(1), "ERROR") {
			continue
		}
		value, err := run(row.Unesc(0))
		if err != nil {
			t.Errorf("%s: %v", row.Where(), err)
			continue
		}
		got, err := canonicalJSON(value)
		if err != nil {
			t.Errorf("%s: %v", row.Where(), err)
			continue
		}
		if got != row.Col(1) {
			t.Errorf("%s: not the canonical JSON\n  got      %s\n  expected %s", row.Where(), got, row.Col(1))
		}
	}
}

// fragPointer is a `#/...` fragment as path segments (RFC 6901), as the
// TypeScript runner's pointer reads one.
func fragPointer(frag string) Path {
	r := []rune(frag)
	if len(r) <= 1 {
		return Path{}
	}
	path := Path{}
	for _, seg := range strings.Split(string(r[2:]), "/") {
		path = append(path, strings.ReplaceAll(strings.ReplaceAll(seg, "~1", "/"), "~0", "~"))
	}
	return path
}

// mergeOver is the TypeScript `{ ...value, ...siblings }`: a new map with
// value's members, then the siblings', a sibling taking an existing
// member's value in that member's place.
func mergeOver(value any, siblings *tabnas.OrderedMap) *tabnas.OrderedMap {
	out := tabnas.NewOrderedMap()
	for _, name := range recordNames(value) {
		v, _ := recordGet(value, name)
		out.Set(name, v)
	}
	for _, name := range siblings.Keys {
		out.Set(name, siblings.Vals[name])
	}
	return out
}

// resolving is the resolver the pointer fixtures use: a `#/...` target
// folded through site.At, kept verbatim when it names nothing or is a
// cycle, with the site's own members laid over a map target.
func resolving(site *Site, op *Op, terms []any) (any, error) {
	if !op.Key {
		return math.NaN(), nil
	}
	ref, ok := terms[0].(string)
	if !ok {
		return nil, fmt.Errorf("resolving: a $ref of %T", terms[0])
	}
	hit, err := site.At(fragPointer(ref))
	if err != nil {
		return nil, err
	}
	if !hit.Found || hit.Cycle {
		return site.Node, nil
	}
	if shapeOf(hit.Value) != shapeRecord {
		return hit.Value, nil
	}
	return mergeOver(hit.Value, terms[1].(*tabnas.OrderedMap)), nil
}

// noDefaults removes every default operator, as the TypeScript NO_DEFAULTS.
func noDefaults(op map[string]interface{}) map[string]interface{} {
	out := map[string]interface{}{
		"positive": nil, "negative": nil, "addition": nil, "subtraction": nil,
		"multiplication": nil, "division": nil, "remainder": nil, "plain": nil,
	}
	for name, def := range op {
		out[name] = def
	}
	return out
}

// treeParser is jsonic with the plugin installed on these options.
func treeParser(t *testing.T, opts map[string]interface{}) *jsonic.Jsonic {
	t.Helper()
	j := jsonic.Make()
	if err := j.Use(Expr, opts); err != nil {
		t.Fatal(err)
	}
	return j
}

func foldParse(j *jsonic.Jsonic, opts FoldOptions) func(string) (any, error) {
	return func(src string) (any, error) {
		raw, err := j.Parse(src)
		if err != nil {
			return nil, err
		}
		return Fold(raw, opts)
	}
}

func refOp() map[string]interface{} {
	return map[string]interface{}{"ref": map[string]interface{}{"key": true, "src": "$ref"}}
}

var refDef = map[string]*OpDef{"ref": {Key: true, Src: "$ref"}}

// A recording resolver: a site reads as [src, value, siblings].
func TestTreeKeyBasic(t *testing.T) {
	op := map[string]interface{}{
		"ref": map[string]interface{}{"key": true, "src": "$ref"},
		"zed": map[string]interface{}{"key": true, "src": "$zed"},
		"dyn": map[string]interface{}{"key": true, "src": "$dyn", "dynamic": true},
	}
	j := treeParser(t, map[string]interface{}{"op": noDefaults(op)})
	opts := FoldOptions{
		Op: map[string]*OpDef{
			"ref": {Key: true, Src: "$ref"},
			"zed": {Key: true, Src: "$zed"},
			"dyn": {Key: true, Src: "$dyn", Dynamic: true},
		},
		Evaluate: func(_ *Site, op *Op, terms []any) (any, error) {
			return []any{op.Src, terms[0], terms[1]}, nil
		},
	}
	runTree(t, "key-basic.tsv", foldParse(j, opts))
}

var atPointerOp = map[string]*OpDef{
	"ref": {Key: true, Src: "$ref"},
	"dyn": {Key: true, Src: "$dynamicRef", Dynamic: true},
}

func atPointerOptions() map[string]interface{} {
	return noDefaults(map[string]interface{}{
		"ref": map[string]interface{}{"key": true, "src": "$ref"},
		"dyn": map[string]interface{}{"key": true, "src": "$dynamicRef", "dynamic": true},
	})
}

func TestTreeAtPointer(t *testing.T) {
	j := treeParser(t, map[string]interface{}{"op": atPointerOptions()})
	runTree(t, "at-pointer.tsv", foldParse(j, FoldOptions{Op: atPointerOp, Evaluate: resolving}))
}

// Deferred mode gives the same value as the standalone fold, row for row
// of the same file.
func TestTreeAtPointerDeferred(t *testing.T) {
	j := treeParser(t, map[string]interface{}{
		"op":   atPointerOptions(),
		"fold": FoldOptions{Evaluate: resolving},
	})
	runTree(t, "at-pointer.tsv", j.Parse)
}

// Token operators and key operators reduce in one fold.
func TestTreeTokens(t *testing.T) {
	j := treeParser(t, map[string]interface{}{"op": refOp()})
	opts := FoldOptions{
		Op: refDef,
		Evaluate: func(site *Site, op *Op, terms []any) (any, error) {
			if op.Key {
				return resolving(site, op, terms)
			}
			a, aok := terms[0].(float64)
			b, bok := terms[1].(float64)
			switch {
			case aok && bok && op.Src == "+":
				return a + b, nil
			case aok && bok && op.Src == "*":
				return a * b, nil
			}
			return math.NaN(), nil
		},
	}
	runTree(t, "tokens.tsv", foldParse(j, opts))
}

// baseInherit sets the bag's base from a map's own base member.
func baseInherit(_ *Site, node any, k map[string]any) (map[string]any, error) {
	base, ok := recordGet(node, "base")
	if text, isText := base.(string); ok && isText {
		next := map[string]any{}
		for name, v := range k {
			next[name] = v
		}
		next["base"] = text
		return next, nil
	}
	return k, nil
}

// The inherited bag: a base member sets the bag for its subtree, and a
// site reached early through a reference gets its own ancestors'.
func TestTreeInherit(t *testing.T) {
	j := treeParser(t, map[string]interface{}{"op": noDefaults(refOp())})
	opts := FoldOptions{
		Op:      refDef,
		Inherit: baseInherit,
		Evaluate: func(site *Site, _ *Op, terms []any) (any, error) {
			if ref, ok := terms[0].(string); ok && strings.HasPrefix(ref, "#/") {
				hit, err := site.At(fragPointer(ref))
				return hit.Value, err
			}
			k, err := site.K()
			if err != nil {
				return nil, err
			}
			return []any{terms[0], k["base"]}, nil
		},
	}
	runTree(t, "inherit.tsv", foldParse(j, opts))
}

// The fold's own failures, pinned by code: a cycle under Cycle "error",
// and nesting past MaxDepth (3 here).
func TestTreeErrors(t *testing.T) {
	j := treeParser(t, map[string]interface{}{"op": noDefaults(refOp())})
	opts := FoldOptions{Op: refDef, Evaluate: resolving, Cycle: "error", MaxDepth: 3}
	runTree(t, "errors.tsv", foldParse(j, opts))
}

// Every file in test/tree is named by a runTree call in this file, as
// TestEveryFixtureRowIsHeldToItsCanonicalJSON checks test/spec, and the
// row total is ratcheted at what is on disk, so a corpus that shrinks
// cannot pass by measuring less.
func TestEveryTreeFixtureIsRun(t *testing.T) {
	files, err := support.LoadSpecDir(treeDir(t), nil)
	if err != nil {
		t.Fatal(err)
	}
	source, err := os.ReadFile("tree_spec_test.go")
	if err != nil {
		t.Fatal(err)
	}
	rows := 0
	for _, file := range files {
		rows += len(file.Rows)
		if !strings.Contains(string(source), `runTree(t, "`+file.Name+`"`) {
			t.Errorf("tree fixture with no runner: %s", file.Name)
		}
	}
	if rows != 52 {
		t.Errorf("the tree fixtures hold %d rows, not the 52 measured", rows)
	}
}
