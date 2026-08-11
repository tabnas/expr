/* Copyright (c) 2021-2025 Richard Rodger and other contributors, MIT License */

// The @hapi/code-shaped assertion helper the in-language tests use.
//
// This file used to also carry a TSV loader. The fixtures are read by
// @tabnas/support now — see spec.test.ts — so only `expect` is left.

import * as assert from 'node:assert'

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
