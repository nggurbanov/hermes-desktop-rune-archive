import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, test } from 'vitest'

import { readUiPreferences, uiPreferencesPath, writeUiPreferences } from './ui-preferences'

const homes: string[] = []

afterEach(() => {
  for (const home of homes.splice(0)) {
    fs.rmSync(home, { force: true, recursive: true })
  }
})

function scratchHome(): string {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-ui-prefs-'))
  homes.push(home)

  return home
}

test('a client’s localStorage import only adds the keys a store lacks', async () => {
  const root = scratchHome()
  const file = uiPreferencesPath(root)

  await writeUiPreferences(file, { values: { 'hermes-desktop-mode-v1': 'dark' } })

  const seeded = await writeUiPreferences(file, {
    seed: true,
    values: { 'hermes-desktop-mode-v1': 'light', 'hermes-desktop-theme-v2': 'nous' }
  })

  assert.deepEqual(seeded.values, { 'hermes-desktop-mode-v1': 'dark', 'hermes-desktop-theme-v2': 'nous' })
  assert.deepEqual(readUiPreferences(file).values, seeded.values)
})

test.skipIf(process.platform === 'win32' || process.getuid?.() === 0)(
  'the native writer never merges over a store it cannot read',
  async () => {
    const home = scratchHome()
    const file = uiPreferencesPath(home)
    await writeUiPreferences(file, { values: { 'hermes-desktop-theme-v2': 'nous', 'hermes.desktop.keybinds': '{}' } })
    const before = fs.readFileSync(file, 'utf8')
    fs.chmodSync(file, 0)

    try {
      await assert.rejects(writeUiPreferences(file, { values: { 'hermes-desktop-mode-v1': 'dark' } }), /unreadable/)
    } finally {
      fs.chmodSync(file, 0o600)
    }

    assert.equal(fs.readFileSync(file, 'utf8'), before)
  }
)
