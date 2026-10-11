/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

// The render, run: trees written as expression documents by the parts the
// package carries (`translate()`), the embedding and then the render, as a
// host composes them for a plain tree, through the `alchemy` command
// (tabnas/alchemy-cli), which this package does not depend on. Set
// TABNAS_ALCHEMY to the command's path to run these tests; without it they
// skip, as `debug-model.test.ts` does without @tabnas/debug.
//
// What the render writes reads back through `parseSimplified` as the tree
// it was given, under the conventions the manifest's loss list declares. A
// list led by an object whose src the reader would read as an operator
// that is no default one has no document that reads back as it, and the
// render refuses it with TARGET_VALUE_UNREPRESENTABLE, naming where.

import * as assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import Fs from 'node:fs'
import Os from 'node:os'
import Path from 'node:path'
import { after, test } from 'node:test'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { findSpecDir, loadSpec } from '@tabnas/support'

import { Expr, parseSimplified, simplify, translate } from '..'

const ALCHEMY = process.env.TABNAS_ALCHEMY
const skip = ALCHEMY ? false : 'TABNAS_ALCHEMY does not name the alchemy command'

const tn = new Tabnas().use(jsonic).use(Expr)

let dir: string | null = null
after(() => {
  if (dir) Fs.rmSync(dir, { recursive: true, force: true })
})

type Rendered = { text?: string, code?: string, message?: string }

// The embedding and the render over a tree: `{ text }` when the render
// writes one, and the failure the command reports (`{ code, message }`)
// when it does not.
function render (tree: any): Rendered {
  if (null == dir) {
    dir = Fs.mkdtempSync(Path.join(Os.tmpdir(), 'expr-render-'))
    const parts = translate()
    Fs.writeFileSync(Path.join(dir, 'translate.alc'),
      `${parts?.render?.source}\n${parts?.embed?.source}\n` +
      'def export [input] (expr-render (expr-embed input))\n')
  }
  const input = Path.join(dir, 'tree.json')
  Fs.writeFileSync(input, JSON.stringify(tree))
  const run = spawnSync(ALCHEMY as string,
    ['run', Path.join(dir, 'translate.alc'), input],
    { encoding: 'utf8', timeout: 600000, maxBuffer: 1 << 28 })
  if (0 === run.status) return { text: run.stdout }
  try {
    return JSON.parse(run.stderr)
  } catch (e) {
    return { code: 'NOT_JSON', message: `${run.stderr} ${run.error ?? ''}` }
  }
}

// The text the render writes for a tree, which fails the test where the
// render refuses it.
function written (tree: any): string {
  const out = render(tree)
  assert.equal(typeof out.text, 'string', JSON.stringify(out))
  return out.text as string
}

// What the reader reads back from a document the render wrote of a tree,
// by the loss list: each operator's description as its source text, which
// `simplify` gives, and a negative number as the operator - applied to its
// magnitude. JSON carries no number that is not finite.
function reading (tree: any): any {
  const back = (value: any): any =>
    'number' === typeof value && value < 0 ? ['-', -value]
      : Array.isArray(value) ? value.map(back)
        : null != value && 'object' === typeof value
          ? Object.fromEntries(Object.entries(value).map(([k, v]) => [k, back(v)]))
          : value
  return back(simplify(tree))
}

// The tree tabnas-c's reader builds for `int x;`, as alchemy-cli's
// `translate --from c` reads it: the children of a node begin with a
// token, an object whose src is the token's text.
const C_INT_X = '{"kind":"translation_unit","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"external_declaration","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"declaration_specifiers","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"token","tname":"KW_INT","src":"int","span":{"start":0,"end":3,"line":1,"col":1}}],"trivia":{"leading":[],"trailing":[]}},{"kind":"init_declarator_list","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"init_declarator","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"declarator","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"direct_declarator","span":{"start":0,"end":0,"line":1,"col":1},"children":[{"kind":"token","tname":"ID","src":"x","span":{"start":4,"end":5,"line":1,"col":5}}],"trivia":{"leading":[],"trailing":[]},"declaredName":"x"}],"trivia":{"leading":[],"trailing":[]},"declaredName":"x"}],"trivia":{"leading":[],"trailing":[]},"declaredName":"x"}],"trivia":{"leading":[],"trailing":[]}},{"kind":"token","tname":"PUNC_SEMI","src":";","span":{"start":5,"end":6,"line":1,"col":6}}],"trivia":{"leading":[],"trailing":[]},"declKind":"declaration","viaPath":"grammar"}],"trivia":{"leading":[],"trailing":[]}}'

// Where the render refuses a list, and the src it names there.
function refuses (tree: any, where: string, src: string) {
  const out = render(tree)
  assert.equal(out.code, 'TARGET_VALUE_UNREPRESENTABLE', JSON.stringify(out))
  const lead = `the object at ${where} is the first element of a list and has the src ${src}, `
  assert.ok((out.message ?? '').startsWith(lead), `${out.message} does not begin ${lead}`)
}

test('the render refuses a C syntax tree, whose tokens lead lists, naming the token and its src', { skip }, () => {
  refuses(JSON.parse(C_INT_X), '.children[0].children[0].children[0]', '"int"')
})

// The reader keeps the src of an object at a list's head alone wherever it
// would read one: TypeScript's simplify any src that is not empty, zero,
// false or null, as the render writes it, so a negative number, written
// with its sign, is an operation there, a list.
test('the render refuses a list led by an object with a src the reader reads, other than a default operator\'s', { skip }, () => {
  refuses([{ kind: 'token', src: 'x' }, 1], '.[0]', '"x"')
  refuses({ a: [1, { 'b c': [{ src: 'q' }] }] }, '.a[1]."b c"[0]', '"q"')
  refuses(['+', [{ src: 'int' }, 1], 2], '.[1][0]', '"int"')
  refuses([[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, [{ src: 'k' }]]], '.[0][11][0]', '"k"')
  refuses([{ src: 'NaN' }], '.[0]', '"NaN"')
  refuses([{ src: 1 }, 2], '.[0]', '1')
  refuses([{ src: -1 }], '.[0]', '-1')
  refuses([{ src: true }], '.[0]', 'true')
  refuses([{ src: { a: 1 } }], '.[0]', '{"a":1}')
  refuses([{ src: [] }], '.[0]', '[]')
})

test('an object whose src no reader reads, or that leads no list, is written as it is', { skip }, () => {
  for (const tree of [
    [{ a: 1, src: '' }, 1],
    [{ src: 0 }, 1],
    [{ src: false }, 1],
    [{ src: null }, 1],
    { a: { src: 'x' } },
    [1, { src: 'x' }],
  ]) {
    assert.deepEqual(parseSimplified(tn, written(tree)), tree, JSON.stringify(tree))
  }
})

// Each default operator, as its source text and as the reader's own value
// describes it, an object whose src is that text.
test('every default operator is written and reads back unchanged', { skip }, () => {
  for (const src of [
    '1+2-3*4/5%6', '-(1+2)*+3', '()', '(1)', '8/2%3-1', '--1', '-+1',
    '1-(2-3)', '(1+2)*(3-4)/(5%6)', '{a:1+2,b:[3*4,(5)]}',
  ]) {
    const tree = parseSimplified(tn, src)
    for (const given of [tree, JSON.parse(JSON.stringify(tn.parse(src)))]) {
      assert.deepEqual(parseSimplified(tn, written(given)), tree, src)
    }
  }
})

// Nothing the reader builds is refused. Every shared fixture row's value,
// as the reader builds it with its file's operators (the row's expected
// cell), is written in one list after a null, which makes the list no
// operation, and reads back as itself through the reader the render
// writes for, the default operators'. So is the reader's own value of each
// row the default operators read, its operators described.
test('nothing the reader builds from the shared fixtures is refused, and all of it reads back', { skip }, () => {
  const SPEC = findSpecDir(__dirname)
  const values: any[] = [null]
  const own: any[] = [null]
  for (const name of Fs.readdirSync(SPEC).filter((n) => n.endsWith('.tsv')).sort()) {
    for (const row of loadSpec(Path.join(SPEC, name)).rows) {
      values.push(JSON.parse(row.col(1)))
      let value
      try {
        value = tn.parse(row.unesc(0))
      } catch (e) {
        continue
      }
      own.push(JSON.parse(JSON.stringify(value)))
    }
  }
  for (const batch of [values, own]) {
    assert.deepEqual(parseSimplified(tn, written(batch)), reading(batch))
  }
})
