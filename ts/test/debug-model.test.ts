// @ts-nocheck
/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

/*  debug-model.test.ts
 *  Composition test: the expr grammar plugin layered with the official
 *  @tabnas/debug plugin, asserting the structured grammar returned by
 *  debug.model(). @tabnas/debug is a devDependency, but this resolves it
 *  dynamically and SKIPS when it is absent so the suite stays runnable
 *  outside the package; set TABNAS_DEBUG_PATH to point at a sibling
 *  checkout's built plugin.
 */

import { describe, test, beforeEach } from 'node:test'
import assert from 'node:assert'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'

import { Expr } from '..'

function loadDebug() {
  const candidates = [process.env.TABNAS_DEBUG_PATH, '@tabnas/debug'].filter(
    Boolean,
  )
  for (const c of candidates) {
    try {
      return require(c).Debug
    } catch {
      /* try next */
    }
  }
  return null
}

const Debug = loadDebug()
const skip = Debug
  ? false
  : '@tabnas/debug not available (set TABNAS_DEBUG_PATH)'

describe('debug-model', () => {
  beforeEach(() => {
    global.console = require('console')
  })

  test('parses normally with the debug plugin installed', { skip }, () => {
    const tn = new Tabnas().use(jsonic).use(Expr)
    tn.use(Debug, { print: false, trace: false })
    // `1+2*3` parses to a precedence-correct expr tree: + at the root with a
    // nested * sub-expression. Each node is `[op, ...terms]` where op.src is
    // the operator source; assert the shape without coupling to op internals.
    const tree = tn.parse('1+2*3')
    assert.equal(tree[0].src, '+')
    assert.equal(tree[1], 1)
    assert.equal(tree[2][0].src, '*')
    assert.equal(tree[2][1], 2)
    assert.equal(tree[2][2], 3)
  })

  test('debug.model() returns the structured expr grammar', { skip }, () => {
    const tn = new Tabnas().use(jsonic).use(Expr)
    tn.use(Debug, { print: false, trace: false })
    const m = tn.debug.model()

    // The structured rule set: the shared json rules plus expr's own
    // `expr` and `paren` rules.
    assert.deepStrictEqual(m.rules.map((r) => r.name).sort(), [
      'elem',
      'expr',
      'list',
      'map',
      'pair',
      'paren',
      'val',
    ])

    // The entry rule of the grammar.
    assert.equal(m.config.start, 'val')

    // The expr plugin is registered.
    assert.ok(
      m.plugins.some((p) => p.name === 'Expr'),
      'plugins should list Expr',
    )

    // val is a choice whose open alts push the expr rule (operator entry)
    // as well as the shared map and list rules.
    const val = m.rules.find((r) => r.name === 'val')
    assert.ok(
      val.open.some((a) => a.push === 'expr'),
      'val should push expr',
    )
    assert.ok(
      val.open.some((a) => a.push === 'map'),
      'val should push map',
    )
    assert.ok(
      val.open.some((a) => a.push === 'list'),
      'val should push list',
    )

    // The expr rule pushes paren (for `(...)` grouping) and back into val
    // (to parse operand sub-values).
    const expr = m.rules.find((r) => r.name === 'expr')
    assert.ok(
      expr.open.some((a) => a.push === 'paren'),
      'expr should push paren',
    )
    assert.ok(
      expr.open.some((a) => a.push === 'val'),
      'expr should push val',
    )

    // The paren rule re-enters val to parse the grouped expression.
    const paren = m.rules.find((r) => r.name === 'paren')
    assert.ok(
      paren.open.some((a) => a.push === 'val'),
      'paren should push val',
    )

    // The model's rule names and entry rule survive JSON serialisation
    // (the structural skeleton round-trips; per-rule action functions do
    // not, so we compare names rather than whole rule objects).
    const round = JSON.parse(JSON.stringify({ rules: m.rules, config: m.config }))
    assert.deepStrictEqual(
      round.rules.map((r) => r.name).sort(),
      ['elem', 'expr', 'list', 'map', 'pair', 'paren', 'val'],
    )
    assert.equal(round.config.start, 'val')
  })
})
