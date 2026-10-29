import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const app = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const binary = process.argv[2] || path.join(app, 'target/debug', process.platform === 'win32' ? 'hermes-rune.exe' : 'hermes-rune')
const out = mkdtempSync(path.join(os.tmpdir(), 'hermes-rune-scenes-'))
const env = { ...process.env, HOME: out, USERPROFILE: out, HERMES_HOME: path.join(out, 'agent'), HERMES_GATEWAY_TOKEN: '' }
let passed = false
function render(name, args) {
  const tree = path.join(out, `${name}.json`)
  const result = spawnSync(binary, [...args, '--tree', tree, '--snap', path.join(out, `${name}.png`)], {
    cwd: app, env, encoding: 'utf8', timeout: 30000, maxBuffer: 1024 * 1024,
  })
  assert.equal(result.status, 0, `${name}: ${result.error || result.stderr}`)
  return JSON.parse(readFileSync(tree, 'utf8'))
}
const hasText = (tree, text) => tree.elements.some(element => element.text === text)
try {
  const dark = render('dark', ['--scene', 'layout', '--theme', 'dark', '--size', '1440x900'])
  const light = render('light-small', ['--scene', 'layout', '--theme', 'light', '--size', '1024x768'])
  for (const tree of [dark, light]) {
    const get = key => tree.elements.find(element => element.key === key)
    assert.ok(tree.elements.some(element => element.role === 'log' && element.label === 'Conversation history'))
    assert.ok(get('workspace').rect[2] > 200, 'the conversation retains readable width')
    assert.ok(get('review').rect[0] >= get('workspace').rect[0] + get('workspace').rect[2] - 1)
    assert.ok(get('terminal').rect[1] > get('review').rect[1])
    assert.ok(hasText(tree, 'Read only'))
    assert.ok(get('sessions-tab'), 'Hermes has a Sessions tab')
    assert.ok(get('bots-tab'), 'Hermes has a distinct Bots tab')
    assert.ok(get('nav-capabilities'), 'Hermes navigation is retained above session search')
    assert.ok(get('nav-capabilities').rect[1] < get('session-search').rect[1])
    assert.ok(get('profile-rail').rect[1] > get('session-search').rect[1] + 200, 'profile scope stays at sidebar foot')
    assert.ok(get('workspace-tabs'), 'open conversations have their own tab strip')
  }
  assert.notEqual(dark.elements[0].fill, light.elements[0].fill)
  const search = render('search', ['--scene', 'reader', '--theme', 'light', '--script', 'fixtures/search.json', '--at', '1'])
  assert.ok(hasText(search, 'Organize my notes'))
  assert.ok(!hasText(search, 'Ideas for the next trip'))
  assert.ok(hasText(search, '1 of 3'))
  const secure = render('secure', ['--theme', 'dark', '--script', 'fixtures/secure-token.json', '--at', '1'])
  assert.ok(!JSON.stringify(secure).includes('synthetic-secret-never-real'))
  assert.ok(secure.elements.some(element => element.secure && element.role === 'textbox'))
  const cancelled = render('cancelled', ['--scene', 'reader', '--script', 'fixtures/cancel-connection.json', '--at', '1'])
  assert.ok(cancelled.elements.some(element => element.role === 'log'))
  assert.ok(!hasText(cancelled, 'Gateway URL'))
  const error = render('error', ['--scene', 'error', '--theme', 'dark'])
  assert.ok(error.elements.some(element => element.role === 'alert'))
  assert.ok(hasText(error, 'Conversation unavailable'))
  const bots = render('bots-tabs', ['--scene', 'reader', '--theme', 'dark', '--script', 'fixtures/bot-tabs.json', '--at', '1'])
  assert.ok(bots.elements.some(e => e.key === 'bots-roster'))
  assert.equal(bots.elements.filter(e => /^conversation-tab-\d+$/.test(e.key || '')).length, 2, 'only explicit new-tab creates an extra tab')
  assert.ok(hasText(bots, 'Привет, Гермес. Хочу возвращаться к нашему разговору, а не искать его среди десятков сессий.'))
  assert.ok(!hasText(bots, 'Покажи, к чему мы пришли в исследовании.'))
  const missingBot = render('bot-empty', ['--scene', 'reader', '--script', 'fixtures/bot-empty.json', '--at', '1'])
  assert.ok(hasText(missingBot, 'No Bot Chat yet'))
  assert.ok(!missingBot.elements.some(e => e.role === 'log'), 'a missing registry chat cannot show an unrelated recent session')
  const simple = render('simple', ['--scene', 'reader', '--script', 'fixtures/simple-mode.json', '--at', '1'])
  assert.ok(!simple.elements.some(e => e.key === 'tools-zone'))
  assert.ok(!simple.elements.some(e => e.key === 'status-bar'))
  assert.ok(simple.elements.some(e => e.key === 'change-connection'))
  const simpleError = render('simple-error', ['--scene', 'error', '--script', 'fixtures/simple-mode.json', '--at', '1'])
  assert.ok(simpleError.elements.some(e => e.key === 'retry-connection'))
  console.log('Passed 10 native scene checks: Hermes structure, appearances, search, secure input, Escape, errors, Bot Chat tabs, missing Bot Chat, Simple mode and recovery')
  passed = true
} finally {
  if (passed) rmSync(out, { recursive: true, force: true })
  else console.error(`Synthetic scene evidence: ${out}`)
}
