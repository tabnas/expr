// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package main

// expr's own contract test, kept out of core_test.go, which is stamped from
// admin's tasks/clib-template and would be overwritten by a restamp. A
// dangling operator is accepted with its missing operand absent, as in
// TypeScript, and the value is finite: the README's format note says so.

import (
	"reflect"
	"testing"
)

func TestDanglingOperatorsReturnFiniteValues(t *testing.T) {
	h := loadHandle(t)
	defer freeGrammar(h)
	for source, expected := range map[string]any{
		"1+": []any{"+", float64(1)},
		"-":  []any{"-"},
	} {
		m := decode(t, parseWith(h, source))
		if m["accept"] != true {
			t.Fatalf("%q was not accepted: %v", source, m)
		}
		if _, has := m["valueError"]; has {
			t.Fatalf("%q returned valueError after cycle repair: %v", source, m)
		}
		if !reflect.DeepEqual(m["value"], expected) {
			t.Fatalf("%q value = %v, want %v", source, m["value"], expected)
		}
	}
}
