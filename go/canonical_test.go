/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

package tabnasexpr

// canonical_test.go — SimplifyOrdered against the canonical JSON.
//
// Every shared fixture row's `expected` cell is exactly what the canonical
// JSON.stringify writes for the TypeScript value, and ts/test/spec.test.ts
// holds each cell to it. runSpec holds SimplifyOrdered's value to the same
// cells byte for byte (holdToCanonicalJSON, in expr_test.go), as
// rs/tests/parity_test.rs does for the Rust port. This file carries the
// writer that makes that comparison possible, the gate that every row went
// through it, and the pins DIVERGENCE.md names.

import (
	"fmt"
	"math"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"testing"

	jsonic "github.com/tabnas/jsonic/go"
	tabnas "github.com/tabnas/parser/go"
	support "github.com/tabnas/support/go"
)

// canonicalJSON writes a value as JSON.stringify writes it: members in
// order, nothing escaped that JSON.stringify leaves alone (encoding/json
// escapes <, > and & and the line and paragraph separators), a number spelt
// as JavaScript spells it, and a number no JSON can hold as null. A plain
// map has no order, so its keys are sorted, which is what makes a value
// that lost its order fail against a cell that kept it.
func canonicalJSON(value interface{}) (string, error) {
	var b strings.Builder
	if err := writeCanonical(&b, value); err != nil {
		return "", err
	}
	return b.String(), nil
}

func writeCanonical(b *strings.Builder, value interface{}) error {
	switch v := value.(type) {
	case nil:
		b.WriteString("null")
	case bool:
		b.WriteString(strconv.FormatBool(v))
	case float64:
		b.WriteString(canonicalNumber(v))
	case int:
		b.WriteString(canonicalNumber(float64(v)))
	case int64:
		b.WriteString(canonicalNumber(float64(v)))
	case string:
		writeCanonicalString(b, v)
	case []interface{}:
		b.WriteByte('[')
		for i, item := range v {
			if i > 0 {
				b.WriteByte(',')
			}
			if err := writeCanonical(b, item); err != nil {
				return err
			}
		}
		b.WriteByte(']')
	case *tabnas.OrderedMap:
		return writeMembers(b, v.Keys, func(k string) interface{} { return v.Vals[k] })
	case map[string]interface{}:
		keys := make([]string, 0, len(v))
		for k := range v {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		return writeMembers(b, keys, func(k string) interface{} { return v[k] })
	default:
		return fmt.Errorf("canonicalJSON: a simplified value holds no %T", value)
	}
	return nil
}

func writeMembers(b *strings.Builder, keys []string, get func(string) interface{}) error {
	b.WriteByte('{')
	for i, k := range keys {
		if i > 0 {
			b.WriteByte(',')
		}
		writeCanonicalString(b, k)
		b.WriteByte(':')
		if err := writeCanonical(b, get(k)); err != nil {
			return err
		}
	}
	b.WriteByte('}')
	return nil
}

// canonicalNumber is ECMA-262 Number::toString: the shortest digits that
// read back as the number, in fixed form from 1e-6 up to 1e21 and in
// exponent form, with an unpadded exponent, outside it. A negative zero is
// 0, and JSON.stringify writes NaN and the infinities as null.
func canonicalNumber(f float64) string {
	if math.IsNaN(f) || math.IsInf(f, 0) {
		return "null"
	}
	if f == 0 {
		return "0"
	}
	if abs := math.Abs(f); abs < 1e-6 || abs >= 1e21 {
		mantissa, exponent, _ := strings.Cut(strconv.FormatFloat(f, 'e', -1, 64), "e")
		return mantissa + "e" + exponent[:1] + strings.TrimLeft(exponent[1:], "0")
	}
	return strconv.FormatFloat(f, 'f', -1, 64)
}

// writeCanonicalString is JSON.stringify's QuoteJSONString for well-formed
// text: the quote, the backslash and the control characters are escaped,
// and nothing else is.
func writeCanonicalString(b *strings.Builder, s string) {
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
				fmt.Fprintf(b, `\u%04x`, r)
			} else {
				b.WriteRune(r)
			}
		}
	}
	b.WriteByte('"')
}

func TestCanonicalJSONSpellsAsJavaScriptDoes(t *testing.T) {
	cases := []struct {
		in   interface{}
		want string
	}{
		{1.0, "1"},
		{math.Copysign(0, -1), "0"},
		{1.5, "1.5"},
		{-2.25, "-2.25"},
		{1e20, "100000000000000000000"},
		{1e21, "1e+21"},
		{0.000001, "0.000001"},
		{1e-7, "1e-7"},
		{-1.25e-7, "-1.25e-7"},
		{math.NaN(), "null"},
		{"<&> \"\\\n\x01", `"<&>` + " " + `\"\\\n\u0001"`},
	}
	for _, c := range cases {
		got, err := canonicalJSON(c.in)
		if err != nil || got != c.want {
			t.Errorf("canonicalJSON(%#v) = %s, %v; want %s", c.in, got, err, c.want)
		}
	}
}

// Every row of every fixture goes through holdToCanonicalJSON: each file
// through its TestSpec function, and every file is named by one, which
// this checks by reading the test sources, as rs/tests/parity_test.rs
// does. The total is ratcheted at what is on disk, so a corpus that shrinks
// cannot pass by measuring less.
func TestEveryFixtureRowIsHeldToItsCanonicalJSON(t *testing.T) {
	dir, err := support.FindSpecDir("")
	if err != nil {
		t.Fatal(err)
	}
	files, err := support.LoadSpecDir(dir, nil)
	if err != nil {
		t.Fatal(err)
	}
	source, err := os.ReadFile("expr_test.go")
	if err != nil {
		t.Fatal(err)
	}
	rows := 0
	for _, file := range files {
		rows += len(file.Rows)
		if !strings.Contains(string(source), `runSpec(t, "`+file.Name+`"`) {
			t.Errorf("no TestSpec runs %s", filepath.Join(dir, file.Name))
		}
	}
	if rows != 1130 {
		t.Errorf("the shared fixtures hold %d rows, not the 1130 measured", rows)
	}
}

// simplified parses src with the default operators and reduces it both ways.
func simplified(t *testing.T, src string) (plain, ordered string) {
	t.Helper()
	j := jsonic.Make()
	if err := j.Use(Expr, nil); err != nil {
		t.Fatal(err)
	}
	value, err := j.Parse(src)
	if err != nil {
		t.Fatalf("%s: %v", src, err)
	}
	plain, err = canonicalJSON(Simplify(value))
	if err != nil {
		t.Fatal(err)
	}
	ordered, err = canonicalJSON(SimplifyOrdered(value))
	if err != nil {
		t.Fatal(err)
	}
	return plain, ordered
}

// SimplifyOrdered reads each of these as the TypeScript and Rust simplify
// do (ts/test/expr.test.ts and rs/tests/expr_test.rs pin the same values).
func TestSimplifyOrderedIsTheCanonicalReading(t *testing.T) {
	for _, c := range []struct{ src, want string }{
		{"{b:1,a:2+3}", `{"b":1,"a":["+",2,3]}`},
		{"1+null", `["+",1,null]`},
		{"[{src:x},1]", `["x",1]`},
	} {
		if _, got := simplified(t, c.src); got != c.want {
			t.Errorf("%s: SimplifyOrdered gives %s, want %s", c.src, got, c.want)
		}
	}
}

// A node inside itself reads as a circle, and a node reached twice by two
// paths does not.
func TestSimplifyOrderedReportsANodeInsideItself(t *testing.T) {
	plus := &Op{Src: "+", Infix: true, Terms: 2}
	loop := []interface{}{plus, 1.0, nil}
	loop[2] = loop
	if got, _ := canonicalJSON(SimplifyOrdered(loop)); got != `["+",1,"[CIRCLE]"]` {
		t.Errorf("a list inside itself: %s", got)
	}
	shared := []interface{}{plus, 1.0, 2.0}
	twice := []interface{}{plus, shared, shared}
	if got, _ := canonicalJSON(SimplifyOrdered(twice)); got != `["+",["+",1,2],["+",1,2]]` {
		t.Errorf("a list reached twice: %s", got)
	}
	// A shorter view of a list's own array is another list, not a circle.
	view := []interface{}{"a", nil}
	view[1] = view[:1]
	if got, _ := canonicalJSON(SimplifyOrdered(view)); got != `["a",["a"]]` {
		t.Errorf("a prefix view inside its list: %s", got)
	}
}

// Every list comes back new, an empty one included: appending to the
// result never writes into the parse's array, even where that array has
// room to spare.
func TestSimplifyOrderedSharesNoListWithTheParse(t *testing.T) {
	inner := make([]interface{}, 0, 4)
	reduced := SimplifyOrdered([]interface{}{inner}).([]interface{})
	_ = append(reduced[0].([]interface{}), "x")
	if got := inner[:1][0]; got != nil {
		t.Errorf("appending to the result wrote %v into the parse", got)
	}
}

// The pin for DIVERGENCE.md's Simplify entry: Simplify loses the member
// order, drops a null term and keeps a src-headed list as data. This fails
// when Simplify is repaired, which is the signal to delete the entry, its
// rows here, and the TypeScript and Rust pins of the canonical values.
func TestSimplifyDivergesFromTheCanonical(t *testing.T) {
	for _, c := range []struct{ src, want string }{
		{"{b:1,a:2+3}", `{"a":["+",2,3],"b":1}`},
		{"1+null", `["+",1]`},
		{"[{src:x},1]", `[{"src":"x"},1]`},
	} {
		if got, _ := simplified(t, c.src); got != c.want {
			t.Errorf("%s: Simplify gives %s where it was measured giving %s", c.src, got, c.want)
		}
	}
}
