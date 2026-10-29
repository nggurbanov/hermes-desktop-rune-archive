import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { test } from 'vitest'

import { sourceHash } from '../../../scripts/build/freshness.mjs'

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'desktop-inputs-'))
  const put = (name, text) => {
    const file = join(root, name)
    mkdirSync(dirname(file), { recursive: true })
    writeFileSync(file, text)
  }
  put('apps/desktop/src/app.ts', 'desktop source')
  put('apps/shared/src/client.ts', 'wire client')
  put('scripts/build/desktop.mjs', 'desktop compiler')
  put('package.json', '{}')
  put('package-lock.json', '{}')
  return { root, put, cleanup: () => rmSync(root, { recursive: true, force: true }) }
}

test('desktop build identity does not depend on an agent source tree or Python environment', () => {
  const { root, put, cleanup } = fixture()
  try {
    const before = sourceHash(root, 'desktop')
    put('pyproject.toml', 'agent dependencies')
    put('uv.lock', 'Python lock')
    put('pm/lock.json', 'Hermes runtime toolchain')
    put('agent/turn.py', 'backend implementation')
    put('scripts/build/agent.py', 'backend builder')
    assert.equal(sourceHash(root, 'desktop'), before)
    put('apps/shared/src/client.ts', 'updated wire client')
    assert.notEqual(sourceHash(root, 'desktop'), before)
  } finally { cleanup() }
})

test('desktop native source and build tooling invalidate products; generated outputs do not', () => {
  const { root, put, cleanup } = fixture()
  try {
    const before = sourceHash(root, 'desktop')
    put('apps/desktop/dist/electron-main.mjs', 'compiled output')
    put('apps/desktop/build/install-stamp.json', '{}')
    assert.equal(sourceHash(root, 'desktop'), before)
    put('apps/desktop/electron/native/monitor.c', 'native source')
    const native = sourceHash(root, 'desktop')
    assert.notEqual(native, before)
    put('scripts/build/desktop.mjs', 'updated compiler')
    assert.notEqual(sourceHash(root, 'desktop'), native)
  } finally { cleanup() }
})
