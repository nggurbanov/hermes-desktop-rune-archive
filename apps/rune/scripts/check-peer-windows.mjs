#!/usr/bin/env node
/** Ordinary Hermes Viewers in actual native windows; only private fake gateways are used. */
import assert from 'node:assert/strict'
import {createHash} from 'node:crypto'
import {createServer} from 'node:http'
import {spawn, execFileSync} from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import {fileURLToPath} from 'node:url'
import {WebSocketServer} from 'ws'

const app = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const args = process.argv.slice(2), option = name => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : null }
const binary = path.resolve(option('--binary') ?? path.join(app, 'target/debug/hermes-rune'))
const out = path.resolve(option('--out') ?? fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-peer-native-evidence-')))
fs.mkdirSync(out, {recursive: true})
assert.equal(fs.readdirSync(out).length, 0, 'Native acceptance requires a fresh evidence directory')
const home = fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-peer-native-home-'))
const state = path.join(home, 'state', 'hermes-desktop-rune'), inspection = path.join(out, 'inspection')
fs.mkdirSync(state, {recursive: true}); fs.mkdirSync(inspection, {recursive: true})
const token = 'synthetic-native-peer-token', calls = [], reads = [], owned = [], screenshots = []
const nativeWindows = new Map(), nativeBindings = []
const pointerActions = []
let failure, stderr = '', phase = 'boot', child, wm, xvfb, bus, env, clipboardInputs = null
const delay = ms => new Promise(resolve => setTimeout(resolve, ms))
const hash = bytes => createHash('sha256').update(bytes).digest('hex')
const harnessSha = hash(fs.readFileSync(fileURLToPath(import.meta.url)))
fs.copyFileSync(fileURLToPath(import.meta.url), path.join(out, 'harness.mjs'))
const title = (id, profile) => id.endsWith('-bot') ? 'Bot Chat' : `${profile} notes`
const catalog = {provider: 'fixture', model: 'fixture-model', providers: [{slug: 'fixture', name: 'Fixture provider', models: ['fixture-model'], authenticated: true}]}
class FakeGateway {
  constructor(name) {
    this.name = name; this.sockets = new Set(); this.sessions = new Map(); this.serial = 0; this.admissions = []
    for (const profile of ['research', 'archive']) for (const suffix of ['notes', 'bot']) {
      const id = `${name}-${profile}-${suffix}`
      this.sessions.set(id, {id, profile, runtime: `live-${id}`, rows: [{row_id: 1, role: 'assistant', content: `${name} / ${profile} source history`}], running: false, generation: 0, sequence: 0, subscribers: new Set()})
    }
    this.http = createServer((request, response) => this.httpRequest(request, response))
    this.ws = new WebSocketServer({server: this.http, path: '/api/ws'})
    this.ws.on('connection', (socket, request) => {
      this.sockets.add(socket); socket.on('close', () => this.sockets.delete(socket))
      try { assert.equal(new URL(request.url, 'http://fixture').searchParams.get('token'), token) } catch (error) { this.fail(error); socket.close(); return }
      socket.on('message', bytes => { try { this.rpc(socket, JSON.parse(bytes)) } catch (error) { this.fail(error); socket.close() } })
    })
  }
  fail(error) { failure ??= error }
  async listen() { await new Promise(resolve => this.http.listen(0, '127.0.0.1', resolve)); this.endpoint = `http://127.0.0.1:${this.http.address().port}`; return this }
  profileRows() { return ['research', 'archive'].map(name => ({name, label: name === 'research' ? 'Research' : 'Archive', canonical_session: {id: `${this.name}-${name}-bot`, resolved_id: `${this.name}-${name}-bot`}})) }
  rows(profile) { return [...this.sessions.values()].filter(s => s.profile === profile && !s.id.endsWith('-bot')).map(s => ({id: s.id, title: title(s.id, profile), profile, cwd: '/fixture', last_active: 1793300000, message_count: s.rows.length, source: 'desktop'})) }
  answer(socket, frame, result) { socket.send(JSON.stringify({jsonrpc: '2.0', id: frame.id, result})) }
  find(id) { const session = [...this.sessions.values()].find(s => s.id === id || s.runtime === id); assert.ok(session, `Unknown session ${id}`); return session }
  snapshot(session) { return {session_id: session.runtime, stored_session_id: session.id, revision: 1, execution_generation: session.generation, authority_epoch: 7, replay_epoch: `${this.name}-epoch`, last_sequence: session.sequence, running: session.running, messages: session.rows, prompts: [], pending: [], info: {profile_name: session.profile, desktop_protocol: 'hermes-gateway-v1', model: 'fixture-model', provider: 'fixture'}} }
  event(session, type, payload) {
    const event = {method: 'event', params: {session_id: session.runtime, type, payload, seq: ++session.sequence, replay_epoch: `${this.name}-epoch`, execution_generation: session.generation, authority_epoch: 7}}
    for (const socket of session.subscribers) if (socket.readyState === 1) socket.send(JSON.stringify(event))
  }
  rpc(socket, frame) {
    calls.push({phase, source: this.name, ...frame})
    const p = frame.params ?? {}
    assert.ok(['research', 'archive', 'default'].includes(p.profile), `Unexpected profile ${p.profile} in ${frame.method}`)
    let result
    switch (frame.method) {
      case 'profiles.list': result = {profiles: this.profileRows(), active: 'research'}; break
      case 'session.list': result = {sessions: p.scope === 'live' ? [] : this.rows(p.profile)}; break
      case 'model.options': result = catalog; break
      case 'config.get': result = {value: p.key === 'approvals.mode' ? 'smart' : null}; break
      case 'setup.runtime_check': result = {ok: true, ready: true}; break
      case 'profiles.get_asset': result = {data_url: null}; break
      case 'process.list': result = {processes: []}; break
      case 'subagent.list': result = {subagents: []}; break
      case 'session.resume': {
        const session = this.find(p.session_id); assert.equal(p.profile, session.profile); session.subscribers.add(socket); result = this.snapshot(session); break
      }
      case 'session.create': {
        assert.equal(p.source, 'gui'); assert.ok(p.idempotency_key || p.request_id)
        const id = `${this.name}-created-${++this.serial}`
        const session = {id, runtime: `live-${id}`, profile: p.profile, rows: [], running: false, generation: 0, sequence: 0, subscribers: new Set([socket])}
        this.sessions.set(id, session); result = this.snapshot(session); break
      }
      case 'prompt.submit': {
        const session = this.find(p.session_id); assert.equal(session.profile, p.profile)
        assert.ok(/^primary native send$|^peer native send$|^surviving peer send$/.test(p.text), `Unexpected prompt ${JSON.stringify(p.text)}`)
        assert.ok(p.submission_id); assert.ok(!this.admissions.some(a => a.submission === p.submission_id), 'Duplicate admission UUID')
        const admission = {source: this.name, profile: session.profile, runtime: session.runtime, stored: session.id, text: p.text, submission: p.submission_id}
        this.admissions.push(admission); session.running = true; session.generation++
        session.rows.push({row_id: session.rows.length + 1, role: 'user', content: p.text})
        result = {status: 'streaming', admission_id: `admit-${p.submission_id}`, execution_generation: session.generation}
        this.answer(socket, frame, result)
        this.event(session, 'message.start', {input_id: p.submission_id, admission_id: result.admission_id})
        setTimeout(() => this.event(session, 'message.delta', {text: `STREAM ${p.text} on ${this.name}/${session.profile}`, turn_id: p.submission_id, segment: 0, offset: 0}), 100)
        return
      }
      case 'session.interrupt': {
        const session = this.find(p.session_id); assert.equal(session.profile, p.profile); assert.equal(session.running, true)
        session.running = false; result = {interrupted: true}; this.answer(socket, frame, result)
        this.event(session, 'message.complete', {status: 'interrupted'}); return
      }
      default: throw new Error(`Forbidden or unexpected RPC ${frame.method}`)
    }
    this.answer(socket, frame, result)
  }
  httpRequest(request, response) {
    try {
      const url = new URL(request.url, 'http://fixture'); assert.equal(request.method, 'GET', 'Unexpected HTTP mutation'); assert.equal(url.searchParams.get('token'), token)
      const profile = url.searchParams.get('profile')
      reads.push({phase, source: this.name, route: url.pathname, query: Object.fromEntries(url.searchParams)})
      let result
      const history = /^\/api\/sessions\/([^/]+)\/messages$/.exec(url.pathname)
      if (history) { const session = this.find(decodeURIComponent(history[1])); assert.equal(profile, session.profile); result = {profile, session_id: session.id, messages: session.rows} }
      else switch (url.pathname) {
        case '/api/profiles': result = {profiles: this.profileRows(), active: 'research'}; break;
        case '/api/profiles/sessions/sidebar': result = {recents: {sessions: this.rows(profile)}, messaging: {sessions: []}, cron: {sessions: []}}; break
        case '/api/profiles/sessions': result = {sessions: profile === 'all' ? [...this.rows('research'), ...this.rows('archive')] : this.rows(profile)}; break
        case '/api/cron/jobs': result = {jobs: []}; break
        case '/api/fs/default-cwd': result = {cwd: '/fixture'}; break
        case '/api/fs/list': result = {entries: []}; break
        case '/api/fs/read-text': assert.ok(url.searchParams.get('path').endsWith('.gitignore')); result = {content: '', exists: false}; break
        case '/api/config': result = {config: {model: {provider: 'fixture', default: 'fixture-model'}, approvals: {mode: 'smart'}}}; break
        case '/api/config/schema': result = {fields: {'agent.max_turns': {type: 'number', category: 'agent'}}}; break
        case '/api/model/options': result = catalog; break
        case '/api/model/auxiliary': result = {main: {provider: 'fixture', model: 'fixture-model'}, tasks: []}; break
        case '/api/model/moa': result = {presets: {default: {aggregator: {provider: 'fixture', model: 'fixture-model'}, reference_models: []}}, default_preset: 'default', active_preset: 'default'}; break
        case '/api/audio/elevenlabs/voices': result = {available: false, voices: []}; break
        case '/api/status': result = {running: true, ready: true}; break
        case '/api/audio/voice-live/status': result = {ok: false}; break
        case '/api/git/review/rev-parse': result = {sha: null}; break
        case '/api/git/review/list': result = {files: [], is_repo: false}; break
        case '/api/git/review/ship-info': result = {ghReady: false}; break
        case '/api/git/status': result = {is_repo: false}; break
        case '/api/git/branches': case '/api/git/base-branches': result = {branches: []}; break
        case '/api/git/worktrees': result = {worktrees: []}; break
        default: throw new Error(`Unexpected HTTP read ${url.pathname}`)
      }
      response.writeHead(200, {'content-type': 'application/json'}); response.end(JSON.stringify(result))
    } catch (error) { this.fail(error); response.writeHead(400); response.end('{}') }
  }
  async close() { for (const socket of this.sockets) socket.terminate(); await new Promise(resolve => this.ws.close(resolve)); await new Promise(resolve => this.http.close(resolve)) }
}
function input(...args) { return execFileSync('xdotool', args.map(String), {env, encoding: 'utf8', timeout: 5000, stdio: ['ignore', 'pipe', 'pipe']}) }
function snapshots() { return fs.readdirSync(inspection).filter(file => file.endsWith('.json')).flatMap(file => { try { return [JSON.parse(fs.readFileSync(path.join(inspection, file)))] } catch { return [] } }) }
function current(id) { return snapshots().find(s => s.id === id) }
async function wait(label, check, ms = 20000) {
  const deadline = Date.now() + ms
  while (Date.now() < deadline) {
    if (failure) throw failure
    if (child && child.exitCode != null) throw new Error(`${label}: app exited ${child.exitCode}\n${stderr.slice(-4000)}`)
    const result = check(); if (result) return result
    await delay(40)
  }
  throw new Error(`${label} timed out; snapshots=${JSON.stringify(snapshots())}\n${stderr.slice(-2500)}`)
}
function visible() { try { return input('search', '--onlyvisible', '--pid', child.pid).trim().split('\n').filter(Boolean) } catch { return [] } }
function geometry(window) {
  const info = execFileSync('xwininfo', ['-id', String(window)], {env, encoding: 'utf8', timeout: 5000})
  const field = label => { const match = info.match(new RegExp(label + ':\\s+(-?\\d+)')); assert.ok(match, `Missing ${label} in ${info}`); return Number(match[1]) }
  return {WINDOW: Number(window), X: field('Absolute upper-left X'), Y: field('Absolute upper-left Y'), WIDTH: field('Width'), HEIGHT: field('Height')}
}
function osWindow(id) {
  const s = current(id); assert.ok(s?.native?.position)
  const windows = visible()
  const prior = nativeWindows.get(id)
  if (prior) { assert.ok(windows.includes(prior), `Native window ${id}/${prior} is no longer visible`); return prior }
  const raw = s.native_raw_handle?.match(/window:\s*(\d+)/)?.[1]
  const candidates = raw ? windows.filter(window => window === raw) : windows.filter(window => ![...nativeWindows.values()].includes(window))
  assert.equal(candidates.length, 1, `Cannot uniquely bind newly opened complete Viewer ${id} to its native window: ${JSON.stringify({windows, bound: [...nativeWindows]})}`)
  const window = candidates[0]; nativeWindows.set(id, window)
  assert.equal([...nativeWindows.values()].filter(xid => xid === window).length, 1, 'Two complete Viewers share a native XID')
  nativeBindings.push({id, xid: window, pid: child.pid, geometry: geometry(window), inspected_native: s.native, raw_handle: s.native_raw_handle ?? null})
  return window
}
function focus(id) {
  const window = osWindow(id)
  const tree = execFileSync('xwininfo', ['-id', window, '-tree'], {env, encoding: 'utf8', timeout: 5000})
  const parent = tree.match(/Parent window id:\s+(0x[0-9a-f]+)/i)?.[1]
  assert.ok(parent, 'The real native window has no X11 frame parent')
  input('windowraise', parent)
  if (input('getwindowfocus').trim() !== window) {
    input('windowactivate', window); input('sleep', '0.25')
    if (input('getwindowfocus').trim() !== window) input('windowfocus', '--sync', window)
    input('sleep', '0.15'); assert.equal(input('getwindowfocus').trim(), window)
  }
  return window
}
function key(id, chord) { focus(id); input('key', '--clearmodifiers', chord) }
function xPointer(window) {
  return JSON.parse(execFileSync('/usr/bin/python3', ['-c', `import ctypes as c,json,os,sys;x=c.CDLL('libX11.so.6');x.XOpenDisplay.restype=c.c_void_p;d=x.XOpenDisplay(os.environ['DISPLAY'].encode());x.XQueryPointer.argtypes=[c.c_void_p,c.c_ulong,c.POINTER(c.c_ulong),c.POINTER(c.c_ulong),c.POINTER(c.c_int),c.POINTER(c.c_int),c.POINTER(c.c_int),c.POINTER(c.c_int),c.POINTER(c.c_uint)];r,h=c.c_ulong(),c.c_ulong();rx,ry,wx,wy=c.c_int(),c.c_int(),c.c_int(),c.c_int();m=c.c_uint();ok=x.XQueryPointer(d,int(sys.argv[1]),c.byref(r),c.byref(h),c.byref(rx),c.byref(ry),c.byref(wx),c.byref(wy),c.byref(m));print(json.dumps(dict(ok=bool(ok),root=[rx.value,ry.value],local=[wx.value,wy.value],child=h.value,mask=m.value)))`, window], {env, encoding: 'utf8', timeout: 5000}))
}
const profileNames = ['default', 'research', 'archive']
function drawnFrame(id) { try { return JSON.parse(fs.readFileSync(path.join(inspection, 'frames', id + '.json'), 'utf8')) } catch { return null } }
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
  for (const profile of profileNames) {
    const prefix = `session-${profile}-`
    if (name.startsWith(prefix)) return sessionKey(id, profile, name.slice(prefix.length)) ?? name
  }
  return name
}
async function profileDoor(id, profile, compact = false) {
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
    const frame = geometry(xid), viewport = picker?.visible ?? [0, 100, 220, Math.max(100, frame.HEIGHT - 300)]
    const direction = element && element.rect[1] < viewport[1] ? 4 : 5
    input('mousemove', frame.X + viewport[0] + Math.min(100, viewport[2] / 2), frame.Y + viewport[1] + viewport[3] / 2, 'sleep', '0.05', 'click', '--repeat', '2', '--delay', '60', direction)
    pointerActions.push({phase, id, name, resolved_key: key, scroll: direction, viewport})
    await delay(150)
  }
  throw new Error(`Native control cannot be revealed: ${name}`)
}
async function click(id, name, button = 1, inset = null) {
  if (profileNames.some(profile => name === `profile-${profile}`)) name = await profileDoor(id, name.slice(8))
  const {xid, element, key} = await reveal(id, name)
  const rect = element.visible, frame = geometry(xid)
  const x = frame.X + rect[0] + (inset ?? rect[2] / 2), y = frame.Y + rect[1] + rect[3] / 2
  assert.ok(rect[2] > 0 && rect[3] > 0 && x >= frame.X && x < frame.X + frame.WIDTH && y >= frame.Y && y < frame.Y + frame.HEIGHT, `${name} is outside its native viewport`)
  input('mousemove', x, y, 'sleep', '0.08')
  const runtime = await wait(`native Runtime pointer agrees with X11 for ${name}`, () => {
    const value = drawnFrame(id)
    return value?.pointer && Math.abs(value.pointer[0] - (x - frame.X)) <= 2 && Math.abs(value.pointer[1] - (y - frame.Y)) <= 2 && value
  }, 5000)
  const hit = runtime.elements.find(item => item.key === key && !item.exiting)
  assert.ok(presented(hit) && hit.visible.every((value, index) => Math.abs(value - rect[index]) <= 0.5), `Actual native target moved before click: ${name}`)
  pointerActions.push({phase, id, xid, name, resolved_key: key, button, rect, drawn_rect: element.rect, pointer: input('getmouselocation', '--shell'), runtime_pointer: runtime.pointer, hold_ms: 40})
  input('mousedown', button, 'sleep', '0.04', 'mouseup', button)
  await delay(120)
}
async function type(id, text, name = 'composer-input') {
  await click(id, name, 1, 12); await delay(100)
  const target = osWindow(id), focusedBefore = input('getwindowfocus').trim()
  assert.equal(focusedBefore, target, `Keyboard focus changed before typing ${name}`)
  input('key', '--clearmodifiers', 'ctrl+a'); await delay(50); input('type', '--clearmodifiers', '--delay', '2', '--', text); await delay(100)
  const focusedAfter = input('getwindowfocus').trim(); pointerActions.push({phase, id, name, typing: true, target, focusedBefore, focusedAfter})
  assert.equal(focusedAfter, target, `Keyboard focus changed during typing ${name}`)
  await wait(`typed ${name}`, () => {
    const s = current(id)
    if (name === 'composer-input') return s?.draft === text
    if (name === 'connection-url' && 'connection_url' in s) return s.connection_url === new URL(text).toString()
    if (name === 'connection-token' && 'connection_token_set' in s) return s.connection_token_set === true
    return true
  })
}
async function capture(name, id) {
  const window = focus(id), target = path.join(out, name + '.png')
  await delay(700)
  const g = geometry(window)
  execFileSync('/usr/bin/python3', ['-c', `import gi,os,sys;gi.require_version('Gdk','3.0');from gi.repository import Gdk;d=Gdk.Display.open(os.environ['DISPLAY']);r=d.get_default_screen().get_root_window();x,y,w,h=map(int,sys.argv[2:6]);p=Gdk.pixbuf_get_from_window(r,x,y,w,h);p.savev(sys.argv[1],'png',[],[])`, target, ...['X', 'Y', 'WIDTH', 'HEIGHT'].map(k => String(g[k]))], {env, stdio: 'pipe'})
  screenshots.push({file: path.basename(target), sha256: hash(fs.readFileSync(target)), window: id})
}
function clipboard() { return execFileSync('/usr/bin/python3', ['-c', `import gi;gi.require_version('Gtk','3.0');from gi.repository import Gtk,Gdk;Gtk.init([]);print(Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD).wait_for_text() or '',end='')`], {env, encoding: 'utf8', timeout: 5000}) }
async function copyDraft(id) { await click(id, 'composer-input', 1, 12); await delay(700); await click(id, 'composer-input', 1, 12); await delay(250); input('key', '--clearmodifiers', 'ctrl+a'); await delay(250); input('key', '--clearmodifiers', 'ctrl+c'); await delay(250); return clipboard() }
async function clipboardLoop(primary) {
  const count = Number(option('--clipboard-windows') ?? '2'), rounds = Number(option('--clipboard-rounds') ?? '3')
  const assertPointer = args.includes('--assert-pointer-sync')
  assert.ok([1, 2].includes(count)); assert.ok(Number.isInteger(rounds) && rounds > 0 && rounds <= 20)
  const windows = [{id: primary.id, expected: 'primary native send'}], probes = []
  await type(primary.id, windows[0].expected)
  if (count === 2) {
    key(primary.id, 'ctrl+shift+n')
    const peer = await wait('copy-loop actual native peer', () => snapshots().find(s => s.id !== primary.id && s.native?.position && !s.loading && !s.error && s.geometry['composer-input'] && sessionKey(s.id, 'research', 'A-research-notes')))
    await delay(700); await type(peer.id, 'peer native send'); windows.push({id: peer.id, expected: 'peer native send'})
  }
  for (let round = 0; round < rounds; round++) for (const window of windows) {
    phase = `copy-loop-${round}-${window.id}`
    const samples = [], sample = stage => {
      const framePath = path.join(inspection, 'frames', window.id + '.json')
      const frame = fs.existsSync(framePath) ? JSON.parse(fs.readFileSync(framePath, 'utf8')) : null
      samples.push({stage, at: Date.now(), snapshot: current(window.id), frame, os_pointer: assertPointer ? xPointer(osWindow(window.id)).local : null, os_focus: input('getwindowfocus').trim()})
    }
    sample('before')
    await click(window.id, 'composer-input', 1, 12)
    await delay(700); sample('first-click')
    await click(window.id, 'composer-input', 1, 12)
    await delay(250); sample('second-click')
    input('key', '--clearmodifiers', 'ctrl+a'); await delay(250); sample('select-all')
    input('key', '--clearmodifiers', 'ctrl+c'); await delay(250); sample('copy')
    const pointerSynced = !assertPointer || samples.filter(s => s.stage !== 'before').every(s => s.frame?.pointer?.length === 2 && s.frame.pointer.every((value, i) => Math.abs(value - s.os_pointer[i]) < 0.5))
    const selectionMatches = !assertPointer || samples.find(s => s.stage === 'select-all')?.snapshot.editor_state?.selected_bytes === Buffer.byteLength(window.expected)
    const copied = clipboard(); probes.push({round, id: window.id, expected: window.expected, copied, pointer_synced: assertPointer ? pointerSynced : null, selection_matches: assertPointer ? selectionMatches : null, pass: copied === window.expected && pointerSynced && selectionMatches, samples})
    assert.equal(current(window.id).draft, window.expected, 'Copy changed the real draft')
  }
  assert.ok(!calls.some(call => ['prompt.submit', 'session.create', 'session.interrupt'].includes(call.method)), 'Copy-only diagnosis mutated a gateway conversation')
  const result = {binary_sha256: hash(fs.readFileSync(binary)), native_windows: count, rounds, clicks: 2, pointer_asserted: assertPointer, passed: probes.every(probe => probe.pass), probes}
  fs.writeFileSync(path.join(out, 'clipboard-loop.json'), JSON.stringify(result, null, 2) + '\n')
  console.log(JSON.stringify({binary_sha256: result.binary_sha256, native_windows: count, rounds, passed: result.passed, copies: probes.map(({round, id, copied, pass}) => ({round, id, copied, pass}))}))
  assert.ok(result.passed, 'Full native Hermes composer Ctrl+A/C did not copy the exact draft')
}
async function stopOwned(process) {
  if (process.exitCode != null || process.signalCode != null) return
  process.kill('SIGTERM')
  const deadline = Date.now() + 2000
  while (process.exitCode == null && process.signalCode == null && Date.now() < deadline) await delay(20)
  if (process.exitCode == null && process.signalCode == null) process.kill('SIGKILL')
}
function windowRecords() { return JSON.parse(fs.readFileSync(path.join(state, 'windows.json'), 'utf8')) }
function draftStates() { const dir = path.join(state, 'drafts'); return fs.existsSync(dir) ? fs.readdirSync(dir).filter(f => f.endsWith('.json')).map(f => ({file: f, state: JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8'))})) : [] }
function drafts() { return draftStates().flatMap(file => file.state.entries ?? []) }
const a = await new FakeGateway('A').listen(), b = await new FakeGateway('B').listen()
async function privateDisplay() {
  xvfb = spawn('Xvfb', ['-displayfd', '3', '-screen', '0', '1920x1200x24', '-nolisten', 'tcp'], {stdio: ['ignore', 'ignore', 'pipe', 'pipe']}); owned.push(xvfb)
  let display = '', error = ''
  xvfb.stderr.on('data', bytes => error += bytes)
  await new Promise((resolve, reject) => { const timer = setTimeout(() => reject(new Error('Xvfb did not publish a private display: ' + error)), 10000); xvfb.stdio[3].on('data', bytes => { display += bytes; if (display.includes('\n')) { clearTimeout(timer); resolve() } }); xvfb.once('error', reject) })
  fs.mkdirSync(path.join(home, 'runtime'), {mode: 0o700})
  const alsa = path.join(home, 'alsa.conf'); fs.writeFileSync(alsa, 'pcm.!default { type null }\n')
  env = {...process.env, DISPLAY: ':' + display.trim(), HOME: home, USERPROFILE: home, XDG_STATE_HOME: path.join(home, 'state'), XDG_RUNTIME_DIR: path.join(home, 'runtime'), HERMES_HOME: path.join(home, 'unused-agent'), HERMES_GATEWAY_TOKEN: token, DBUS_SESSION_BUS_ADDRESS: `unix:path=${home}/no-session-bus`, PULSE_SERVER: `unix:${home}/no-pulse`, ALSA_CONFIG_PATH: alsa}
  bus = spawn('dbus-daemon', ['--session', '--nofork', '--print-address=1'], {env, stdio: ['ignore', 'pipe', 'pipe']}); owned.push(bus)
  env.DBUS_SESSION_BUS_ADDRESS = await new Promise((resolve, reject) => { let text = ''; const timer = setTimeout(() => reject(new Error('Private D-Bus did not start')), 5000); bus.stdout.on('data', bytes => { text += bytes; if (text.includes('\n')) { clearTimeout(timer); resolve(text.trim().split('\n')[0]) } }); bus.once('error', reject) })
  wm = spawn('xfwm4', ['--replace', '--compositor=off'], {env, stdio: ['ignore', 'ignore', 'pipe']}); owned.push(wm)
  let diagnostic = ''; wm.stderr.on('data', bytes => diagnostic += bytes)
  const deadline = Date.now() + 10000
  while (Date.now() < deadline) {
    assert.equal(wm.exitCode, null, 'The private window manager exited: ' + diagnostic)
    const property = execFileSync('xprop', ['-root', '_NET_SUPPORTING_WM_CHECK'], {env, encoding: 'utf8'})
    if (/window id # 0x[0-9a-f]+/i.test(property)) return
    await delay(100)
  }
  throw new Error('Private window manager did not become ready: ' + diagnostic)
}
try {
  await privateDisplay()
  fs.writeFileSync(path.join(state, 'sounds.json'), JSON.stringify({version: 1, preset: 0, muted: true}))
  fs.writeFileSync(path.join(state, 'notifications.json'), JSON.stringify({version: 1, enabled: false}))
  fs.writeFileSync(path.join(state, 'preferences.json'), JSON.stringify({theme: false, speak_replies: false, sidebar_open: true, inspector_open: false}))
  const traceArgs = args.includes('--native-trace') ? ['--native', '--trace', path.join(out, 'primary-native-trace.jsonl'), '--trace-all'] : []
  child = spawn(binary, ['--gateway', a.endpoint, '--profile', 'research', '--peer-window-proof-dir', inspection, ...traceArgs], {cwd: app, env, stdio: ['ignore', 'ignore', 'pipe']}); owned.push(child); child.stderr.on('data', bytes => stderr += bytes)
  const primary = await wait('primary connected native window', () => snapshots().find(s => s.kind === 'primary_launch' && s.native?.position && s.profile === 'research' && s.principal && !s.loading && !s.error && s.geometry['composer-input'] && sessionKey(s.id, 'research', 'A-research-notes')))
  await delay(700)
  if (args.includes('--clipboard-only')) {
    await clipboardLoop(primary)
  } else {
  assert.equal(visible().length, 1); await capture('primary-light', primary.id)
  phase = 'independent-drafts'
  await type(primary.id, 'primary native send')
  key(primary.id, 'ctrl+shift+n')
  const peer = await wait('new complete peer', () => snapshots().find(s => s.id !== primary.id && s.native?.position && !s.loading && !s.error && s.geometry['composer-input'] && sessionKey(s.id, 'research', 'A-research-notes')))
  await delay(700)
  assert.equal(visible().length, 2); assert.equal(peer.profile, 'research'); assert.equal(peer.principal, primary.principal); assert.notEqual(peer.editor, primary.editor); assert.notEqual(peer.source_epoch, primary.source_epoch)
  await type(peer.id, 'peer native send')
  await wait('both independent persisted inputs', () => drafts().some(e => e.editor === primary.editor && e.draft?.text === 'primary native send') && drafts().some(e => e.editor === peer.editor && e.draft?.text === 'peer native send'))
  assert.equal(new Set(Object.values(windowRecords().records).map(r => r.editor)).size, 2)
  for (const s of [primary, peer]) assert.ok(draftStates().some(file => file.state.owner_slots?.some(slot => slot.window === windowRecords().records[s.id].editor && slot.editor === s.editor && slot.profile === 'research' && slot.session == null)), `Missing persisted window/editor ownership for ${s.id}`)
  fs.writeFileSync(path.join(out, 'independent-drafts.json'), JSON.stringify({windows: windowRecords(), drafts: draftStates()}, null, 2) + '\n')
  if (args.includes('--clipboard-diagnostic')) {
    const primaryCopied = await copyDraft(primary.id), peerCopied = await copyDraft(peer.id)
    clipboardInputs = primaryCopied === 'primary native send' && peerCopied === 'peer native send'
    fs.writeFileSync(path.join(out, 'clipboard-diagnostic.json'), JSON.stringify({copied_both_native_inputs: clipboardInputs, primaryCopied, peerCopied}) + '\n')
    assert.equal(primaryCopied, 'primary native send', 'Primary native Ctrl+A/C did not copy its exact input')
    assert.equal(peerCopied, 'peer native send', 'Peer native Ctrl+A/C did not copy its exact input')
  }
  assert.equal(current(primary.id).draft, 'primary native send'); assert.equal(current(peer.id).draft, 'peer native send')
  await capture('peer-independent-draft-light', peer.id)
  phase = 'ordinary-settings'
  await click(peer.id, 'settings'); await wait('peer settings', () => current(peer.id)?.route === 'Settings'); await delay(3000); await capture('peer-settings-before-expose', peer.id)
  const settingsWindow = osWindow(peer.id), settingsGeometry = geometry(settingsWindow)
  input('windowsize', settingsWindow, settingsGeometry.WIDTH + 1, settingsGeometry.HEIGHT + 1)
  await delay(700); assert.equal(current(peer.id)?.route, 'Settings'); await capture('peer-settings-light', peer.id)
  assert.equal(current(primary.id).route, 'Chat'); assert.equal(current(primary.id).draft, 'primary native send')
  await click(peer.id, 'page-back'); await wait('peer back to chat', () => current(peer.id)?.route === 'Chat' && current(peer.id).geometry['composer-input'])
  phase = 'concurrent-streams'
  await click(peer.id, 'composer-send')
  await wait('peer streamed text', () => current(peer.id)?.running && JSON.stringify(current(peer.id)).includes('STREAM peer native send'))
  await click(primary.id, 'composer-send')
  await wait('primary streamed text', () => current(primary.id)?.running && JSON.stringify(current(primary.id)).includes('STREAM primary native send'))
  assert.ok(!JSON.stringify(current(primary.id)).includes('STREAM peer native send')); assert.ok(!JSON.stringify(current(peer.id)).includes('STREAM primary native send'))
  const peerAdmission = a.admissions.find(x => x.text === 'peer native send'), primaryAdmission = a.admissions.find(x => x.text === 'primary native send')
  assert.ok(peerAdmission && primaryAdmission); assert.notEqual(peerAdmission.runtime, primaryAdmission.runtime)
  await capture('peer-streaming-light', peer.id); await capture('primary-streaming-light', primary.id)
  await click(peer.id, 'composer-stop'); await wait('owned peer stop', () => current(peer.id)?.running === false)
  assert.equal(current(primary.id).running, true)
  const interrupts = calls.filter(c => c.method === 'session.interrupt'); assert.equal(interrupts.length, 1); assert.equal(interrupts[0].params.session_id, peerAdmission.runtime)
  await click(primary.id, 'composer-stop'); await wait('primary stop', () => current(primary.id)?.running === false)
  phase = 'source-capture'
  await click(primary.id, 'change-connection'); await wait('connection form', () => current(primary.id)?.geometry['connection-url']); await delay(700)
  await type(primary.id, b.endpoint, 'connection-url'); await type(primary.id, token, 'connection-token'); await capture('source-connection-form', primary.id); await click(primary.id, 'connect-url')
  await wait('primary new source', () => current(primary.id)?.principal && current(primary.id).principal !== peer.principal && !current(primary.id).loading && !current(primary.id).error && current(primary.id).geometry['composer-input'])
  assert.equal(current(peer.id).principal, peer.principal)
  await click(primary.id, 'profile-picker-toggle'); await wait('profile picker', () => current(primary.id)?.geometry['pick-profile-archive']); await delay(350); await capture('primary-profile-picker', primary.id); await click(primary.id, 'pick-profile-archive')
  await wait('archive profile', () => current(primary.id)?.profile === 'archive' && !current(primary.id).loading)
  key(primary.id, 'ctrl+shift+n')
  const peerB = await wait('captured source and profile peer', () => snapshots().find(s => ![primary.id, peer.id].includes(s.id) && s.native?.position && s.profile === 'archive' && s.principal === current(primary.id).principal && !s.loading && s.geometry['composer-input']))
  assert.equal(visible().length, 3); assert.equal(current(peer.id).profile, 'research'); assert.equal(current(peer.id).principal, peer.principal)
  assert.equal(current(peerB.id).dark, false)
  await click(peerB.id, 'theme-toggle'); await wait('actual dark theme', () => current(peerB.id)?.dark === true); await capture('captured-source-profile-peer-dark', peerB.id)
  phase = 'existing-session-window'
  const beforeSession = new Set(snapshots().map(s => s.id))
  await click(peer.id, 'session-research-A-research-notes', 3)
  await wait('session context', () => current(peer.id)?.geometry['sidebar-session-actions-item6']); await delay(350)
  await click(peer.id, 'sidebar-session-actions-item6')
  const sessionWindow = await wait('existing-session native window', () => snapshots().find(s => !beforeSession.has(s.id) && s.kind === 'interactive_session' && s.profile === 'research' && s.session === 'A-research-notes' && !s.loading && !s.error))
  assert.equal(sessionWindow.principal, peer.principal); assert.equal(visible().length, 4)
  assert.ok(JSON.stringify(sessionWindow.messages).includes('A / research source history')); await capture('existing-session-window', sessionWindow.id)
  await click(peer.id, 'session-research-A-research-notes', 3); await wait('repeat context', () => current(peer.id)?.geometry['sidebar-session-actions-item6']); await delay(350); await click(peer.id, 'sidebar-session-actions-item6')
  await delay(250); assert.equal(visible().length, 4, 'Reopening the same owned session duplicated its window')
  assert.equal(input('getwindowfocus').trim(), osWindow(sessionWindow.id), 'Reopening did not focus the existing session window')
  key(sessionWindow.id, 'ctrl+shift+w'); await wait('session window local close', () => windowRecords().records[sessionWindow.id]?.closed && visible().length === 3)
  phase = 'profile-pinned-window'
  const beforeBot = new Set(snapshots().map(s => s.id))
  await click(peerB.id, 'bots-tab'); await wait('bot roster', () => current(peerB.id)?.geometry['bot-research']); await delay(700); await click(peerB.id, 'bot-research', 3)
  await wait('bot context', () => current(peerB.id)?.geometry['conversation-actions-item2']); await delay(350); await click(peerB.id, 'conversation-actions-item2')
  const profileWindow = await wait('profile-pinned Bot Chat window', () => snapshots().find(s => !beforeBot.has(s.id) && s.kind === 'profile_pinned' && s.profile === 'research' && s.session === 'B-research-bot' && !s.loading && !s.error))
  assert.equal(profileWindow.principal, peerB.principal); assert.equal(visible().length, 4)
  const pinned = windowRecords().records[profileWindow.id].pinned_route
  assert.equal(pinned.profile, 'research'); assert.equal(pinned.session, null); assert.equal(pinned.canonical_bot, false)
  assert.ok(JSON.stringify(profileWindow.messages).includes('B / research source history'))
  fs.writeFileSync(path.join(out, 'profile-window-presentation.json'), JSON.stringify({opened_from_dark: current(peerB.id).dark, actual_dark: profileWindow.dark, initial_record: windowRecords().records[profileWindow.id]}, null, 2) + '\n')
  await capture('profile-pinned-bot-window', profileWindow.id)
  key(profileWindow.id, 'ctrl+n'); await wait('profile-pinned fresh chat', () => current(profileWindow.id)?.session == null && current(profileWindow.id)?.profile === 'research' && current(profileWindow.id)?.principal === profileWindow.principal && !current(profileWindow.id).loading)
  assert.deepEqual(windowRecords().records[profileWindow.id].pinned_route, pinned)
  key(profileWindow.id, 'ctrl+shift+w'); await wait('profile window local close', () => windowRecords().records[profileWindow.id]?.closed && visible().length === 3)
  phase = 'independent-native-lifetime'
  key(primary.id, 'ctrl+shift+w'); await wait('primary native close', () => windowRecords().records[primary.id]?.closed && visible().length === 2)
  assert.equal(child.exitCode, null)
  await type(peer.id, 'surviving peer send'); await click(peer.id, 'composer-send')
  await wait('peer works after primary close', () => current(peer.id)?.running && JSON.stringify(current(peer.id)).includes('STREAM surviving peer send'))
  assert.equal(a.admissions.filter(x => x.text === 'surviving peer send').length, 1); assert.equal(b.admissions.length, 0)
  await click(peer.id, 'composer-stop'); await wait('surviving stop', () => current(peer.id)?.running === false); await capture('peer-after-primary-close', peer.id)
  key(peerB.id, 'ctrl+shift+w'); await wait('local close retains last peer', () => windowRecords().records[peerB.id]?.closed && visible().length === 1); assert.equal(child.exitCode, null)
  key(peer.id, 'ctrl+q')
  // The normal app command is used; no process kill is acceptance evidence.
  const quitDeadline = Date.now() + 10000
  while (child.exitCode == null && Date.now() < quitDeadline) await delay(40)
  assert.equal(child.exitCode, 0, 'Global Quit did not close the application cleanly')
  assert.equal(visible().length, 0)
  assert.ok(!JSON.stringify(windowRecords()).includes(token), 'Credentials leaked into window persistence')
  const result = {binary_sha256: hash(fs.readFileSync(binary)), harness_sha256: harnessSha, actual_native_windows: true, same_process: true, complete_viewers: true, independent_editors: true, clipboard_inputs: clipboardInputs, persisted_inputs: true, settings: true, concurrent_streams: true, scoped_stop: true, captured_source_and_profile: true, existing_session_window: true, existing_session_window_reused: true, profile_pinned_bot_window: true, profile_pinned_new_session: true, light_and_dark: true, primary_close_keeps_peers: true, surviving_peer_interactive: true, local_close: true, global_quit: true, physical_audio_used: false, live_agents_used: false, rpc_methods: [...new Set(calls.map(c => c.method))], nativeBindings, screenshots}
  fs.writeFileSync(path.join(out, 'result.json'), JSON.stringify(result, null, 2) + '\n'); console.log(JSON.stringify(result))
  }
} catch (error) {
  fs.writeFileSync(path.join(out, 'failure.json'), JSON.stringify({phase, error: String(error?.stack ?? error), snapshots: snapshots(), nativeBindings, screenshots}, null, 2) + '\n')
  for (const snapshot of snapshots()) {
    try { await capture(`failure-${snapshot.id}`, snapshot.id) } catch {}
  }
  throw error
} finally {
  fs.writeFileSync(path.join(out, 'app.log'), stderr); fs.writeFileSync(path.join(out, 'requests.json'), JSON.stringify({rpc: calls, http: reads}, null, 2) + '\n'); fs.writeFileSync(path.join(out, 'pointer-actions.json'), JSON.stringify(pointerActions, null, 2) + '\n')
  for (const name of ['windows.json']) if (fs.existsSync(path.join(state, name))) fs.copyFileSync(path.join(state, name), path.join(out, name))
  for (const process of [...owned].reverse()) await stopOwned(process)
  await a.close(); await b.close()
  fs.rmSync(home, {recursive: true, force: true})
}
