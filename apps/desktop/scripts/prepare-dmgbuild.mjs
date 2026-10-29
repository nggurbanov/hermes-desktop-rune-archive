import fs from 'node:fs'
import path from 'node:path'

/**
 * Optional DMGs consume a separately supplied dmgbuild tool tree. ZIP packages
 * are the default and need no Python supplier or Hermes package manager.
 * @param {{ source: string, out: string, cache: string, binary?: string }} options
 * @returns {string}
 */
export function prepareDmgbuild({ out, binary }) {
  if (!binary) throw new Error('DMG packaging needs --dmgbuild pointing to a prepared dmgbuild tool tree; use zip to package without Python')
  const vendor = path.dirname(binary)
  if (path.basename(binary) !== 'dmgbuild' || !fs.existsSync(binary) ||
      !fs.statSync(binary).isFile() || !fs.existsSync(path.join(vendor, 'python/bin/python3'))) {
    throw new Error(`dmgbuild launcher or paired Python is missing: ${binary}`)
  }
  const destination = path.join(out, 'dmgbuild')
  fs.rmSync(destination, { recursive: true, force: true })
  fs.cpSync(vendor, destination, { recursive: true, verbatimSymlinks: true })
  return path.join(destination, 'dmgbuild')
}
