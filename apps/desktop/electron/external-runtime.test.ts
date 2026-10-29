import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { test } from 'vitest'

import { resolveExternalRuntime } from './external-runtime'

test('desktop discovery skips its own checkout and attaches to a separate installation', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'desktop-external-runtime-'))
  const desktop = path.join(root, 'desktop')
  const installed = path.join(root, 'installed-hermes')
  const probes: string[] = []
  fs.mkdirSync(desktop)
  fs.mkdirSync(installed)

  try {
    const backend = await resolveExternalRuntime({
      desktopRoot: desktop,
      installedRoots: [desktop, path.join(desktop, 'agent'), installed],
      resolveRoot: async candidate => {
        probes.push(candidate)

        return { command: path.join(candidate, 'hermes') }
      },
      resolveCommand: async () => assert.fail('installed runtime must precede PATH')
    })

    assert.deepEqual(probes, [installed])
    assert.deepEqual(backend, { command: path.join(installed, 'hermes') })
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test('missing runtime resolves without an install and explicit unusable selections fail', async () => {
  const probes: string[] = []

  const options = {
    desktopRoot: path.resolve('desktop'),
    installedRoots: [path.resolve('external-hermes')],
    resolveRoot: async (candidate: string) => { probes.push(candidate);

 return null },
    resolveCommand: async (command: string) => { probes.push(command);

 return null }
  }

  assert.equal(await resolveExternalRuntime(options), null)
  await assert.rejects(resolveExternalRuntime({ ...options, overrideRoot: options.desktopRoot }), /outside the desktop/)
  await assert.rejects(resolveExternalRuntime({ ...options, overrideCommand: 'selected-hermes' }), /selected Hermes command/)
  assert.deepEqual(probes, [path.resolve('external-hermes'), 'selected-hermes'])
})

test('an external launcher cannot select an installation inside the desktop checkout', async () => {
  const desktop = path.resolve('desktop')

  const options = {
    desktopRoot: desktop,
    installedRoots: [],
    resolveRoot: async () => null,
    resolveCommand: async () => ({ command: '/usr/local/bin/hermes', root: desktop })
  }

  assert.equal(await resolveExternalRuntime({ ...options, pathCommand: 'hermes' }), null)
  await assert.rejects(
    resolveExternalRuntime({ ...options, overrideCommand: '/usr/local/bin/hermes' }),
    /selected Hermes command/
  )
})
