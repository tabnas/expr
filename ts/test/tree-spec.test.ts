/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// The tree layer's fixtures, `test/tree/*.tsv`. They are kept beside,
// not inside, `test/spec/`, because that directory is the three-runtime
// parity contract and the Go and Rust ports do not carry `fold` yet; the
// files move into `test/spec/` with the ports. Each file folds the parse
// of its input with its own operator table and resolver, and the cell is
// the folded value's canonical JSON, byte for byte, as the other fixtures
// are held.

import { describe, test, beforeEach } from 'node:test'
import assert from 'node:assert'
import Fs from 'node:fs'
import Path from 'node:path'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'

import { findSpecDir, loadSpec, makeRunner } from '@tabnas/support'

import { Expr, fold } from '..'
import type { FoldOptions, Site, Op } from '..'


const TREE = Path.join(findSpecDir(__dirname), '..', 'tree')

const C = (x: any) => JSON.parse(JSON.stringify(x))

// A `#/...` fragment as path segments (RFC 6901).
const pointer = (frag: string) =>
  '' === frag.slice(1) ? [] :
    frag.slice(2).split('/').map((p) => p.replace(/~1/g, '/').replace(/~0/g, '~'))

const registered = new Set<string>()

function runTree(specName: string, run: (s: string) => any) {
  registered.add(specName)
  makeRunner({ parse: (input) => run(input) }).file(Path.join(TREE, specName))

  test(`${specName}: every cell is the canonical JSON, byte for byte`, () => {
    for (const row of loadSpec(Path.join(TREE, specName)).rows) {
      assert.equal(JSON.stringify(run(row.unesc(0))), row.col(1), row.where())
    }
  })
}

// The resolver the pointer fixtures use: a `#/...` target folded through
// `site.at`, kept verbatim when it names nothing or is a cycle, with the
// site's own members laid over an object target.
const resolving = (site: Site, op: Op, [value, siblings]: any[]) => {
  if (!op.key) return NaN
  const hit = site.at(pointer(value))
  if (!hit.found || hit.cycle) return site.node
  if (null == hit.value || 'object' !== typeof hit.value || Array.isArray(hit.value)) return hit.value
  return { ...hit.value, ...siblings }
}

const NO_DEFAULTS = {
  positive: null, negative: null, addition: null, subtraction: null,
  multiplication: null, division: null, remainder: null, plain: null,
}


describe('tree', () => {

  beforeEach(() => {
    global.console = require('console')
  })


  // A recording resolver: a site reads as `[src, value, siblings]`.
  describe('key-basic', () => {
    const op = { ref: { key: true, src: '$ref' }, other: { key: true, src: '$other' } }
    const tn = new Tabnas().use(jsonic).use(Expr, { op: { ...NO_DEFAULTS, ...op } })
    const opts: FoldOptions = { op, evaluate: (_site, op, [value, siblings]) => [op.src, value, siblings] }
    runTree('key-basic.tsv', (s) => C(fold(tn.parse(s), opts)))
  })


  describe('at-pointer', () => {
    const op = { ref: { key: true, src: '$ref' }, dyn: { key: true, src: '$dynamicRef', dynamic: true } }
    const tn = new Tabnas().use(jsonic).use(Expr, { op: { ...NO_DEFAULTS, ...op } })
    const opts: FoldOptions = { op, evaluate: resolving }
    runTree('at-pointer.tsv', (s) => C(fold(tn.parse(s), opts)))
  })


  // Deferred mode gives the same value as the standalone fold, row for
  // row of the same file.
  describe('at-pointer, deferred', () => {
    const op = { ref: { key: true, src: '$ref' }, dyn: { key: true, src: '$dynamicRef', dynamic: true } }
    const tn = new Tabnas().use(jsonic).use(Expr, { op: { ...NO_DEFAULTS, ...op }, fold: { evaluate: resolving } })
    runTree('at-pointer.tsv', (s) => C(tn.parse(s)))
  })


  // Token operators and key operators reduce in one fold.
  describe('tokens', () => {
    const op = { ref: { key: true, src: '$ref' } }
    const tn = new Tabnas().use(jsonic).use(Expr, { op })
    const opts: FoldOptions = {
      op,
      evaluate: (site, op, terms) =>
        op.key ? resolving(site, op, terms)
          : '+' === op.src ? terms[0] + terms[1]
            : '*' === op.src ? terms[0] * terms[1] : NaN,
    }
    runTree('tokens.tsv', (s) => C(fold(tn.parse(s), opts)))
  })


  // The inherited bag: a `base` member sets the bag for its subtree, and
  // a site reached early through a reference gets its own ancestors'.
  describe('inherit', () => {
    const op = { ref: { key: true, src: '$ref' } }
    const tn = new Tabnas().use(jsonic).use(Expr, { op: { ...NO_DEFAULTS, ...op } })
    const opts: FoldOptions = {
      op,
      inherit: (_site, node, k) => 'string' === typeof node.base ? { ...k, base: node.base } : k,
      evaluate: (site, _op, [value]) =>
        'string' === typeof value && value.startsWith('#/')
          ? site.at(pointer(value)).value
          : [value, site.k.base],
    }
    runTree('inherit.tsv', (s) => C(fold(tn.parse(s), opts)))
  })


  test('every tree fixture is run', () => {
    const files = Fs.readdirSync(TREE).filter((name) => name.endsWith('.tsv'))
    const unrun = files.filter((name) => !registered.has(name))
    assert.deepEqual(unrun, [], 'tree fixtures with no runner: ' + unrun.join(', '))
  })
})
