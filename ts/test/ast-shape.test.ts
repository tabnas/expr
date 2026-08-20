/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

import { describe, test } from 'node:test'
import assert from 'node:assert'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'

import { Expr } from '..'

// The TypeScript twin of go/ast_shape_test.go. Together they pin a
// DIVERGENCE, not a contract — see DIVERGENCE.md: the two ports serialise
// the parsed AST to different shapes, and closing that is a breaking
// change to the Go port's public types.
//
// Pinned on BOTH sides so the record cannot outlive what it records:
// repairing either port turns that port's test red and forces the pair
// and the DIVERGENCE.md entry to be deleted together (ADR-14).
//
// Measured on `{a:1+2}`, the simplest expression that shows it:
//
//   TypeScript  a -> [ {src:"+", left:…, name:"addition-infix", …} ]
//   Go          a -> { Val: [ {Name:"addition-infix", Src:"+", …} ], … }
describe('AST shape', () => {
  test('this port yields the term list directly, where Go wraps it', () => {
    const tn = new Tabnas().use(jsonic).use(Expr)
    const out: any = tn.parse('{a:1+2}')

    const a = out?.a
    assert.notEqual(a, undefined, 'no `a` key, so this test proves nothing')

    // 1. No wrapper. Go yields { Val: [...], Child, Implicit, Meta }.
    assert.ok(Array.isArray(a),
      '`a` is not an array — if this port has grown Go\'s wrapper, delete ' +
      'this test, its twin in go/ast_shape_test.go, and the ' +
      'DIVERGENCE.md entry together')
    assert.notEqual(a.length, 0,
      'no terms, so the naming check below would pass vacuously')

    // 2. Lower-case naming. Go's structs carry no json tags, so it emits
    // the exported Go names.
    const op = a[0]
    assert.ok('name' in op,
      'the first term has no `name` key — see the note above')
    assert.equal('Name' in op, false,
      'the first term has a PascalCase `Name` key, so this port now ' +
      'matches Go — see the note above')
  })
})
