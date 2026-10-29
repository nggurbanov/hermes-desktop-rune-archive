import assert from 'node:assert/strict'
import { test } from 'vitest'

import {
  FALLBACK_BRANCH,
  FALLBACK_COMMIT,
  fromCI,
  fromFallback,
  fromLocalGit,
  isFallbackCommit,
  resolveStamp
} from './write-build-stamp.mjs'

test('fromCI reads GITHUB_SHA / GITHUB_REF_NAME', () => {
  assert.deepEqual(
    fromCI({ GITHUB_SHA: 'a'.repeat(40), GITHUB_REF_NAME: 'release' }),
    { commit: 'a'.repeat(40), branch: 'release', dirty: false, source: 'ci' }
  )
  assert.equal(fromCI({}), null)
})

test('fromLocalGit reads HEAD + branch + dirty status', () => {
  const calls = []
  const execFn = (argv) => {
    const cmd = argv.join(' ')
    calls.push(cmd)
    if (cmd === 'git rev-parse HEAD') return 'b'.repeat(40)
    if (cmd === 'git rev-parse --abbrev-ref HEAD') return 'main'
    if (cmd === 'git status --porcelain -uno') return ' M apps/desktop/package.json'
    return null
  }
  assert.deepEqual(fromLocalGit('/repo', execFn), {
    commit: 'b'.repeat(40),
    branch: 'main',
    dirty: true,
    source: 'local'
  })
  assert.ok(calls.includes('git rev-parse HEAD'))
})

test('fromFallback uses the all-zero placeholder commit', () => {
  assert.deepEqual(fromFallback(), {
    commit: FALLBACK_COMMIT,
    branch: FALLBACK_BRANCH,
    dirty: false,
    source: 'fallback'
  })
  assert.equal(isFallbackCommit(FALLBACK_COMMIT), true)
  assert.equal(isFallbackCommit('a'.repeat(40)), false)
})

test('resolveStamp prefers CI over local git over fallback', () => {
  const ci = resolveStamp({
    env: { GITHUB_SHA: 'c'.repeat(40), GITHUB_REF_NAME: 'main' },
    execFn: () => 'should-not-run'
  })
  assert.equal(ci.source, 'ci')
  assert.equal(ci.commit, 'c'.repeat(40))

  const local = resolveStamp({
    env: {},
    execFn: (argv) => {
      const cmd = argv.join(' ')
      if (cmd === 'git rev-parse HEAD') return 'd'.repeat(40)
      if (cmd === 'git rev-parse --abbrev-ref HEAD') return 'main'
      if (cmd === 'git status --porcelain -uno') return ''
      return null
    }
  })
  assert.equal(local.source, 'local')
  assert.equal(local.commit, 'd'.repeat(40))
  assert.equal(local.dirty, false)
})

test('resolveStamp falls back when neither CI nor git is available', () => {
  const stamp = resolveStamp({ env: {}, execFn: () => null })
  assert.deepEqual(stamp, {
    commit: FALLBACK_COMMIT,
    branch: FALLBACK_BRANCH,
    dirty: false,
    source: 'fallback'
  })
})

import { buildStampPayload } from './write-build-stamp.mjs'

const baseStamp = { commit: 'c'.repeat(40), branch: 'main', dirty: false, source: 'ci' }

test('client stamp records desktop provenance and never an embedded runtime', () => {
  const stamp = buildStampPayload(baseStamp, {})
  assert.equal(stamp.schemaVersion, 1)
  assert.equal(stamp.commit, baseStamp.commit)
  assert.equal(stamp.source, 'ci')
  assert.equal(stamp.payload, 'standalone')
  assert.equal(stamp.distribution, 'desktop-app')
  assert.equal(stamp.updateMechanism, 'external')
  assert.equal(stamp.runtime, undefined)
})

test('agent bundle selectors cannot reintroduce a runtime into the client', () => {
  for (const variant of ['bundled', 'bootstrap', 'light', 'store']) {
    assert.throws(() => buildStampPayload(baseStamp, { HERMES_DESKTOP_VARIANT: variant }), /standalone/)
  }
  assert.throws(() => buildStampPayload(baseStamp, { HERMES_BUILD_COMMIT: 'a'.repeat(40) }), /identity/)
})
