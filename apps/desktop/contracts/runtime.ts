import fs from 'node:fs'
import path from 'node:path'

import { externalPythonRuntime } from '../e2e/external-runtime'

/** Contract tests require an intentional source + interpreter selection, never an ambient Python. */
export function contractRuntime(env: NodeJS.ProcessEnv = process.env): { root: string; python: string } {
  if (!env.HERMES_DESKTOP_HERMES_ROOT?.trim() || !env.HERMES_E2E_PYTHON?.trim()) {
    throw new Error(
      'test:external requires HERMES_DESKTOP_HERMES_ROOT pointing to a separately installed Hermes checkout '
      + 'and HERMES_E2E_PYTHON pointing to its prepared interpreter. It never provisions a runtime.'
    )
  }

  const runtime = externalPythonRuntime(env)

  if (!path.isAbsolute(runtime.python) || !fs.statSync(runtime.python).isFile()) {
    throw new Error('HERMES_E2E_PYTHON must name an existing absolute interpreter path')
  }

  return runtime
}
