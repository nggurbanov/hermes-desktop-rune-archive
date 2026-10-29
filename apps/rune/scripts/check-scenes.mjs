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
  const dark = render('dark', ['--scene', 'reader', '--theme', 'dark', '--size', '1440x900'])
  const light = render('light-small', ['--scene', 'reader', '--theme', 'light', '--size', '1024x768'])
  for (const tree of [dark, light]) {
    const get = key => tree.elements.find(element => element.key === key)
    assert.ok(tree.elements.some(element => element.role === 'log' && element.label === 'Conversation history'))
    assert.ok(get('workspace').rect[2] > 200, 'the conversation retains readable width')
    assert.ok(get('review').rect[0] >= get('workspace').rect[0] + get('workspace').rect[2] - 1)
    assert.ok(get('terminal').rect[1] > get('review').rect[1])
    assert.ok(hasText(tree, 'Read only'))
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
  console.log('Passed 6 native scene checks: layout, appearances, search, secure input, Escape and error state')
  passed = true
} finally {
  if (passed) rmSync(out, { recursive: true, force: true })
  else console.error(`Synthetic scene evidence: ${out}`)
}
