import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { test } from 'vitest'

import { applyDesktopIdentity, PRODUCT_IDENTITY } from './product-identity'

test('standalone desktop has an independent OS and deep-link identity', () => {
  assert.equal(PRODUCT_IDENTITY.appId, 'io.github.nggurbanov.hermes-desktop-rune')
  assert.equal(PRODUCT_IDENTITY.protocolScheme, 'hermes-rune')
  assert.equal(PRODUCT_IDENTITY.light, false)
  assert.equal(PRODUCT_IDENTITY.store, false)
})

test('desktop preferences are pinned outside the official app state before startup', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'standalone-userdata-'))
  const paths: Record<string, string> = { appData: root, userData: path.join(root, 'Hermes') }
  let name = 'Hermes'

  try {
    const result = applyDesktopIdentity({
      getPath: key => paths[key],
      setPath: (key, value) => { paths[key] = value },
      setName: value => { name = value }
    })

    assert.equal(result, PRODUCT_IDENTITY.displayName)
    assert.equal(name, PRODUCT_IDENTITY.displayName)
    assert.equal(paths.userData, path.join(root, 'HermesDesktopRune'))
    assert.ok(fs.statSync(paths.userData).isDirectory())
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})
