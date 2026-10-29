import assert from 'node:assert/strict'
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises'
import { createRequire } from 'node:module'
import os from 'node:os'
import path from 'node:path'
import { test } from 'vitest'

import { buildStampPayload, writeDesktopStamp } from './write-build-stamp.mjs'
import { stageMsixAssets, writeMsixExtensions } from './before-build.mjs'

const require = createRequire(import.meta.url)
const identity = require('../product-identity.cjs')
const config = require('../electron-builder.config.cjs')
const { parseXml } = require('builder-util-runtime')
const provenance = { commit: 'a'.repeat(40), branch: 'main', dirty: false, source: 'local' }

test('desktop packaging owns a distinct identity and contains no agent runtime', () => {
  assert.equal(config.appId, identity.appId)
  assert.notEqual(config.appId, 'com.nousresearch.hermes')
  assert.deepEqual(config.protocols[0].schemes, ['hermes-rune'])
  assert.deepEqual(config.publish, [{ provider: 'github', owner: 'nggurbanov', repo: 'hermes-desktop-rune' }])
  assert.deepEqual(config.extraResources, [{ from: 'build/install-stamp.json', to: 'install-stamp.json' }])
  assert.equal(config.extraMetadata.productName, identity.displayName)
})

test('desktop stamp owns its version and writes only app provenance', async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'desktop-provenance-'))
  try {
    const stamp = buildStampPayload(provenance, { HERMES_PAYLOAD_VERSION: '99.0.0', HERMES_PAYLOAD_TAG: 'v99.0.0' })
    const manifest = require('../package.json')
    assert.equal(stamp.payload, 'standalone')
    assert.equal(stamp.displayVersion, manifest.version)
    assert.equal(stamp.updateMechanism, 'external')
    assert.equal(stamp.runtime, undefined)
    writeDesktopStamp(directory, stamp)
    assert.deepEqual(await readdir(directory), ['install-stamp.json'])
    assert.deepEqual(JSON.parse(await readFile(path.join(directory, 'install-stamp.json'), 'utf8')), stamp)
    assert.throws(() => buildStampPayload(provenance, { HERMES_DESKTOP_VARIANT: 'bundled' }), /standalone/)
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})

test('Windows asset preparation needs only desktop artwork', async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'desktop-msix-'))
  const names = ['Square44x44Logo.png', 'Square150x150Logo.png', 'StoreLogo.png', 'Wide310x150Logo.png']
  try {
    await mkdir(path.join(directory, 'assets/appx'), { recursive: true })
    for (const name of names) await writeFile(path.join(directory, 'assets/appx', name), name)
    stageMsixAssets(directory)
    assert.deepEqual((await readdir(path.join(directory, 'build/appx'))).sort(), [...names].sort())
    assert.deepEqual(await readdir(path.join(directory, 'build')), ['appx'])
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})

test('Windows Copilot key activates this client and its hold events without agent aliases', async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'desktop-copilot-'))
  try {
    const displayName = 'Hermes & "Friends" <alpha>'
    const output = writeMsixExtensions(directory, { ...identity, displayName })
    assert.equal(output, path.join(directory, config.msix.customExtensionsPath))
    const extension = parseXml(await readFile(output, 'utf8'))
    assert.equal(extension.attribute('Category'), 'windows.appExtension')
    const provider = extension.element('uap3:AppExtension')
    assert.equal(provider.attribute('Name'), 'com.microsoft.windows.copilotkeyprovider')
    assert.equal(provider.attribute('DisplayName'), displayName)
    const properties = provider.element('uap3:Properties')
    assert.equal(properties.element('SingleTap').value, 'hermes-rune://copilot-key/start?state=Tap')
    assert.equal(properties.element('PressAndHoldStart').value, 'hermes-rune://copilot-key/start?state=Down')
    assert.equal(properties.element('PressAndHoldStop').value, 'hermes-rune://copilot-key/stop?state=Up')
    assert.equal(extension.getElements('uap5:Extension').length, 0)
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})
