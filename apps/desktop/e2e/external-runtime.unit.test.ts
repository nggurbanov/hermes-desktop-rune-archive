import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, expect, test } from 'vitest'

import { externalPythonRuntime, externalRuntime, externalRuntimeEnvironment } from './external-runtime'

let scratch: string
let client: string
let runtime: string

beforeEach(() => {
  scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'external-runtime-'))
  client = path.join(scratch, 'desktop')
  runtime = path.join(scratch, 'hermes')
  fs.mkdirSync(client)
  fs.mkdirSync(path.join(runtime, 'hermes_cli'), { recursive: true })
  fs.writeFileSync(path.join(runtime, 'hermes_cli', 'main.py'), '')
})

afterEach(() => fs.rmSync(scratch, { recursive: true, force: true }))

test('no selector leaves the desktop free to connect remotely without a local runtime', () => {
  expect(externalRuntimeEnvironment({ HERMES_HOME: '/user-state', OPENAI_API_KEY: 'unused' }, client)).toEqual({})
})

test('an external installation crosses as connection selectors, without inherited user state', () => {
  const env = {
    HERMES_DESKTOP_HERMES_ROOT: runtime,
    HERMES_DESKTOP_HERMES: '/external/bin/hermes',
    HERMES_E2E_PYTHON: '/external/bin/python',
    HOME: '/real-home',
    HERMES_HOME: '/real-agent',
    OPENAI_API_KEY: 'unused'
  }

  expect(externalRuntimeEnvironment(env, client)).toEqual({
    HERMES_DESKTOP_HERMES_ROOT: fs.realpathSync(runtime),
    HERMES_DESKTOP_HERMES: env.HERMES_DESKTOP_HERMES,
    HERMES_DESKTOP_PYTHON: env.HERMES_E2E_PYTHON
  })
})

test('the client checkout and nested checkouts cannot become the test agent', () => {
  expect(() => externalRuntime({ HERMES_DESKTOP_HERMES_ROOT: client }, client)).toThrow('outside the desktop repository')
  const nested = path.join(client, 'agent')
  fs.mkdirSync(nested)
  expect(() => externalRuntime({ HERMES_DESKTOP_HERMES_ROOT: nested }, client)).toThrow('outside the desktop repository')
})

test.skipIf(process.platform === 'win32')('a symlink cannot disguise the client checkout as an external agent', () => {
  const link = path.join(scratch, 'external-link')
  fs.symlinkSync(client, link)
  expect(() => externalRuntime({ HERMES_DESKTOP_HERMES_ROOT: link }, client)).toThrow('outside the desktop repository')
})

test('a root without the Hermes entry point is rejected before launch', () => {
  expect(() => externalRuntime({ HERMES_DESKTOP_HERMES_ROOT: scratch }, client)).toThrow('hermes_cli/main.py')
})

test('direct backend probes require an explicitly selected prepared interpreter and source', () => {
  expect(() => externalPythonRuntime({})).toThrow('external Hermes installation')
  expect(() => externalPythonRuntime({ HERMES_DESKTOP_HERMES: '/external/bin/hermes' })).toThrow('HERMES_E2E_PYTHON')
})
