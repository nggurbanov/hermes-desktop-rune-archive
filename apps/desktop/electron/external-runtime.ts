import fs from 'node:fs'
import path from 'node:path'

export const EXTERNAL_RUNTIME_REQUIRED_MESSAGE =
  'No local Hermes installation was found. Install Hermes separately, then check again, or connect to an existing remote gateway.'

interface ExternalRuntimeOptions<Backend> {
  desktopRoot: string
  overrideRoot?: string
  overrideCommand?: string
  installedRoots: Array<string | null | undefined>
  pathCommand?: string | null
  resolveRoot: (root: string, explicit: boolean) => Promise<Backend | null>
  resolveCommand: (command: string, explicit: boolean) => Promise<Backend | null>
}

function canonicalPath(value: string): string {
  const resolved = path.resolve(value)

  try {
    return fs.realpathSync(resolved)
  } catch {
    return resolved
  }
}

/** The desktop checkout is never an implicit or explicitly selected agent installation. */
export function isDesktopRuntimeRoot(root: string, desktopRoot: string): boolean {
  const relative = path.relative(canonicalPath(desktopRoot), canonicalPath(root))

  return relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative))
}

/** Discover existing installations only. No candidate ever triggers an installer. */
export async function resolveExternalRuntime<Backend extends { root?: string; command?: string | null }>({
  desktopRoot,
  overrideRoot,
  overrideCommand,
  installedRoots,
  pathCommand,
  resolveRoot,
  resolveCommand
}: ExternalRuntimeOptions<Backend>): Promise<Backend | null> {
  // Launchers can live outside this checkout while reporting an in-tree install.
  const external = (backend: Backend | null): Backend | null =>
    backend?.root && isDesktopRuntimeRoot(backend.root, desktopRoot) ? null : backend

  if (overrideRoot) {
    if (isDesktopRuntimeRoot(overrideRoot, desktopRoot)) {
      throw new Error('Choose a Hermes installation outside the desktop source checkout.')
    }

    const backend = external(await resolveRoot(overrideRoot, true))

    if (!backend) {
      throw new Error(`The selected Hermes installation is not usable: ${overrideRoot}`)
    }

    return backend
  }

  if (overrideCommand) {
    const backend = external(await resolveCommand(overrideCommand, true))

    if (!backend) {
      throw new Error(`The selected Hermes command is not usable: ${overrideCommand}`)
    }

    return backend
  }

  const checked = new Set<string>()

  for (const root of installedRoots) {
    if (!root || isDesktopRuntimeRoot(root, desktopRoot)) {
      continue
    }

    const canonical = canonicalPath(root)

    if (checked.has(canonical)) {
      continue
    }

    checked.add(canonical)
    const backend = external(await resolveRoot(root, false))

    if (backend) {
      return backend
    }
  }

  return pathCommand ? external(await resolveCommand(pathCommand, false)) : null
}
