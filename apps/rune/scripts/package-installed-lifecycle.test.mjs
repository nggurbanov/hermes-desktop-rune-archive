import { test } from 'node:test'
import assert from 'node:assert/strict'
import { desktopFields, verify } from './package-installed-lifecycle.mjs'
import { mkdtemp, writeFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

test('desktop fields preserve field-code arguments', () => {
  assert.deepEqual(desktopFields('[Desktop Entry]\nExec=hermes-desktop-rune %u\nIcon=example\n'), { Exec: 'hermes-desktop-rune %u', Icon: 'example' })
})
test('hash verification rejects altered inputs', async () => {
  const root = await mkdtemp(join(tmpdir(), 'lifecycle-unit-')); const path = join(root, 'fixture')
  try { await writeFile(path, 'abc'); await verify(path, 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad'); await writeFile(path, 'changed'); await assert.rejects(verify(path, 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad')) } finally { await rm(root, { recursive: true }) }
})
