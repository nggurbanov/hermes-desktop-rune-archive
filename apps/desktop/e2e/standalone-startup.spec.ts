import fs from 'node:fs'
import path from 'node:path'

import { buildAppEnv, createSandbox, launchDesktop } from './fixtures'
import { expect, test } from './test'

interface SetupWindow extends Window {
  hermesDesktop: {
    getBootstrapState: () => Promise<{ active: boolean; setupChoice?: { local?: string } }>
  }
}

test('starts without a local agent and offers an existing remote connection', async () => {
  const sandbox = createSandbox('standalone-no-agent')
  const { app, page } = await launchDesktop(buildAppEnv(sandbox, {
    HERMES_DESKTOP_IGNORE_EXISTING: '1',
    HERMES_DESKTOP_HERMES_ROOT: '',
    HERMES_DESKTOP_HERMES: '',
    HERMES_DESKTOP_PYTHON: ''
  }))

  try {
    await expect(page.getByRole('heading', { name: 'Set up Hermes Desktop' })).toBeVisible()
    await expect(page.getByRole('button', { name: /Connect to existing Hermes/ })).toBeVisible()
    await expect(page.getByRole('button', { name: /Check for local Hermes/ })).toBeVisible()

    const state = await page.evaluate(() => (window as unknown as SetupWindow).hermesDesktop.getBootstrapState())
    expect(state.active).toBe(false)
    expect(state.setupChoice?.local).toBe('unavailable')
    expect(fs.existsSync(path.join(sandbox.hermesHome, 'hermes-agent'))).toBe(false)

    await page.getByRole('button', { name: /Connect to existing Hermes/ }).click()
    await expect(page.getByRole('heading', { name: 'Connect to existing Hermes' })).toBeVisible()
  } finally {
    await app.close()
    sandbox.cleanup()
  }
})
