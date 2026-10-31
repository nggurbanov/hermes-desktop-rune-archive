import { copyFileSync, cpSync, existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

export const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..')
export const id = 'io.github.nggurbanov.hermes-desktop-rune'
export const product = 'Hermes Desktop Rune'
export const xml = value => String(value).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;').replaceAll("'", '&apos;')
export function options(argv) {
  const result = {}
  for (let i = 0; i < argv.length; i += 2) {
    if (!argv[i]?.startsWith('--') || !argv[i + 1] || argv[i + 1].startsWith('--')) throw new Error('Expected --name value pairs')
    const key = argv[i].slice(2)
    if (key in result) throw new Error(`Duplicate option: ${key}`)
    result[key] = argv[i + 1]
  }
  return result
}
export function required(o, key) { if (!o[key]) throw new Error(`Missing --${key}`); return o[key] }
export function version(value) {
  if (!/^\d+\.\d+\.\d+(?:\.\d+)?$/.test(value) || value.split('.').some(n => Number(n) > 65535)) throw new Error('Version must have 3 or 4 numeric components, each <= 65535')
  return value
}
export function directory(path) { mkdirSync(path, { recursive: true }); return path }
export function copy(source, destination) { directory(dirname(destination)); copyFileSync(source, destination) }
export function run(command, args, opts = {}) {
  const r = spawnSync(command, args, { stdio: 'inherit', ...opts })
  if (r.error || r.status !== 0) throw new Error(`${command} failed: ${r.error?.message || `exit ${r.status}`}`)
}
export function inputs(o) {
  const binary = resolve(required(o, 'binary')); const out = resolve(required(o, 'out'))
  if (!statSync(binary).isFile()) throw new Error('binary must be a regular file')
  if (existsSync(out)) throw new Error(`Output already exists: ${out}`)
  return { binary, out, version: version(required(o, 'version')) }
}
export function notices(dest, sourceRoot = root) {
  copy(resolve(sourceRoot, 'LICENSE'), resolve(dest, 'LICENSE'))
  copy(resolve(sourceRoot, 'apps/rune/THIRD_PARTY_NOTICES.md'), resolve(dest, 'THIRD_PARTY_NOTICES.md'))
  cpSync(resolve(sourceRoot, 'apps/rune/licenses'), resolve(dest, 'licenses'), { recursive: true })
  copy(resolve(sourceRoot, 'apps/rune/assets/plugin-host/THIRD_PARTY_LICENSES.txt'), resolve(dest, 'PLUGIN-HOST-LICENSES.txt'))
  for (const crate of ['symphonia-core', 'symphonia-metadata', 'symphonia-format-riff', 'symphonia-format-isomp4']) {
    cpSync(resolve(sourceRoot, 'apps/rune/vendor', crate), resolve(dest, 'modified-source', crate), { recursive: true })
  }
  writeFileSync(resolve(dest, 'SOURCE-AND-RUNTIME.txt'), 'Modified MPL-covered Symphonia sources are included in modified-source. Obtain the complete corresponding application source archive alongside a release. This client does not include a Hermes agent. Linux requires compatible host GTK3, WebKitGTK 4.1, ALSA, D-Bus and graphics libraries; Windows requires WebView2 and applicable VC runtime.\n')
}
