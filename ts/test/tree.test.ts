/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

import { describe, test } from 'node:test'
import assert from 'node:assert'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'

import { Expr, fold, evaluation, simplify, opify, isOp, keyOpOf, makeKeyMap, recordPositions, FoldError } from '..'
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
      (err: any) => err instanceof FoldError && 'fold_cycle' === err.code && '/a' === err.at
        && /cycle at "\/a"/.test(err.message))
  })


  test('a target that names nothing is reported and the site kept', () => {
    let report: FoldReport | undefined
    const out = fold(parser().parse('{a:{$ref:"#/nope/x"}}'), { op: REF, evaluate: resolving, report: (r) => { report = r } })
    assert.deepEqual(C(out), { a: { $ref: '#/nope/x' } })
    assert.deepEqual(report!.unresolved, [{ doc: '', at: '/a', targetDoc: '', target: '/nope/x' }])
  })


  test('a dynamic key is reported and folded as a plain map', () => {
    const op = { ...REF, dyn: { key: true, src: '$dynamicRef', dynamic: true } }
    let report: FoldReport | undefined
    const tn = new Tabnas().use(jsonic).use(Expr, { op: { ...NO_DEFAULTS, ...op } })
    const out = fold(tn.parse('{a:{$dynamicRef:"#/b",c:{$ref:"#/b"}},b:1}'),
      { op, evaluate: resolving, report: (r) => { report = r } })
    assert.deepEqual(C(out), { a: { $dynamicRef: '#/b', c: 1 }, b: 1 })
    assert.deepEqual(report!.dynamic, [{ doc: '', at: '/a', src: '$dynamicRef' }])
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


  // Neither declaration order nor object key order decides: a Go option
  // table has no declaration order, and a JavaScript object lists
  // integer-like names first. Static before dynamic, then code points.
  test('a map that owns two key members: static first, then by code point', () => {
    const keys = makeKeyMap({
      z: { key: true, src: '$z' },
      dyn: { key: true, src: '$a', dynamic: true },
      two: { key: true, src: '2' },
      astral: { key: true, src: '\u{1F600}' },
      bmp: { key: true, src: '\uFF61' },
    })
    assert.deepEqual([...keys.keys()], ['$z', '2', '\uFF61', '\u{1F600}', '$a'])
    assert.equal(keyOpOf(keys, { $a: 1, $z: 2, 2: 3 })!.src, '$z')
    assert.equal(keyOpOf(keys, { $a: 1, 2: 3 })!.src, '2')
    assert.equal(keyOpOf(keys, { $a: 1 })!.name, 'dyn-key')
    assert.equal(keyOpOf(keys, [{ $z: 1 }]), undefined)
    assert.equal(keyOpOf(keys, 'x'), undefined)
    assert.equal(keyOpOf(keys, new Map([['$z', 1]])), undefined)
  })


  test('two key operators on one member name are refused', () => {
    assert.throws(
      () => makeKeyMap({ ref: { key: true, src: '$ref' }, again: { key: true, src: '$ref' } }),
      (err: any) => err instanceof FoldError && 'fold_key' === err.code)
    assert.throws(
      () => new Tabnas().use(jsonic).use(Expr, { op: { a: { key: true, src: 'x' }, b: { key: true, src: 'x' } } }),
      (err: any) => 'fold_key' === err.code)
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


// The second review: errors, leaves, indices, the deferred filter, the
// cycle rule, several documents, recognition, positions and depth.
describe('fold: second review', () => {

  test('a parse that fails is not folded, and a resolver error is not masked', () => {
    let calls = 0
    const tn = new Tabnas().use(jsonic).use(Expr, {
      op: { ...NO_DEFAULTS, ...REF },
      fold: { evaluate: (site: Site, op: Op, terms: any[]) => { calls++; return resolving(site, op, terms) } },
    })
    for (const src of ['{a:{$ref:"#/b"},b:1} ]', '{a:{$ref:"#/b"},b:1}}', '{a:{$ref:"#/b"},b:1} "x']) {
      assert.throws(() => tn.parse(src), (err: any) => !(err instanceof FoldError), src)
    }
    assert.equal(calls, 0)
    const boom = new Tabnas().use(jsonic).use(Expr, {
      op: { ...NO_DEFAULTS, ...REF }, fold: { evaluate: () => { throw new Error('resolver: boom') } },
    })
    assert.throws(() => boom.parse('{a:{$ref:"#/b"},b:1}'), /resolver: boom/)
  })


  test('a start rule with no close alternates is still folded', () => {
    const tn = new Tabnas().use(jsonic).use(Expr, { op: { ...NO_DEFAULTS, ...REF }, fold: { evaluate: resolving } })
    tn.rule('top', (rs: any) => rs.open([{ p: 'val' }]).close([]).bc((r: any) => { r.node = r.child.node }))
    tn.options({ rule: { start: 'top' } })
    assert.deepEqual(C(tn.parse('{a:{$ref:"#/b"},b:1}')), { a: 1, b: 1 })
  })


  test('objects that are not plain records are leaves, kept by identity', () => {
    const when = new Date(0)
    const map = new Map([['k', 1]])
    class Point { constructor(public x: number) {} }
    const point = new Point(3)
    const raw = { when, map, point, a: { $ref: '#/when' }, b: { $ref: '#/point/x' } }
    const direct = (site: Site, _op: Op, [v]: any[]) => {
      const h = site.at(pointer(v))
      return h.found && !h.cycle ? h.value : site.node
    }
    const out: any = fold(raw, { op: REF, evaluate: direct })
    assert.equal(out.when, when)
    assert.equal(out.map, map)
    assert.equal(out.point, point)
    assert.equal(out.a, when)
    assert.deepEqual(out.b, { $ref: '#/point/x' }, 'a class instance is not stepped into')
    assert.equal(out.when.getTime(), 0)
    assert.equal(JSON.stringify(out.when), '"1970-01-01T00:00:00.000Z"')
    const viaEval = evaluation(null as any, null as any, { d: when }, (_r, _c, o, t) => t, { objects: true })
    assert.equal(viaEval.d, when)
  })


  test('array indices are RFC 6901: no leading zero, sign, space or fraction', () => {
    const doc = () => ({ b: ['zero', 'one', 'two'] })
    const hit = (seg: string) => {
      let got: any
      fold({ ...doc(), a: { $ref: 'x' } }, {
        op: REF, evaluate: (site) => { got = site.at(['b', seg]); return 1 },
      })
      return got.found ? got.value : '-'
    }
    assert.deepEqual(['0', '1', '2'].map(hit), ['zero', 'one', 'two'])
    assert.deepEqual(['', ' ', '01', '1.0', '1e0', '0x1', '+1', '-0', '-', '3', 'length'].map(hit),
      Array(11).fill('-'))
    let located: any
    fold({ ...doc(), a: { $ref: 'x' } }, { op: REF, evaluate: (site) => { located = site.locate(['b', '01']); return 1 } })
    assert.deepEqual(located, { found: false })
  })


  test('a cycle hit never carries a value, on any branch', () => {
    const hits: any[] = []
    const record = (site: Site, op: Op, terms: any[]) => {
      const h = site.at(pointer(terms[0]))
      hits.push(h)
      return h.found && !h.cycle ? h.value : site.node
    }
    fold(parser().parse('{a:{$ref:"#/a"},b:{$ref:"#/a"},x:{$ref:"#/y"},y:{$ref:"#/x"},z:{$ref:"#/x/q"}}'),
      { op: REF, evaluate: record })
    assert.ok(hits.length >= 5)
    for (const h of hits) {
      assert.equal(h.cycle, true)
      assert.equal('value' in h, false)
    }
  })


  test('the bag at a site includes its own map, and inherit must return one', () => {
    const inherit = (_s: Site, node: any, k: any) => 'string' === typeof node.base ? { ...k, base: node.base } : k
    const out: any = fold(parser().parse('{base:x,a:{$ref:r,base:z}}'),
      { op: REF, inherit, evaluate: (site) => site.k.base })
    assert.equal(out.a, 'z')
    for (const bad of [() => undefined, () => null, (_s: Site, _n: any, k: any) => { k.base = 1 }]) {
      assert.throws(
        () => fold(parser().parse('{a:{b:{$ref:x}}}'), { op: REF, inherit: bad as any, evaluate: (site) => site.k }),
        (err: any) => err instanceof FoldError && 'fold_inherit' === err.code)
    }
    // The root's `inherit` folds `q`, whose bag needs the root's: refused.
    assert.throws(
      () => fold(parser().parse('{a:{b:{$ref:x}},q:{$ref:y}}'), {
        op: REF,
        inherit: (site, _n, k) => { if (0 === site.path.length) site.at(['q']); return k },
        evaluate: (site) => site.k,
      }),
      (err: any) => 'fold_inherit' === err.code)
  })


  test('report rows are kept once each', () => {
    let report: FoldReport | undefined
    fold({ a: { $ref: '#/nope' }, c: { $ref: '#/c' } }, {
      op: REF,
      evaluate: (site, _op, [v]) => { site.at(pointer(v)); site.at(pointer(v)); return site.node },
      report: (r) => { report = r },
    })
    assert.deepEqual(report!.unresolved, [{ doc: '', at: '/a', targetDoc: '', target: '/nope' }])
    assert.deepEqual(report!.cycles, [{ doc: '', at: '/c', targetDoc: '', target: '/c' }])
  })


  // Node -> Tree, Tree/properties/child -> Node: a cycle through a
  // container is cut once, at the site whose target was being folded,
  // in either member order; only the place of the cut follows the walk.
  test('a cycle through a container is cut once, in either member order', () => {
    const tree = '{properties:{child:{$ref:"#/Node"}}}'
    for (const [src, cut] of [
      [`{Node:{$ref:"#/Tree"},Tree:${tree}}`, '/Tree/properties/child'],
      [`{Tree:${tree},Node:{$ref:"#/Tree"}}`, '/Node'],
    ]) {
      let report: FoldReport | undefined
      const out: any = fold(parser().parse(src), { op: REF, evaluate: resolving, report: (r) => { report = r } })
      assert.deepEqual(report!.cycles.map((c) => c.at + '>' + c.target), [cut + ('/Node' === cut ? '>/Tree' : '>/Node')], src)
      assert.equal(JSON.stringify(out).includes('properties'), true)
    }
  })


  test('a pointer through a site that was cut is a cycle, not a miss', () => {
    let report: FoldReport | undefined
    const out: any = fold(parser().parse(
      '{Tree:{properties:{child:{$ref:"#/Node"}}},Node:{$ref:"#/Tree"},Q:{$ref:"#/Node/properties"}}'),
    { op: REF, evaluate: resolving, report: (r) => { report = r } })
    assert.deepEqual(C(out.Q), { $ref: '#/Node/properties' })
    assert.deepEqual(report!.unresolved, [])
    assert.ok(report!.cycles.some((c) => '/Q' === c.at && '/Node' === c.target))
  })


  test('a path through a dynamic map, or one recognise refused, reads on', () => {
    const op = { ...REF, dyn: { key: true, src: '$dynamicRef', dynamic: true } }
    let report: FoldReport | undefined
    const keep = (r: FoldReport) => { report = r }
    let out: any = fold({ a: { $dynamicRef: '#/b', c: { $ref: '#/a/d' }, d: 1 }, b: 2 },
      { op, evaluate: resolving, report: keep })
    assert.equal(out.a.c, 1)
    assert.deepEqual(report!.cycles, [])
    let asked = 0
    out = fold({ P: { $ref: { type: 'string' }, other: { $ref: '#/P/third' }, third: { t: 3 } } }, {
      op: REF, evaluate: resolving, report: keep,
      recognise: (site) => { if (0 === site.path.length || 'P' === site.path[0] && 1 === site.path.length) asked++; return 'P' !== site.path[site.path.length - 1] },
    })
    assert.deepEqual(C(out.P.other), { t: 3 })
    assert.deepEqual(report!.cycles, [])
    assert.equal(asked, 1, 'recognise is asked once per map')
  })


  test('several documents: loaded once, shared targets, cycles across them', () => {
    const docs: any = {
      'b.json': { Z: { z: 1 }, X: { $ref: 'a.json#/Y' } },
    }
    const loads: string[] = []
    const split = (ref: string) => {
      const [doc, frag] = ref.split('#')
      return { doc: '' === doc ? undefined : doc, path: pointer('#' + (frag || '')) }
    }
    let report: FoldReport | undefined
    const out: any = fold({ P: { $ref: 'b.json#/Z' }, Q: { $ref: 'b.json#/Z' }, Y: { $ref: 'b.json#/X' }, R: { $ref: 'c.json#/W' } }, {
      op: REF,
      doc: 'a.json',
      load: (key) => { loads.push(key); return 'a.json' === key ? undefined : docs[key] },
      evaluate: (site, _op, [ref]) => {
        const { doc, path } = split(ref)
        const hit = site.at(path, doc)
        return hit.found && !hit.cycle ? hit.value : site.node
      },
      report: (r) => { report = r },
    })
    assert.deepEqual(loads, ['b.json', 'c.json'])
    assert.equal(out.P, out.Q, 'one folded object per target, across documents')
    assert.deepEqual(C(out.Y), { $ref: 'b.json#/X' })
    assert.deepEqual(report!.cycles.map((c) => [c.doc, c.at, c.targetDoc, c.target].join(' ')).sort(), [
      'a.json /Y b.json /X',
      'b.json /X a.json /Y',
    ])
    assert.deepEqual(report!.unresolved, [{ doc: 'a.json', at: '/R', targetDoc: 'c.json', target: '/W' }])
  })


  test('recognise and plain: a map that owns $ref need not be a site', () => {
    const src = '{S:{properties:{$ref:{type:string},other:{$ref:"#/T"}}},T:{t:1},Q:{$ref:"#/S/properties/other"}}'
    const calls: string[] = []
    const out: any = fold(parser().parse(src), {
      op: REF,
      recognise: (site) => 'properties' !== site.path[site.path.length - 1],
      evaluate: (site, op, terms) => { calls.push(site.path.join('/')); return resolving(site, op, terms) },
    })
    assert.deepEqual(C(out.S.properties), { $ref: { type: 'string' }, other: { t: 1 } })
    assert.deepEqual(C(out.Q), { t: 1 })
    assert.deepEqual(calls.sort(), ['Q', 'S/properties/other'])

    // A resolver that finds no operation returns site.plain(): the map,
    // folded, members in their own order.
    const plain: any = fold(parser().parse('{a:{x:{$ref:"#/b"},$ref:42,y:1},b:2}'), {
      op: REF,
      evaluate: (site, op, terms) => 'string' === typeof terms[0] ? resolving(site, op, terms) : site.plain(),
    })
    assert.deepEqual(Object.keys(plain.a), ['x', '$ref', 'y'])
    assert.deepEqual(C(plain.a), { x: 2, $ref: 42, y: 1 })
  })


  test('positions: a lookup in the function form, true only when deferred', () => {
    const tn = new Tabnas().use(jsonic).use(Expr, { op: { ...NO_DEFAULTS, ...REF } })
    const positions = recordPositions(tn)
    const raw = tn.parse('{\n  a: { $ref: "#/b" },\n  b: 1\n}')
    const at: string[] = []
    fold(raw, {
      op: REF, positions,
      evaluate: (site, op, terms) => { at.push(site.token!.rI + ':' + site.token!.cI); return resolving(site, op, terms) },
    })
    assert.deepEqual(at, ['2:6'])
    assert.throws(() => fold(raw, { op: REF, positions: true, evaluate: resolving }),
      (err: any) => err instanceof FoldError && 'fold_option' === err.code)
  })


  test('tokens off keeps an operator node an operator node', () => {
    const tn = new Tabnas().use(jsonic).use(Expr, { op: REF })
    const raw: any = tn.parse('{a:1+{$ref:"#/b"},b:2}')
    const out: any = fold(raw, { op: REF, tokens: false, evaluate: resolving })
    assert.ok(isOp(out.a))
    assert.equal(out.a[0], raw.a[0])
    assert.deepEqual(out.a.slice(1), [1, 2])
    const math = (_r: any, _c: any, op: Op, terms: any[]) => '+' === op.src ? terms[0] + terms[1] : NaN
    assert.equal(evaluation(null as any, null as any, out.a, math), 3)
  })


  test('depth: a deep chain or nesting raises fold_depth, never a RangeError', () => {
    const chain = (n: number) => {
      const o: any = {}
      for (let i = 0; i < n; i++) o['a' + i] = i < n - 1 ? { $ref: '#/a' + (i + 1) } : { v: 1 }
      return o
    }
    assert.deepEqual(C(fold(chain(500), { op: REF, evaluate: resolving }).a0), { v: 1 })
    for (const n of [1200, 20000]) {
      assert.throws(() => fold(chain(n), { op: REF, evaluate: resolving }),
        (err: any) => err instanceof FoldError && 'fold_depth' === err.code, String(n))
    }
    let deep: any = { x: 1 }
    for (let i = 0; i < 1200; i++) deep = { c: deep }
    assert.throws(() => fold(deep, { op: REF, evaluate: resolving }), (err: any) => 'fold_depth' === err.code)
    assert.ok(fold(deep, { op: REF, evaluate: resolving, maxDepth: 1300 }))
    const cyclic: any = { a: {} }
    cyclic.a.self = cyclic.a
    assert.throws(() => fold(cyclic, { op: REF, evaluate: resolving }), (err: any) => 'fold_depth' === err.code)
  })


  test('a cycle error escapes member names and bounds the message', () => {
    const name = 'l1\nl2\u001b[31m' + 'K'.repeat(300)
    const doc: any = { [name]: { $ref: 'self' } }
    try {
      fold(doc, { op: REF, cycle: 'error', evaluate: (site) => site.at([name]) })
      assert.fail('no error')
    }
    catch (err: any) {
      assert.equal(err.code, 'fold_cycle')
      assert.equal(err.at, '/' + name)
      assert.ok(!/[\n\u001b]/.test(err.message))
      assert.ok(err.message.length < 220, String(err.message.length))
    }
  })
})
