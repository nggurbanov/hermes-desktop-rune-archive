import assert from 'node:assert/strict'
import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { listPackage, extractFile } from '@electron/asar'

const require = createRequire(import.meta.url)
const identity = require('../product-identity.cjs')
const appRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const release = path.join(appRoot, 'release')
const arch = process.arch === 'arm64' ? 'arm64' : 'x64'
const packageDirs = process.platform === 'darwin'
  ? [`mac-${arch}/${identity.windowsExecutableName}.app`, `mac/${identity.windowsExecutableName}.app`]
  : [`${process.platform === 'win32' ? 'win' : 'linux'}-${arch === 'x64' ? '' : `${arch}-`}unpacked`]
const packaged = process.argv[2] || packageDirs.map(dir => path.join(release, dir)).find(existsSync)
assert.ok(packaged, `No unpacked app found under ${release}; run npm run pack first`)
const resources = path.join(packaged, process.platform === 'darwin' ? 'Contents/Resources' : 'resources')
const binary = path.join(packaged, process.platform === 'darwin'
  ? `Contents/MacOS/${identity.windowsExecutableName}`
  : `${identity.windowsExecutableName}${process.platform === 'win32' ? '.exe' : ''}`)

assert.ok(existsSync(binary), `Missing application binary: ${binary}`)
assert.ok(!existsSync(path.join(resources, 'agent-payload')), 'Agent payload must not be packaged')
const stamp = JSON.parse(readFileSync(path.join(resources, 'install-stamp.json'), 'utf8'))
assert.equal(stamp.payload, 'standalone')
assert.equal(stamp.updateMechanism, 'external')
const asar = path.join(resources, 'app.asar')
const files = listPackage(asar).map(name => name.replaceAll('\\', '/').replace(/^\/+/, ''))
assert.ok(files.includes('dist/index.html'), 'Missing renderer')
assert.ok(files.includes('dist/electron-main.mjs'), 'Missing Electron entry')
assert.ok(!files.some(name => /^(agent|gateway|hermes_cli|tui_gateway|pm|agent-payload)\//.test(name)),
  'Agent sources must not be packaged')
const manifest = JSON.parse(extractFile(asar, 'package.json').toString())
assert.equal(manifest.productName, identity.displayName)
const pty = path.join(resources, 'app.asar.unpacked/dist/node_modules/node-pty')
assert.ok(existsSync(path.join(pty, 'package.json')), 'Missing native terminal package')
assert.ok(readdirSync(pty, { recursive: true }).some(name => name.endsWith('.node')),
  'Missing native terminal binding')
console.log(`Verified standalone package: ${packaged}`)
