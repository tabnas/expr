/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

// The translation parts (admin ADR-27): what the manifest names and what
// the package hands a host are the same texts. ../embed-translate.js
// generates src/translate.ts from tabnas.plugin.json and alchemy/*.alc,
// so a part changed without `npm run embed` fails here.

import * as assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import * as path from 'node:path'
import { test } from 'node:test'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'

import { Expr, translate } from '..'

const root = path.resolve(__dirname, '..', '..')
const read = (rel: string) => readFileSync(path.join(root, rel), 'utf8')

// The names a library of alchemy definitions defines, in its order.
function definitions (text: string): string[] {
  return text
    .split('\n')
    .filter((line) => line.startsWith('def '))
    .map((line) => line.slice(4).split(/\s/)[0])
}

test('translation parts expose the manifest, the render and the embedding', () => {
  const parts = translate()
  assert.ok(parts)
  assert.equal(parts.manifest, read('tabnas.plugin.json'))
  assert.equal(parts.lift, undefined)
  assert.equal(parts.render?.entry, 'expr-render')
  assert.equal(parts.render?.source, read('alchemy/render.alc'))
  assert.equal(parts.embed?.entry, 'expr-embed')
  assert.equal(parts.embed?.source, read('alchemy/embed.alc'))
})

test('the manifest says what the parts are', () => {
  const spec = JSON.parse(read('tabnas.plugin.json'))
  assert.equal(spec.languageId, 'expr')
  const t = spec.translate
  assert.equal(t.reads, 'tree')
  assert.equal(t.writes, 'tree')
  assert.equal(t.root, 'any')
  assert.equal(t.schema, 'expr')
  assert.equal(t.embed, 'alchemy/embed.alc')
  assert.equal(t.render, 'alchemy/render.alc')
  assert.equal(t.lift, undefined)
  assert.ok(0 < t.loss.length)
  for (const line of t.loss) {
    assert.match(line, /^[A-Z].*\.$/, `${line} is not a sentence`)
  }
})

// A host links the render and the embedding with its own program and
// other formats' parts into one namespace, so every definition is named
// for the format, neither file defines an export, and no name is defined
// in both.
test('the parts are libraries named for the format', () => {
  const parts = translate()
  const render = definitions(parts?.render?.source ?? '')
  const embed = definitions(parts?.embed?.source ?? '')
  assert.ok(render.includes('expr-render'), render.join(' '))
  assert.ok(embed.includes('expr-embed'), embed.join(' '))
  assert.ok(embed.includes('expr-unembed'), embed.join(' '))
  for (const name of [...render, ...embed]) {
    assert.ok(name.startsWith('expr-'), `${name} is not named for expr`)
  }
  for (const name of embed) {
    assert.ok(!render.includes(name), `${name} is defined by the render too`)
  }
})

// The render reads an operation's operator from the tree a host hands it:
// the operator's source text, or the object that describes it, by its src
// member, which is what the reader's own value puts first in an
// operation's list. So the reader must describe every default operator by
// its source text there, and a group by `(`; were it to stop, the render
// would write the description as an object, which test/spec/render.tsv,
// reading documents back, cannot see.
test('the reader describes every default operator by the src the render reads', () => {
  const tn = new Tabnas().use(jsonic).use(Expr)
  // Each operation as its operator's src and its terms.
  const read = (value: any): any => {
    if (!Array.isArray(value)) return value
    assert.equal(typeof value[0], 'object', `${JSON.stringify(value)} has no description first`)
    return [value[0].src, ...value.slice(1).map(read)]
  }
  assert.deepEqual(read(tn.parse('1+2*3')), ['+', 1, ['*', 2, 3]])
  assert.deepEqual(read(tn.parse('8/2%3-1')), ['-', ['%', ['/', 8, 2], 3], 1])
  assert.deepEqual(read(tn.parse('-+1')), ['-', ['+', 1]])
  assert.deepEqual(read(tn.parse('(1)')), ['(', 1])
  assert.deepEqual(read(tn.parse('()')), ['('])
})
