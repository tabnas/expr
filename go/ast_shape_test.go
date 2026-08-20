// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package tabnasexpr

import (
	"encoding/json"
	"testing"
)

// TestASTShapeDivergesFromTypeScript pins a DIVERGENCE, not a contract.
// See DIVERGENCE.md: this port's parsed AST serialises to a different
// shape from the TypeScript port's, and closing that is a breaking change
// to this port's public Go types.
//
// It is pinned on BOTH sides — the TypeScript twin is
// ts/test/ast-shape.test.ts — so the record cannot outlive the
// divergence: repairing either port turns that port's test red and forces
// the pair and the DIVERGENCE.md entry to be deleted together (ADR-14).
//
// Measured on `{a:1+2}`, the simplest expression that shows it:
//
//	TypeScript  a -> [ {src:"+", left:…, name:"addition-infix", …} ]
//	Go          a -> { Val: [ {Name:"addition-infix", Src:"+", …} ], … }
func TestASTShapeDivergesFromTypeScript(t *testing.T) {
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
		t.Fatal("no `a` key, so this test proves nothing")
	}

	// 1. The wrapper. TypeScript yields the term list directly.
	obj, isObj := a.(map[string]any)
	if !isObj {
		t.Fatalf("`a` is %T, not an object — if the wrapper has been "+
			"removed to match TypeScript, delete this test, its twin in "+
			"ts/test/ast-shape.test.ts, and the DIVERGENCE.md entry", a)
	}
	terms, hasVal := obj["Val"]
	if !hasVal {
		t.Error("`a` no longer carries Val — see the note above")
	}

	// 2. The naming. No json tags, so encoding/json emits the exported
	// Go names where TypeScript emits lower-case ones.
	list, isList := terms.([]any)
	if !isList || 0 == len(list) {
		t.Fatalf("Val is %T with no terms, so the naming check below "+
			"would pass vacuously", terms)
	}
	op, isOp := list[0].(map[string]any)
	if !isOp {
		t.Fatalf("first term is %T, not an object", list[0])
	}
	if _, pascal := op["Name"]; !pascal {
		t.Error("the first term has no `Name` key — if json tags have " +
			"been added to match TypeScript's `name`, delete this test, " +
			"its twin, and the DIVERGENCE.md entry together")
	}
	if _, lower := op["name"]; lower {
		t.Error("the first term has a lower-case `name` key, so this port " +
			"now agrees with TypeScript — see the note above")
	}
}
