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

const app = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const args = process.argv.slice(2)
const option = name => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : null }
const binary = path.resolve(option('--binary') ?? path.join(app, 'target/debug/hermes-rune'))
const out = path.resolve(option('--out') ?? fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-legacy-settings-evidence-')))
const only = option('--case')
fs.mkdirSync(out, {recursive: true})
assert.equal(fs.readdirSync(out).length, 0, 'Use a fresh evidence directory')
const suiteHome = fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-legacy-settings-home-'))
const owned = [], requests = [], screenshots = [], gestures = [], bindings = [], results = []
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

class Gateway {
  constructor(name) {
    this.name = name; this.sockets = new Set()
    this.http = createServer((req, res) => this.read(req, res))
    this.ws = new WebSocketServer({server: this.http, path: '/api/ws'})
    this.ws.on('connection', (socket, req) => {
      this.sockets.add(socket); socket.on('close', () => this.sockets.delete(socket))
      try {
        const url = new URL(req.url, 'http://fixture')
        assert.equal(url.searchParams.has('token'), false, 'Fixture connection unexpectedly used credentials')
        assert.equal(req.headers.authorization, undefined)
      } catch (error) { failure ??= error; socket.close(); return }
      socket.on('message', bytes => {
        try { this.rpc(socket, JSON.parse(bytes)) } catch (error) { failure ??= error; socket.close() }
      })
    })
  }
  async listen() {
    await new Promise(resolve => this.http.listen(0, '127.0.0.1', resolve))
    this.endpoint = new URL(`http://127.0.0.1:${this.http.address().port}`).toString()
    return this
  }
  profileRows() {
    const calls = requests.slice(active?.started ?? requests.length).filter(request => request.transport === 'rpc' && request.source === this.name && request.method === 'profiles.list').length
    const roster = active?.config.dropProfileBeforeViewer && this.name === 'B' && calls >= 3 ? names.filter(name => name !== 'research') : names
    return roster.map(name => ({name, label: name, description: `Private ${this.name}/${name}`, is_default: name === 'default', canonical_session: {id: `${this.name}-${name}-bot`, resolved_id: `${this.name}-${name}-bot`}}))
  }
  sessions(profile) {
    assert.ok(names.includes(profile), `Unknown session-list profile ${profile}`)
    return [{id: `${this.name}-${profile}-notes`, profile, title: `${this.name} ${profile} notes`, cwd: '/private-fixture', source: 'desktop', last_active: 1793310000, message_count: 1}]
  }
  session(id, profile) {
    assert.ok(names.includes(profile))
    assert.ok([`${this.name}-${profile}-notes`, `${this.name}-${profile}-bot`, `live-${this.name}-${profile}-notes`, `live-${this.name}-${profile}-bot`].includes(id), `Foreign session ${id} on ${this.name}/${profile}`)
    const stored = id.replace(/^live-/, '')
    return {session_id: `live-${stored}`, stored_session_id: stored, revision: 1, authority_epoch: 7, execution_generation: 0, replay_epoch: `${this.name}-fixture`, last_sequence: 0, running: false, pending: [], prompts: [], messages: [{row_id: 1, role: 'assistant', content: `Private history on ${this.name}/${profile}`}], info: {profile_name: profile, desktop_protocol: 'hermes-gateway-v1', model: 'fixture-model', provider: 'fixture'}}
  }
  rpc(socket, frame) {
    requests.push({...scoped(), transport: 'rpc', source: this.name, ...frame})
    const p = frame.params ?? {}
    assert.ok(frame.method === 'runtime.describe' ? p.profile === undefined : names.includes(p.profile), `Unexpected profile ${p.profile} for ${frame.method}`)
    let result
    switch (frame.method) {
      case 'runtime.describe': result = {profile_id: `/private-fixture/${this.name}/root`, instance_id: `private-${this.name}`, authority_epoch: 7, capabilities: [], session_create: {sources: ['desktop'], parameters: []}}; break
      case 'profiles.list':
        result = {profiles: this.profileRows()}
        if (this.holdNextProfileRead) {
          this.holdNextProfileRead = false
          this.heldProfileRead = {socket, frame, result}
          return
        }
        break
      case 'session.list': result = {sessions: p.scope === 'live' ? [] : this.sessions(p.profile)}; break
      case 'session.resume': result = this.session(p.session_id, p.profile); break
      case 'model.options': result = catalog; break
      case 'config.get': result = {value: p.key === 'approvals.mode' ? 'smart' : null}; break
      case 'setup.runtime_check': result = {ok: true, ready: true}; break
      case 'profiles.get_asset': result = {data_url: null}; break
      case 'process.list': result = {processes: []}; break
      case 'subagent.list': result = {subagents: []}; break
      default: throw new Error(`Forbidden RPC: ${frame.method}; this acceptance never creates or sends a prompt`)
    }
    if (socket.readyState === 1) socket.send(JSON.stringify({jsonrpc: '2.0', id: frame.id, result}))
  }
  read(req, res) {
    try {
      const url = new URL(req.url, 'http://fixture')
      requests.push({...scoped(), transport: 'http', source: this.name, method: req.method, path: url.pathname, query: Object.fromEntries(url.searchParams)})
      assert.equal(req.method, 'GET', 'Forbidden HTTP mutation')
      assert.equal(url.searchParams.has('token'), false, 'Anonymous fixtures must not retrieve OS credentials')
      assert.equal(req.headers.authorization, undefined)
      const profile = url.searchParams.get('profile') ?? 'default'
      assert.ok(profile === 'all' || names.includes(profile), `Unknown HTTP profile ${profile}`)
      let result
      const history = /^\/api\/sessions\/([^/]+)\/messages$/.exec(url.pathname)
      if (history) {
        const session = this.session(decodeURIComponent(history[1]), profile)
        result = {profile, session_id: session.stored_session_id, messages: session.messages}
      } else switch (url.pathname) {
        case '/api/profiles': result = {profiles: this.profileRows()}; break;
        case '/api/profiles/active': result = {active: 'default', current: 'default'}; break;
        case '/api/profiles/sessions/sidebar': result = {recents: {sessions: this.sessions(profile)}, messaging: {sessions: []}, cron: {sessions: []}}; break
        case '/api/profiles/sessions': result = {sessions: profile === 'all' ? names.flatMap(p => this.sessions(p)) : this.sessions(profile)}; break
        case '/api/cron/jobs': result = {jobs: []}; break
        case '/api/fs/default-cwd': result = {cwd: '/private-fixture'}; break
        case '/api/fs/list': result = {entries: []}; break
        case '/api/fs/read-text': assert.ok(url.searchParams.get('path')?.endsWith('.gitignore')); result = {exists: false, content: ''}; break
        case '/api/config': result = {config: {model: {provider: 'fixture', default: 'fixture-model'}, approvals: {mode: 'smart'}}}; break
        case '/api/config/schema': result = {fields: {}}; break
        case '/api/model/options': result = catalog; break
        case '/api/model/auxiliary': result = {main: {provider: 'fixture', model: 'fixture-model'}, tasks: []}; break
        case '/api/model/moa': result = {presets: {default: {aggregator: {provider: 'fixture', model: 'fixture-model'}, reference_models: []}}, default_preset: 'default', active_preset: 'default'}; break
        case '/api/audio/elevenlabs/voices': result = {available: false, voices: []}; break
        case '/api/audio/voice-live/status': result = {ok: false}; break
        case '/api/status': result = {running: true, ready: true}; break
        case '/api/git/review/rev-parse': result = {sha: null}; break
        case '/api/git/review/list': result = {files: [], is_repo: false}; break
        case '/api/git/review/ship-info': result = {ghReady: false}; break
        case '/api/git/status': result = {is_repo: false}; break
        case '/api/git/branches': case '/api/git/base-branches': result = {branches: []}; break
        case '/api/git/worktrees': result = {worktrees: []}; break
        default: throw new Error(`Forbidden HTTP read: ${url.pathname}`)
      }
      res.writeHead(200, {'content-type': 'application/json'}); res.end(JSON.stringify(result))
    } catch (error) { failure ??= error; res.writeHead(400, {'content-type': 'application/json'}); res.end('{}') }
  }
  releaseProfileRead() {
    const held = this.heldProfileRead
    assert.ok(held, 'No private profile read was held')
    this.heldProfileRead = null
    if (held.socket.readyState === 1) held.socket.send(JSON.stringify({jsonrpc: '2.0', id: held.frame.id, result: held.result}))
  }
  async close() {
    for (const socket of this.sockets) socket.terminate()
    await new Promise(resolve => this.ws.close(resolve)); await new Promise(resolve => this.http.close(resolve))
  }
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
  const deadline = Date.now() + 10000
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

const gateways = {A: await new Gateway('A').listen(), B: await new Gateway('B').listen()}
function seededRegistry({single = false, launchMode = 'primary', lastUsed = 'B'} = {}) {
  return {version: 1, rows: (single ? ['A'] : ['A', 'B']).map(source => ({id: IDS[source], revision: REVISIONS[source], label: `Private ${source}`, destination: {kind: 'remote', url: gateways[source].endpoint}, credential: null})), selected: null, primary: IDS.A, lastUsed: IDS[lastUsed] ?? lastUsed, launchMode, lastProfiles: {[IDS.A]: 'research', [IDS.B]: 'archive'}, cleanup: []}
}
async function runCase(name, config, check) {
  if (only && only !== name) return
  const directory = path.join(out, name), home = path.join(suiteHome, name), state = path.join(home, 'state', 'hermes-desktop-rune'), inspection = path.join(directory, 'inspection')
  fs.mkdirSync(state, {recursive: true, mode: 0o700}); fs.mkdirSync(inspection, {recursive: true})
  active = {name, config, directory, home, state, inspection, child: null, log: '', windows: new Map(), env: {...env, HOME: home, USERPROFILE: home, XDG_STATE_HOME: path.join(home, 'state'), XDG_CONFIG_HOME: path.join(home, 'config'), XDG_DATA_HOME: path.join(home, 'data'), XDG_CACHE_HOME: path.join(home, 'cache'), HERMES_HOME: path.join(home, 'unused-agent')}}
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
async function globalForm() {
  const main = await primary(); await ready(main.id, 'A', 'research')
  assert.equal(current(main.id).dark, false)
  await type(main.id, 'Unsent native input preserved by routing settings')
  const workspace = current(main.id)
  phase = 'ordinary-global-form-invalid-authored-secret'
  await openRouting(main.id); await mode(main.id, 'url')
  await fill(main.id, 'legacy-field-url', gateways.B.endpoint)
  await click(main.id, 'legacy-token-set')
  await fill(main.id, 'legacy-field-token', 'NATIVE-UNSAVED-SECRET', true)
  await click(main.id, 'legacy-header-add')
  const header = await wait('authored header row', () => legacy(main.id)?.form?.headers?.at(0))
  await fill(main.id, `legacy-field-header-name-${header.id}`, 'Authorization')
  await fill(main.id, `legacy-field-header-value-${header.id}`, 'NATIVE-UNSAVED-HEADER', true)
  const beforeValidation = fs.readFileSync(storeFile('connections.json'), 'utf8')
  await click(main.id, 'legacy-save')
  await wait('reserved header error with actual authored secret retained', () => legacy(main.id)?.error?.includes('reserved') && legacy(main.id).dirty && legacy(main.id).form.token_authored && legacy(main.id).form.headers[0].authored)
  assert.equal(fs.readFileSync(storeFile('connections.json'), 'utf8'), beforeValidation)
  assertWorkspace(main.id, workspace); noCredentials()
  await reveal(main.id, 'legacy-error'); await capture('global-error-retained-authored-secret-light', main.id)
  phase = 'discard-before-real-metadata-save'
  await click(main.id, 'legacy-discard')
  await wait('discard loaded actual unchanged record', () => legacy(main.id)?.form?.mode === 'inherit' && !legacy(main.id).dirty && !legacy(main.id).loading && !legacy(main.id).form.token_authored)
  await mode(main.id, 'url'); await fill(main.id, 'legacy-field-url', gateways.B.endpoint)
  phase = 'real-private-transaction-refusal-retains-input'
  const before = fs.readFileSync(storeFile('connections.json'), 'utf8')
  const release = await holdRegistryWriter()
  try {
    await click(main.id, 'legacy-save')
    await wait('accepted save visibly pending', () => legacy(main.id)?.pending)
    await wait('actual worker transaction refusal', () => legacy(main.id)?.error?.includes('Another window') && legacy(main.id).dirty && !legacy(main.id).pending)
  } finally { await release() }
  assert.equal(await readField(main.id, 'legacy-field-url', gateways.B.endpoint), gateways.B.endpoint)
  assert.equal(fs.readFileSync(storeFile('connections.json'), 'utf8'), before)
  await reveal(main.id, 'legacy-error'); await capture('global-real-save-error-retained-input-light', main.id)
  phase = 'retry-actual-private-save'
  await save(main.id)
  assert.equal(registryDocument().legacy.global.value.remote.url, gateways.B.endpoint)
  assert.equal(legacy(main.id).form.token_stored, false); assertWorkspace(main.id, workspace); noCredentials()
  await capture('global-saved-light', main.id)
  phase = 'cold-reopen-persisted-form'
  await quit()
  active.windows.clear()
  fs.rmSync(active.inspection, {recursive: true, force: true}); fs.mkdirSync(active.inspection, {recursive: true})
  active.child = spawn(binary, ['--peer-window-proof-dir', active.inspection], {cwd: app, env: active.env, stdio: ['ignore', 'pipe', 'pipe']}); owned.push(active.child)
  active.child.stderr.on('data', bytes => active.log += bytes); active.child.stdout.on('data', bytes => active.log += bytes)
  const reopened = await primary(); await ready(reopened.id, 'A', 'research')
  await openRouting(reopened.id)
  assert.equal(legacy(reopened.id).form.mode, 'url')
  assert.equal(await readField(reopened.id, 'legacy-field-url', gateways.B.endpoint), gateways.B.endpoint)
  assert.equal(legacy(reopened.id).form.token_stored, false)
  noCredentials(); await capture('global-cold-reopen-light', reopened.id)
}
async function scopedForms() {
  const main = await primary(); await ready(main.id, 'A', 'research')
  assert.equal(current(main.id).dark, true)
  await type(main.id, 'Main draft survives profile routing')
  const mainWorkspace = current(main.id), existing = new Set(snapshots().map(s => s.id))
  key(main.id, 'ctrl+shift+n')
  const peer = await wait('ordinary independent peer Viewer', () => snapshots().find(s => !existing.has(s.id) && s.kind === 'peer_inherited' && !s.loading && !s.error && s.geometry['composer-input']))
  await ready(peer.id, 'A', 'research')
  await type(peer.id, 'Peer draft survives stale CAS')
  const peerWorkspace = current(peer.id)
  phase = 'two-captured-profile-editors'
  await openRouting(main.id); await profileScope(main.id, 'research')
  await openRouting(peer.id); await profileScope(peer.id, 'research')
  await mode(peer.id, 'cloud')
  for (const [control, value] of [['url', gateways.B.endpoint], ['cloud-name', 'Native retained cloud'], ['org', 'private-fixture-org'], ['remote-profile', 'backend-research']]) await fill(peer.id, `legacy-field-${control}`, value)
  phase = 'real-full-ssh-form-save'
  await mode(main.id, 'ssh')
  for (const [control, value] of [['host', 'private-host.invalid'], ['user', 'private-user'], ['port', '2222'], ['key-path', '/private-fixture/id_ed25519'], ['hermes-path', '/opt/private-hermes'], ['profile', 'backend-research']]) await fill(main.id, `legacy-field-ssh-${control}`, value)
  await save(main.id)
  const value = registryDocument().legacy.profiles.research.value
  assert.equal(value.mode, 'ssh'); assert.equal(value.host, 'private-host.invalid'); assert.equal(value.user, 'private-user'); assert.equal(value.port, 2222)
  assert.equal(value.key_path, '/private-fixture/id_ed25519'); assert.equal(value.remote_hermes_path, '/opt/private-hermes'); assert.equal(value.remote_profile, 'backend-research')
  await capture('profile-ssh-saved-dark', main.id)
  phase = 'real-stale-cloud-cas-retains-form'
  await click(peer.id, 'legacy-save')
  await wait('actual stale slot refusal', () => legacy(peer.id)?.error?.includes('another window') && legacy(peer.id).dirty && !legacy(peer.id).pending)
  assert.equal(legacy(peer.id).form.mode, 'cloud')
  for (const [control, value] of [['url', gateways.B.endpoint], ['cloud-name', 'Native retained cloud'], ['org', 'private-fixture-org'], ['remote-profile', 'backend-research']]) assert.equal(await readField(peer.id, `legacy-field-${control}`, value), value)
  assertWorkspace(main.id, mainWorkspace); assertWorkspace(peer.id, peerWorkspace); noCredentials()
  await reveal(peer.id, 'legacy-error'); await capture('profile-stale-cloud-retained-dark', peer.id)
  phase = 'discard-and-preserve-dormant-ssh'
  await click(peer.id, 'legacy-discard')
  await wait('actual saved SSH replaces explicitly discarded form', () => legacy(peer.id)?.form?.mode === 'ssh' && !legacy(peer.id).dirty && !legacy(peer.id).loading)
  assert.equal(await readField(peer.id, 'legacy-field-ssh-hermes-path', '/opt/private-hermes'), '/opt/private-hermes')
  await mode(peer.id, 'inherit'); await save(peer.id)
  const inherited = registryDocument().legacy.profiles.research.value
  assert.equal(inherited.mode, 'local'); assert.equal(inherited.saved_ssh.host, 'private-host.invalid'); assert.equal(inherited.saved_ssh.remote_hermes_path, '/opt/private-hermes')
  await mode(peer.id, 'ssh')
  assert.equal(await readField(peer.id, 'legacy-field-ssh-key-path', '/private-fixture/id_ed25519'), '/private-fixture/id_ed25519')
  await click(peer.id, 'legacy-discard')
  await wait('discard returns to persisted inheritance', () => legacy(peer.id)?.form?.mode === 'inherit' && !legacy(peer.id).dirty && !legacy(peer.id).loading)
  const xid = nativeWindow(peer.id)
  input('windowsize', xid, '1024', '760')
  await wait('short native window actually resized', () => current(peer.id)?.native?.inner_size?.[0] <= 1025 && current(peer.id)?.native?.inner_size?.[1] <= 761)
  await click(peer.id, 'legacy-mode-inherit')
  await capture('profile-inherit-short-dark', peer.id)
  assertWorkspace(main.id, mainWorkspace); assertWorkspace(peer.id, peerWorkspace); noCredentials()
}
try {
  await privateDisplay()
  await runCase('global-light', {lastUsed: 'A'}, globalForm)
  await runCase('profile-dark', {lastUsed: 'A', dark: true}, scopedForms)
  assert.ok(results.length, 'No acceptance case matched --case')
  assert.equal(requests.filter(request => ['prompt.submit', 'session.create', 'session.interrupt'].includes(request.method)).length, 0)
  assert.equal(hash(fs.readFileSync(binary)), binarySha, 'Candidate binary changed during acceptance')
  const result = {binary_sha256: binarySha, harness_sha256: harnessSha, actual_native_windows: true, complete_viewers: true, private_xvfb_dbus_xfwm: true, pointer_hold_ms: 40, pointer_wiggle_used: false, live_agents_used: false, provider_requests: 0, agent_mutations: 0, os_credentials_used: false, metadata_only_successful_saves: true, cases: results, screenshots, native_bindings: bindings}
  atomic(path.join(out, 'result.json'), result)
  console.log(JSON.stringify(result))
} finally {
  atomic(path.join(out, 'requests.json'), requests)
  atomic(path.join(out, 'pointer-actions.json'), gestures)
  for (const process of [...owned].reverse()) await stopOwned(process)
  await gateways.A.close(); await gateways.B.close()
  fs.rmSync(suiteHome, {recursive: true, force: true})
}
