/* Copyright (c) 2021-2025 Richard Rodger and other contributors, MIT License */

import { describe, test, beforeEach } from 'node:test'

import { Jsonic, util } from 'jsonic'

import {
  Expr,
} from '..'

import { loadSpec, expect } from './spec-util'


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
      (null != x && 'object' === typeof (x) ? omap(x, ([n, v]) => [n, S(v, seen)]) : x)))

const mj =
  (je: Jsonic) => (s: string, m?: any) => C(S(je(s, m)))

const _mo_ = 'equal'


function runSpec(specName: string, j: (s: string) => any) {
  const entries = loadSpec(specName)
  for (const entry of entries) {
    expect(j(entry.input))[_mo_](entry.expected)
  }
}


describe('spec', () => {

  beforeEach(() => {
    global.console = require('console')
  })


  test('happy', () => {
    const j = mj(Jsonic.make().use(Expr))
    runSpec('happy.tsv', j)
  })


  test('binary', () => {
    const j = mj(Jsonic.make().use(Expr))
    runSpec('binary.tsv', j)
  })


  test('structure', () => {
    const j = mj(Jsonic.make().use(Expr))
    runSpec('structure.tsv', j)
  })


  test('unary-prefix-basic', () => {
    const j = mj(Jsonic.make().use(Expr))
    runSpec('unary-prefix-basic.tsv', j)
  })


  test('unary-prefix-edge', () => {
    const je = Jsonic.make().use(Expr, {
      op: {
        at: { prefix: true, right: 15000, src: '@' },
        tight: { infix: true, left: 120_000, right: 130_000, src: '~' },
      }
    })
    const j = mj(je)
    runSpec('unary-prefix-edge.tsv', j)
  })


  test('unary-suffix-basic', () => {
    const je = Jsonic.make().use(Expr, {
      op: {
        factorial: { suffix: true, left: 15000, src: '!' },
        question: { suffix: true, left: 13000, src: '?' },
      }
    })
    const j = mj(je)
    runSpec('unary-suffix-basic.tsv', j)
  })


  test('unary-suffix-edge', () => {
    const je = Jsonic.make().use(Expr, {
      op: {
        factorial: { suffix: true, left: 15000, src: '!' },
        question: { suffix: true, left: 13000, src: '?' },
        tight: { infix: true, left: 120_000, right: 130_000, src: '~' },
      }
    })
    const j = mj(je)
    runSpec('unary-suffix-edge.tsv', j)
  })


  test('unary-suffix-structure', () => {
    const je = Jsonic.make().use(Expr, {
      op: {
        factorial: { suffix: true, left: 15000, src: '!' },
        question: { suffix: true, left: 13000, src: '?' },
      }
    })
    const j = mj(je)
    runSpec('unary-suffix-structure.tsv', j)
  })


  test('unary-suffix-prefix', () => {
    const je = Jsonic.make().use(Expr, {
      op: {
        factorial: { suffix: true, left: 15000, src: '!' },
        question: { suffix: true, left: 13000, src: '?' },
      }
    })
    const j = mj(je)
    runSpec('unary-suffix-prefix.tsv', j)
  })


  test('unary-suffix-paren', () => {
    const je = Jsonic.make().use(Expr, {
      op: {
        factorial: { suffix: true, left: 15000, src: '!' },
        question: { suffix: true, left: 13000, src: '?' },
      }
    })
    const j = mj(je)
    runSpec('unary-suffix-paren.tsv', j)
  })


  test('paren-basic', () => {
    const j = mj(Jsonic.make().use(Expr))
    runSpec('paren-basic.tsv', j)
  })


  test('implicit-list-top-basic', () => {
    const j = mj(Jsonic.make().use(Expr))
    runSpec('implicit-list-top-basic.tsv', j)
  })


  test('ternary-basic', () => {
    const je = Jsonic.make().use(Expr, {
      op: {
        factorial: { suffix: true, src: '!', left: 15000 },
        ternary: { ternary: true, src: ['?', ':'] },
      }
    })
    const j = mj(je)
    runSpec('ternary-basic.tsv', j)
  })


  test('ternary-implicit-list', () => {
    const je = Jsonic.make().use(Expr, {
      op: {
        factorial: { suffix: true, src: '!', left: 15000 },
        ternary: { ternary: true, src: ['?', ':'] },
      }
    })
    const j = mj(je)
    runSpec('ternary-implicit-list.tsv', j)
  })


  test('ternary-paren-preval', () => {
    const je = Jsonic.make().use(Expr, {
      op: {
        ternary: { ternary: true, src: ['?', ':'] },
        plain: { paren: true, osrc: '(', csrc: ')', preval: { active: true } },
      }
    })
    const j = mj(je)
    runSpec('ternary-paren-preval.tsv', j)
  })


  test('ternary-many-2', () => {
    const je = Jsonic.make().use(Expr, {
      op: {
        foo: { ternary: true, src: ['?', ':'] },
        bar: { ternary: true, src: ['QQ', 'CC'] },
      }
    })
    const j = mj(je)
    runSpec('ternary-many-2.tsv', j)
  })


  test('ternary-many-3', () => {
    const je = Jsonic.make().use(Expr, {
      op: {
        foo: { ternary: true, src: ['?', ':'] },
        bar: { ternary: true, src: ['QQ', 'CC'] },
        zed: { ternary: true, src: ['%%', '@@'] },
      }
    })
    const j = mj(je)
    runSpec('ternary-many-3.tsv', j)
  })


  test('paren-preval-chain', () => {
    const je = Jsonic.make().use(Expr, {
      op: {
        index: { paren: true, osrc: '[', csrc: ']', preval: { required: true } },
        call: { paren: true, osrc: '(', csrc: ')', preval: { active: true } },
        plain: null as any,
      }
    })
    const j = mj(je)
    runSpec('paren-preval-chain.tsv', j)
  })


  test('json-base', () => {
    const j = mj(Jsonic.make().use(Expr))
    runSpec('json-base.tsv', j)
  })


  test('implicit-list-top-paren', () => {
    const j = mj(Jsonic.make().use(Expr))
    runSpec('implicit-list-top-paren.tsv', j)
  })


  test('paren-implicit-list', () => {
    const j = mj(Jsonic.make().use(Expr))
    runSpec('paren-implicit-list.tsv', j)
  })


  test('paren-implicit-map', () => {
    const j = mj(Jsonic.make().use(Expr))
    runSpec('paren-implicit-map.tsv', j)
  })


  test('map-implicit-list-paren', () => {
    const j = mj(Jsonic.make().use(Expr))
    runSpec('map-implicit-list-paren.tsv', j)
  })


  test('paren-map-implicit-structure-comma', () => {
    const j = mj(Jsonic.make().use(Expr))
    runSpec('paren-map-implicit-structure-comma.tsv', j)
  })


  test('paren-map-implicit-structure-space', () => {
    const j = mj(Jsonic.make().use(Expr))
    runSpec('paren-map-implicit-structure-space.tsv', j)
  })


  test('paren-list-implicit-structure-comma', () => {
    const j = mj(Jsonic.make().use(Expr))
    runSpec('paren-list-implicit-structure-comma.tsv', j)
  })


  test('paren-list-implicit-structure-space', () => {
    const j = mj(Jsonic.make().use(Expr))
    runSpec('paren-list-implicit-structure-space.tsv', j)
  })


  test('jsonic-base', () => {
    const j = mj(Jsonic.make().use(Expr))
    runSpec('jsonic-base.tsv', j)
  })


  test('add-infix', () => {
    const je = Jsonic.make().use(Expr, {
      op: {
        foo: { infix: true, left: 180, right: 190, src: 'foo' },
      }
    })
    const j = mj(je)
    runSpec('add-infix.tsv', j)
  })


  test('add-paren', () => {
    const je = Jsonic.make().use(Expr, {
      op: {
        angle: { paren: true, osrc: '<', csrc: '>' },
      }
    })
    const j = mj(je)
    runSpec('add-paren.tsv', j)
  })


  test('paren-preval-basic', () => {
    const je = Jsonic.make().use(Expr, {
      op: {
        angle: { osrc: '<', csrc: '>', paren: true, preval: { active: true } },
      }
    })
    const j = mj(je)
    runSpec('paren-preval-basic.tsv', j)
  })


  test('paren-preval-overload', () => {
    const je = Jsonic.make().use(Expr, {
      op: {
        factorial: { suffix: true, left: 15000, src: '!' },
        square: { osrc: '[', csrc: ']', paren: true, preval: { required: true } },
        brace: { osrc: '{', csrc: '}', paren: true, preval: { required: true } },
      }
    })
    const j = mj(je)
    runSpec('paren-preval-overload.tsv', j)
  })


  test('paren-preval-implicit', () => {
    const je = Jsonic.make().use(Expr, {
      op: {
        plain: { preval: true },
      }
    })
    const j = mj(je)
    runSpec('paren-preval-implicit.tsv', j)
  })


  test('infix-in-paren-map', () => {
    const j = mj(Jsonic.make().use(Expr))
    runSpec('infix-in-paren-map.tsv', j)
  })


  test('evaluate-math', () => {
    // Math expression grammar with evaluate callback.
    // Tests the full pipeline: parse → S-expression → evaluate → result.
    // This catches bugs where nested/chained expressions produce
    // intermediate results instead of the final computed value.
    // Includes: +, -, *, /, prefix negation, suffix factorial (!),
    // and function parens min(x,y), max(x,y).
    const factorial = (n: number): number => n <= 1 ? 1 : n * factorial(n - 1)

    const je = Jsonic.make().use(Expr, {
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
    const j = (s: string) => C(je(s))
    runSpec('evaluate-math.tsv', j)
  })

})
