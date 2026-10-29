import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, expect, test, vi } from 'vitest'

const electron = vi.hoisted(() => ({
  paths: {} as Record<string, string>,
  handlers: new Map<string, (...args: unknown[]) => unknown>()
}))

vi.mock('electron', () => ({
  app: { getPath: (name: string) => electron.paths[name] },
  BrowserWindow: { getAllWindows: () => [], fromWebContents: () => null },
  dialog: {},
  ipcMain: {
    handle: (channel: string, handler: (...args: unknown[]) => unknown) => electron.handlers.set(channel, handler),
    on: (channel: string, handler: (...args: unknown[]) => unknown) => electron.handlers.set(channel, handler)
  },
  shell: { trashItem: vi.fn() }
}))

import { registerFsIpc } from './fs-ipc'
import { registerMachineUiPreferences } from './ui-preferences-ipc'

const roots: string[] = []
const disposers: (() => void)[] = []

afterEach(() => {
  for (const dispose of disposers.splice(0)) {
    dispose()
  }

  for (const root of roots.splice(0)) {
    fs.rmSync(root, { recursive: true, force: true })
  }

  electron.handlers.clear()
})

function setup() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-client-state-'))
  roots.push(root)
  const agentHome = path.join(root, 'agent')
  const userData = path.join(root, 'client')
  fs.mkdirSync(agentHome)
  fs.mkdirSync(userData)
  electron.paths = { home: root, userData }

  return { agentHome, userData }
}

test('desktop preference writes stay in client userData and leave the agent store intact', async () => {
  const { agentHome, userData } = setup()
  const agentStore = path.join(agentHome, 'ui-preferences.json')
  fs.writeFileSync(agentStore, 'agent-owned store')
  disposers.push(registerMachineUiPreferences())

  await electron.handlers.get('hermes:ui-preferences:write')!(
    {},
    {
      values: { 'hermes-desktop-theme-v2': 'nous' }
    }
  )

  const clientStore = JSON.parse(fs.readFileSync(path.join(userData, 'ui-preferences.json'), 'utf8'))
  expect(clientStore.values).toEqual({ 'hermes-desktop-theme-v2': 'nous' })
  expect(fs.readFileSync(agentStore, 'utf8')).toBe('agent-owned store')
})

test('plugin reconciliation and removal own client copies while leaving all agent plugins intact', async () => {
  const { agentHome, userData } = setup()
  const legacy = path.join(agentHome, 'profiles', 'bot', 'desktop-plugins', 'legacy', 'plugin.js')
  const source = path.join(agentHome, 'plugins', 'unified', 'desktop', 'plugin.js')

  for (const entry of [legacy, source]) {
    fs.mkdirSync(path.dirname(entry), { recursive: true })
    fs.writeFileSync(entry, 'export default {}')
  }

  registerFsIpc({
    hermesHome: agentHome,
    readActiveDesktopProfile: () => 'bot',
    expandUserPath: value => value,
    resolveRequestedPathForIpc: value => value,
    directoryExists: fs.existsSync,
    resolveGitBinary: () => 'git'
  })

  const pluginRoot = await electron.handlers.get('hermes:fs:desktopPluginsRoot')!({})
  expect(pluginRoot).toBe(path.join(userData, 'desktop-plugins'))
  expect(fs.readFileSync(path.join(String(pluginRoot), 'unified', 'plugin.js'), 'utf8')).toBe('export default {}')
  expect(fs.readFileSync(legacy, 'utf8')).toBe('export default {}')
  expect(fs.readFileSync(source, 'utf8')).toBe('export default {}')
  expect(fs.existsSync(path.join(agentHome, 'desktop-plugins'))).toBe(false)

  const standalone = path.join(String(pluginRoot), 'manual', 'plugin.js')
  fs.mkdirSync(path.dirname(standalone))
  fs.writeFileSync(standalone, 'client plugin')
  await expect(electron.handlers.get('hermes:plugin:removeDesktop')!({}, { name: 'manual' })).resolves.toMatchObject({
    ok: true
  })
  expect(fs.existsSync(standalone)).toBe(false)
  await expect(electron.handlers.get('hermes:plugin:removeDesktop')!({}, { name: 'unified' })).resolves.toMatchObject({
    ok: false
  })
  expect(fs.readFileSync(source, 'utf8')).toBe('export default {}')
  expect(fs.existsSync(path.join(path.dirname(source), '.hermes-package.json'))).toBe(false)
})
