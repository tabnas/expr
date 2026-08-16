/* Copyright (c) 2021-2025 Richard Rodger and other contributors, MIT License */

import { describe, test, beforeEach } from 'node:test'
import Path from 'node:path'

import { Tabnas, util } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'

import { findSpecDir, makeRunner } from '@tabnas/support'

import {
  Expr,
} from '..'


const { omap } = util

const C = (x: any) => JSON.parse(JSON.stringify(x))

const S = (x: any, seen?: WeakSet<any>): any => (
  seen = seen ?? new WeakSet(),
  seen?.has(x) ? '[CIRCLE]' : (
    (x && 'object' === typeof x ? seen?.add(x) : null),
    (x && Array.isArray(x)) ?
      (0 === x.length ? x : [
        x[0].src || S(x[0], seen),
        ...(1 < x.length ? (x.slice(1).map((t: any) => S(t, seen))) : [])]
        .filter(t => undefined !== t)) :
      (null != x && 'object' === typeof (x) ? omap(x, ([n, v]: [any, any]) => [n, S(v, seen)]) : x)))

const mj =
  (je: Tabnas) => (s: string, m?: any) => C(S(je.parse(s, m)))


// The fixtures live at the repo root in `test/spec/*.tsv` and are read by
// @tabnas/support, whose Go half `go/expr_test.go` uses to run the SAME
// files — so the two implementations cannot drift without one going red,
// and neither can the two loaders.
//
// What varies per case is the CONFIGURED PARSER, which cannot live in an
// `opts` column: several fixtures need operators defined in the plugin's
// options. So each test builds its own and hands it here, and each ROW
// becomes its own test case rather than one assertion inside a per-file
// test.
const SPEC = findSpecDir(__dirname)

function runSpec(specName: string, j: (s: string) => any) {
  makeRunner({ parse: (input) => j(input) })
    .file(Path.join(SPEC, specName))
}


describe('spec', () => {

  beforeEach(() => {
    global.console = require('console')
  })


  describe('happy', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('happy.tsv', j)
  })


  describe('binary', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('binary.tsv', j)
  })


  describe('arithmetic-mixed', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('arithmetic-mixed.tsv', j)
  })


  describe('prefix-infix-mixed', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('prefix-infix-mixed.tsv', j)
  })


  describe('paren-deep-nest', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('paren-deep-nest.tsv', j)
  })


  describe('structure-arith', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('structure-arith.tsv', j)
  })


  describe('structure', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('structure.tsv', j)
  })


  describe('unary-prefix-basic', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('unary-prefix-basic.tsv', j)
  })


  describe('unary-prefix-edge', () => {
    const je = new Tabnas().use(jsonic).use(Expr, {
      op: {
        at: { prefix: true, right: 5000000, src: '@' },
        tight: { infix: true, left: 7000000, right: 7100000, src: '~' },
      }
    })
    const j = mj(je)
    runSpec('unary-prefix-edge.tsv', j)
  })


  describe('unary-suffix-basic', () => {
    const je = new Tabnas().use(jsonic).use(Expr, {
      op: {
        factorial: { suffix: true, left: 6000000, src: '!' },
        question: { suffix: true, left: 3500000, src: '?' },
      }
    })
    const j = mj(je)
    runSpec('unary-suffix-basic.tsv', j)
  })


  describe('unary-suffix-arith', () => {
    const je = new Tabnas().use(jsonic).use(Expr, {
      op: {
        factorial: { suffix: true, left: 6000000, src: '!' },
        question: { suffix: true, left: 3500000, src: '?' },
      }
    })
    const j = mj(je)
    runSpec('unary-suffix-arith.tsv', j)
  })


  describe('unary-suffix-edge', () => {
    const je = new Tabnas().use(jsonic).use(Expr, {
      op: {
        factorial: { suffix: true, left: 6000000, src: '!' },
        question: { suffix: true, left: 3500000, src: '?' },
        tight: { infix: true, left: 7000000, right: 7100000, src: '~' },
      }
    })
    const j = mj(je)
    runSpec('unary-suffix-edge.tsv', j)
  })


  describe('unary-suffix-structure', () => {
    const je = new Tabnas().use(jsonic).use(Expr, {
      op: {
        factorial: { suffix: true, left: 6000000, src: '!' },
        question: { suffix: true, left: 3500000, src: '?' },
      }
    })
    const j = mj(je)
    runSpec('unary-suffix-structure.tsv', j)
  })


  describe('unary-suffix-prefix', () => {
    const je = new Tabnas().use(jsonic).use(Expr, {
      op: {
        factorial: { suffix: true, left: 6000000, src: '!' },
        question: { suffix: true, left: 3500000, src: '?' },
      }
    })
    const j = mj(je)
    runSpec('unary-suffix-prefix.tsv', j)
  })


  describe('unary-suffix-paren', () => {
    const je = new Tabnas().use(jsonic).use(Expr, {
      op: {
        factorial: { suffix: true, left: 6000000, src: '!' },
        question: { suffix: true, left: 3500000, src: '?' },
      }
    })
    const j = mj(je)
    runSpec('unary-suffix-paren.tsv', j)
  })


  describe('paren-basic', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('paren-basic.tsv', j)
  })


  describe('implicit-list-top-basic', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('implicit-list-top-basic.tsv', j)
  })


  describe('ternary-basic', () => {
    const je = new Tabnas().use(jsonic).use(Expr, {
      op: {
        factorial: { suffix: true, src: '!', left: 6000000 },
        ternary: { ternary: true, src: ['?', ':'] },
      }
    })
    const j = mj(je)
    runSpec('ternary-basic.tsv', j)
  })


  describe('ternary-implicit-list', () => {
    const je = new Tabnas().use(jsonic).use(Expr, {
      op: {
        factorial: { suffix: true, src: '!', left: 6000000 },
        ternary: { ternary: true, src: ['?', ':'] },
      }
    })
    const j = mj(je)
    runSpec('ternary-implicit-list.tsv', j)
  })


  describe('ternary-paren-preval', () => {
    const je = new Tabnas().use(jsonic).use(Expr, {
      op: {
        ternary: { ternary: true, src: ['?', ':'] },
        plain: { paren: true, osrc: '(', csrc: ')', preval: { active: true } },
      }
    })
    const j = mj(je)
    runSpec('ternary-paren-preval.tsv', j)
  })


  describe('ternary-many-2', () => {
    const je = new Tabnas().use(jsonic).use(Expr, {
      op: {
        foo: { ternary: true, src: ['?', ':'] },
        bar: { ternary: true, src: ['QQ', 'CC'] },
      }
    })
    const j = mj(je)
    runSpec('ternary-many-2.tsv', j)
  })


  describe('ternary-many-3', () => {
    const je = new Tabnas().use(jsonic).use(Expr, {
      op: {
        foo: { ternary: true, src: ['?', ':'] },
        bar: { ternary: true, src: ['QQ', 'CC'] },
        zed: { ternary: true, src: ['%%', '@@'] },
      }
    })
    const j = mj(je)
    runSpec('ternary-many-3.tsv', j)
  })


  describe('paren-preval-chain', () => {
    const je = new Tabnas().use(jsonic).use(Expr, {
      op: {
        index: { paren: true, osrc: '[', csrc: ']', preval: { required: true } },
        call: { paren: true, osrc: '(', csrc: ')', preval: { active: true } },
        plain: null as any,
      }
    })
    const j = mj(je)
    runSpec('paren-preval-chain.tsv', j)
  })


  describe('json-base', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('json-base.tsv', j)
  })


  describe('implicit-list-top-paren', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('implicit-list-top-paren.tsv', j)
  })


  describe('paren-implicit-list', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('paren-implicit-list.tsv', j)
  })


  describe('paren-implicit-map', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('paren-implicit-map.tsv', j)
  })


  describe('map-implicit-list-paren', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('map-implicit-list-paren.tsv', j)
  })


  describe('paren-map-implicit-structure-comma', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('paren-map-implicit-structure-comma.tsv', j)
  })


  describe('paren-map-implicit-structure-space', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('paren-map-implicit-structure-space.tsv', j)
  })


  describe('paren-list-implicit-structure-comma', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('paren-list-implicit-structure-comma.tsv', j)
  })


  describe('paren-list-implicit-structure-space', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('paren-list-implicit-structure-space.tsv', j)
  })


  describe('jsonic-base', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('jsonic-base.tsv', j)
  })


  describe('add-infix', () => {
    const je = new Tabnas().use(jsonic).use(Expr, {
      op: {
        foo: { infix: true, left: 3500000, right: 3600000, src: 'foo' },
      }
    })
    const j = mj(je)
    runSpec('add-infix.tsv', j)
  })


  describe('add-paren', () => {
    const je = new Tabnas().use(jsonic).use(Expr, {
      op: {
        angle: { paren: true, osrc: '<', csrc: '>' },
      }
    })
    const j = mj(je)
    runSpec('add-paren.tsv', j)
  })


  describe('paren-preval-basic', () => {
    const je = new Tabnas().use(jsonic).use(Expr, {
      op: {
        angle: { osrc: '<', csrc: '>', paren: true, preval: { active: true } },
      }
    })
    const j = mj(je)
    runSpec('paren-preval-basic.tsv', j)
  })


  describe('paren-preval-overload', () => {
    const je = new Tabnas().use(jsonic).use(Expr, {
      op: {
        factorial: { suffix: true, left: 6000000, src: '!' },
        square: { osrc: '[', csrc: ']', paren: true, preval: { required: true } },
        brace: { osrc: '{', csrc: '}', paren: true, preval: { required: true } },
      }
    })
    const j = mj(je)
    runSpec('paren-preval-overload.tsv', j)
  })


  describe('paren-preval-implicit', () => {
    const je = new Tabnas().use(jsonic).use(Expr, {
      op: {
        plain: { preval: true },
      }
    })
    const j = mj(je)
    runSpec('paren-preval-implicit.tsv', j)
  })


  describe('infix-in-paren-map', () => {
    const j = mj(new Tabnas().use(jsonic).use(Expr))
    runSpec('infix-in-paren-map.tsv', j)
  })


  describe('evaluate-math', () => {
    // Math expression grammar with evaluate callback.
    // Tests the full pipeline: parse → S-expression → evaluate → result.
    // This catches bugs where nested/chained expressions produce
    // intermediate results instead of the final computed value.
    // Includes: +, -, *, /, prefix negation, suffix factorial (!),
    // and function parens min(x,y), max(x,y).
    const factorial = (n: number): number => n <= 1 ? 1 : n * factorial(n - 1)

    const je = new Tabnas().use(jsonic).use(Expr, {
      op: {
        addition:       { infix: true, src: '+', left: 140, right: 150 },
        subtraction:    { infix: true, src: '-', left: 140, right: 150 },
        multiplication: { infix: true, src: '*', left: 160, right: 170 },
        division:       { infix: true, src: '/', left: 160, right: 170 },
        negative:       { prefix: true, src: '-', right: 200 },
        positive:       { prefix: true, src: '+', right: 200 },
        factorial:      { suffix: true, src: '!', left: 300 },
        func:           { paren: true, preval: { active: true }, osrc: '(', csrc: ')' },
      },
      evaluate: (_r: any, _ctx: any, op: any, terms: any) => {
        switch (op.name) {
          case 'addition-infix': return terms[0] + terms[1]
          case 'subtraction-infix': return terms[0] - terms[1]
          case 'multiplication-infix': return terms[0] * terms[1]
          case 'division-infix': return terms[0] / terms[1]
          case 'negative-prefix': return -terms[0]
          case 'positive-prefix': return +terms[0]
          case 'factorial-suffix': return factorial(terms[0])
          case 'func-paren': {
            const fname = terms[0]
            if (typeof fname === 'string') {
              // Preval function call: terms = [fname, [arg1, arg2, ...]] or [fname, arg1]
              const rawArgs = terms.slice(1)
              // Flatten: args may be wrapped in an array (implicit list)
              const args = rawArgs.length === 1 && Array.isArray(rawArgs[0])
                ? rawArgs[0] : rawArgs
              if (fname === 'min') return Math.min(...args)
              if (fname === 'max') return Math.max(...args)
              return args[0]
            }
            // Plain parens (no preval) — return inner value
            return fname
          }
          case 'plain-paren': return terms[0]
          default: return terms[0]
        }
      },
    })
    const j = (s: string) => C(je.parse(s))
    runSpec('evaluate-math.tsv', j)
  })

})
