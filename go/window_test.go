package tabnasexpr

import (
	"testing"

	jsonic "github.com/tabnas/jsonic/go"
	tabnas "github.com/tabnas/parser/go"
)

// TestMakeJsonicDeclaresNoWindow: the parser MakeJsonic builds declares
// lex.window false, its own setting, so that ParseReader reads a reader
// whole: the plugin's matchers are not yet verified at the window's edge.
func TestMakeJsonicDeclaresNoWindow(t *testing.T) {
	if MakeJsonic().Config().Window {
		t.Error("MakeJsonic declares lex.window")
	}
	if MakeJsonic(map[string]interface{}{"evaluate": nil}).Config().Window {
		t.Error("MakeJsonic with options declares lex.window")
	}
}

// TestMakeJsonicOnAWindowedBase: the parser's own setting wins over its
// base's. A base that declares lex.window, as jsonic's own constructor
// may, given the constructor's steps, comes out with the window off.
func TestMakeJsonicOnAWindowedBase(t *testing.T) {
	yes := true
	base := jsonic.Make()
	base.SetOptions(tabnas.Options{Lex: &tabnas.LexOptions{Window: &yes}})
	if !base.Config().Window {
		t.Fatal("the base does not declare lex.window")
	}
	j := makeJsonicOn(base, nil)
	if j.Config().Window {
		t.Error("the base's lex.window reaches the expr parser")
	}
	if got, err := j.Parse("1+2*3"); err != nil {
		t.Errorf("parse: %v", err)
	} else if got == nil {
		t.Error("parse gave nothing")
	}
}
