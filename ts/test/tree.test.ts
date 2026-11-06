/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

import { describe, test } from 'node:test'
import assert from 'node:assert'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'

import { Expr, fold, evaluation, simplify, opify, isOp, keyOpOf, makeKeyMap } from '..'
import type { FoldOptions, FoldReport, Site, Op } from '..'


const C = (x: any) => JSON.parse(JSON.stringify(x))

const pointer = (frag: string) =>
  '' === frag.slice(1) ? [] :
    frag.slice(2).split('/').map((p) => p.replace(/~1/g, '/').replace(/~0/g, '~'))

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

const REF = { ref: { key: true, src: '$ref' } }

function parser() {
  return new Tabnas().use(jsonic).use(Expr, { op: { ...NO_DEFAULTS, ...REF } })
}


describe('fold', () => {

  test('leaves the raw tree as it was and returns a new one', () => {
    const raw = parser().parse('{a:{$ref:"#/b"},b:{c:1}}')
    const text = JSON.stringify(raw)
    const out = fold(raw, { op: REF, evaluate: resolving })
    assert.equal(JSON.stringify(raw), text)
    assert.notEqual(out, raw)
    assert.deepEqual(C(out), { a: { c: 1 }, b: { c: 1 } })
    assert.equal(null, Object.getPrototypeOf(out), 'the engine\'s null prototype is kept')
  })


  test('one target, one folded object, shared by every site', () => {
    const raw = parser().parse('{a:{$ref:"#/c"},b:{$ref:"#/c"},c:{d:1}}')
    const out: any = fold(raw, { op: REF, evaluate: (site, op, terms) => site.at(pointer(terms[0])).value })
    assert.equal(out.a, out.c)
    assert.equal(out.b, out.c)
  })


  test('a pointer that ends at a site, and one that passes through it', () => {
    const raw = parser().parse('{a:{$ref:"#/b"},b:{$ref:"#/c"},c:{d:{e:1}},f:{$ref:"#/b/d"}}')
    const out: any = fold(raw, { op: REF, evaluate: resolving })
    assert.deepEqual(C(out), { a: { d: { e: 1 } }, b: { d: { e: 1 } }, c: { d: { e: 1 } }, f: { e: 1 } })
  })


  test('cycles are reported and kept verbatim, or thrown', () => {
    const src = '{a:{$ref:"#/a"},b:{c:{$ref:"#/b"}},x:{$ref:"#/y"},y:{$ref:"#/x"},r:{$ref:"#"}}'
    let report: FoldReport | undefined
    const out = fold(parser().parse(src), { op: REF, evaluate: resolving, report: (r) => { report = r } })
    assert.deepEqual(C(out), C(parser().parse(src)))
    // A reference INTO a cycle is one: `x` names `y`, whose own resolution
    // met the cycle back to `x`, so both ring members stay verbatim.
    assert.deepEqual(report!.cycles.map((c) => c.at + '>' + c.target).sort(), [
      '/a>/a', '/b/c>/b', '/r>', '/x>/y', '/y>/x',
    ])
    assert.throws(
      () => fold(parser().parse('{a:{$ref:"#/a"}}'), { op: REF, evaluate: resolving, cycle: 'error' }),
      /cycle at \/a/)
  })


  test('a target that names nothing is reported and the site kept', () => {
    let report: FoldReport | undefined
    const out = fold(parser().parse('{a:{$ref:"#/nope/x"}}'), { op: REF, evaluate: resolving, report: (r) => { report = r } })
    assert.deepEqual(C(out), { a: { $ref: '#/nope/x' } })
    assert.deepEqual(report!.unresolved, [{ at: '/a', target: '/nope/x' }])
  })


  test('a dynamic key is reported and folded as a plain map', () => {
    const op = { ...REF, dyn: { key: true, src: '$dynamicRef', dynamic: true } }
    let report: FoldReport | undefined
    const tn = new Tabnas().use(jsonic).use(Expr, { op: { ...NO_DEFAULTS, ...op } })
    const out = fold(tn.parse('{a:{$dynamicRef:"#/b",c:{$ref:"#/b"}},b:1}'),
      { op, evaluate: resolving, report: (r) => { report = r } })
    assert.deepEqual(C(out), { a: { $dynamicRef: '#/b', c: 1 }, b: 1 })
    assert.deepEqual(report!.dynamic, [{ at: '/a', src: '$dynamicRef' }])
  })


  test('the inherited bag comes from the target\'s own ancestors', () => {
    // `d` is folded first and reaches `/b/c` through a reference; the bag
    // at `/b/c` is computed from `/b`, not from `d`'s ancestors.
    const raw = parser().parse('{d:{$ref:"#/b/c"},base:x,b:{base:y,c:{$ref:r}}}')
    const seen: string[] = []
    fold(raw, {
      op: REF,
      inherit: (_site, node, k) => 'string' === typeof node.base ? { ...k, base: node.base } : k,
      evaluate: (site, _op, [value]) => {
        if ('string' === typeof value && value.startsWith('#/')) return site.at(pointer(value)).value
        seen.push(site.path.join('/') + '=' + site.k.base)
        return value
      },
    })
    assert.deepEqual(seen, ['b/c=y'])
  })


  test('deferred mode folds the finished root, with positions', () => {
    const sites: string[] = []
    const tn = new Tabnas().use(jsonic).use(Expr, {
      op: { ...NO_DEFAULTS, ...REF },
      fold: {
        positions: true,
        evaluate: (site: Site, op: Op, terms: any[]) => {
          sites.push(site.path.join('/') + '@' + site.token?.rI + ':' + site.token?.cI)
          return resolving(site, op, terms)
        },
      },
    })
    const out = tn.parse('{\n  a: { $ref: "#/b" },\n  b: 1\n}')
    assert.deepEqual(C(out), { a: 1, b: 1 })
    assert.deepEqual(sites, ['a@2:6'])
    // A second parse on the same instance folds again, from scratch.
    assert.deepEqual(C(tn.parse('{c:{$ref:"#/d"},d:2}')), { c: 2, d: 2 })
  })


  test('eager token evaluation and the deferred fold compose', () => {
    const tn = new Tabnas().use(jsonic).use(Expr, {
      op: REF,
      evaluate: (_r: any, _c: any, op: Op, terms: any[]) => '+' === op.src ? terms[0] + terms[1] : NaN,
      fold: { evaluate: resolving },
    })
    assert.deepEqual(C(tn.parse('{a:1+2,b:{$ref:"#/a"}}')), { a: 3, b: 3 })
  })


  test('a throwing resolver surfaces from parse and the instance survives', () => {
    const tn = new Tabnas().use(jsonic).use(Expr, {
      op: { ...NO_DEFAULTS, ...REF },
      fold: { evaluate: () => { throw new Error('resolver: boom') } },
    })
    assert.throws(() => tn.parse('{a:{$ref:"#/b"},b:1}'), /resolver: boom/)
    assert.deepEqual(C(tn.parse('{b:1}')), { b: 1 })
  })


  test('a member named __proto__ is data in the folded tree', () => {
    const raw = parser().parse('{"__proto__":{x:1},a:{$ref:"#/__proto__"}}')
    const out: any = fold(raw, { op: REF, evaluate: resolving })
    assert.deepEqual(Object.keys(out), ['__proto__', 'a'])
    assert.deepEqual(C(out.a), { x: 1 })
  })


  test('the first declared key wins when a map owns two', () => {
    const keys = makeKeyMap({ one: { key: true, src: '$a' }, two: { key: true, src: '$b' } })
    assert.equal(keyOpOf(keys, { $b: 1, $a: 2 })!.src, '$a')
    assert.equal(keyOpOf(keys, { $b: 1 })!.name, 'two-key')
    assert.equal(keyOpOf(keys, [{ $a: 1 }]), undefined)
    assert.equal(keyOpOf(keys, 'x'), undefined)
  })
})


describe('evaluation over objects', () => {
  const math = (_r: any, _c: any, op: Op, terms: any[]) =>
    '+' === op.src ? terms[0] + terms[1] : NaN

  test('passes a plain object through by default, as documented', () => {
    const tree = new Tabnas().use(jsonic).use(Expr).parse('{a:1+2}')
    const out = evaluation(null as any, null as any, tree, math)
    assert.equal(out, tree)
    assert.deepEqual(C(simplify(out)), { a: ['+', 1, 2] })
  })

  test('descends into objects when asked, into a new object', () => {
    const tree = new Tabnas().use(jsonic).use(Expr).parse('{a:1+2,b:{c:[3+4]}}')
    const out = evaluation(null as any, null as any, tree, math, { objects: true })
    assert.notEqual(out, tree)
    assert.deepEqual(C(out), { a: 3, b: { c: [7] } })
    assert.deepEqual(C(simplify(tree)), { a: ['+', 1, 2], b: { c: [['+', 3, 4]] } }, 'the input is untouched')
  })

  test('a tree passed through once is still reducible', () => {
    const tree = new Tabnas().use(jsonic).use(Expr).parse('{a:1+2}')
    evaluation(null as any, null as any, tree, math)
    assert.deepEqual(C(evaluation(null as any, null as any, tree, math, { objects: true })), { a: 3 })
  })
})


describe('opify and isOp', () => {
  test('a hand-built operator node is recognised by isOp and evaluation', () => {
    const op = opify({ name: 'ref-key', src: '$ref', terms: 2, key: true })
    const node: any = [op, 'x', {}]
    assert.ok(isOp(node))
    assert.ok(!isOp(['$ref', 'x', {}]))
    assert.deepEqual(
      evaluation(null as any, null as any, node, (_r, _c, o, terms) => [o.src, ...terms]),
      ['$ref', 'x', {}])
  })
})


// Shapes the first review of the layer found wanting.
describe('fold: token terms, rings and a __proto__ key', () => {

  test('a site inside a token expression is folded at its own path', () => {
    const tn = new Tabnas().use(jsonic).use(Expr, { op: REF })
    const raw = tn.parse('{a:5,e:1+2*{$ref:"#/a"}}')
    const paths: any[] = []
    const out: any = fold(raw, {
      op: REF,
      evaluate: (site, op, terms) => {
        if (op.key) { paths.push(site.path); return site.at(pointer(terms[0])).value }
        return '+' === op.src ? terms[0] + terms[1] : terms[0] * terms[1]
      },
    })
    assert.deepEqual(paths, [['e', 2, 2]])
    assert.deepEqual(C(out), { a: 5, e: 11 })
  })


  test('a pointer into a ring is a cycle in every member order', () => {
    for (const src of [
      '{a:{$ref:"#/b/x"},b:{$ref:"#/c",x:7},c:{$ref:"#/b"}}',
      '{b:{$ref:"#/c",x:7},c:{$ref:"#/b"},a:{$ref:"#/b/x"}}',
    ]) {
      let report: FoldReport | undefined
      const out: any = fold(parser().parse(src), { op: REF, evaluate: resolving, report: (r) => { report = r } })
      assert.deepEqual(C(out.a), { $ref: '#/b/x' }, src)
      assert.ok(report!.cycles.some((c) => '/a' === c.at && '/b' === c.target), src)
    }
  })


  test('a key operator may be named __proto__', () => {
    const op = { p: { key: true, src: '__proto__' } }
    const calls: any[] = []
    const evaluate = (site: Site, o: Op, terms: any[]) => { calls.push([o.src, terms[0]]); return site.node }
    fold({ name: 'ordinary', src: 'data' }, { op, evaluate })
    assert.deepEqual(calls, [], 'a map owning a member of Op is not a site')
    fold(JSON.parse('{"__proto__":42,"k":1}'), { op, evaluate })
    assert.deepEqual(calls, [['__proto__', 42]])
  })
})
