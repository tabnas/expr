/* Copyright (c) 2026 Richard Rodger, MIT License */

// The exported VERSION must equal package.json "version".
//
// This is the CI check for version drift. It exists because the constant HAS
// drifted: @tabnas/json exported Version = '1.0.0' for several releases while
// the package shipped 0.4.x, because nothing rewrote it. jsonic-cli shipped
// 0.4.1 and 0.4.2 with its const stuck at 0.4.0 for the same reason. Both
// were invisible until someone read the file. A release that bumps
// package.json and forgets the constant now fails here.

import { describe, test } from 'node:test'
import * as assert from 'node:assert'
import * as Fs from 'node:fs'
import * as Path from 'node:path'

import { VERSION } from '..'


// Read package.json directly rather than importing it: the failure mode this
// test exists to prevent is the check not running at all, so an unreadable
// package.json must FAIL the test, never skip it.
function readPkg(): { name?: string; version?: string } {
  const path = Path.join(__dirname, '..', 'package.json')
  let raw: string
  try {
    raw = Fs.readFileSync(path, 'utf8')
  }
  catch (err: any) {
    assert.fail(
      `cannot read ${path}, so VERSION cannot be checked: ${err.message}`)
  }
  try {
    return JSON.parse(raw)
  }
  catch (err: any) {
    assert.fail(`${path} is not readable JSON: ${err.message}`)
  }
}


describe('version', () => {

  test('VERSION matches package.json', () => {
    const pkg = readPkg()
    assert.ok(
      null != pkg.version && '' !== pkg.version,
      'package.json has no version field')

    assert.equal(
      VERSION,
      pkg.version,
      `VERSION drift: ${pkg.name} exports ${VERSION} but package.json is ` +
      `${pkg.version}. Both are rewritten by admin/publish.sh at release; ` +
      `if you bumped one by hand, bump the other.`)
  })


  test('VERSION is exported from the package root', () => {
    const api = require('..')
    assert.equal(
      typeof api.VERSION, 'string', 'VERSION must be exported as a string')
    assert.match(api.VERSION, /^\d+\.\d+\.\d+/, 'VERSION must be a semver')
  })

})
