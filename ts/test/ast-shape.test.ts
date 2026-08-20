/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

import { describe, test } from 'node:test'
import assert from 'node:assert'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'

import { Expr } from '..'

// The TypeScript twins of go/expr_test.go's TestASTShape* pair. Together
// they pin a DIVERGENCE, not a contract — see DIVERGENCE.md.
//
// TWO pins, deliberately separate. The halves can be repaired
// independently: adding json tags on the Go side closes the naming half
// and leaves the Val wrapper. One test covering both would fail on that
// partial repair and, following its own instruction, take the record for
// a still-live wrapper divergence with it.
//
// Both sides measure the SERIALISED shape. This file round-trips through
// JSON rather than inspecting the live object, because the divergence and
// the Go twin are about what a consumer serialises — if these objects ever
// gain `toJSON` emitting Go-compatible keys, inspecting the live object
// would keep passing while the consumer-visible difference was gone.
function astShapeDoc(): any {
  const tn = new Tabnas().use(jsonic).use(Expr)
  const out = JSON.parse(JSON.stringify(tn.parse('{a:1+2}')))
  assert.notEqual(out?.a, undefined,
    'no `a` key, so these tests prove nothing')
  return out.a
}

describe('AST shape', () => {
  // The FIRST half: this port yields the term list directly, Go wraps it
  // under Val.
  test('yields the term list directly, where Go wraps it', () => {
    const a = astShapeDoc()
    assert.ok(Array.isArray(a),
      'serialised `a` is not an array. If this port has grown Go\'s ' +
      'wrapper, delete THIS test, its twin in go/expr_test.go, and the ' +
      'wrapper paragraph of the DIVERGENCE.md entry — leave the naming ' +
      'pin and paragraph alone unless that half was repaired too')
  })

  // The SECOND half: lower-case keys, where Go emits the exported Go names.
  test('serialises lower-case keys, where Go emits Go names', () => {
    const a = astShapeDoc()
    assert.ok(Array.isArray(a) && 0 !== a.length,
      'no terms, so the checks below would pass vacuously')
    const op = a[0]
    assert.ok('name' in op, 'the first term has no `name` key')
    assert.equal('Name' in op, false,
      'the first term has a PascalCase `Name` key, so this port now ' +
      'matches Go. Delete THIS test, its twin, and the naming paragraph ' +
      'of the DIVERGENCE.md entry — leave the wrapper pin and paragraph ' +
      'alone unless that half was repaired too')
  })
})
