package tabnasexpr

import (
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	support "github.com/tabnas/support/go"
)

// The render, run: trees written as expression documents by the parts the
// package carries (Translate), the embedding and then the render, as a host
// composes them for a plain tree, through the `alchemy` command
// (tabnas/alchemy-cli), which this module does not depend on. Set
// TABNAS_ALCHEMY to the command's path to run these tests; without it they
// skip, as the TypeScript half, ts/test/translate-render.test.ts, does.
//
// A Go host hands the render the tree SimplifyOrdered makes, since an
// event walker cannot read an *Op, and what the render writes reads back
// through SimplifyOrdered as that tree, under the conventions the
// manifest's loss list declares. A list led by an object whose src a
// reader would read as an operator that is no default one has no document
// that reads back as it, and the render refuses it with
// TARGET_VALUE_UNREPRESENTABLE, naming where.

// alchemyCommand is the alchemy command TABNAS_ALCHEMY names, and skips
// the test where it names none.
func alchemyCommand(t *testing.T) string {
	t.Helper()
	path := os.Getenv("TABNAS_ALCHEMY")
	if path == "" {
		t.Skip("TABNAS_ALCHEMY does not name the alchemy command")
	}
	return path
}

// rendered is what the render does with a tree: the document it writes,
// or the failure the command reports.
type rendered struct {
	Text    string
	Code    string `json:"code"`
	Message string `json:"message"`
}

// renderJSON runs the embedding and the render over a tree's JSON text, as
// a host composes them for a plain tree.
func renderJSON(t *testing.T, alchemy, tree string) rendered {
	t.Helper()
	parts := Translate()
	dir := t.TempDir()
	program := filepath.Join(dir, "translate.alc")
	source := parts.Render.Source + "\n" + parts.Embed.Source +
		"\ndef export [input] (expr-render (expr-embed input))\n"
	if err := os.WriteFile(program, []byte(source), 0o600); err != nil {
		t.Fatal(err)
	}
	input := filepath.Join(dir, "tree.json")
	if err := os.WriteFile(input, []byte(tree), 0o600); err != nil {
		t.Fatal(err)
	}
	var stdout, stderr strings.Builder
	cmd := exec.Command(alchemy, "run", program, input)
	cmd.Stdout, cmd.Stderr = &stdout, &stderr
	if err := cmd.Run(); err == nil {
		return rendered{Text: stdout.String()}
	}
	var out rendered
	if err := json.Unmarshal([]byte(stderr.String()), &out); err != nil {
		t.Fatalf("the command failed without a JSON failure: %s", stderr.String())
	}
	return out
}

// writtenText is the text the render writes for a tree, and fails the test
// where the render refuses it.
func writtenText(t *testing.T, alchemy string, tree interface{}) string {
	t.Helper()
	data, err := json.Marshal(tree)
	if err != nil {
		t.Fatal(err)
	}
	out := renderJSON(t, alchemy, string(data))
	if out.Code != "" {
		t.Fatalf("%s is refused: %s: %s", data, out.Code, out.Message)
	}
	return out.Text
}

// plain is a value as JSON reads it back, so that two trees compare by
// their members and not by the types or the order that hold them.
func plain(t *testing.T, value interface{}) interface{} {
	t.Helper()
	data, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	var out interface{}
	if err := json.Unmarshal(data, &out); err != nil {
		t.Fatal(err)
	}
	return out
}

// readBack reads a document as the reader with the default operators reads
// it, the reader the render writes for.
func readBack(t *testing.T, text string) interface{} {
	t.Helper()
	value, err := Parse(text)
	if err != nil {
		t.Fatalf("%s: %v", text, err)
	}
	return plain(t, SimplifyOrdered(value))
}

// reading is what the reader reads back from a document the render wrote
// of a plain tree, by the loss list: a negative number as the operator -
// applied to its magnitude. JSON carries no number that is not finite.
func reading(value interface{}) interface{} {
	switch v := value.(type) {
	case float64:
		if v < 0 {
			return []interface{}{"-", -v}
		}
	case []interface{}:
		out := make([]interface{}, len(v))
		for i, item := range v {
			out[i] = reading(item)
		}
		return out
	case map[string]interface{}:
		out := make(map[string]interface{}, len(v))
		for k, member := range v {
			out[k] = reading(member)
		}
		return out
	}
	return value
}

// refusesAt checks that the render refuses a tree, naming the object that
// leads a list and its src.
func refusesAt(t *testing.T, alchemy, tree, at, src string) {
	t.Helper()
	out := renderJSON(t, alchemy, tree)
	if out.Code != "TARGET_VALUE_UNREPRESENTABLE" {
		t.Fatalf("%s: the render gave %+v", tree, out)
	}
	lead := "the object at " + at + " is the first element of a list and has the src " + src + ", "
	if !strings.HasPrefix(out.Message, lead) {
		t.Errorf("%s: %q does not begin %q", tree, out.Message, lead)
	}
}

// cIntX is the tree tabnas-c's reader builds for `int x;`, as alchemy-cli's
// `translate --from c` reads it: the children of a node begin with a token,
// an object whose src is the token's text.
const cIntX = `{"kind":"translation_unit","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"external_declaration","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"declaration_specifiers","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"token","tname":"KW_INT","src":"int","span":{"start":0,"end":3,"line":1,"col":1}}],"trivia":{"leading":[],"trailing":[]}},{"kind":"init_declarator_list","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"init_declarator","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"declarator","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"direct_declarator","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"token","tname":"ID","src":"x","span":{"start":4,"end":5,"line":1,"col":5}}],"trivia":{"leading":[],"trailing":[]},"declaredName":"x"}],"trivia":{"leading":[],"trailing":[]},"declaredName":"x"}],"trivia":{"leading":[],"trailing":[]},"declaredName":"x"}],"trivia":{"leading":[],"trailing":[]}},{"kind":"token","tname":"PUNC_SEMI","src":";","span":{"start":5,"end":6,"line":1,"col":6}}],"trivia":{"leading":[],"trailing":[]},"declKind":"declaration","viaPath":"grammar"}],"trivia":{"leading":[],"trailing":[]}}`

func TestRenderRefusesACSyntaxTree(t *testing.T) {
	refusesAt(t, alchemyCommand(t), cIntX, ".children[0].children[0].children[0]", `"int"`)
}

// The reader keeps the src of an object at a list's head alone wherever it
// would read one: the canonical any src that is not empty, zero, false or
// null, as the render writes it, so a negative number, written with its
// sign, is an operation there, a list.
func TestRenderRefusesAListLedByAnObjectWithASrcTheReaderReads(t *testing.T) {
	alchemy := alchemyCommand(t)
	cases := []struct{ tree, at, src string }{
		{`[{"kind":"token","src":"x"},1]`, ".[0]", `"x"`},
		{`{"a":[1,{"b c":[{"src":"q"}]}]}`, `.a[1]."b c"[0]`, `"q"`},
		{`["+",[{"src":"int"},1],2]`, ".[1][0]", `"int"`},
		{`[[0,1,2,3,4,5,6,7,8,9,10,[{"src":"k"}]]]`, ".[0][11][0]", `"k"`},
		{`[{"src":"NaN"}]`, ".[0]", `"NaN"`},
		{`[{"src":1},2]`, ".[0]", "1"},
		{`[{"src":-1}]`, ".[0]", "-1"},
		{`[{"src":true}]`, ".[0]", "true"},
		{`[{"src":{"a":1}}]`, ".[0]", `{"a":1}`},
		{`[{"src":[]}]`, ".[0]", "[]"},
	}
	for _, c := range cases {
		refusesAt(t, alchemy, c.tree, c.at, c.src)
	}
}

func TestRenderWritesAnObjectWhoseSrcNoReaderReads(t *testing.T) {
	alchemy := alchemyCommand(t)
	for _, tree := range []string{
		`[{"a":1,"src":""},1]`,
		`[{"src":0},1]`,
		`[{"src":false},1]`,
		`[{"src":null},1]`,
		`{"a":{"src":"x"}}`,
		`[1,{"src":"x"}]`,
	} {
		var value interface{}
		if err := json.Unmarshal([]byte(tree), &value); err != nil {
			t.Fatal(err)
		}
		if got := readBack(t, writtenText(t, alchemy, value)); !reflect.DeepEqual(got, value) {
			t.Errorf("%s reads back as %v", tree, got)
		}
	}
}

func TestRenderWritesEveryDefaultOperator(t *testing.T) {
	alchemy := alchemyCommand(t)
	for _, src := range []string{
		"1+2-3*4/5%6", "-(1+2)*+3", "()", "(1)", "8/2%3-1", "--1", "-+1",
		"1-(2-3)", "(1+2)*(3-4)/(5%6)", "{a:1+2,b:[3*4,(5)]}",
	} {
		tree := readBack(t, src)
		if got := readBack(t, writtenText(t, alchemy, tree)); !reflect.DeepEqual(got, tree) {
			t.Errorf("%s: %v reads back as %v", src, tree, got)
		}
	}
}

// Nothing the reader builds is refused: every shared fixture row's value,
// as the reader builds it with its file's operators (the row's expected
// cell), is written in one list after a null, which makes the list no
// operation, and reads back as itself through the reader the render writes
// for, the default operators'.
func TestRenderRefusesNothingTheReaderBuilds(t *testing.T) {
	alchemy := alchemyCommand(t)
	dir, err := support.FindSpecDir("")
	if err != nil {
		t.Fatal(err)
	}
	files, err := support.LoadSpecDir(dir, nil)
	if err != nil {
		t.Fatal(err)
	}
	values := []interface{}{nil}
	for _, file := range files {
		for _, row := range file.Rows {
			var value interface{}
			if err := json.Unmarshal([]byte(row.Col(1)), &value); err != nil {
				t.Fatalf("%s: %v", row.Where(), err)
			}
			values = append(values, value)
		}
	}
	if got, want := readBack(t, writtenText(t, alchemy, values)), reading(plain(t, values)); !reflect.DeepEqual(got, want) {
		t.Errorf("the shared fixtures' values do not read back as themselves")
	}
}
