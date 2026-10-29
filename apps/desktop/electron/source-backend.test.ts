import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { test } from 'vitest'

import { createSourcePythonBackend } from './source-backend'

test('Windows console selection uses only the selected interpreter directory', (): void => {
  const temp: string = fs.mkdtempSync(path.join(os.tmpdir(), 'desktop-console-policy-'))
  const root: string = path.join(temp, 'repo')
  const selected: string = path.join(temp, 'external', 'pythonw.exe')
  const consolePython: string = path.join(path.dirname(selected), 'python.exe')

  try {
    fs.mkdirSync(path.join(root, 'venv', 'Scripts'), { recursive: true })
    fs.writeFileSync(path.join(root, 'venv', 'Scripts', 'python.exe'), 'stale')
    fs.mkdirSync(path.dirname(selected), { recursive: true })
    fs.writeFileSync(selected, '')
    assert.equal(createSourcePythonBackend(root, selected, [], { isWindows: true })?.command, selected)
    fs.writeFileSync(consolePython, '')
    const backend = createSourcePythonBackend(root, selected, ['serve'], { isWindows: true })
    assert.equal(backend?.command, consolePython)
    assert.equal(backend?.env.PYTHONPATH, root)
    assert.equal(backend?.env.PYTHONHOME, '')
    assert.equal(createSourcePythonBackend(root, selected, [], { isWindows: false })?.command, selected)
    assert.equal(createSourcePythonBackend(root, null, []), null)
  } finally {
    fs.rmSync(temp, { recursive: true, force: true })
  }
})
