/* Copyright (c) 2021-2025 Richard Rodger and other contributors, MIT License */

import * as fs from 'fs'
import * as path from 'path'
import * as assert from 'node:assert'


export type SpecEntry = {
  input: string
  expected: any
}

type ExpectValue = {
  equal: (expected: any) => void
}

type ExpectFn = {
  throw: (matcher?: RegExp | string) => void
}

// Strip null-prototype containers so deepStrictEqual treats them as
// equivalent to plain objects (matching @hapi/code's behavior).
function normalize(value: any, seen = new WeakMap()): any {
  if (value === null || typeof value !== 'object') return value
  if (seen.has(value)) return seen.get(value)
  if (Array.isArray(value)) {
    const out: any[] = []
    seen.set(value, out)
    for (const v of value) out.push(normalize(v, seen))
    return out
  }
  const proto = Object.getPrototypeOf(value)
  if (proto === null || proto === Object.prototype) {
    const out: Record<string, any> = {}
    seen.set(value, out)
    for (const k of Object.keys(value)) out[k] = normalize(value[k], seen)
    return out
  }
  return value
}

export function expect(actual: any): ExpectValue & ExpectFn {
  return {
    equal(expected: any) {
      assert.deepStrictEqual(normalize(actual), normalize(expected))
    },
    throw(matcher?: RegExp | string) {
      let threw: unknown
      try {
        actual()
      } catch (err) {
        threw = err
      }
      if (threw === undefined) {
        throw new assert.AssertionError({
          message: 'Expected function to throw',
          actual: undefined,
          expected: matcher,
        })
      }
      if (matcher instanceof RegExp) {
        const msg = (threw as Error)?.message ?? String(threw)
        if (!matcher.test(msg)) {
          throw new assert.AssertionError({
            message: `Error message "${msg}" does not match ${matcher}`,
            actual: msg,
            expected: matcher,
          })
        }
      } else if (typeof matcher === 'string') {
        const msg = (threw as Error)?.message ?? String(threw)
        if (!msg.includes(matcher)) {
          throw new assert.AssertionError({
            message: `Error message "${msg}" does not include "${matcher}"`,
            actual: msg,
            expected: matcher,
          })
        }
      }
    },
  }
}

export function loadSpec(name: string): SpecEntry[] {
  // Resolve spec files relative to the project root test/spec directory,
  // since compiled tests run from dist-test/ but specs live in test/spec/.
  const rootDir = path.resolve(__dirname, '..', '..')
  const specPath = path.join(rootDir, 'test', 'spec', name)
  const content = fs.readFileSync(specPath, 'utf8')
  const entries: SpecEntry[] = []

  for (const line of content.split('\n')) {
    const trimmed = line.trim()
    if (trimmed === '' || trimmed.startsWith('#')) continue

    const tabIdx = trimmed.indexOf('\t')
    if (tabIdx === -1) continue

    const input = trimmed.substring(0, tabIdx)
    const expectedJson = trimmed.substring(tabIdx + 1)

    entries.push({
      input,
      expected: JSON.parse(expectedJson),
    })
  }

  return entries
}
