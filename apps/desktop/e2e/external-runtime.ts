import fs from 'node:fs'
import path from 'node:path'

const CLIENT_ROOT = path.resolve(import.meta.dirname, '../../..')

interface ExternalRuntime {
  root?: string
  launcher?: string
  python?: string
}

/** Tests select an installation explicitly; the desktop checkout never supplies an agent. */
export function externalRuntime(
  env: NodeJS.ProcessEnv = process.env,
  clientRoot = CLIENT_ROOT
): ExternalRuntime {
  const configuredRoot = env.HERMES_DESKTOP_HERMES_ROOT?.trim()
  const launcher = env.HERMES_DESKTOP_HERMES?.trim()
  const python = env.HERMES_E2E_PYTHON?.trim() || env.HERMES_DESKTOP_PYTHON?.trim()
  let root: string | undefined

  if (configuredRoot) {
    root = fs.realpathSync(configuredRoot)
    const relative = path.relative(fs.realpathSync(clientRoot), root)

    if (!relative || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative))) {
      throw new Error('E2E Hermes runtime must be installed outside the desktop repository')
    }

    if (!fs.existsSync(path.join(root, 'hermes_cli', 'main.py'))) {
      throw new Error(`Selected external Hermes root has no hermes_cli/main.py: ${root}`)
    }
  }

  return { root, launcher, python }
}

/** Only connection selectors cross into the app; HOME and credentials belong to its sandbox. */
export function externalRuntimeEnvironment(
  env: NodeJS.ProcessEnv = process.env,
  clientRoot = CLIENT_ROOT
): Record<string, string> {
  const { root, launcher, python } = externalRuntime(env, clientRoot)

  return {
    ...(root ? { HERMES_DESKTOP_HERMES_ROOT: root } : {}),
    ...(launcher ? { HERMES_DESKTOP_HERMES: launcher } : {}),
    ...(python ? { HERMES_DESKTOP_PYTHON: python } : {})
  }
}

/** Direct backend probes need both the external source and its prepared interpreter. */
export function externalPythonRuntime(env: NodeJS.ProcessEnv = process.env): { root: string; python: string } {
  const { root, python } = externalRuntime(env)

  if (!root || !python) {
    throw new Error(
      'This integration test needs an external Hermes installation. Set HERMES_DESKTOP_HERMES_ROOT '
      + 'to its checkout and HERMES_E2E_PYTHON to its prepared Python interpreter.'
    )
  }

  return { root, python }
}
