#!/usr/bin/env node
/** Actual complete native Viewers against two private anonymous gateways. No agent writes. */
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
const out = path.resolve(option('--out') ?? fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-startup-native-evidence-')))
const only = option('--case')
fs.mkdirSync(out, {recursive: true})
assert.equal(fs.readdirSync(out).length, 0, 'Use a fresh evidence directory')
const suiteHome = fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-startup-native-home-'))
const owned = [], requests = [], screenshots = [], gestures = [], bindings = [], results = []
const IDS = {A: '11111111-1111-4111-8111-111111111111', B: '22222222-2222-4222-8222-222222222222'}
const REVISIONS = {A: '33333333-3333-4333-8333-333333333333', B: '44444444-4444-4444-8444-444444444444'}
const names = ['default', 'research', 'archive', ...Array.from({length: 14}, (_, i) => `profile${i + 1}`)]
const catalog = {provider: 'fixture', model: 'fixture-model', providers: [{slug: 'fixture', name: 'Private fixture', authenticated: true, models: ['fixture-model']}]}
const delay = ms => new Promise(resolve => setTimeout(resolve, ms))
const hash = value => createHash('sha256').update(value).digest('hex')
const harnessSha = hash(fs.readFileSync(fileURLToPath(import.meta.url)))
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
    this.endpoint = `http://127.0.0.1:${this.http.address().port}`
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
    assert.ok(names.includes(p.profile), `Unexpected profile ${p.profile} for ${frame.method}`)
    let result
    switch (frame.method) {
      case 'profiles.list':
        result = {profiles: this.profileRows(), active: 'default'}
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
        case '/api/profiles': result = {profiles: this.profileRows(), active: 'default'}; break;
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
function sessionKey(id, profile, stored) {
  const snapshot = current(id)
  const row = snapshot?.fleet?.sidebar_groups?.filter(group => group.source?.principal === snapshot.principal).flatMap(group => group.rows).find(row => row.profile === profile && (row.id === stored || row.root === stored))
  const exact = row && `session-${row.key}`
  if (exact && drawnFrame(id)?.elements.some(e => e.key === exact && !e.exiting)) return exact
  return drawnFrame(id)?.elements.find(e => {
    if (!e.key?.startsWith('session-[') || e.exiting) return false
    try { const [source, owner, root] = JSON.parse(e.key.slice(8)); return source.principal === snapshot?.principal && owner === profile && root === stored } catch { return false }
  })?.key
}
function resolvedKey(id, name) {
  if (drawnFrame(id)?.elements.some(e => e.key === name && !e.exiting)) return name
  for (const profile of names) {
    const prefix = `session-${profile}-`
    if (name.startsWith(prefix)) return sessionKey(id, profile, name.slice(prefix.length)) ?? name
  }
  return name
}
async function profileDoor(id, profile, compact = active.config.compact) {
  if (presented(drawnFrame(id)?.elements.find(e => e.key === `profile-${profile}` && !e.exiting))) return `profile-${profile}`
  if (!drawnFrame(id)?.elements.some(e => e.key === 'profile-picker' && !e.exiting)) {
    await click(id, compact ? 'status-profile' : 'profile-picker-toggle')
  }
  return `pick-profile-${profile}`
}
async function reveal(id, name) {
  const xid = focus(id)
  await delay(300)
  for (let attempt = 0; attempt < 28; attempt++) {
    const key = resolvedKey(id, name)
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
    const frame = geometry(xid), viewport = picker?.visible ?? [0, 100, 220, Math.max(100, frame.height - 300)]
    const direction = element && element.rect[1] < viewport[1] ? 4 : 5
    input('mousemove', frame.x + viewport[0] + Math.min(100, viewport[2] / 2), frame.y + viewport[1] + viewport[3] / 2, 'sleep', '0.05', 'click', '--repeat', '2', '--delay', '60', direction)
    gestures.push({...scoped(), id, name, resolved_key: key, scroll: direction, viewport})
    await delay(150)
  }
  throw new Error(`Native control cannot be revealed: ${name}`)
}
async function click(id, name, button = 1, inset = null) {
  if (names.some(profile => name === `profile-${profile}`)) name = await profileDoor(id, name.slice(8))
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
async function captureXid(name, xid, id = null) {
  focusXid(xid); await delay(450)
  const frame = geometry(xid), file = path.join(active.directory, name + '.png')
  execFileSync('/usr/bin/python3', ['-c', `import gi,os,sys;gi.require_version('Gdk','3.0');from gi.repository import Gdk;d=Gdk.Display.open(os.environ['DISPLAY']);r=d.get_default_screen().get_root_window();x,y,w,h=map(int,sys.argv[2:]);p=Gdk.pixbuf_get_from_window(r,x,y,w,h);p.savev(sys.argv[1],'png',[],[])`, file, frame.x, frame.y, frame.width, frame.height].map(String), {env: active.env, stdio: 'pipe'})
  screenshots.push({...scoped(), window: id, file: path.relative(out, file), sha256: hash(fs.readFileSync(file))})
}
async function capture(name, id) { return captureXid(name, nativeWindow(id), id) }
function storeFile(name) { return path.join(active.state, name) }
function defaultDocument() { return JSON.parse(fs.readFileSync(storeFile('startup.json'), 'utf8')) }
function registryDocument() { return JSON.parse(fs.readFileSync(storeFile('connections.json'), 'utf8')) }
function setDefault(source, profile) {
  const document = defaultDocument()
  document.revision++
  document.defaultRoute = source == null ? null : {connectionId: IDS[source], profile}
  atomic(storeFile('startup.json'), document)
}
function draftEntries() {
  const directory = storeFile('drafts')
  return fs.existsSync(directory) ? fs.readdirSync(directory).filter(file => file.endsWith('.json')).flatMap(file => {
    try { return (JSON.parse(fs.readFileSync(path.join(directory, file), 'utf8')).entries ?? []).map(entry => ({...entry, file})) } catch { return [] }
  }) : []
}
async function retainedDraft(editor, profile, text, source) {
  await wait('old owner draft retained in its exact source file', () => draftEntries().some(entry => entry.file === principal(source) + '.json' && entry.editor === editor && entry.profile === profile && entry.draft?.text === text))
}
async function mirror(id, source, profile) {
  return wait('observed device default', () => {
    const snapshot = current(id), route = snapshot?.default_profile
    return snapshot && (source == null ? route === null : route?.connectionId === IDS[source] && route.profile === profile) && snapshot
  })
}
async function openConnections(id) {
  await click(id, 'change-connection')
  await wait('connection chooser', () => current(id)?.connection_form && current(id)?.geometry['saved-connections'])
  await click(id, 'saved-connections')
  await wait('complete Connections page', () => current(id)?.route === 'Connections' && current(id)?.geometry['startup-last-used'])
  await delay(350)
}
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
    for (const file of ['startup.json', 'connections.json', 'windows.json']) if (fs.existsSync(storeFile(file))) fs.copyFileSync(storeFile(file), path.join(directory, file))
    if (fs.existsSync(storeFile('drafts'))) fs.cpSync(storeFile('drafts'), path.join(directory, 'drafts'), {recursive: true})
    await stopOwned(active.child)
    active = null
  }
}
async function primary() {
  return wait('initial complete primary Viewer', () => snapshots().find(snapshot => snapshot.kind === 'primary_launch' && snapshot.native?.position && snapshot.geometry['composer-input'] && !snapshot.loading && !snapshot.route_pending && !snapshot.error))
}


// Successful startup may populate the newly ported at-rest fleet. These reads
// do not authorize opening another foreground session.
function passiveFleetRead(read) {
  return (read.transport === 'http' && read.method === 'GET' && [
    '/api/profiles', '/api/status', '/api/profiles/sessions/sidebar', '/api/profiles/sessions', '/api/cron/jobs'
  ].includes(read.path)) || (read.transport === 'rpc' && read.method === 'session.list' && read.params?.scope === 'live')
}
async function checkBoot(source, profile, {started}) {
  const first = await primary()
  await ready(first.id, source, profile)
  await delay(400)
  assert.equal(visible().length, 1)
  assert.equal(current(first.id).registry.loaded, true)
  assert.equal(current(first.id).registry.primary, IDS.A)
  const bootReads = requests.slice(started)
  assert.ok(bootReads.some(read => read.source === source && read.method === 'session.list'))
  assert.ok(bootReads.every(read => read.source === source || passiveFleetRead(read)), 'Startup activated a different source instead of the selected default/policy')
  await capture(`startup-${source}-${profile}`, first.id)
}
async function profileMenu(id, profile, compact = false) {
  const door = await profileDoor(id, profile, compact || active.config.compact)
  await click(id, door, 3)
  await wait('profile route context menu', () => current(id)?.geometry['profile-route-actions-item1'])
  await delay(200)
}
async function selectProfile(id, name) {
  await click(id, `profile-${name}`)
  await wait('explicit profile selection', () => current(id)?.profile === name && !current(id)?.loading && !current(id)?.route_pending && current(id)?.geometry['composer-input'])
}
async function windowDefaultIntents() {
  const main = await primary(); await ready(main.id, 'A', 'research')
  const mainText = 'Unsent primary A research draft'
  await type(main.id, mainText)
  const mainEditor = current(main.id).editor
  phase = 'default-mirror-does-not-navigate'
  setDefault('B', 'archive')
  await mirror(main.id, 'B', 'archive')
  assert.ok(sameSource(current(main.id), 'A')); assert.equal(current(main.id).profile, 'research'); assert.equal(current(main.id).draft, mainText)
  await capture('default-mirror-preserved-primary', main.id)

  phase = 'ordinary-peer-boot-only-inheritance'
  const beforePeer = new Set(snapshots().map(s => s.id))
  key(main.id, 'ctrl+shift+n')
  const peer = await wait('ordinary complete peer', () => snapshots().find(s => !beforePeer.has(s.id) && s.kind === 'peer_inherited' && !s.loading && !s.error && s.geometry['composer-input']))
  await ready(peer.id, 'A', 'research'); await mirror(peer.id, 'B', 'archive')
  assert.equal(visible().length, 2); assert.notEqual(peer.editor, mainEditor)
  const peerText = 'Unsent ordinary peer A research draft'
  await type(peer.id, peerText)
  const peerEditor = current(peer.id).editor
  assert.equal(current(main.id).draft, mainText)

  phase = 'profile-menu-exact-route-without-selection'
  await profileMenu(main.id, 'archive')
  await capture('profile-default-menu-light', main.id)
  await click(main.id, 'profile-route-actions-item1')
  await mirror(main.id, 'A', 'archive'); await mirror(peer.id, 'A', 'archive')
  assert.equal(current(main.id).profile, 'research'); assert.ok(sameSource(current(main.id), 'A')); assert.equal(current(main.id).draft, mainText)
  assert.deepEqual(defaultDocument().defaultRoute, {connectionId: IDS.A, profile: 'archive'})
  await profileMenu(main.id, 'archive')
  await wait('checked default offers clear', () => current(main.id)?.geometry['profile-route-actions-item2'])
  await capture('checked-default-menu-light', main.id)
  await click(main.id, 'profile-route-actions-item2')
  await mirror(main.id, null); assert.equal(defaultDocument().defaultRoute, null)
  assert.equal(current(main.id).draft, mainText)
  setDefault('B', 'archive'); await mirror(main.id, 'B', 'archive'); await mirror(peer.id, 'B', 'archive')

  phase = 'ordinary-peer-new-session-device-default'
  key(peer.id, 'ctrl+n'); await ready(peer.id, 'B', 'archive')
  assert.equal(current(peer.id).session, null)
  await retainedDraft(peerEditor, 'research', peerText, 'A')
  assert.equal(current(main.id).draft, mainText); assert.ok(sameSource(current(main.id), 'A'))
  await capture('ordinary-peer-device-default', peer.id)

  phase = 'profile-pinned-bot-window'
  await click(main.id, 'bots-tab')
  await wait('current-source canonical Bot door', () => current(main.id)?.geometry['bot-research'])
  await click(main.id, 'bot-research', 3)
  await wait('Bot context menu', () => current(main.id)?.geometry['conversation-actions-item2'])
  const beforePin = new Set(snapshots().map(s => s.id))
  await click(main.id, 'conversation-actions-item2')
  const pinned = await wait('profile-pinned complete Bot Viewer', () => snapshots().find(s => !beforePin.has(s.id) && s.kind === 'profile_pinned' && s.session === 'A-research-bot' && !s.loading && !s.error))
  await ready(pinned.id, 'A', 'research')
  const pinnedRecord = JSON.parse(fs.readFileSync(storeFile('windows.json'), 'utf8')).records[pinned.id].pinned_route
  assert.equal(pinnedRecord.profile, 'research')
  await selectProfile(pinned.id, 'archive'); await ready(pinned.id, 'A', 'archive')
  key(pinned.id, 'ctrl+n'); await ready(pinned.id, 'A', 'research')
  assert.equal(current(pinned.id).session, null)
  assert.deepEqual(JSON.parse(fs.readFileSync(storeFile('windows.json'), 'utf8')).records[pinned.id].pinned_route, pinnedRecord)
  assert.deepEqual(defaultDocument().defaultRoute, {connectionId: IDS.B, profile: 'archive'})
  await capture('profile-pinned-beats-device-default', pinned.id)

  phase = 'profile-pin-delayed-return-cancelled-by-current-owner-noop'
  await openConnections(pinned.id)
  await click(pinned.id, `saved-connect-${IDS.B}`)
  await wait('explicit B source in pinned Viewer', () => sameSource(current(pinned.id), 'B') && current(pinned.id)?.profile === 'archive' && !current(pinned.id)?.loading && !current(pinned.id)?.registry.busy)
  await click(pinned.id, 'page-back'); await ready(pinned.id, 'B', 'archive')
  const explicitB = current(pinned.id)
  const release = await holdRegistryWriter()
  try {
    key(pinned.id, 'ctrl+n')
    await wait('pinned A return held while B stays visible', () => current(pinned.id)?.loading && sameSource(current(pinned.id), 'B'))
    await click(pinned.id, 'profile-archive') // Already current: still a newer explicit owner intent.
  } finally { await release() }
  await delay(1200)
  await ready(pinned.id, 'B', 'archive')
  assert.equal(current(pinned.id).source_epoch, explicitB.source_epoch)
  assert.equal(current(pinned.id).editor, explicitB.editor)
  await capture('same-owner-profile-cancels-delayed-pin-return', pinned.id)
  key(pinned.id, 'ctrl+n'); await ready(pinned.id, 'A', 'research')
  assert.deepEqual(JSON.parse(fs.readFileSync(storeFile('windows.json'), 'utf8')).records[pinned.id].pinned_route, pinnedRecord)

  phase = 'explicit-current-owner-session-and-profile'
  await click(main.id, 'sessions-tab')
  await wait('explicit current-source session door', () => sessionKey(main.id, 'research', 'A-research-notes'))
  await click(main.id, 'session-research-A-research-notes')
  await wait('explicit stored session owner', () => current(main.id)?.session === 'A-research-notes' && !current(main.id)?.loading)
  assert.ok(sameSource(current(main.id), 'A'))
  assert.ok(JSON.stringify(current(main.id).messages).includes('Private history on A/research'))
  await selectProfile(main.id, 'archive'); await ready(main.id, 'A', 'archive')
  const archiveText = 'Unsent explicit A archive draft'
  await type(main.id, archiveText)
  const archiveEditor = current(main.id).editor
  assert.ok(sameSource(current(main.id), 'A'), 'Same-named device-default profile rehomed an explicit owner action')
  await click(main.id, 'theme-toggle'); await wait('actual dark appearance', () => current(main.id)?.dark === true)
  await profileMenu(main.id, 'archive'); await capture('profile-menu-dark', main.id)
  key(main.id, 'Escape'); await delay(200)

  phase = 'generic-new-session-captures-device-default-and-retains-old-inputs'
  key(main.id, 'ctrl+n'); await ready(main.id, 'B', 'archive')
  assert.equal(current(main.id).session, null)
  await retainedDraft(mainEditor, 'research', mainText, 'A')
  await retainedDraft(archiveEditor, 'archive', archiveText, 'A')
  assert.ok(sameSource(current(pinned.id), 'A')); assert.equal(current(pinned.id).profile, 'research')
  assert.notEqual(current(main.id).editor, archiveEditor)
  await capture('generic-default-retained-old-owner-inputs-dark', main.id)
}
async function profileMemoryHooks() {
  const main = await primary(); await ready(main.id, 'A', 'research')
  setDefault('B', 'archive'); await mirror(main.id, 'B', 'archive')
  // Another window's last-used source must survive profile-only actions in this A window.
  const document = registryDocument(); document.lastUsed = IDS.B; atomic(storeFile('connections.json'), document)
  await click(main.id, 'session-research-A-research-notes')
  await wait('stored research tab', () => current(main.id)?.session === 'A-research-notes' && !current(main.id)?.loading)
  phase = 'accepted-bot-profile-memory'
  await click(main.id, 'bots-tab')
  await click(main.id, 'bot-archive', 3)
  await wait('Bot context new-tab door', () => drawnFrame(main.id)?.elements.some(element => element.key === 'conversation-actions-item1' && !element.exiting))
  await click(main.id, 'conversation-actions-item1')
  await wait('explicit archive Bot opened', () => current(main.id)?.session === 'A-archive-bot' && current(main.id)?.profile === 'archive' && !current(main.id)?.loading && !current(main.id)?.error)
  await wait('Bot response confirmed per-source profile', () => registryDocument().lastProfiles[IDS.A] === 'archive')
  assert.equal(registryDocument().lastUsed, IDS.B)
  assert.ok(sameSource(current(main.id), 'A'))
  assert.deepEqual(defaultDocument().defaultRoute, {connectionId: IDS.B, profile: 'archive'})
  await capture('bot-open-updates-source-profile-only', main.id)
  phase = 'accepted-old-tab-profile-memory'
  await click(main.id, 'sessions-tab')
  await click(main.id, 'activate-tab-0')
  await wait('old stored research tab opened across profiles', () => current(main.id)?.session === 'A-research-notes' && current(main.id)?.profile === 'research' && !current(main.id)?.loading && !current(main.id)?.error)
  await wait('old-tab response confirmed per-source profile', () => registryDocument().lastProfiles[IDS.A] === 'research')
  assert.equal(registryDocument().lastUsed, IDS.B)
  assert.ok(sameSource(current(main.id), 'A'))
  await capture('stored-tab-updates-source-profile-only', main.id)
  phase = 'profile-pick-retains-other-window-last-used'
  await selectProfile(main.id, 'archive'); await ready(main.id, 'A', 'archive')
  await wait('profile-only pick was confirmed', () => registryDocument().lastProfiles[IDS.A] === 'archive')
  assert.equal(registryDocument().lastUsed, IDS.B)
  assert.deepEqual(defaultDocument().defaultRoute, {connectionId: IDS.B, profile: 'archive'})
}
async function pendingInputCancelsRoute() {
  const main = await primary(); await ready(main.id, 'A', 'research')
  await type(main.id, 'Original unsent text before route intent')
  setDefault('B', 'archive'); await mirror(main.id, 'B', 'archive')
  const before = current(main.id), registry = registryDocument(), started = requests.length
  phase = 'pending-route-new-input'
  gateways.B.holdNextProfileRead = true
  key(main.id, 'ctrl+n')
  await wait('target validation held with the outgoing composer alive', () => gateways.B.heldProfileRead && current(main.id)?.route_pending)
  const newer = 'New text entered while the other workspace was opening'
  await type(main.id, newer)
  gateways.B.releaseProfileRead()
  await wait('newer input cancels the old navigation intent', () => !current(main.id)?.route_pending && sameSource(current(main.id), 'A') && current(main.id)?.draft === newer)
  await delay(400)
  assert.equal(current(main.id).source_epoch, before.source_epoch)
  assert.equal(current(main.id).editor, before.editor)
  assert.equal(current(main.id).profile, 'research')
  assert.equal(registryDocument().lastUsed, registry.lastUsed)
  assert.equal(requests.slice(started).filter(request => request.source === 'B' && request.method === 'session.list' && request.params?.scope !== 'live').length, 0)
  await capture('new-input-fences-delayed-route', main.id)
  await retainedDraft(before.editor, 'research', newer, 'A')
  key(main.id, 'ctrl+n'); await ready(main.id, 'B', 'archive')
  await retainedDraft(before.editor, 'research', newer, 'A')
}
async function singleSourceControls() {
  const main = await primary(); await ready(main.id, 'A', 'research')
  await openConnections(main.id)
  assert.ok(current(main.id).geometry[`saved-primary-${IDS.A}`], 'Primary control disappeared with one saved source')
  assert.ok(current(main.id).geometry['startup-last-used'], 'Startup mode control disappeared with one saved source')
  await capture('single-source-startup-controls-light', main.id)
  await click(main.id, 'startup-last-used')
  await wait('saved last-used mode', () => !current(main.id)?.registry.busy && current(main.id)?.registry.launch_mode === 'last-used')
  assert.equal(registryDocument().launchMode, 'last-used'); assert.equal(registryDocument().primary, IDS.A)
  assert.ok(sameSource(current(main.id), 'A')); assert.equal(current(main.id).profile, 'research')
  await click(main.id, 'startup-last-used')
  await wait('saved primary mode', () => !current(main.id)?.registry.busy && current(main.id)?.registry.launch_mode === 'primary')
  await click(main.id, 'device-default-clear'); await mirror(main.id, null)
  assert.equal(defaultDocument().defaultRoute, null)
  assert.equal(current(main.id).route, 'Connections'); assert.ok(sameSource(current(main.id), 'A'))
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
async function policyWriteError() {
  const main = await primary(); await ready(main.id, 'A', 'research')
  await openConnections(main.id)
  const before = fs.readFileSync(storeFile('connections.json'))
  phase = 'rejected-make-primary-write'
  let release = await holdRegistryWriter()
  try {
    await click(main.id, `saved-primary-${IDS.B}`)
    await wait('primary write is actually pending', () => current(main.id)?.registry.busy)
    await wait('truthful primary write contention error', () => !current(main.id)?.registry.busy && /another window|try again|locked/i.test(current(main.id)?.registry.error ?? ''))
    assert.deepEqual(fs.readFileSync(storeFile('connections.json')), before)
    assert.equal(current(main.id).registry.primary, IDS.A); assert.ok(sameSource(current(main.id), 'A'))
    await capture('make-primary-write-contention-dark', main.id)
  } finally { await release() }
  phase = 'rejected-startup-mode-write'
  release = await holdRegistryWriter()
  try {
    await click(main.id, 'startup-last-used')
    await wait('startup-mode write is actually pending', () => current(main.id)?.registry.busy)
    await wait('truthful startup-mode write contention error', () => !current(main.id)?.registry.busy && /another window|try again|locked/i.test(current(main.id)?.registry.error ?? ''))
    assert.deepEqual(fs.readFileSync(storeFile('connections.json')), before)
    assert.equal(current(main.id).registry.launch_mode, 'primary')
    assert.ok(sameSource(current(main.id), 'A'))
    await capture('launch-mode-write-contention-dark', main.id)
  } finally { await release() }
  await click(main.id, `saved-primary-${IDS.B}`)
  await wait('explicit primary policy save', () => !current(main.id)?.registry.busy && current(main.id)?.registry.primary === IDS.B)
  assert.equal(registryDocument().primary, IDS.B)
  assert.ok(sameSource(current(main.id), 'A'), 'Make Primary moved the current workspace')
  assert.equal(current(main.id).profile, 'research')
}
async function compactProfileDoors() {
  const main = await primary(); await ready(main.id, 'A', 'research')
  await click(main.id, 'status-profile')
  await wait('condensed default and named profile doors', () => current(main.id)?.geometry['pick-profile-default'] && current(main.id)?.geometry['pick-profile-research'] && current(main.id)?.geometry['pick-profile-archive'])
  await capture(`condensed-profile-doors-${current(main.id).dark ? 'dark' : 'light'}`, main.id)
  await click(main.id, 'pick-profile-default', 3)
  await wait('condensed profile context menu', () => current(main.id)?.geometry['profile-route-actions-item1'])
  await capture(`condensed-default-context-${current(main.id).dark ? 'dark' : 'light'}`, main.id)
  await click(main.id, 'profile-route-actions-item1'); await mirror(main.id, 'A', 'default')
  assert.equal(current(main.id).profile, 'research', 'Setting the condensed default selected it')
  key(main.id, 'Escape'); await delay(150)
  key(main.id, 'ctrl+n'); await ready(main.id, 'A', 'default')
}
async function invalidDefault({started}) {
  const before = fs.readFileSync(storeFile('startup.json'))
  const xid = await wait('visible error Viewer', () => visible()[0])
  await delay(3000)
  assert.equal(requests.length, started, 'Invalid/future device preferences silently fell back to a gateway')
  assert.deepEqual(fs.readFileSync(storeFile('startup.json')), before)
  assert.equal(visible().length, 1)
  // Initial invalid preferences deliberately never authenticate a source, so proof inspection may remain gated.
  await captureXid('startup-preference-error-no-fallback', xid)
  const errors = snapshots().filter(s => s.client_error || s.device_default_error || s.error)
  if (snapshots().length) assert.ok(errors.length, 'Inspected error Viewer did not expose its startup failure')
  fs.writeFileSync(path.join(active.directory, 'no-fallback.json'), JSON.stringify({zero_gateway_requests: true, file_unchanged: true, inspector_available: snapshots().length > 0, native_window: xid, error_snapshots: errors}, null, 2) + '\n')
}
async function missingDefaultProfile({started}) {
  const before = fs.readFileSync(storeFile('startup.json'))
  const xid = await wait('visible missing-profile Viewer', () => visible()[0])
  const inspected = await wait('profile validation reached B', () => requests.slice(started).some(request => request.source === 'B' && request.method === 'profiles.list'))
  assert.ok(inspected)
  await delay(3000)
  const attempted = requests.slice(started)
  assert.ok(attempted.every(request => request.source === 'B'), 'Missing explicit profile fell back to primary A')
  const sessionReads = attempted.filter(request => request.method === 'session.list' || request.method === 'session.resume' || /\/api\/(?:profiles\/sessions|sessions\/)/.test(request.path ?? ''))
  assert.equal(sessionReads.filter(request => (request.params?.profile ?? request.query?.profile) !== active.config.defaultProfile).length, 0, 'Missing profile silently opened another profile or session')
  assert.equal(sessionReads.filter(request => request.transport === 'rpc').length, 0, 'Final Viewer connection continued to session reads after its explicit profile disappeared')
  if (!active.config.dropProfileBeforeViewer) assert.equal(sessionReads.length, 0, 'An initially absent default profile reached session data')
  assert.deepEqual(fs.readFileSync(storeFile('startup.json')), before)
  const observed = snapshots()
  if (observed.length) assert.ok(observed.some(snapshot => snapshot.error || snapshot.client_error), 'Missing explicit profile did not expose an error')
  if (active.config.dropProfileBeforeViewer) {
    const rejected = await wait('inspected final rejected profile', () => snapshots().find(snapshot => snapshot.error || snapshot.client_error))
    await wait('unavailable conversation is actually presented after profile rejection', () => drawnFrame(rejected.id)?.elements.some(element => element.key === 'conversation-activity' && element.text === 'Unavailable' && presented(element)))
    await wait('Cron truthful connection state is actually presented after profile rejection', () => drawnFrame(rejected.id)?.elements.some(element => element.key === 'cron-connection-state' && element.text === 'Connect this profile to view scheduled jobs.' && presented(element)))
  }
  await captureXid('missing-explicit-profile-no-fallback', xid, observed[0]?.id ?? null)
  fs.writeFileSync(path.join(active.directory, 'no-profile-fallback.json'), JSON.stringify({only_target_source_validated: true, no_wrong_profile_session_reads: true, no_viewer_session_rpc: true, file_unchanged: true, snapshots: observed}, null, 2) + '\n')
}
async function defaultWriteError() {
  const main = await primary(); await ready(main.id, 'A', 'research')
  const text = 'Unsent draft preserved through default write rejection'
  await type(main.id, text)
  const valid = defaultDocument()
  const future = JSON.stringify({version: 9, revision: 50, defaultRoute: {connectionId: IDS.B, profile: 'archive'}, future_required: true})
  atomic(storeFile('startup.json'), future)
  await profileMenu(main.id, 'archive')
  await click(main.id, 'profile-route-actions-item1')
  await wait('truthful default write error', () => /newer|invalid|retained/i.test(current(main.id)?.client_error ?? current(main.id)?.device_default_error ?? ''))
  assert.equal(fs.readFileSync(storeFile('startup.json'), 'utf8'), future)
  assert.ok(sameSource(current(main.id), 'A')); assert.equal(current(main.id).profile, 'research'); assert.equal(current(main.id).draft, text)
  await capture('default-future-file-write-error', main.id)
  // Repair must retry the user's actual rejected Set intent, without navigating its workspace.
  atomic(storeFile('startup.json'), valid)
  await mirror(main.id, 'A', 'archive')
  await wait('owned startup error cleared after repair', () => !current(main.id)?.device_default_error && !current(main.id)?.client_error)
  assert.deepEqual(defaultDocument().defaultRoute, {connectionId: IDS.A, profile: 'archive'})
  assert.ok(sameSource(current(main.id), 'A')); assert.equal(current(main.id).profile, 'research'); assert.equal(current(main.id).draft, text)
  await capture('default-write-repaired-intent-retained', main.id)
}

try {
  await privateDisplay()
  await runCase('default-precedence', {defaultSource: 'B', defaultProfile: 'research', lastUsed: 'B', launchMode: 'last-used'}, info => checkBoot('B', 'research', info))
  await runCase('primary-policy', {}, info => checkBoot('A', 'research', info))
  await runCase('last-used-policy', {launchMode: 'last-used'}, info => checkBoot('B', 'archive', info))
  await runCase('missing-last-used', {launchMode: 'last-used', lastUsed: 'removed-source'}, info => checkBoot('A', 'research', info))
  await runCase('window-default-intents', {defaultSource: 'A', defaultProfile: 'research', lastUsed: 'A'}, windowDefaultIntents)
  await runCase('profile-memory-hooks', {lastUsed: 'A'}, profileMemoryHooks)
  await runCase('pending-new-input', {defaultSource: 'A', defaultProfile: 'research', lastUsed: 'A'}, pendingInputCancelsRoute)
  await runCase('single-source-controls', {single: true, lastUsed: 'A', defaultSource: 'A', defaultProfile: 'research'}, singleSourceControls)
  await runCase('policy-write-error', {dark: true}, policyWriteError)
  await runCase('compact-profile-light', {compact: true, lastUsed: 'A'}, compactProfileDoors)
  await runCase('compact-profile-dark', {compact: true, dark: true, lastUsed: 'A'}, compactProfileDoors)
  await runCase('invalid-default', {invalidDefault: 'not valid JSON'}, invalidDefault)
  await runCase('future-default', {invalidDefault: {version: 9, revision: 4, defaultRoute: {connectionId: IDS.B, profile: 'archive'}}}, invalidDefault)
  await runCase('missing-default-profile', {defaultSource: 'B', defaultProfile: 'missing-profile'}, missingDefaultProfile)
  await runCase('profile-disappears-before-connect', {defaultSource: 'B', defaultProfile: 'research', dropProfileBeforeViewer: true}, missingDefaultProfile)
  await runCase('default-write-error', {defaultSource: 'A', defaultProfile: 'research', lastUsed: 'A'}, defaultWriteError)
  assert.ok(results.length, 'No acceptance case matched --case')
  assert.equal(requests.filter(request => ['prompt.submit', 'session.create', 'session.interrupt'].includes(request.method)).length, 0)
  const result = {binary_sha256: hash(fs.readFileSync(binary)), harness_sha256: harnessSha, actual_native_windows: true, complete_viewers: true, private_xvfb_dbus_xfwm: true, pointer_hold_ms: 40, pointer_wiggle_used: false, physical_audio_used: false, live_agents_used: false, provider_requests: 0, agent_mutations: 0, cases: results, screenshots, native_bindings: bindings}
  fs.writeFileSync(path.join(out, 'result.json'), JSON.stringify(result, null, 2) + '\n')
  console.log(JSON.stringify(result))
} finally {
  fs.writeFileSync(path.join(out, 'requests.json'), JSON.stringify(requests, null, 2) + '\n')
  fs.writeFileSync(path.join(out, 'pointer-actions.json'), JSON.stringify(gestures, null, 2) + '\n')
  for (const process of [...owned].reverse()) await stopOwned(process)
  await gateways.A.close(); await gateways.B.close()
  fs.rmSync(suiteHome, {recursive: true, force: true})
}
