#!/usr/bin/env node
/** Production Viewer navigation against an isolated, read-only gateway. Never presses Play. */
import assert from 'node:assert/strict'
import {createHash} from 'node:crypto'
import {createServer} from 'node:http'
import {spawn} from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import {fileURLToPath} from 'node:url'
import {WebSocketServer} from 'ws'

const app = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const args = process.argv.slice(2)
const option = name => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : null }
const binary = path.resolve(option('--binary') ?? path.join(app, 'target/debug/hermes-rune'))
const out = path.resolve(option('--out') ?? fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-audio-navigation-')))
fs.mkdirSync(out, {recursive: true})
const token = 'private-audio-navigation-fixture'
const reads = [], calls = [], sockets = new Set()
let failure, activeCase = '', count = 0
const sha = value => createHash('sha256').update(value).digest('hex')
const tap = (key, t) => ({type: 'tap', key, t, hold: .02})
const get = (tree, key) => tree.elements.find(e => e.key === key && !e.exiting)
const has = (tree, value) => tree.elements.some(e => e.text === value && !e.exiting)
const session = (profile, id, title) => ({id, profile, title, cwd: '/fixture', message_count: 2, last_active: 1793300000})
const researchSession = session('research', 'audio-session', 'Field notes')
const archiveSession = session('archive', 'archive-audio', 'Archived recording')
const history = owner => owner === 'archive' ? [
  {row_id: 21, role: 'assistant', content: 'MEDIA: /fixture/archive.wav', timestamp: 1793300000},
] : [
  {row_id: 11, role: 'user', content: [{type: 'file', path: '/fixture/voice.wav', name: 'Field notes.wav'}]},
  {row_id: 12, role: 'assistant', content: 'The original recording is attached above.'},
]
function tone(seconds) {
  const wav = Buffer.alloc(44 + seconds * 48000 * 2)
  wav.write('RIFF'); wav.writeUInt32LE(wav.length - 8, 4); wav.write('WAVEfmt ', 8)
  wav.writeUInt32LE(16, 16); wav.writeUInt16LE(1, 20); wav.writeUInt16LE(1, 22)
  wav.writeUInt32LE(48000, 24); wav.writeUInt32LE(96000, 28); wav.writeUInt16LE(2, 32)
  wav.writeUInt16LE(16, 34); wav.write('data', 36); wav.writeUInt32LE(wav.length - 44, 40)
  for (let i = 0; i < seconds * 48000; i++) wav.writeInt16LE(Math.round(Math.sin(i / 48000 * Math.PI * 440) * 4000), 44 + i * 2)
  return wav
}
const files = new Map([['/fixture/voice.wav', tone(8)], ['/fixture/second.wav', tone(12)], ['/fixture/archive.wav', tone(16)]])
const catalog = {provider: 'fixture', model: 'fixture-model', providers: [{slug: 'fixture', name: 'Fixture provider', models: ['fixture-model'], authenticated: true}]}
const profiles = {profiles: [{name: 'research', label: 'Research'}, {name: 'archive', label: 'Archive'}], active: 'research'}
function json(response, value) { response.writeHead(200, {'content-type': 'application/json'}); response.end(JSON.stringify(value)) }
const http = createServer((request, response) => {
  try {
    const url = new URL(request.url, 'http://fixture')
    assert.ok(['GET', 'HEAD'].includes(request.method), 'The native flow attempted an HTTP mutation')
    assert.equal(url.searchParams.get('token'), token)
    const profile = url.searchParams.get('profile')
    reads.push({case: activeCase, method: request.method, route: url.pathname, query: Object.fromEntries(url.searchParams), range: request.headers.range ?? null})
    if (['/api/fs/download', '/api/files/stream'].includes(url.pathname)) {
      const source = url.searchParams.get('path'), wav = files.get(source)
      assert.ok(wav, 'The audio player requested an unknown file')
      assert.equal(profile, source === '/fixture/archive.wav' ? 'archive' : 'research')
      assert.equal(url.searchParams.get('session_id'), source === '/fixture/archive.wav' ? 'archive-audio' : 'audio-session')
      const etag = '"' + source.split('/').pop() + '-v1"'
      let start = 0, end = wav.length - 1, status = 200
      if (request.headers.range) {
        const match = /^bytes=(\d+)-(\d+)$/.exec(request.headers.range)
        assert.ok(match); start = Number(match[1]); end = Number(match[2])
        assert.ok(start <= end && end < wav.length); assert.equal(request.headers['if-range'], etag); status = 206
      }
      response.writeHead(status, {'content-type': 'audio/wav', 'content-length': end - start + 1, etag, 'accept-ranges': 'bytes', ...(status === 206 ? {'content-range': `bytes ${start}-${end}/${wav.length}`} : {})})
      response.end(request.method === 'HEAD' ? undefined : wav.subarray(start, end + 1)); return
    }
    assert.equal(request.method, 'GET')
    const route = url.pathname
    if (route === '/api/profiles/sessions/sidebar') return json(response, {recents: {sessions: [researchSession]}, messaging: {sessions: []}, cron: {sessions: []}})
    if (route === '/api/profiles/sessions') { assert.equal(profile, 'all'); return json(response, {sessions: [archiveSession]}) }
    if (route === '/api/cron/jobs') return json(response, {jobs: []})
    if (route === '/api/sessions/audio-session/messages') { assert.equal(profile, 'research'); return json(response, {profile, session_id: 'audio-session', messages: history(profile)}) }
    if (route === '/api/sessions/archive-audio/messages') { assert.equal(profile, 'archive'); return json(response, {profile, session_id: 'archive-audio', messages: history(profile)}) }
    if (route === '/api/fs/default-cwd') return json(response, {cwd: '/fixture'})
    if (route === '/api/fs/list') {
      assert.equal(profile, 'research'); assert.equal(url.searchParams.get('path'), '/fixture')
      return json(response, {entries: ['voice.wav', 'second.wav'].map(name => ({name, path: '/fixture/' + name, isDirectory: false}))})
    }
    if (route === '/api/fs/read-text') { assert.ok(url.searchParams.get('path').endsWith('.gitignore')); return json(response, {content: '', exists: false}) }
    if (route === '/api/config') return json(response, {config: {model: {provider: 'fixture', default: 'fixture-model'}, approvals: {mode: 'smart'}}})
    if (route === '/api/status') return json(response, {running: true, ready: true})
    if (route === '/api/model/options') return json(response, catalog)
    if (route === '/api/audio/voice-live/status') return json(response, {ok: false})
    if (route === '/api/git/review/rev-parse') return json(response, {sha: null})
    if (route === '/api/git/review/list') return json(response, {files: [], is_repo: false})
    if (route === '/api/git/review/ship-info') return json(response, {ghReady: false})
    if (route === '/api/git/status') return json(response, {is_repo: false})
    if (['/api/git/branches', '/api/git/base-branches'].includes(route)) return json(response, {branches: []})
    if (route === '/api/git/worktrees') return json(response, {worktrees: []})
    throw new Error('Unexpected HTTP read: ' + url.pathname)
  } catch (error) { failure = error; if (!response.headersSent) response.writeHead(400); response.end('{}') }
})
const ws = new WebSocketServer({server: http, path: '/api/ws'})
ws.on('connection', (socket, request) => {
  try { assert.equal(new URL(request.url, 'http://fixture').searchParams.get('token'), token) } catch (error) { failure = error; socket.close(); return }
  sockets.add(socket); socket.on('close', () => sockets.delete(socket))
  socket.on('message', bytes => {
    try {
      const frame = JSON.parse(bytes); calls.push({case: activeCase, ...frame})
      assert.ok(['profiles.list', 'session.list', 'model.options', 'config.get', 'setup.runtime_check', 'profiles.get_asset', 'session.resume', 'process.list', 'subagent.list'].includes(frame.method), 'The flow attempted an unexpected RPC: ' + frame.method)
      let result
      switch (frame.method) {
        case 'profiles.list': result = profiles; break
        case 'session.list': result = {sessions: frame.params.scope === 'live' ? [] : [researchSession]}; break
        case 'model.options': result = catalog; break
        case 'config.get': result = {value: 'smart'}; break
        case 'setup.runtime_check': result = {ok: true, ready: true}; break
        case 'profiles.get_asset': result = {data_url: null}; break
        case 'process.list': result = {processes: []}; break
        case 'subagent.list': result = {subagents: []}; break
        case 'session.resume':
          assert.equal(frame.params.profile, 'research'); assert.equal(frame.params.session_id, 'audio-session')
          result = {session_id: 'live-audio', stored_session_id: 'audio-session', running: false, messages: [], prompts: [], pending: [], info: {profile_name: 'research'}}; break
      }
      socket.send(JSON.stringify({jsonrpc: '2.0', id: frame.id, result}))
    } catch (error) { failure = error; socket.close() }
  })
})
await new Promise(resolve => http.listen(0, '127.0.0.1', resolve))
const endpoint = `http://127.0.0.1:${http.address().port}`
const group = (id, panes) => ({type: 'group', id, panes, active: panes[0]})
async function render(name, events, {theme = 'light', size = '1440x900', filesVisible = false, at = 4.4} = {}) {
  activeCase = name
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-audio-navigation-home-'))
  const state = path.join(home, 'state', 'hermes-desktop-rune'); fs.mkdirSync(state, {recursive: true})
  fs.writeFileSync(path.join(state, 'preferences.json'), JSON.stringify({theme: theme === 'dark', inspector_open: filesVisible, inspector_width: 300,
    layout: {type: 'split', id: 'fixture-columns', orientation: 'row', weights: [237, 903, 300], children: [group('fixture-sidebar', ['sessions']), group('fixture-workspace', ['workspace']), group('fixture-files', ['files'])]},
    decks: {active: 'custom', hidden: filesVisible ? ['review', 'terminal'] : ['files', 'review', 'terminal']}}))
  const script = path.join(out, name + '-script.json'), tree = path.join(out, name + '.json')
  // The renderer advances a virtual clock; leave enough frames for real HTTP workers.
  fs.writeFileSync(script, JSON.stringify({events: events.map(event => ({...event, t: event.t * 2}))}))
  const child = spawn(binary, ['--gateway', endpoint, '--profile', 'research', '--theme', theme, '--size', size, '--scale', '1', '--script', script, '--at', String(at * 2), '--tree', tree, '--snap', path.join(out, name + '.png')], {
    cwd: app, env: {...process.env, HOME: home, USERPROFILE: home, XDG_STATE_HOME: path.join(home, 'state'), HERMES_HOME: path.join(home, 'unused-agent'), HERMES_GATEWAY_TOKEN: token}, stdio: ['ignore', 'ignore', 'pipe'],
  })
  let stderr = '', timedOut = false
  child.stderr.on('data', bytes => { stderr += bytes })
  const timer = setTimeout(() => { timedOut = true; child.kill('SIGTERM') }, 45000)
  try {
    await new Promise((resolve, reject) => { child.once('error', reject); child.once('close', resolve) })
    fs.writeFileSync(path.join(out, name + '.log'), stderr)
    if (failure) throw failure
    assert.equal(timedOut, false, name + ' timed out')
    assert.equal(child.exitCode, 0, name + ': ' + stderr.slice(-2500))
    count++
    return JSON.parse(fs.readFileSync(tree, 'utf8'))
  } finally { clearTimeout(timer); if (child.exitCode == null) child.kill('SIGTERM'); fs.rmSync(home, {recursive: true, force: true}) }
}
const boot = [tap('reconnect', .2), tap('session-research-audio-session', 1.2)]
const fileOpen = [...boot, tap('file-row-%2Ffixture%2Fvoice.wav', 2.4)]
const transcriptOpen = [...boot, tap('row-11-part-0', 2.4)]
function paused(tree, label, duration, owner = 'research · audio-session') {
  assert.ok(get(tree, 'native-audio-preview'), 'The actual workspace did not mount the production audio player')
  assert.equal(get(tree, 'audio-state')?.text, 'Paused')
  assert.equal(get(tree, 'audio-duration')?.text, duration)
  assert.ok(has(tree, label)); assert.ok(has(tree, owner))
  assert.ok(!tree.elements.some(e => e.role === 'alert' && !e.exiting), 'An audio/navigation error reached the native view')
  assert.ok(get(tree, 'audio-play-pause') && get(tree, 'audio-close'))
}
try {
  for (const theme of ['light', 'dark']) {
    const tree = await render('files-audio-' + theme, fileOpen, {theme, filesVisible: true})
    paused(tree, 'voice.wav', '0:08')
  }
  const short = await render('files-audio-short-dark', fileOpen, {theme: 'dark', size: '1024x520', filesVisible: true})
  paused(short, 'voice.wav', '0:08')
  const [x, y, w, h] = get(short, 'native-audio-preview').rect
  assert.ok(x >= 0 && y >= 0 && x + w <= 1025 && y + h <= 521, 'Audio viewport escaped the short Viewer')
  const closed = await render('files-close-chat', [...fileOpen, tap('audio-close', 3.5)], {filesVisible: true})
  assert.ok(!get(closed, 'native-audio-preview') && get(closed, 'composer-input') && get(closed, 'files'), 'Close did not return to the original chat and Files pane')
  const replaced = await render('files-replace-audio', [...fileOpen, tap('file-row-%2Ffixture%2Fsecond.wav', 3.5)], {filesVisible: true, at: 5})
  paused(replaced, 'second.wav', '0:12')
  assert.ok(!replaced.elements.some(e => e.text === 'voice.wav' && e.path.startsWith(get(replaced, 'native-audio-preview').path)), 'The replaced source remained in the audio player')
  const transcript = await render('transcript-audio-dark', transcriptOpen, {theme: 'dark'})
  paused(transcript, 'voice.wav', '0:08')
  assert.ok(get(transcript, 'files'), 'Opening a transcript file did not reveal its source file pane')
  const transcriptClosed = await render('transcript-close-chat', [...transcriptOpen, tap('audio-close', 3.5)])
  assert.ok(!get(transcriptClosed, 'native-audio-preview') && get(transcriptClosed, 'row-11-part-0'), 'Closing the transcript preview lost the source message')
  const artifactStart = [...boot, tap('nav-artifacts', 2.4)]
  const gallery = await render('artifact-audio-gallery', artifactStart)
  const key = 'artifact-' + sha('archive\0archive-audio\0/fixture/archive.wav')
  const record = get(gallery, key); assert.ok(record, 'The real cross-profile index did not produce the audio file')
  const label = gallery.elements.find(e => e.text === 'archive.wav' && e.path.startsWith(record.path))
  assert.ok(label)
  const select = {type: 'tap', x: label.rect[0] + label.rect[2] / 2, y: label.rect[1] + label.rect[3] / 2, t: 3.5, hold: .02}
  const artifactOpen = [...artifactStart, select, tap('artifact-native-preview', 4.2)]
  const artifact = await render('artifact-audio-owned-dark', artifactOpen, {theme: 'dark', at: 5.3})
  paused(artifact, 'archive.wav', '0:16', 'archive · archive-audio')
  const artifactClosed = await render('artifact-close-selection', [...artifactOpen, tap('audio-close', 5.2)], {at: 6})
  assert.ok(!get(artifactClosed, 'native-audio-preview') && get(artifactClosed, 'artifacts-page') && get(artifactClosed, 'artifact-native-preview'), 'Closing audio did not restore the original Artifacts selection')
  const retire = [...fileOpen, tap('change-connection', 3.4), tap('connection-token', 3.8), {paste: token, t: 3.9}, tap('connect-url', 4.1)]
  const retired = await render('source-retirement-dark', retire, {theme: 'dark', filesVisible: true, at: 5.4})
  assert.ok(!get(retired, 'native-audio-preview'), 'Changing the source incarnation retained the old player')
  assert.ok(!get(retired, 'connection-form'), 'The fresh scoped connection did not settle')
  assert.ok(reads.some(r => r.query.path === '/fixture/archive.wav' && r.query.profile === 'archive' && r.query.session_id === 'archive-audio'))
  const result = {binary_sha256: sha(fs.readFileSync(binary)), native_rune_viewer: true, cases: count, actual_navigation: ['Files', 'transcript', 'Artifacts'], cross_profile_artifact_owner: true, return_selection: true, replacement: true, source_retirement: true, themes: true, short_window: true, play_clicked: false, physical_audio_used: false, rpc_methods: [...new Set(calls.map(c => c.method))], screenshots: out}
  fs.writeFileSync(path.join(out, 'result.json'), JSON.stringify(result, null, 2) + '\n')
  console.log(JSON.stringify(result))
} finally {
  fs.writeFileSync(path.join(out, 'requests.json'), JSON.stringify({http: reads, rpc: calls}, null, 2) + '\n')
  for (const socket of sockets) socket.terminate()
  await new Promise(resolve => http.close(resolve)); await new Promise(resolve => ws.close(resolve))
}
