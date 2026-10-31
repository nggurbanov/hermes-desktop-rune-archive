#!/usr/bin/env node
/** Ordinary native Settings flows over real private metadata actors. Adapted from the ninth native acceptance skeleton. */
import assert from 'node:assert/strict'
import {createHash, randomUUID} from 'node:crypto'
import {createServer} from 'node:http'
import {spawn, execFileSync} from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import {fileURLToPath} from 'node:url'
import {WebSocketServer} from 'ws'
import {startPrivateCredentials} from './lib/private-credentials.mjs'
import {captureGeneration, inspectGeneration} from './lib/private-ssh-generation.mjs'
import readline from 'node:readline'
import net from 'node:net'

const app = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const args = process.argv.slice(2)
const option = name => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : null }
const binary = path.resolve(option('--binary') ?? path.join(app, 'target/debug/hermes-rune'))
const out = path.resolve(option('--out') ?? fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-legacy-settings-evidence-')))
const only = option('--case')
let bridge, fixture, credentials, bridgeLog = '', bridgeId = 0; const responses = new Map(), evidence = []
const gateways = {A: {endpoint: ''}, B: {endpoint: ''}}
fs.mkdirSync(out, {recursive: true, mode: 0o700})
fs.chmodSync(out, 0o700)
assert.equal(fs.readdirSync(out).length, 0, 'Use a fresh evidence directory')
const suiteHome = fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-legacy-settings-home-'))
const owned = [], requests = [], screenshots = [], gestures = [], bindings = [], results = [], fixtures = []
const IDS = {A: '11111111-1111-4111-8111-111111111111', B: '22222222-2222-4222-8222-222222222222'}
const REVISIONS = {A: '33333333-3333-4333-8333-333333333333', B: '44444444-4444-4444-8444-444444444444'}
const names = ['default', 'research', 'archive', ...Array.from({length: 14}, (_, i) => `profile${i + 1}`)]
const catalog = {provider: 'fixture', model: 'fixture-model', providers: [{slug: 'fixture', name: 'Private fixture', authenticated: true, models: ['fixture-model']}]}
const delay = ms => new Promise(resolve => setTimeout(resolve, ms))
const hash = value => createHash('sha256').update(value).digest('hex')
const harnessSha = hash(fs.readFileSync(fileURLToPath(import.meta.url)))
const binarySha = hash(fs.readFileSync(binary))
if (option('--expect-sha')) assert.equal(binarySha, option('--expect-sha'), 'Immutable binary SHA mismatch')
fs.copyFileSync(fileURLToPath(import.meta.url), path.join(out, 'harness.mjs'))
let env, active = null, phase = 'setup', failure = null, displayProcess, wm, bus
const scoped = () => ({case: active?.name ?? 'suite', phase})
const atomic = (file, value) => {
  fs.mkdirSync(path.dirname(file), {recursive: true, mode: 0o700})
  const temporary = file + '.' + randomUUID()
  fs.writeFileSync(temporary, typeof value === 'string' || Buffer.isBuffer(value) ? value : JSON.stringify(value, null, 2), {mode: 0o600})
  fs.renameSync(temporary, file)
}

function input(...values) { return execFileSync('xdotool', values.map(String), {env: active?.env ?? env, encoding: 'utf8', timeout: 5000, stdio: ['ignore', 'pipe', 'pipe']}) }
function snapshots() {
  if (!active || !fs.existsSync(active.inspection)) return []
  return fs.readdirSync(active.inspection).filter(file => file.endsWith('.json')).flatMap(file => {
    try { return [JSON.parse(fs.readFileSync(path.join(active.inspection, file), 'utf8'))] } catch { return [] }
  })
}
const current = id => snapshots().find(snapshot => snapshot.id === id)
async function wait(label, check, ms = 20000) {
  const end = Date.now() + ms
  while (Date.now() < end) {
    if (failure) throw failure
    if (active?.child && active.child.exitCode != null) throw new Error(`${label}: app exited ${active.child.exitCode}\n${active.log.slice(-3500)}`)
    const result = check(); if (result) return result
    await delay(40)
  }
  throw new Error(`${label}: timed out in ${active?.name}/${phase}; snapshots=${JSON.stringify(snapshots())}\n${active?.log.slice(-3000)}`)
}
function visible() {
  try { return input('search', '--onlyvisible', '--pid', active.child.pid).trim().split('\n').filter(Boolean) } catch { return [] }
}
function geometry(xid) {
  const value = execFileSync('xwininfo', ['-id', String(xid)], {env: active.env, encoding: 'utf8', timeout: 5000})
  const read = name => { const result = value.match(new RegExp(name + ':\\s+(-?\\d+)')); assert.ok(result, `No ${name} in native geometry`); return Number(result[1]) }
  return {x: read('Absolute upper-left X'), y: read('Absolute upper-left Y'), width: read('Width'), height: read('Height')}
}
function nativeWindow(id) {
  const snapshot = current(id), windows = visible()
  assert.ok(snapshot?.native?.position, `Native frame not ready for ${id}`)
  const known = active.windows.get(id)
  if (known) { assert.ok(windows.includes(known), `Native window disappeared: ${id}`); return known }
  const xid = snapshot.native_raw_handle?.match(/window:\s*(\d+)/)?.[1]
  assert.ok(xid && windows.includes(xid), `Cannot bind ${id} to a visible native XID`)
  assert.ok(![...active.windows.values()].includes(xid), 'Two Viewers share one native window')
  active.windows.set(id, xid)
  bindings.push({...scoped(), id, xid, pid: active.child.pid, geometry: geometry(xid)})
  return xid
}
function focusXid(xid) {
  const tree = execFileSync('xwininfo', ['-id', xid, '-tree'], {env: active.env, encoding: 'utf8', timeout: 5000})
  const parent = tree.match(/Parent window id:\s+(0x[0-9a-f]+)/i)?.[1]
  assert.ok(parent)
  input('windowraise', parent)
  if (input('getwindowfocus').trim() !== xid) {
    input('windowactivate', xid); input('sleep', '0.2')
    if (input('getwindowfocus').trim() !== xid) input('windowfocus', '--sync', xid)
    input('sleep', '0.1')
  }
  assert.equal(input('getwindowfocus').trim(), xid)
}
function focus(id) { const xid = nativeWindow(id); focusXid(xid); return xid }
function key(id, chord) { focus(id); input('key', '--clearmodifiers', chord) }
function drawnFrame(id) {
  try { return JSON.parse(fs.readFileSync(path.join(active.inspection, 'frames', id + '.json'), 'utf8')) } catch { return null }
}
const presented = element => element && !element.exiting && !element.inert && element.opacity > 0.95 && element.visible[2] > 1 && element.visible[3] > 1
async function reveal(id, name) {
  const xid = focus(id)
  await delay(300)
  for (let attempt = 0; attempt < 28; attempt++) {
    const key = name
    const element = drawnFrame(id)?.elements.find(e => e.key === key && !e.exiting)
    if (presented(element)) {
      let stable = true, latest = element
      for (let sample = 0; sample < 3; sample++) {
        await delay(100)
        const next = drawnFrame(id)?.elements.find(e => e.key === key && !e.exiting)
        if (!presented(next) || next.visible.some((value, index) => Math.abs(value - latest.visible[index]) > 0.35)) { stable = false; break }
        latest = next
      }
      if (stable) return {xid, element: latest, key}
      continue
    }
    const enteringPicker = drawnFrame(id)?.elements.find(e => e.key === 'profile-picker' && !e.exiting)
    const picker = presented(enteringPicker) ? enteringPicker : null
    if ((enteringPicker && !picker) || (element && element.opacity < 0.95) || (!picker && !element && attempt < 4)) {
      await delay(100); continue
    }
    const frame = geometry(xid), settings = drawnFrame(id)?.elements.find(e => e.key === 'settings-page' && presented(e)), panel = drawnFrame(id)?.elements.find(e => e.key === 'legacy-routing-settings' && presented(e))
    const viewport = name.startsWith('settings-section-') ? [settings?.visible[0] ?? 238, 100, 175, Math.max(150, frame.height - 150)] : panel?.visible ?? [430, 100, Math.max(200, frame.width - 450), Math.max(100, frame.height - 180)]
    const direction = element && element.rect[1] < viewport[1] ? 4 : 5
    input('mousemove', frame.x + viewport[0] + Math.min(100, viewport[2] / 2), frame.y + viewport[1] + viewport[3] / 2, 'sleep', '0.05', 'click', '--repeat', '2', '--delay', '60', direction)
    gestures.push({...scoped(), id, name, resolved_key: key, scroll: direction, viewport})
    await delay(150)
  }
  throw new Error(`Native control cannot be revealed: ${name}`)
}
async function click(id, name, button = 1, inset = null) {
  const {xid, element, key} = await reveal(id, name)
  const rect = element.visible, frame = geometry(xid)
  const x = frame.x + rect[0] + (inset ?? rect[2] / 2), y = frame.y + rect[1] + rect[3] / 2
  assert.ok(rect[2] > 0 && rect[3] > 0 && x >= frame.x && x < frame.x + frame.width && y >= frame.y && y < frame.y + frame.height, `${name} is outside its native viewport`)
  input('mousemove', x, y, 'sleep', '0.08')
  const runtime = await wait(`native Runtime pointer agrees with X11 for ${name}`, () => {
    const value = drawnFrame(id)
    return value?.pointer && Math.abs(value.pointer[0] - (x - frame.x)) <= 2 && Math.abs(value.pointer[1] - (y - frame.y)) <= 2 && value
  }, 5000)
  const hit = runtime.elements.find(item => item.key === key && !item.exiting)
  assert.ok(presented(hit) && hit.visible.every((value, index) => Math.abs(value - rect[index]) <= 0.5), `Actual native target moved before click: ${name}`)
  gestures.push({...scoped(), id, xid, name, resolved_key: key, button, rect, drawn_rect: element.rect, pointer: input('getmouselocation', '--shell'), runtime_pointer: runtime.pointer, hold_ms: 40})
  input('mousedown', button, 'sleep', '0.04', 'mouseup', button)
  await delay(120)
}
async function type(id, value) {
  await click(id, 'composer-input', 1, 12)
  const xid = nativeWindow(id)
  assert.equal(input('getwindowfocus').trim(), xid)
  input('key', '--clearmodifiers', 'ctrl+a'); await delay(50)
  input('type', '--clearmodifiers', '--delay', '2', '--', value)
  await wait('actual entered draft', () => current(id)?.draft === value)
  assert.equal(input('getwindowfocus').trim(), xid)
}
function principal(source) {
  const inner = hash(`url:${new URL(gateways[source].endpoint).toString()}\0`)
  return hash(`saved:${IDS[source]}:${REVISIONS[source]}:${inner}`)
}
function sameSource(snapshot, source) { return snapshot?.principal === principal(source) }
async function ready(id, source, profile) {
  return wait(`connected ${source}/${profile}`, () => {
    const snapshot = current(id)
    return snapshot && sameSource(snapshot, source) && snapshot.profile === profile && !snapshot.loading && !snapshot.route_pending && !snapshot.error && snapshot.geometry['composer-input'] && (snapshot.geometry['status-profile'] || snapshot.geometry[`profile-${profile}`] || drawnFrame(id)?.elements.some(e => e.key === 'profile-picker-toggle' && presented(e))) && snapshot
  })
}
async function captureXid(name, xid, id = null, focusFirst = true) {
  if (focusFirst) focusXid(xid); await delay(450)
  const frame = geometry(xid), file = path.join(active.directory, name + '.png')
  execFileSync('/usr/bin/python3', ['-c', `import gi,os,sys;gi.require_version('Gdk','3.0');from gi.repository import Gdk;d=Gdk.Display.open(os.environ['DISPLAY']);r=d.get_default_screen().get_root_window();x,y,w,h=map(int,sys.argv[2:]);p=Gdk.pixbuf_get_from_window(r,x,y,w,h);p.savev(sys.argv[1],'png',[],[])`, file, frame.x, frame.y, frame.width, frame.height].map(String), {env: active.env, stdio: 'pipe'})
  screenshots.push({...scoped(), window: id, file: path.relative(out, file), sha256: hash(fs.readFileSync(file))})
}
async function capture(name, id) { return captureXid(name, nativeWindow(id), id) }
function storeFile(name) { return path.join(active.state, name) }
async function stopOwned(process) {
  if (!process || process.exitCode != null || process.signalCode != null) return
  process.kill('SIGTERM')
  const deadline = Date.now() + 2000
  while (process.exitCode == null && process.signalCode == null && Date.now() < deadline) await delay(20)
  if (process.exitCode == null && process.signalCode == null) process.kill('SIGKILL')
}
async function quit() {
  if (!active?.child || active.child.exitCode != null) return
  const xid = visible()[0]; assert.ok(xid, 'No actual native window to quit')
  focusXid(xid); input('key', '--clearmodifiers', 'ctrl+q')
  const deadline = Date.now() + 30000
  while (active.child.exitCode == null && Date.now() < deadline) await delay(40)
  assert.equal(active.child.exitCode, 0, 'Native global Quit did not finish cleanly')
}
async function privateDisplay() {
  displayProcess = spawn('Xvfb', ['-displayfd', '3', '-screen', '0', '1920x1200x24', '-nolisten', 'tcp'], {stdio: ['ignore', 'ignore', 'pipe', 'pipe']})
  owned.push(displayProcess)
  let display = '', diagnostic = ''
  displayProcess.stderr.on('data', bytes => diagnostic += bytes)
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Private Xvfb did not start: ' + diagnostic)), 10000)
    displayProcess.stdio[3].on('data', bytes => { display += bytes; if (display.includes('\n')) { clearTimeout(timer); resolve() } })
    displayProcess.once('error', reject)
  })
  const runtime = path.join(suiteHome, 'runtime'); fs.mkdirSync(runtime, {mode: 0o700})
  const alsa = path.join(suiteHome, 'alsa.conf'); fs.writeFileSync(alsa, 'pcm.!default { type null }\n')
  env = {...process.env, DISPLAY: ':' + display.trim(), HOME: suiteHome, USERPROFILE: suiteHome, XDG_RUNTIME_DIR: runtime, XDG_STATE_HOME: path.join(suiteHome, 'state'), XDG_CONFIG_HOME: path.join(suiteHome, 'config'), XDG_DATA_HOME: path.join(suiteHome, 'data'), XDG_CACHE_HOME: path.join(suiteHome, 'cache'), HERMES_HOME: path.join(suiteHome, 'unused-agent'), HERMES_GATEWAY_TOKEN: '', PULSE_SERVER: `unix:${suiteHome}/no-pulse`, PIPEWIRE_REMOTE: `${suiteHome}/no-pipewire`, ALSA_CONFIG_PATH: alsa}
  bus = spawn('dbus-daemon', ['--session', '--nofork', '--print-address=1'], {env, stdio: ['ignore', 'pipe', 'pipe']}); owned.push(bus)
  env.DBUS_SESSION_BUS_ADDRESS = await new Promise((resolve, reject) => {
    let address = ''; const timer = setTimeout(() => reject(new Error('Private D-Bus did not start')), 5000)
    bus.stdout.on('data', bytes => { address += bytes; if (address.includes('\n')) { clearTimeout(timer); resolve(address.trim().split('\n')[0]) } }); bus.once('error', reject)
  })
  wm = spawn('xfwm4', ['--replace', '--compositor=off'], {env, stdio: ['ignore', 'ignore', 'pipe']}); owned.push(wm)
  let wmLog = ''; wm.stderr.on('data', bytes => wmLog += bytes)
  const deadline = Date.now() + 10000
  while (Date.now() < deadline) {
    assert.equal(wm.exitCode, null, 'Private window manager exited: ' + wmLog)
    if (/window id # 0x[0-9a-f]+/i.test(execFileSync('xprop', ['-root', '_NET_SUPPORTING_WM_CHECK'], {env, encoding: 'utf8'}))) return
    await delay(100)
  }
  throw new Error('Private xfwm did not become ready: ' + wmLog)
}

function seededRegistry({single = false, launchMode = 'primary', lastUsed = 'B'} = {}) {
  return {version: 1, rows: (single ? ['A'] : ['A', 'B']).map(source => ({id: IDS[source], revision: REVISIONS[source], label: `Private ${source}`, destination: {kind: 'remote', url: gateways[source].endpoint}, credential: null})), selected: null, primary: IDS.A, lastUsed: IDS[lastUsed] ?? lastUsed, launchMode, lastProfiles: {[IDS.A]: 'research', [IDS.B]: 'archive'}, cleanup: []}
}
async function runCase(name, config, check) {
  if (only && only !== name) return
  const directory = path.join(out, name), home = path.join(suiteHome, name), state = path.join(home, 'state', 'hermes-desktop-rune'), inspection = path.join(directory, 'inspection')
  fs.mkdirSync(state, {recursive: true, mode: 0o700}); fs.mkdirSync(inspection, {recursive: true})
  credentials = await startPrivateCredentials({env}); active = {name, config, directory, home, state, inspection, child: null, log: '', windows: new Map(), env: {...credentials.env, DISPLAY: env.DISPLAY, XDG_STATE_HOME: path.join(home, 'state'), HERMES_HOME: path.join(home, 'unused-agent'), PATH: path.dirname(fixture.ssh_binary) + ':' + process.env.PATH, PULSE_SERVER: env.PULSE_SERVER, PIPEWIRE_REMOTE: env.PIPEWIRE_REMOTE, ALSA_CONFIG_PATH: env.ALSA_CONFIG_PATH}}
  phase = 'boot'
  atomic(storeFile('connections.json'), seededRegistry(config))
  atomic(storeFile('startup.json'), config.invalidDefault ?? {version: 1, revision: 1, defaultRoute: config.defaultSource ? {connectionId: IDS[config.defaultSource], profile: config.defaultProfile} : null})
  atomic(storeFile('preferences.json'), {theme: !!config.dark, speak_replies: false, sidebar_open: true, inspector_open: false, show_statusbar: true, show_profile_rail: !config.compact, simple_mode: false})
  atomic(storeFile('sounds.json'), {version: 1, preset: 0, muted: true})
  atomic(storeFile('notifications.json'), {version: 1, enabled: false})
  atomic(storeFile('sidebar.json'), {rail: true, all_profiles: false})
  const started = requests.length
  active.started = started
  try {
    active.child = spawn(binary, ['--peer-window-proof-dir', inspection], {cwd: app, env: active.env, stdio: ['ignore', 'pipe', 'pipe']}); owned.push(active.child)
    active.child.stderr.on('data', bytes => active.log += bytes); active.child.stdout.on('data', bytes => active.log += bytes)
    await check({started})
    if (failure) throw failure
    const nativeCount = visible().length
    await quit()
    results.push({name, passed: true, requests: requests.length - started, native_windows: nativeCount, inspected_windows: active.windows.size})
    fs.writeFileSync(path.join(directory, 'result.json'), JSON.stringify(results.at(-1), null, 2) + '\n')
  } catch (error) {
    for (const snapshot of snapshots()) { try { await capture(`failure-${snapshot.id}`, snapshot.id) } catch {} }
    if (!snapshots().length) for (const [index, xid] of visible().entries()) { try { await captureXid(`failure-native-${index}`, xid) } catch {} }
    fs.writeFileSync(path.join(directory, 'failure.json'), JSON.stringify({phase, error: String(error?.stack ?? error), snapshots: snapshots()}, null, 2) + '\n')
    throw error
  } finally {
    fs.writeFileSync(path.join(directory, 'app.log'), active.log)
    for (const file of ['startup.json', 'connections.json', 'windows.json', 'preferences.json']) if (fs.existsSync(storeFile(file))) fs.copyFileSync(storeFile(file), path.join(directory, file))
    if (fs.existsSync(storeFile('drafts'))) fs.cpSync(storeFile('drafts'), path.join(directory, 'drafts'), {recursive: true})
    await stopOwned(active.child)
    await credentials.stop()
    active = null
  }
}
async function primary() {
  return wait('initial complete primary Viewer', () => snapshots().find(snapshot => snapshot.kind === 'primary_launch' && snapshot.native?.position && snapshot.geometry['composer-input'] && !snapshot.loading && !snapshot.route_pending && !snapshot.error))
}


async function holdRegistryWriter() {
  const guard = spawn('/usr/bin/python3', ['-u', '-c', `import fcntl,sys;f=open(sys.argv[1],'a+b');fcntl.flock(f,fcntl.LOCK_EX);print('locked',flush=True);sys.stdin.read();fcntl.flock(f,fcntl.LOCK_UN);f.close()`, storeFile('connections.lock')], {env: active.env, stdio: ['pipe', 'pipe', 'pipe']})
  owned.push(guard)
  let text = '', diagnostic = ''
  guard.stderr.on('data', bytes => diagnostic += bytes)
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Private registry guard did not acquire: ' + diagnostic)), 5000)
    guard.stdout.on('data', bytes => { text += bytes; if (text.includes('locked\n')) { clearTimeout(timer); resolve() } })
    guard.once('error', reject)
  })
  return async () => {
    guard.stdin.end()
    const deadline = Date.now() + 3000
    while (guard.exitCode == null && Date.now() < deadline) await delay(20)
    assert.equal(guard.exitCode, 0, 'Private registry guard did not release cleanly')
  }
}
function registryDocument() { return JSON.parse(fs.readFileSync(storeFile('connections.json'), 'utf8')) }
const legacy = id => current(id)?.legacy_settings
function assertWorkspace(id, before) {
  const after = current(id)
  for (const name of ['editor', 'source_epoch', 'principal', 'profile', 'draft']) assert.equal(after[name], before[name], `Editing routing settings changed ${name}`)
}
async function openRouting(id) {
  await click(id, 'settings')
  await wait('ordinary Settings route', () => current(id)?.route === 'Settings' && drawnFrame(id)?.elements.some(e => e.key === 'settings-section-connections' && !e.exiting))
  await click(id, 'settings-section-connections')
  await wait('Connections settings tabs', () => drawnFrame(id)?.elements.some(e => e.key === 'connections-routing-tab' && presented(e)))
  await click(id, 'connections-routing-tab')
  await wait('real Legacy metadata form', () => legacy(id)?.form && !legacy(id).loading && !legacy(id).pending && !legacy(id).error)
}
async function readField(id, control, expected) {
  await click(id, control, 1, 12)
  await wait(`actual focused ${control} input with expected public text`, () => drawnFrame(id)?.elements.some(element => element.focused && element.path.includes(`/${control}/`) && element.text === expected))
  input('key', '--clearmodifiers', 'ctrl+a'); await delay(150)
  const frame = drawnFrame(id), focused = frame?.elements.find(element => element.focused && element.path.includes(`/${control}/`))
  assert.ok(focused && typeof focused.text === 'string', `Focused public input is missing: ${control}`)
  const sequence = active.readSequence = (active.readSequence ?? 0) + 1
  const evidence = `${sequence}-${control}-selected-before-copy`
  atomic(path.join(active.directory, evidence + '.json'), {window: current(id), frame})
  input('key', '--clearmodifiers', 'ctrl+c'); await delay(180)
  let value, matched = false
  for (let attempt = 0; attempt < 15; attempt++) {
    value = execFileSync('/usr/bin/python3', ['-c', `import gi;gi.require_version('Gtk','3.0');from gi.repository import Gtk,Gdk;Gtk.init([]);print(Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD).wait_for_text() or '',end='')`], {env: active.env, encoding: 'utf8', timeout: 5000})
    if (value === expected) { matched = true; break }
    await delay(100)
  }
  if (!matched) await captureXid(evidence, nativeWindow(id), id, false)
  assert.equal(value, expected, `Native clipboard did not copy the focused public ${control} input`)
  input('key', '--clearmodifiers', 'End'); await delay(80)
  return value
}
async function fill(id, control, value, secure = false) {
  await click(id, control, 1, 12)
  input('key', '--clearmodifiers', 'ctrl+a'); await delay(100)
  input('type', '--clearmodifiers', '--delay', '2', '--', value)
  await delay(150)
  if (!secure) assert.equal(await readField(id, control, value), value, `The actual input did not retain ${control}`)
}
async function mode(id, name) {
  await click(id, `legacy-mode-${name}`)
  await wait(`actual ${name} form`, () => legacy(id)?.form?.mode === name)
}
async function save(id) {
  await click(id, 'legacy-save')
  await wait('actual private atomic save receipt', () => legacy(id)?.notice === 'Saved.' && !legacy(id).dirty && !legacy(id).pending && !legacy(id).error)
}
async function profileScope(id, name) {
  await fill(id, 'legacy-profile-name', name)
  await click(id, 'legacy-profile-open')
  await wait(`captured settings scope ${name}`, () => legacy(id)?.scope?.kind === 'profile' && legacy(id).scope.name === name && legacy(id).form && !legacy(id).loading && !legacy(id).error)
}
function noCredentials() {
  const contents = fs.readFileSync(storeFile('connections.json'), 'utf8')
  assert.ok(!contents.includes('legacy-auth-'), 'Metadata-only acceptance created a credential reference')
  assert.ok(!contents.includes('NATIVE-UNSAVED-SECRET'), 'An authored token escaped into metadata')
  assert.ok(!contents.includes('NATIVE-UNSAVED-HEADER'), 'An authored header value escaped into metadata')
}

async function command(op, extra = {}) {
  const id = ++bridgeId
  const response = new Promise((resolve, reject) => { const timer = setTimeout(() => reject(new Error(`bridge timeout ${op}`)), 12000); responses.set(id, value => {clearTimeout(timer); resolve(value)}) })
  bridge.stdin.write(JSON.stringify({id, op, ...extra}) + '\n')
  const value = await response; evidence.push({op, value}); return value.result ?? value
}
async function launchBridge() {
  bridge = spawn('/tmp/hermes-ssh-fanout-bridge-5hki58p_/rune-ssh-fanout-bridge', ['--ignored', '--nocapture', '--exact', 'installed_ssh::test_fixture::tests::native_bridge'], {stdio:['pipe','pipe','pipe']}); owned.push(bridge)
  bridge.stderr.on('data', b => bridgeLog += b)
  const lines = readline.createInterface({input:bridge.stdout})
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('bridge ready timeout')), 10000)
    lines.on('line', line => { const i = line.indexOf('{'); if(i < 0) return; let v; try {v = JSON.parse(line.slice(i))} catch {return}; if (v.event === 'ready') {clearTimeout(timer); resolve(v)} else if (responses.has(v.id)) {responses.get(v.id)(v); responses.delete(v.id)} })
    bridge.once('exit', code => {if (!fixture) reject(new Error(`bridge exited ${code}`))})
  })
}
function incarnation(record) {
  assert.ok(record && Number.isSafeInteger(record.pid) && record.pid > 0 && Number.isSafeInteger(record.start_ticks) && Number.isSafeInteger(record.pgid) && record.pgid > 0, 'Malformed process custody')
  fs.statSync('/proc/self/stat')
  try {
    const stat = fs.readFileSync(`/proc/${record.pid}/stat`, 'utf8')
    const close = stat.lastIndexOf(')'); assert.ok(close >= 0, 'Malformed proc stat')
    const fields = stat.slice(close + 1).trim().split(/\s+/)
    assert.ok(fields.length > 19 && /^[0-9]+$/.test(fields[19]) && /^[0-9]+$/.test(fields[2]), 'Malformed proc identity')
    const same = BigInt(fields[19]) === BigInt(record.start_ticks)
    if (same) assert.equal(Number(fields[2]), record.pgid, 'Live process group changed')
    return same && fields[0] !== 'Z'
  } catch (error) { if (error.code === 'ENOENT') return false; throw error }
}
async function portAccepts(port) {
  assert.ok(Number.isInteger(port) && port > 0 && port < 65536)
  return new Promise((resolve, reject) => {
    const socket = net.createConnection({host:'127.0.0.1',port}); socket.setTimeout(1000)
    socket.once('connect', () => {socket.destroy(); resolve(true)})
    socket.once('error', error => {socket.destroy(); if (error.code === 'ECONNREFUSED') resolve(false); else reject(error)})
    socket.once('timeout', () => {socket.destroy(); reject(new Error('Port inspection timed out'))})
  })
}
let custody = null
async function sshCase() {
 const main = await primary(); await ready(main.id, 'A', 'research')
 phase = 'configure-ssh'; await openRouting(main.id); await profileScope(main.id, 'research'); await mode(main.id, 'ssh')
 for (const [control,value] of [['host',fixture.host],['user',fixture.user],['port',String(fixture.port)],['hermes-path',fixture.launcher],['profile','research']]) await fill(main.id, `legacy-field-ssh-${control}`, value)
 await save(main.id); await capture('ssh-saved',main.id)
 phase = 'hold-proof'; await command('hold_proof'); await click(main.id, 'legacy-connect')
 const held = await command('wait_proof',{timeout_ms:8000}); assert.equal(held.entered,true)
 const callsHeld = await command('calls'); evidence.push({proof_held_calls:callsHeld})
 const owners = (await command('owners')).owners; assert.equal(owners.length,1); const owner = owners[0]
 const generation = await command('generation',{owner}); assert.equal(generation.running.Ok,true)
 const localGeneration = captureGeneration(fixture.home,owner)
 const stat = fs.readFileSync(`/proc/${localGeneration.pid}/stat`,'utf8').split(')').at(-1).trim().split(/\s+/)
 const forward = (await command('forward')).record; assert.ok(forward?.helper); assert.ok(incarnation(forward) && incarnation(forward.helper)); assert.equal(await portAccepts(forward.local_port),true)
 custody = {localGeneration, backendPgid:Number(stat[2]), forward}; evidence.push({live_custody:custody})
 phase = 'edit-held-proof'; await fill(main.id,'legacy-field-ssh-profile','archive'); await save(main.id)
 const saved = registryDocument(); atomic(path.join(active.directory,'saved-held-edit.json'),saved)
 await command('release_proof'); await delay(1500)
 const after = snapshots(); atomic(path.join(active.directory,'post-edit-snapshots.json'),after)
 assert.ok(!after.some(s => s.source_stamp?.version?.id?.kind === 'legacy' && !s.route_pending && !s.error && s.profile === 'research'), 'Stale research route published')
 await retirement(owner)
}
async function retirement(owner) {
 const {localGeneration, forward} = custody
 phase = 'global-quit'; let quitError = null
 try { await quit() } catch (error) { quitError = error; evidence.push({quit_error:String(error)}) }
 let state, forwardState, local, port
 const deadline = Date.now() + 25000
 do {
   state = await command('inspect',{owner}); forwardState = await command('inspect_forward')
   local = {backend:inspectGeneration(localGeneration), listener:incarnation(forward), helper:incarnation(forward.helper)}
   port = await portAccepts(forward.local_port)
   if (state.running?.Ok === false && state.files_retired?.Ok === true && !local.backend.running && !local.listener && !local.helper && !port) break
   await delay(100)
 } while(Date.now() < deadline)
 evidence.push({retirement_before_bridge_shutdown:true,state,forwardState,local,port})
 assert.deepEqual(state,{running:{Ok:false},files_retired:{Ok:true},lock_exists:false,token_exists:false,log_exists:false})
 assert.deepEqual(local,{backend:{running:false,files:{lock:false,token:false,log:false}},listener:false,helper:false})
 assert.equal(port,false); assert.deepEqual(forwardState,{running:false,helper_running:false,port_accepts:false})
 if (quitError) throw quitError
 custody = null
 atomic(path.join(active.directory,'final-calls.json'),await command('calls'))
}
let passed = false
try {
 assert.equal(binarySha,'92e0e4ccd714f682d1d54888c8a00ded50033c9c72138e55f81f2b04c8271825')
 for (const [f,sha] of [['/tmp/hermes-connection-checkpoint-ylr6rl4f/source.tar.gz','c20fd7cb8a33bd8768ff28dd70ad746f0eb72b96eb5a7ed9540b217eb38b80cc'],['/tmp/hermes-connection-checkpoint-ylr6rl4f/source-manifest.json','cfde04922bd8b5a52242b75f894491ffd47a54dac269dd0757e4583eaaeaa8fd'],['/tmp/hermes-ssh-fanout-bridge-5hki58p_/rune-ssh-fanout-bridge','080fde0f291c544878dbefcbde3282bb1676e53f3690138bda86f707a64ecd7c'],['/tmp/hermes-ssh-fanout-bridge-5hki58p_/source.tar.gz','55741b7bed2d790a439a29bfff51a6bf776037f2b898ebff282cb86ec7c425a8']]) assert.equal(hash(fs.readFileSync(f)),sha)
 await privateDisplay()
 for (const [name,dark,quitOnly] of [['held-edit-light',false,false],['held-edit-dark',true,false]]) {
   if (only && only !== name) continue
   fixture = await launchBridge(); fixtures.push({...fixture, ssh_binary_sha256:hash(fs.readFileSync(fixture.ssh_binary))}); fs.copyFileSync(fixture.ssh_binary,path.join(out,name+'-fake-ssh')); fs.writeFileSync(path.join(fixture.home, 'forward-helper'), ''); gateways.A.endpoint = fixture.anonymous_base; gateways.B.endpoint = fixture.anonymous_base
   try { await runCase(name,{single:true,lastUsed:'A',dark,quitOnly},sshCase) }
   catch(error) { results.push({name,passed:false,phase,error:String(error.stack??error)}); evidence.push({case_failure:name,phase,error:String(error.stack??error),custody}); }
   if (custody) { evidence.push({fallback_before_shutdown:name,custody,backend:inspectGeneration(custody.localGeneration),listener:incarnation(custody.forward),helper:incarnation(custody.forward.helper),port:await portAccepts(custody.forward.local_port)}); }
   await command('shutdown'); await stopOwned(bridge); bridge = null; fixture = null
   if (custody) { for (const item of [custody.forward.helper,custody.forward]) if (incarnation(item)) {process.kill(item.pid,'SIGTERM'); evidence.push({fallback_signal:item.pid})}; custody = null }
 }

 passed=results.length === (only ? 1 : 2) && results.every(r => r.passed)
 if (!passed) process.exitCode=1
} catch(error) { failure=error; console.error(error.stack ?? error); process.exitCode=1 }
finally {
 if(custody) {
   evidence.push({failed_custody_before_cleanup:custody, phase, failure:String(failure)})
   atomic(path.join(out,'failed-custody.json'), evidence.at(-1))
   for (const item of [custody.forward.helper,custody.forward]) {
     try { if (incarnation(item)) { process.kill(item.pid, 'SIGTERM'); evidence.push({cleanup_pid:item.pid,signal:'SIGTERM'}) } } catch (error) { evidence.push({cleanup_error:String(error)}) }
   }
 }
 if(bridge && bridge.exitCode===null) {try {await command('shutdown')} catch(e) {evidence.push({shutdown_error:String(e)})}}
 for(const child of [...owned].reverse()) await stopOwned(child)
 atomic(path.join(out,'bridge-evidence.json'),evidence); atomic(path.join(out,'bridge.log'),bridgeLog)
 atomic(path.join(out,'pointer-actions.json'),gestures); atomic(path.join(out,'result.json'),{passed,scope:'private SSH held proof edit and global Quit', full_acceptance_passed:false,phase,failure:failure?String(failure.stack??failure):null,binary_sha256:binarySha,bridge_sha256:'080fde0f291c544878dbefcbde3282bb1676e53f3690138bda86f707a64ecd7c',harness_sha256:harnessSha,fixtures,results,screenshots,bindings,owned:owned.map(c=>({pid:c.pid,exitCode:c.exitCode,signalCode:c.signalCode}))})
 console.log(JSON.stringify({passed,phase,out,failure:failure?String(failure):null}))
}
