#!/usr/bin/env node
/** Real complete native Hermes Viewers; mutations go only to strict private loopback fixtures. */
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

const app = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const args = process.argv.slice(2)
const option = name => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : null }
const binary = path.resolve(option('--binary') ?? path.join(app, 'target/debug/hermes-rune'))
const out = path.resolve(option('--out') ?? fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-fleet-native-evidence-')))
const only = option('--case')
fs.mkdirSync(out, {recursive: true})
assert.equal(fs.readdirSync(out).length, 0, 'Use a fresh evidence directory')
const suiteHome = fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-fleet-native-home-'))
const owned = [], requests = [], screenshots = [], gestures = [], bindings = [], results = [], checks = []
const IDS = {A: '11111111-1111-4111-8111-111111111111', B: '22222222-2222-4222-8222-222222222222'}
const REVISIONS = {A: '33333333-3333-4333-8333-333333333333', B: '44444444-4444-4444-8444-444444444444'}
const TOKEN = 'private-native-fixture-token'
const ROOT = 'shared-root', TIP = 'shared-tip', PROFILE = 'default', RENAME = 'B renamed from native fleet'
const catalog = {provider: 'fixture', model: 'fixture-model', providers: [{slug: 'fixture', name: 'Private fixture', authenticated: true, models: ['fixture-model']}]}
const delay = ms => new Promise(resolve => setTimeout(resolve, ms))
const hash = value => createHash('sha256').update(value).digest('hex')
const harnessSha = hash(fs.readFileSync(fileURLToPath(import.meta.url)))
const binarySha = hash(fs.readFileSync(binary))
assert.ok(option('--expect-sha'), 'An immutable candidate hash is required')
assert.equal(binarySha, option('--expect-sha'), 'Immutable binary SHA mismatch')
fs.copyFileSync(fileURLToPath(import.meta.url), path.join(out, 'harness.mjs'))
const credentialsFixture = path.join(app, 'scripts/lib/private-credentials.mjs')
const credentialsFixtureSha = hash(fs.readFileSync(credentialsFixture))
fs.mkdirSync(path.join(out, 'lib'))
fs.copyFileSync(credentialsFixture, path.join(out, 'lib/private-credentials.mjs'))
let env, active = null, phase = 'setup', failure = null, displayProcess, wm, bus
const scoped = () => ({case: active?.name ?? 'suite', phase})
const atomic = (file, value) => {
  fs.mkdirSync(path.dirname(file), {recursive: true, mode: 0o700})
  const temporary = file + '.' + randomUUID()
  fs.writeFileSync(temporary, typeof value === 'string' || Buffer.isBuffer(value) ? value : JSON.stringify(value, null, 2), {mode: 0o600})
  fs.renameSync(temporary, file)
}
const stable = value => Array.isArray(value) ? value.map(stable) : value && typeof value === 'object' ? Object.fromEntries(Object.keys(value).sort().map(key => [key, stable(value[key])])) : value
const version = source => ({id: {kind: 'saved', id: IDS[source]}, revision: REVISIONS[source]})
const routeKey = (source, profile) => hash(JSON.stringify(stable([version(source), profile])))
const profileKey = (source, profile = PROFILE, picker = false) => `${picker ? 'fleet-pick-profile-' : 'fleet-profile-'}${routeKey(source, profile)}`
function checked(name, value) { checks.push({...scoped(), name, value}); assert.ok(value, name) }

class Gateway {
  constructor(name) {
    this.name = name; this.sockets = new Set(); this.reset()
    this.http = createServer((req, res) => this.read(req, res).catch(error => this.fail(error, res)))
    this.ws = new WebSocketServer({server: this.http, path: '/api/ws'})
    this.ws.on('connection', (socket, req) => {
      this.sockets.add(socket); socket.on('close', () => this.sockets.delete(socket))
      try { this.auth(req) } catch (error) { failure ??= error; socket.close(); return }
      socket.on('message', bytes => { try { this.rpc(socket, JSON.parse(bytes)) } catch (error) { failure ??= error; socket.close() } })
    })
  }
  reset() {
    for (const session of this.sessions?.values() ?? []) clearInterval(session.timer)
    this.count = 3; this.sessions = new Map(); this.receipts = new Map(); this.mutations = []; this.admissions = []; this.events = []
  }
  names() { return ['default', 'dixie', 'archive'] }
  profiles() { return this.names().map(name => ({name, label: name === PROFILE ? 'Research' : name, display_name: name === PROFILE ? 'Research' : name, description: `Private ${this.name}/${name}`, path: name === 'dixie' ? `/private-fixture/${this.name}/root` : `/private-fixture/${this.name}/${name}`, canonical_session: {id: `shared-${name}-bot`, resolved_id: `shared-${name}-bot`}})) }
  session(profile, id = TIP) {
    assert.ok(this.names().includes(profile), `Unknown profile ${profile}`)
    const stored = id.replace(/^live-/, '')
    assert.ok([TIP, ROOT, 'old-tip', 'project-session', `shared-${profile}-bot`].includes(stored), `Unexpected stored identity ${id}`)
    const canonical = stored.endsWith('-bot'), key = `${profile}/${canonical || stored === 'project-session' ? stored : TIP}`
    if (!this.sessions.has(key)) this.sessions.set(key, {id: canonical || stored === 'project-session' ? stored : TIP, runtime: `live-${canonical || stored === 'project-session' ? stored : TIP}`, profile, rows: [{row_id: 1, role: 'assistant', content: `Distinct private history ${this.name}/${profile}/${stored}`}], revision: 7, generation: 4, sequence: 0, running: false, pinned: false, unread: false, title: `${this.name} shared notes`, subscribers: new Set(), text: '', tick: 0})
    return this.sessions.get(key)
  }
  rows(profile) { return (profile === 'all' ? this.names() : [profile]).map(name => {
    const session = this.session(name)
    return {id: TIP, profile: name, title: session.title, source: 'desktop', last_active: 1793310000, message_count: session.rows.length, _lineage_root_id: ROOT, _lineage_ids: [ROOT, 'old-tip'], pinned: session.pinned, unread: session.unread, cwd: '/private-fixture'}
  }) }
  auth(req) {
    const url = new URL(req.url, 'http://fixture')
    if (this.name === 'B') {
      if (req.headers.upgrade?.toLowerCase() === 'websocket') assert.equal(url.searchParams.get('token'), TOKEN)
      else assert.equal(req.headers['x-hermes-session-token'], TOKEN)
    } else { assert.equal(url.searchParams.has('token'), false); assert.equal(req.headers['x-hermes-session-token'], undefined) }
    assert.equal(req.headers.authorization, undefined)
    url.searchParams.delete('token')
    return url
  }
  async listen() { await new Promise(resolve => this.http.listen(0, '127.0.0.1', resolve)); this.endpoint = `http://127.0.0.1:${this.http.address().port}`; return this }
  fail(error, res) { failure ??= error; if (res && !res.headersSent) { res.writeHead(400, {'content-type': 'application/json'}); res.end('{}') } }
  answer(socket, frame, result) { if (socket.readyState === 1) socket.send(JSON.stringify({jsonrpc: '2.0', id: frame.id, result})) }
  snapshot(session) { return {session_id: session.runtime, stored_session_id: session.id, revision: session.revision, authority_epoch: 7, execution_generation: session.generation, replay_epoch: `${this.name}-fleet`, last_sequence: session.sequence, running: session.running, pending: [], prompts: [], messages: session.rows, info: {profile_name: session.profile, desktop_protocol: 'hermes-gateway-v1', model: 'fixture-model', provider: 'fixture'}} }
  event(session, type, payload) {
    const frame = {method: 'event', params: {session_id: session.runtime, type, payload, seq: ++session.sequence, replay_epoch: `${this.name}-fleet`, execution_generation: session.generation, authority_epoch: 7}}
    this.events.push({...scoped(), at: Date.now(), stored: session.id, type, payload})
    for (const socket of session.subscribers) if (socket.readyState === 1) socket.send(JSON.stringify(frame))
  }
  rpc(socket, frame) {
    requests.push({...scoped(), transport: 'rpc', source: this.name, ...frame})
    const p = frame.params ?? {}; const profile = p.profile ?? 'dixie'; assert.ok(this.names().includes(profile), `Unknown RPC profile ${profile} for ${frame.method}`)
    let result
    switch (frame.method) {
      case 'runtime.describe': assert.deepEqual(p, {}); result = {profile_id: `/private-fixture/${this.name}/root`, instance_id: `private-${this.name}`, authority_epoch: 7, capabilities: [], session_create: {sources: ['desktop', 'gui'], parameters: []}}; break
      case 'projects.tree': result = {projects:[{id:'same-project',label:`${this.name} Project`,path:`/private/${this.name}/project`,sessionCount:1,previewSessions:[{id:TIP,profile,title:`${this.name} Project Session`}]}]}; break
      case 'projects.project_sessions': result = {project:{id:p.project_id,label:`${this.name} Project`,path:`/private/${this.name}/project`,repos:[{id:'repo',label:`${this.name} Repo`,groups:[{id:'lane',label:`${this.name} Lane`,sessions:[{id:'project-session',profile,title:`${this.name} Project Session`}]}]}]}}; break
      case 'session.create': { const session = this.session(profile); session.subscribers.add(socket); result = this.snapshot(session); break }
      case 'profiles.list': result = {profiles: this.profiles(), active: 'dixie'}; break
      case 'session.list': result = {profile, sessions: p.scope === 'live' ? [...this.sessions.values()].filter(s => s.profile === profile && s.running).map(s => ({id: s.runtime, profile: s.profile, status: 'working', running: true})) : this.rows(profile)}; break
      case 'session.resume': { const session = this.session(profile, p.session_id); session.subscribers.add(socket); result = this.snapshot(session); break }
      case 'model.options': result = catalog; break
      case 'config.get': result = {value: p.key === 'approvals.mode' ? 'smart' : null}; break
      case 'config.set': assert.equal(p.key, 'approvals.mode'); assert.equal(p.value, 'smart'); result = {key: p.key, value: p.value}; break
      case 'setup.runtime_check': result = {ok: true, ready: true}; break
      case 'profiles.get_asset': result = {data_url: null}; break
      case 'process.list': result = {processes: []}; break
      case 'subagent.list': result = {subagents: []}; break
      case 'prompt.submit': {
        const session = this.session(profile, p.session_id)
        this.admissions.push({profile, text:p.text, submission:p.submission_id})
        this.answer(socket, frame, {status:'streaming', admission_id:`admit-${p.submission_id}`, execution_generation:session.generation})
        this.event(session, 'message.complete', {status:'completed', text:`${this.name} synthetic response`}); return
      }
      case 'session.interrupt': {
        assert.equal(this.name, 'B'); const session = this.session(profile, p.session_id); assert.equal(session.running, true)
        session.running = false; clearInterval(session.timer)
        if (session.text) session.rows.push({row_id: session.rows.length + 1, role: 'assistant', content: session.text})
        this.answer(socket, frame, {interrupted: true}); this.event(session, 'message.complete', {status: 'interrupted', text: session.text, partial: true}); return
      }
      default: throw new Error(`Forbidden RPC ${frame.method}; native acceptance uses only the private allowlist`)
    }
    this.answer(socket, frame, result)
  }
  async read(req, res) {
    const url = this.auth(req), chunks = []
    for await (const chunk of req) { chunks.push(chunk); assert.ok(chunks.reduce((sum, chunk) => sum + chunk.length, 0) < 16384) }
    const body = chunks.length ? JSON.parse(Buffer.concat(chunks).toString()) : null
    const query = Object.fromEntries(url.searchParams), profile = query.profile ?? 'dixie'
    requests.push({...scoped(), transport: 'http', source: this.name, method: req.method, path: url.pathname, query, body})
    assert.ok(profile === 'all' || this.names().includes(profile), `Unknown HTTP profile ${profile}`)
    let result
    const history = /^\/api\/sessions\/([^/]+)\/messages$/.exec(url.pathname)
    {
      assert.equal(req.method, 'GET', `Forbidden HTTP write ${req.method} ${url.pathname}`)
      if (history) { const session = this.session(profile, decodeURIComponent(history[1])); result = {profile, session_id: session.id, messages: session.rows} }
      else switch (url.pathname) {
        case '/api/profiles': result = {profiles: this.profiles(), active: 'dixie'}; break
        case '/api/profiles/sessions/sidebar': result = {recents: {sessions: this.rows(query.recents_profile ?? profile)}, messaging: {sessions: []}, cron: {sessions: []}}; break
        case '/api/profiles/sessions': result = {sessions: this.rows(profile)}; break
        case '/api/cron/jobs': result = {jobs: []}; break
        case '/api/fs/default-cwd': result = {cwd: '/private-fixture'}; break
        case '/api/fs/list': result = {entries: []}; break
        case '/api/fs/read-text': assert.ok(query.path?.endsWith('.gitignore')); result = {exists: false, content: ''}; break
        case '/api/config': result = {config: {model: {provider: 'fixture', default: 'fixture-model'}, approvals: {mode: 'smart'}}}; break
        case '/api/config/schema': result = {fields: {}}; break
        case '/api/model/options': result = catalog; break
        case '/api/model/auxiliary': result = {main: {provider: 'fixture', model: 'fixture-model'}, tasks: []}; break
        case '/api/model/moa': result = {presets: {default: {aggregator: {provider: 'fixture', model: 'fixture-model'}, reference_models: []}}, default_preset: 'default', active_preset: 'default'}; break
        case '/api/audio/elevenlabs/voices': result = {available: false, voices: []}; break
        case '/api/audio/voice-live/status': result = {ok: false}; break
        case '/api/status': result = {running: true, ready: true, install_id: `private-fleet-install-${this.name}`}; break
        case '/api/git/review/rev-parse': result = {sha: null}; break
        case '/api/git/review/list': result = {files: [], is_repo: false}; break
        case '/api/git/review/ship-info': result = {ghReady: false}; break
        case '/api/git/status': result = {is_repo: false}; break
        case '/api/git/branches': case '/api/git/base-branches': result = {branches: []}; break
        case '/api/git/worktrees': result = {worktrees: []}; break
        default: throw new Error(`Forbidden HTTP read ${url.pathname}`)
      }
    }
    res.writeHead(200, {'content-type': 'application/json'}); res.end(JSON.stringify(result))
  }
  async close() { for (const session of this.sessions.values()) clearInterval(session.timer); for (const socket of this.sockets) socket.terminate(); await new Promise(resolve => this.ws.close(resolve)); await new Promise(resolve => this.http.close(resolve)) }
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
async function click(id, name, button = 1, inset = null) {
  await wait(`native control ${name}`, () => current(id)?.geometry?.[name] || drawnFrame(id)?.elements.some(element => element.key === name && !element.exiting))
  const xid = focus(id)
  await delay(250)
  const element = await wait(`actually drawn control ${name}`, () => drawnFrame(id)?.elements.find(element => element.key === name && !element.exiting && element.opacity > 0 && element.visible[2] > 0 && element.visible[3] > 0))
  const rect = element.visible, frame = geometry(xid)
  const x = frame.x + rect[0] + (inset ?? rect[2] / 2), y = frame.y + rect[1] + rect[3] / 2
  assert.ok(rect[2] > 0 && rect[3] > 0 && x >= frame.x && x < frame.x + frame.width && y >= frame.y && y < frame.y + frame.height, `${name} is outside its native viewport`)
  input('mousemove', x, y, 'sleep', '0.08')
  const runtime = await wait(`native Runtime pointer agrees with X11 for ${name}`, () => {
    const value = drawnFrame(id)
    return value?.pointer && Math.abs(value.pointer[0] - (x - frame.x)) <= 2 && Math.abs(value.pointer[1] - (y - frame.y)) <= 2 && value
  }, 5000)
  assert.ok(runtime.elements.some(item => item.key === name && !item.exiting), `Actual native pointer does not hit ${name}`)
  gestures.push({...scoped(), id, xid, name, button, rect, drawn_rect: element.rect, pointer: input('getmouselocation', '--shell'), runtime_pointer: runtime.pointer, hold_ms: 40})
  input('mousedown', button, 'sleep', '0.04', 'mouseup', button)
  await delay(120)
}
async function type(id, value, target = 'composer-input') {
  await click(id, target, 1, 12)
  const xid = nativeWindow(id)
  assert.equal(input('getwindowfocus').trim(), xid)
  input('key', '--clearmodifiers', 'ctrl+a'); await delay(50)
  input('type', '--clearmodifiers', '--delay', '2', '--', value)
  if (target === 'composer-input') await wait('actual entered draft', () => current(id)?.draft === value)
  assert.equal(input('getwindowfocus').trim(), xid)
}
async function captureXid(name, xid, id = null) {
  focusXid(xid); await delay(450)
  const frame = geometry(xid), file = path.join(active.directory, name + '.png')
  execFileSync('/usr/bin/python3', ['-c', `import gi,os,sys;gi.require_version('Gdk','3.0');from gi.repository import Gdk;d=Gdk.Display.open(os.environ['DISPLAY']);r=d.get_default_screen().get_root_window();x,y,w,h=map(int,sys.argv[2:]);p=Gdk.pixbuf_get_from_window(r,x,y,w,h);p.savev(sys.argv[1],'png',[],[])`, file, frame.x, frame.y, frame.width, frame.height].map(String), {env: active.env, stdio: 'pipe'})
  const proof = file.replace(/\.png$/, '.json')
  atomic(proof, {at: Date.now(), ...scoped(), id, xid, pid: active.child.pid, geometry: frame, snapshot: id ? current(id) : null, frame: id ? drawnFrame(id) : null})
  screenshots.push({...scoped(), window: id, file: path.relative(out, file), sha256: hash(fs.readFileSync(file)), proof: path.relative(out, proof)})
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
  env = {PATH: process.env.PATH, LANG: 'C.UTF-8', TZ: 'UTC', DISPLAY: ':' + display.trim(), HOME: suiteHome, USERPROFILE: suiteHome, XDG_RUNTIME_DIR: runtime, XDG_STATE_HOME: path.join(suiteHome, 'state'), XDG_CONFIG_HOME: path.join(suiteHome, 'config'), XDG_DATA_HOME: path.join(suiteHome, 'data'), XDG_CACHE_HOME: path.join(suiteHome, 'cache'), HERMES_HOME: path.join(suiteHome, 'unused-agent'), HERMES_GATEWAY_TOKEN: '', PULSE_SERVER: `unix:${suiteHome}/no-pulse`, PIPEWIRE_REMOTE: `${suiteHome}/no-pipewire`, ALSA_CONFIG_PATH: alsa}
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
function principal(source, raw = false) {
  const inner = hash(`url:${new URL(gateways[source].endpoint).toString()}\0`)
  return raw ? inner : hash(`saved:${IDS[source]}:${REVISIONS[source]}:${inner}`)
}
function sameSource(snapshot, source) { return snapshot?.principal === principal(source, active.config.raw && source === 'A') }
async function ready(id, source, profile = PROFILE) {
  return wait(`connected ${source}/${profile}`, () => {
    const snapshot = current(id)
    return snapshot && sameSource(snapshot, source) && snapshot.profile === profile && !snapshot.loading && !snapshot.route_pending && !snapshot.error && snapshot.route === 'Chat' && frame(id).elements.some(e => e.key === 'composer-input' && !e.exiting) && snapshot
  })
}
const frame = id => drawnFrame(id) ?? {elements: []}
const available = element => !element.exiting && !element.inert && element.opacity > 0.95 && element.visible[2] > 1 && element.visible[3] > 1
function matchedRow(element, source) {
  if (!element.key?.startsWith('session-[')) return false
  try {
    const [stamp, profile, root] = JSON.parse(element.key.slice('session-'.length))
    return profile === PROFILE && root === ROOT && stamp.principal === principal(source, active.config.raw && source === 'A')
  } catch { return false }
}
function row(id, source) { return frame(id).elements.find(element => matchedRow(element, source) && !element.exiting) }
async function actualReveal(id, predicate, description) {
  const xid = focus(id)
  await delay(300)
  let element
  for (let attempt = 0; attempt < 24; attempt++) {
    element = frame(id).elements.find(predicate)
    if (element && available(element)) return {xid, element}
    const g = geometry(xid)
    const picker = frame(id).elements.find(e => e.key === 'profile-picker' && available(e))
    const viewport = picker?.visible ?? [0, 100, 220, Math.max(100, g.height - 300)]
    const direction = element && element.rect[1] < viewport[1] ? 4 : 5
    const x = g.x + viewport[0] + Math.min(100, viewport[2] / 2)
    const y = g.y + viewport[1] + viewport[3] / 2
    input('mousemove', x, y, 'sleep', '0.05', 'click', '--repeat', '2', '--delay', '60', direction)
    gestures.push({...scoped(), id, description, scroll: direction, viewport, os_pointer: input('getmouselocation', '--shell')})
    await delay(150)
  }
  assert.ok(element && available(element), `Actual native control unavailable: ${description}`)
}
async function actualClick(id, predicate, description, button = 1, inset = null) {
  const {xid, element} = await actualReveal(id, predicate, description)
  const rect = element.visible, g = geometry(xid)
  const x = g.x + rect[0] + (inset ?? rect[2] / 2), y = g.y + rect[1] + rect[3] / 2
  assert.ok(x >= g.x && x < g.x + g.width && y >= g.y && y < g.y + g.height, `Outside native window: ${description}`)
  input('mousemove', x, y, 'sleep', '0.08')
  const presented = await wait(`actual pointer agrees: ${description}`, () => {
    const currentFrame = frame(id)
    return currentFrame.pointer && Math.abs(currentFrame.pointer[0] - (x - g.x)) <= 2 && Math.abs(currentFrame.pointer[1] - (y - g.y)) <= 2 && currentFrame
  }, 5000)
  gestures.push({...scoped(), id, xid, description, key: element.key, path: element.path, label: element.label, button, rect, pointer: input('getmouselocation', '--shell'), runtime_pointer: presented.pointer, hold_ms: 40})
  input('mousedown', button, 'sleep', '0.04', 'mouseup', button)
  await delay(180)
}
async function clickRow(id, source, button = 1) {
  await wait(`fetched source-qualified row ${source}`, () => row(id, source))
  return actualClick(id, element => matchedRow(element, source), `source ${source} same-id row`, button)
}
function menuPrefix(id, source) {
  const elements = frame(id).elements
  if (sameSource(current(id), source)) return 'sidebar-session-actions'
  return elements.find(e => e.key?.startsWith('owned-session-actions-') && e.key.endsWith('-item0') && !e.exiting)?.key.slice(0, -'-item0'.length)
}
async function rowAction(id, source, index) {
  await clickRow(id, source, 3)
  const prefix = await wait(`actual ${source} row context menu`, () => menuPrefix(id, source))
  await actualClick(id, element => element.key === `${prefix}-item${index}`, `${source} row action ${index}`)
}
function pendingReceipts(source) {
  const directory = storeFile('sidebar-edits')
  if (!fs.existsSync(directory)) return []
  return fs.readdirSync(directory).filter(file => file.endsWith('.json')).flatMap(file => {
    const receipt = JSON.parse(fs.readFileSync(path.join(directory, file), 'utf8'))
    return receipt?.mutation?.source?.version?.id?.id === IDS[source] ? [receipt] : []
  })
}
async function copiedInput(id, expected) {
  await click(id, 'composer-input', 1, 12)
  await wait('focused composer displays exact typed draft', () => frame(id).elements.some(e => e.focused && e.key === 'composer-input' && e.text === expected))
  input('key', '--clearmodifiers', 'ctrl+a'); await delay(150)
  input('key', '--clearmodifiers', 'ctrl+c')
  let copied
  for (let attempt = 0; attempt < 15; attempt++) {
    copied = execFileSync('/usr/bin/python3', ['-c', `import gi;gi.require_version('Gtk','3.0');from gi.repository import Gtk,Gdk;Gtk.init([]);print(Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD).wait_for_text() or '',end='')`], {env: active.env, encoding: 'utf8', timeout: 5000})
    if (copied === expected) break
    await delay(100)
  }
  assert.equal(copied, expected, 'Actual native clipboard must copy the exact selected draft')
  input('key', '--clearmodifiers', 'End')
}
async function persistedInput(source, editor, text) {
  await wait('private persisted input retains its actual source/editor identity', () => {
    const file = storeFile(`drafts/${principal(source, active.config.raw && source === 'A')}.json`)
    if (!fs.existsSync(file)) return false
    const document = JSON.parse(fs.readFileSync(file, 'utf8'))
    return document.entries?.some(entry => entry.editor === editor && entry.profile === PROFILE && entry.draft?.text === text)
  })
  checked('source-owned input is retained in its private persisted editor file', true)
}
function foreground(snapshot) { return {principal: snapshot.principal, source_epoch: snapshot.source_epoch, editor: snapshot.editor, profile: snapshot.profile, session: snapshot.session, draft: snapshot.draft} }
function assertForeground(id, expected, label) {
  assert.deepEqual(foreground(current(id)), expected, `${label}: source/profile/editor/session/input changed`)
  assert.ok(JSON.stringify(current(id).messages).includes('Distinct private history A/research'), `${label}: A history was replaced`)
  checked(label, true)
}
function sourceFacts(id) { return current(id)?.fleet?.sources ?? [] }
async function fleetCount(id, count) {
  return wait(`verified whole-fleet ${count} profiles`, () => {
    const sources = sourceFacts(id)
    const saved = sources.filter(s => (s.version ?? s.descriptor?.version)?.id?.kind === 'saved')
    return saved.length === 2 && saved.reduce((sum, source) => sum + source.profiles.length, 0) === count && saved
  }, 30000)
}
function squares(id) { return frame(id).elements.filter(e => !e.exiting && (gateways.A.names().some(name => e.key === `profile-${name}`) || gateways.B.names().some(name => e.key === profileKey('B', name)))) }
async function selectRowSource(id, source) {
  await clickRow(id, source)
  await ready(id, source)
  await wait(`source ${source} same-id history`, () => current(id)?.session === TIP && JSON.stringify(current(id).messages).includes(`Distinct private history ${source}/research`))
  return current(id)
}
async function flipTheme(id, dark, filename) {
  if (current(id).dark !== dark) await click(id, 'theme-toggle')
  await wait(`actual ${dark ? 'dark' : 'light'} appearance`, () => current(id)?.dark === dark)
  await capture(filename, id)
}
async function primary() { return wait('complete native primary Viewer', () => snapshots().find(s => s.kind === 'primary_launch' && s.native?.position && !s.loading && !s.route_pending && !s.error && frame(s.id).elements.some(e => e.key === 'composer-input' && !e.exiting))) }
function seededRegistry() { return {version: 1, rows: ['A'].map(source => ({id: IDS[source], revision: REVISIONS[source], label: `Private ${source}`, destination: {kind: 'remote', url: gateways[source].endpoint}, credential: null})), selected: null, primary: null, lastUsed: null, launchMode: 'primary', lastProfiles: {[IDS.A]: PROFILE, [IDS.B]: PROFILE}, cleanup: []} }
async function runCase(name, config, check) {
  if (only && only !== name) return
  for (const gateway of Object.values(gateways)) gateway.reset()
  const directory = path.join(out, name), home = path.join(suiteHome, name), state = path.join(home, 'state', 'hermes-desktop-rune'), inspection = path.join(directory, 'inspection')
  fs.mkdirSync(state, {recursive: true, mode: 0o700}); fs.mkdirSync(inspection, {recursive: true})
  active = {name, config, directory, home, state, inspection, child: null, log: '', windows: new Map(), env: {...env, HOME: home, USERPROFILE: home, XDG_STATE_HOME: path.join(home, 'state'), XDG_CONFIG_HOME: path.join(home, 'config'), XDG_DATA_HOME: path.join(home, 'data'), XDG_CACHE_HOME: path.join(home, 'cache'), HERMES_HOME: path.join(home, 'unused-agent')}}
  let credentials = null
  if (config.alias || config.root) {
    credentials = await startPrivateCredentials({env: active.env})
    active.env = {...credentials.env, USERPROFILE: credentials.env.HOME, HERMES_HOME: path.join(credentials.env.HOME, 'unused-agent')}
    const reference = `legacy-auth-${randomUUID()}`
    await credentials.putLegacyBundle(reference, TOKEN, {})
    config.reference = reference
  }
  phase = 'boot'
  const registry = seededRegistry()
  if (config.alias || config.root) registry.version = 2
  if (config.alias || config.root) registry.legacy = {version: 1, global: null, profiles: {default: {id: randomUUID(), edit_revision: randomUUID(), route_revision: randomUUID(), value: {mode: 'remote', kind: 'url', url: gateways.B.endpoint, auth_mode: 'token', remote_profile: config.root ? null : 'dixie', token_present: true, credential: {id: config.reference, version: 1}}}}, tombstones: {}}
  atomic(storeFile('connections.json'), registry)
  atomic(storeFile('startup.json'), {version: 1, revision: 1, defaultRoute: null})
  atomic(storeFile('preferences.json'), {theme: !!config.dark, speak_replies: false, sidebar_open: true, inspector_open: false, show_statusbar: true, show_profile_rail: !config.hidden, simple_mode: false})
  atomic(storeFile('display.json'), {version:1,default_project_dir:'/private/client-default'})
  atomic(storeFile('sounds.json'), {version: 1, preset: 0, muted: true})
  atomic(storeFile('notifications.json'), {version: 1, enabled: false})
  atomic(storeFile('sidebar.json'), {rail: true, all_profiles: false})
  if (!config.root) { active.env.HERMES_DESKTOP_REMOTE_URL = gateways.B.endpoint; active.env.HERMES_DESKTOP_REMOTE_TOKEN = TOKEN }
  const started = requests.length
  try {
    active.child = spawn(binary, ['--peer-window-proof-dir', inspection, ...(config.raw ? ['--gateway', gateways.A.endpoint, '--profile', PROFILE] : [])], {cwd: app, env: active.env, stdio: ['ignore', 'pipe', 'pipe']}); owned.push(active.child)
    active.child.stderr.on('data', bytes => active.log += bytes); active.child.stdout.on('data', bytes => active.log += bytes)
    await check({started})
    if (failure) throw failure
    const result = {name, passed: true, requests: requests.length - started, native_windows: active.windows.size, mutations: gateways.B.mutations, admissions: gateways.B.admissions, windows: snapshots()}
    await quit(); if (failure) throw failure; results.push(result); atomic(path.join(directory, 'result.json'), result)
  } catch (error) {
    for (const snapshot of snapshots()) { try { await capture(`failure-${snapshot.id}`, snapshot.id) } catch (captureError) { checks.push({...scoped(), capture_error: String(captureError)}) } }
    if (!snapshots().length) for (const [index, xid] of visible().entries()) { try { await captureXid(`failure-native-${index}`, xid) } catch {} }
    atomic(path.join(directory, 'failure.json'), {phase, error: String(error.stack ?? error), windows: snapshots(), frames: snapshots().map(s => frame(s.id))})
    throw error
  } finally {
    atomic(path.join(directory, 'app.log'), active.log)
    for (const file of ['startup.json', 'connections.json', 'windows.json', 'sidebar.json', 'preferences.json']) if (fs.existsSync(storeFile(file))) fs.copyFileSync(storeFile(file), path.join(directory, file))
    for (const directoryName of ['drafts', 'sidebar-edits']) if (fs.existsSync(storeFile(directoryName))) fs.cpSync(storeFile(directoryName), path.join(directory, directoryName), {recursive: true})
    try { await quit() } catch (error) { checks.push({...scoped(), quit_error:String(error)}) }
    await stopOwned(active.child); if (credentials) await credentials.stop(); active = null
  }
}
async function vertical() {
  const main = await primary(), id = main.id
  phase = 'independent-home-all'
  const byLabel = label => e => !e.exiting && e.role === 'button' && e.label === label
  const tree = await wait('project tree RPC', () => requests.find(r => r.case === active.name && r.source === 'A' && r.method === 'projects.tree'))
  assert.equal(tree.params.profile, PROFILE); assert.equal(tree.params.preview_limit, 3)
  await wait('project tree rendered', () => frame(id).elements.some(byLabel('A Project (1)')))
  await capture('project-tree', id)
  const previewVisible = frame(id).elements.some(byLabel('A Project Session'))
  checks.push({...scoped(), name:'tree preview session visibly rendered', value:previewVisible})
  const initialEditor = current(id).editor
  await type(id, 'parked original input')
  await click(id, 'nav-new-session')
  await wait('new draft has independent input', () => current(id).draft === '' && current(id).editor !== initialEditor)
  await persistedInput('A', initialEditor, 'parked original input')
  checked('fresh new chat rotates editor and parks prior input', true)
  await capture('new-draft', id)
  await actualClick(id, byLabel('Home'), 'A Home navigation')
  await click(id, 'nav-new-session')
  await type(id, 'home native synthetic')
  await click(id, 'composer-send')
  await wait('home create RPC', () => requests.filter(r => r.case === active.name && r.source === 'A' && r.method === 'session.create').length >= 1)
  const home = requests.filter(r => r.case === active.name && r.source === 'A' && r.method === 'session.create').at(-1)
  assert.equal(Object.hasOwn(home.params,'cwd'), false)
  checked('A Home session.create has no cwd', true)
  await actualClick(id, byLabel('All projects'), 'A All navigation')
  await click(id, 'nav-new-session')
  await type(id, 'all native synthetic')
  await capture('all-new-draft', id)
  await click(id, 'composer-send')
  await wait('All create RPC', () => requests.filter(r => r.case === active.name && r.source === 'A' && r.method === 'session.create').length >= 2)
  const all = requests.filter(r => r.case === active.name && r.source === 'A' && r.method === 'session.create').at(-1)
  assert.equal(all.params.cwd, '/private/client-default'); assert.equal(all.params.cwd_explicit, false)
  checked('A All inherits configured client default', true)
  phase = 'entered-project'
  await actualClick(id, byLabel('A Project (1)'), 'enter A project')
  const entered = await wait('entered project RPC', () => requests.find(r => r.case === active.name && r.source === 'A' && r.method === 'projects.project_sessions' && r.params.project_id === 'same-project'))
  assert.equal(entered.params.profile, PROFILE)
  await wait('project session rendered', () => frame(id).elements.some(byLabel('A Project Session')))
  await capture('entered-project', id)
  assert.notEqual(current(id).session, 'project-session', 'project target must not already be selected')
  const matchingResume = r => r.case === active.name && r.source === 'A' && r.method === 'session.resume' && r.params.session_id === 'project-session' && r.params.profile === PROFILE
  const beforeClick = requests.filter(matchingResume).length
  await actualClick(id, byLabel('A Project Session'), 'open project session')
  await wait('new matching project session resume RPC after click', () => requests.filter(matchingResume).length > beforeClick)
  await wait('source-distinct project history after click', () => current(id)?.session === 'project-session' && JSON.stringify(current(id).messages).includes('Distinct private history A/default/project-session'))
  checked('project session click resumes exact source profile and id', true)
  await click(id, 'nav-new-session')
  await type(id, 'project native synthetic')
  await click(id, 'composer-send')
  const project = await wait('project create RPC', () => requests.filter(r => r.case === active.name && r.source === 'A' && r.method === 'session.create').at(2))
  assert.equal(project.params.profile, PROFILE); assert.equal(project.params.cwd, '/private/A/project'); assert.equal(project.params.cwd_explicit, true)
  checked('project create captures source profile and explicit root', true)
  checked('tree preview session visibly rendered', previewVisible)
}
let passed = false
try {
  assert.ok(fs.statSync(binary).isFile(), 'Build the ordinary native application before acceptance')
  await privateDisplay()
  for (const [name, config] of [['projects-light',{raw:true}],['projects-dark',{raw:true,dark:true}]]) {
    try { await runCase(name, config, vertical) } catch (error) { results.push({name,passed:false,error:String(error.stack??error)}); failure = null }
  }
  assert.ok(results.length, 'No selected native case ran')
  if (failure) throw failure
  assert.equal(hash(fs.readFileSync(binary)), binarySha, 'Candidate changed during acceptance')
  passed = results.every(r => r.passed)
  if (!passed) process.exitCode = 1
} catch (error) {
  failure ??= error
  console.error(error.stack ?? error)
  process.exitCode = 1
} finally {
  for (const process of owned.slice().reverse()) await stopOwned(process)
  await Promise.all(Object.values(gateways).map(gateway => gateway.close()))
  atomic(path.join(out, 'requests.json'), requests)
  atomic(path.join(out, 'gestures.json'), gestures)
  atomic(path.join(out, 'result.json'), {passed, binary, binary_sha256: binarySha, harness_sha256: harnessSha, credentials_fixture_sha256: credentialsFixtureSha, source_head: execFileSync('git', ['rev-parse', 'HEAD'], {cwd: app, encoding: 'utf8'}).trim(), suite_home: suiteHome, failure: failure ? String(failure.stack ?? failure) : null, cases: results, checks, bindings, screenshots, unproved: ['retry and loading', 'scoped sidebar exclusion', 'independent editor and parked input', 'canonical Bot create', 'voice and slash create', 'deferred activation and cross-source ownership', 'NativeSelf create', 'restart restoration'], owned: owned.map(c => ({pid:c.pid, exitCode:c.exitCode, signalCode:c.signalCode}))})
  console.log(JSON.stringify({passed, out, cases: results.map(({name, passed}) => ({name, passed})), failure: failure ? String(failure) : null}))
}
