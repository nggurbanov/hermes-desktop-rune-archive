#!/usr/bin/env node
// Private manual AppImage extraction/replacement acceptance; never installs on the host.
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { createReadStream, existsSync, mkdirSync, readdirSync, readFileSync, readlinkSync, renameSync, rmSync, writeFileSync, copyFileSync, chmodSync } from 'node:fs'
import { mkdtemp } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { spawn, spawnSync } from 'node:child_process'
import { createServer } from 'node:http'
import { WebSocketServer } from 'ws'
import { fileURLToPath } from 'node:url'

export const ID = 'io.github.nggurbanov.hermes-desktop-rune'
export function desktopFields(text) {
  return Object.fromEntries(text.split('\n').filter(line => line.includes('=')).map(line => { const i = line.indexOf('='); return [line.slice(0, i), line.slice(i + 1)] }))
}
export async function sha(path) {
  const hash = createHash('sha256')
  for await (const chunk of createReadStream(path)) hash.update(chunk)
  return hash.digest('hex')
}
export async function verify(path, expected) { const actual = await sha(path); assert.equal(actual, expected, `SHA-256 mismatch: ${path}`); return actual }
function command(program, args, options = {}) {
  const p = spawnSync(program, args, { encoding: 'utf8', maxBuffer: 16 * 1024 * 1024, ...options })
  assert.equal(p.status, 0, `${program}: ${p.stderr || p.error}`)
  return p.stdout
}
const versions = [
  { name: 'old', dir: '/tmp/Hermes abce checkpoint ü', checkpoint: '/tmp/hermes-rune-linux-checkpoint-xdf4iyiu', image: 'db14636eeb895c90e0cb96ecc01824f043a4b099323fa55ebca9ae1da59659f6', binary: 'abce8b7eb193fbebc4f98b8c52a7483133d1b6b6b25ae0b438e49693349d75a0', archive: '0f6a1149502e5900002d4f64465a6ec05249c9d1594aa65275c091b8f5d04c6e', manifest: '04cb798745a2a30cf05e50af975159c1de54b5d46f6a02693b6807e6a0a9eaa9', manifestName: 'source-manifest.json' },
  { name: 'new', dir: '/tmp/Hermes combined sdk ü 1793372534', checkpoint: '/tmp/hermes-rune-combined-20261030-1793372534', image: '40cc7348cac939d7172096e45e4b27ef60c83f38c75e0b5c04fdf909552ffdf0', binary: 'dc20959bb9e79fd2201edd9603af397cb637d314282de66ce6ce275a92aae455', archive: 'aef3a0ad4ff671f7123793ffeabc6e9aecdd88d9ce929f30d0301ba3904ac635', manifest: '3161695692ac8800787bdc8f3a1838cdfa84a245d48bd0214bce3b9edb48fed2', manifestName: 'manifest.json' },
]
function proc(pid) { const stat = readFileSync(`/proc/${pid}/stat`, 'utf8'); return { pid, startTicks: stat.slice(stat.lastIndexOf(')') + 2).split(' ')[19], exe: readlinkSync(`/proc/${pid}/exe`), argv: readFileSync(`/proc/${pid}/cmdline`, 'utf8').split('\0').filter(Boolean) } }
async function main() {
  const root = await mkdtemp(join(tmpdir(), 'hermes-rune-lifecycle-'))
  const prefix = join(root, 'installed space 測試 ü'); mkdirSync(prefix)
  const env = { PATH: '/usr/bin:/bin', HOME: join(root, 'home'), XDG_CONFIG_HOME: join(root, 'config'), XDG_DATA_HOME: join(root, 'data'), XDG_CACHE_HOME: join(root, 'cache'), XDG_RUNTIME_DIR: join(root, 'runtime'), LANG: 'C.UTF-8' }
  for (const path of Object.values(env).slice(1, 6)) mkdirSync(path, { recursive: true, mode: 0o700 })
  const sentinels = [join(env.XDG_CONFIG_HOME, 'client-sentinel'), join(env.XDG_DATA_HOME, 'client-sentinel'), join(prefix, 'unrelated.txt')]
  for (const p of sentinels) writeFileSync(p, 'preserve fixture\n')
  const report = { root, prefix, cases: [], limitations: ['Manual AppImage extraction and placement only; no installer, rollback, signed updater, GUI, tray, protocol activation, or macOS/Windows tested.', 'Bare desktop Exec requires appropriate PATH or desktop integration; no host registration was attempted.', 'Hashes bind package content to recorded inputs, not reproducible build or authenticity.'] }
  const save = () => { writeFileSync(join(root, 'report.json'), JSON.stringify(report, null, 2) + '\n'); writeFileSync(join(root, 'report.txt'), `${report.cases.map(c => `${c.status.toUpperCase()} ${c.name}: ${JSON.stringify(c.details)}`).join('\n')}\n\n${report.limitations.join('\n')}\n`) }
  async function test(name, fn) { try { const details = await fn(); report.cases.push({ name, status: 'passed', details }); save(); return details } catch (e) { report.cases.push({ name, status: 'failed', details: String(e.stack || e) }); save(); throw e } }
  const server = createServer((req, res) => { res.writeHead(403); res.end() })
  const ws = new WebSocketServer({ server, path: '/api/ws' }); let calls = [], fault, observed, childPid
  ws.on('connection', (socket, req) => {
    if (req.headers.authorization || new URL(req.url, 'http://localhost').searchParams.has('token')) { fault = 'credential received'; socket.close(); return }
    try { observed = proc(childPid) } catch (e) { fault = String(e) }
    socket.on('message', bytes => { const frame = JSON.parse(bytes); calls.push(frame.method); const results = { 'profiles.list': { profiles: [{ name: 'default' }] }, 'session.list': { sessions: [] } }; if (!Object.hasOwn(results, frame.method)) { fault = `forbidden method ${frame.method}`; socket.close(); return } socket.send(JSON.stringify({ jsonrpc: '2.0', id: frame.id, result: results[frame.method] })) })
  })
  await new Promise(r => server.listen(0, '127.0.0.1', r))
  const gateway = `http://127.0.0.1:${server.address().port}`
  async function launch(path, expected) {
    calls = []; fault = null; observed = null
    const args = ['--check-connection', '--gateway', gateway]; const child = spawn(path, args, { env }); childPid = child.pid
    let stdout = '', stderr = ''; child.stdout.on('data', b => { stdout += b }); child.stderr.on('data', b => { stderr += b })
    const timer = setTimeout(() => child.kill(), 15000)
    const result = await new Promise((res, rej) => { child.on('error', rej); child.on('close', (code, signal) => res({ code, signal })) }); clearTimeout(timer)
    assert.equal(result.code, 0, stderr); assert.ifError(fault); assert.equal(observed.exe, expected); assert.deepEqual(calls, ['profiles.list', 'session.list'])
    const response = JSON.parse(stdout.trim()); assert.equal(response.connected, true); assert.equal(response.read_only, true)
    return { requested: path, observed, env, calls, response, exit: result }
  }
  try {
    for (const v of versions) {
      await test(`${v.name} frozen input hashes`, async () => {
        const paths = [[join(v.dir, 'Hermes.AppImage'), v.image], [join(v.checkpoint, 'hermes-rune'), v.binary], [join(v.checkpoint, 'source.tar.gz'), v.archive], [join(v.checkpoint, v.manifestName), v.manifest]]
        for (const [p, h] of paths) await verify(p, h)
        return Object.fromEntries(paths)
      })
      const image = join(root, `${v.name}.AppImage`); copyFileSync(join(v.dir, 'Hermes.AppImage'), image); chmodSync(image, 0o755); await verify(image, v.image)
      const staging = join(root, `${v.name}-staging`); mkdirSync(staging)
      await test(`${v.name} AppImage extraction and source correspondence`, async () => {
        command(image, ['--appimage-extract'], { cwd: staging, env })
        const tree = join(staging, 'squashfs-root'); const exe = join(tree, 'usr/bin/hermes-desktop-rune'); await verify(exe, v.binary)
        const doc = join(tree, 'usr/share/doc/hermes-desktop-rune'); const source = join(root, `${v.name}-source`); mkdirSync(source)
        command('tar', ['-xzf', join(v.checkpoint, 'source.tar.gz'), '-C', source], { env })
        for (const [a, b] of [['LICENSE', 'LICENSE'], ['THIRD_PARTY_NOTICES.md', 'apps/rune/THIRD_PARTY_NOTICES.md'], ['PLUGIN-HOST-LICENSES.txt', 'apps/rune/assets/plugin-host/THIRD_PARTY_LICENSES.txt']]) assert.equal(await sha(join(doc, a)), await sha(join(source, b)), a)
        for (const [a, b] of [['licenses', 'apps/rune/licenses'], ...['symphonia-core', 'symphonia-metadata', 'symphonia-format-riff', 'symphonia-format-isomp4'].map(c => [`modified-source/${c}`, `apps/rune/vendor/${c}`])]) command('diff', ['-qr', join(doc, a), join(source, b)], { env })
        if (v.name === 'new') { await verify(join(doc, 'source.tar.gz'), v.archive); await verify(join(doc, 'source-manifest.json'), v.manifest) }
        assert.deepEqual(readdirSync(join(tree, 'usr/bin')), ['hermes-desktop-rune'])
        const desktop = desktopFields(readFileSync(join(tree, `${ID}.desktop`), 'utf8')); assert.equal(desktop.Exec, 'hermes-desktop-rune %u'); assert.equal(desktop.Icon, ID); assert.ok(existsSync(join(tree, `${ID}.png`)))
        const libs = command('ldd', [exe], { env }); assert.ok(!libs.includes('not found')); writeFileSync(join(root, `${v.name}-ldd.txt`), libs)
        return { executableSha256: v.binary, noticesMatchArchive: true, packagedBinEntries: ['hermes-desktop-rune'], desktop, appRun: readFileSync(join(tree, 'AppRun'), 'utf8'), hostLibraries: `${v.name}-ldd.txt` }
      })
      const installed = join(prefix, 'Hermes.AppDir'); if (v.name === 'new') rmSync(installed, { recursive: true })
      renameSync(join(staging, 'squashfs-root'), installed)
      const exe = join(installed, 'usr/bin/hermes-desktop-rune'); await verify(exe, v.binary)
      for (const entry of ['AppRun', 'usr/bin/hermes-desktop-rune']) await test(`${v.name} installed ${entry}`, () => launch(join(installed, entry), exe))
      await test(`${v.name} sentinels`, async () => { for (const p of sentinels) assert.equal(readFileSync(p, 'utf8'), 'preserve fixture\n'); return sentinels })
    }
    await test('original input hashes after client checks', async () => { for (const v of versions) for (const [p, h] of [[join(v.dir, 'Hermes.AppImage'), v.image], [join(v.checkpoint, 'hermes-rune'), v.binary], [join(v.checkpoint, 'source.tar.gz'), v.archive], [join(v.checkpoint, v.manifestName), v.manifest]]) await verify(p, h); return 'unchanged' })
    await test('private owned AppDir removal', async () => { rmSync(join(prefix, 'Hermes.AppDir'), { recursive: true }); assert.ok(!existsSync(join(prefix, 'Hermes.AppDir'))); for (const p of sentinels) assert.equal(readFileSync(p, 'utf8'), 'preserve fixture\n'); return { preserved: sentinels, removed: 'Hermes.AppDir' } })
  } finally { for (const client of ws.clients) client.terminate(); await new Promise(r => ws.close(r)); await new Promise(r => server.close(r)); report.server = { pid: process.pid, startTicks: proc(process.pid).startTicks, closed: true }; save(); console.log(`Evidence: ${root}`) }
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main()
