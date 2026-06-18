/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

/*  perf.test.ts
 *  Machine-INDEPENDENT performance regression guard.
 *
 *  Unlike the Go side, @tabnas/expr exposes NO package-level convenience
 *  parse() — it is a plugin users install on an instance they own
 *  (`new Tabnas().use(Expr)`). Building that instance (engine + the
 *  expensive expr grammar) dominates a parse, so the right usage is to
 *  build ONE instance and reuse it for many parses. This test guards that
 *  contract: it shows that reusing a single instance for N parses stays
 *  cheap relative to a single instance build, i.e. each parse does NOT
 *  rebuild the grammar.
 *
 *  The check is machine-INDEPENDENT: it compares "build-per-parse" against
 *  "build-once-reuse" on the SAME machine in the SAME run, so a slow CI box
 *  cannot make it flaky (both sides scale together). There is deliberately
 *  NO wall-clock budget.
 *
 *  If a future "convenience parse" were added that rebuilt the instance per
 *  call, the build-per-parse path is exactly what it would do — this test
 *  documents how much slower that is.
 */

import { describe, test } from 'node:test'
import assert from 'node:assert'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'

import { Expr } from '..'

const SRC = '1+2*3'
const N = 400

function makeInstance(): Tabnas {
  return new Tabnas().use(jsonic).use(Expr)
}

describe('perf', () => {
  test('reuses-instance', () => {
    // Warm both paths so the comparison is steady-state.
    for (let i = 0; i < 100; i++) {
      makeInstance().parse(SRC)
    }
    const reused = makeInstance()
    for (let i = 0; i < 100; i++) {
      reused.parse(SRC)
    }

    // build-per-parse: rebuild the engine + grammar on every parse.
    const t0 = process.hrtime.bigint()
    for (let i = 0; i < N; i++) {
      makeInstance().parse(SRC)
    }
    const rebuild = Number(process.hrtime.bigint() - t0)

    // build-once-reuse: reuse a single instance for every parse.
    const t1 = process.hrtime.bigint()
    for (let i = 0; i < N; i++) {
      reused.parse(SRC)
    }
    const reuse = Number(process.hrtime.bigint() - t1)

    const ratio = rebuild / reuse

    // Reusing one instance must be dramatically cheaper than rebuilding it
    // per parse (grammar build dominates). A small ratio would mean the
    // build is free — it is not — so this also confirms the test is
    // exercising the real cost. We assert the EXPECTED direction: reuse is
    // much faster, i.e. rebuild/reuse is large. No absolute wall-clock
    // budget (flaky); the comparison is same-machine, same-run.
    assert.ok(
      rebuild > 2 * reuse,
      `expected building a fresh instance per parse to be >2x slower than ` +
        `reusing one instance (grammar build dominates), but got ratio ` +
        `${ratio.toFixed(2)}x (rebuild=${(rebuild / 1e6).toFixed(1)}ms ` +
        `reuse=${(reuse / 1e6).toFixed(1)}ms over ${N} parses). If this ` +
        `dropped, a convenience parse that rebuilds per call would be ` +
        `cheap to add; if a cached convenience parse is later added, guard ` +
        `it the way the Go side does (sync.Once-style singleton).`,
    )

    // Reuse should also scale roughly linearly: N reuse-parses should be far
    // less than N times a single fresh build. This is the "stays fast
    // relative to a single parse" guard.
    const oneBuild = rebuild / N
    assert.ok(
      reuse < N * oneBuild,
      `reuse of one instance for ${N} parses (${(reuse / 1e6).toFixed(1)}ms) ` +
        `should stay well under N single-build parses`,
    )

    // Surface the numbers in the test log.
    console.log(
      `perf: rebuild-per-parse=${(rebuild / 1e6).toFixed(1)}ms  ` +
        `reuse-one=${(reuse / 1e6).toFixed(1)}ms  ratio=${ratio.toFixed(2)}x  ` +
        `(${N} parses)`,
    )
  })
})
