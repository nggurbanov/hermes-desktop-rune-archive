import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const app = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const binary = process.argv[2] || path.join(app, 'target/debug', process.platform === 'win32' ? 'hermes-rune.exe' : 'hermes-rune')
const out = mkdtempSync(path.join(os.tmpdir(), 'hermes-composer-'))
const env = { ...process.env, HOME: out, USERPROFILE: out, HERMES_HOME: path.join(out, 'agent'), HERMES_GATEWAY_TOKEN: '' }
let passed = false
const tap = (key, t) => ({ type: 'tap', key, t })
const key = (name, t, mods) => ({ type: 'key', name, t, ...mods })
function render(name, { scene = 'composer', events = [], theme = 'light', size = '1440x900' } = {}) {
  const script = path.join(out, `${name}-script.json`)
  const tree = path.join(out, `${name}.json`)
  writeFileSync(script, JSON.stringify({ events }))
  const result = spawnSync(binary, ['--scene', scene, '--theme', theme, '--size', size, '--script', script, '--at', '2', '--tree', tree, '--snap', path.join(out, `${name}.png`)], {
    cwd: app, env, encoding: 'utf8', timeout: 30000, maxBuffer: 1024 * 1024,
  })
  assert.equal(result.status, 0, `${name}: ${result.error || result.stderr}`)
  return JSON.parse(readFileSync(tree, 'utf8'))
}
const get = (tree, key) => tree.elements.find(e => e.key === key && !e.exiting)
const text = (tree, value) => tree.elements.some(e => e.text === value && !e.exiting)
const options = tree => tree.elements.filter(e => /^model-option-\d+-\d+$/.test(e.key || '') && !e.exiting)
try {
  const field = render('field')
  assert.ok(get(field, 'composer-input'))
  assert.ok(get(field, 'attachment-0'))
  assert.ok(get(field, 'model-picker-trigger').rect[1] > get(field, 'composer-input').rect[1])
  assert.ok(get(field, 'attach-file').rect[0] > get(field, 'permissions-trigger').rect[0])
  const typed = render('search-ready', { scene: 'models', events: [{ type: 'type', t: .1, text: 'sonnet', every: .03 }] })
  assert.equal(options(typed).length, 1, 'opening the picker is immediately ready to type')
  assert.ok(get(typed, 'model-option-1-0'))
  const filteredChoice = render('filtered-keyboard-choice', { scene: 'models', events: [{ type: 'type', t: .1, text: 'sonnet', every: .03 }, { type: 'key', name: 'Enter', code: 'Enter', t: .5 }] })
  assert.ok(!get(filteredChoice, 'model-picker'))
  assert.ok(text(filteredChoice, 'claude-sonnet-4-5'), 'physical Enter selects the filtered result')
  const providers = render('provider-filter', { scene: 'models', events: [tap('provider-1', .1)] })
  assert.equal(options(providers).length, 2)
  const favorites = render('favorite-filter', { scene: 'models', events: [tap('models-favorites', .1), tap('favorite-1-0', .3)] })
  assert.equal(options(favorites).length, 1)
  assert.ok(get(favorites, 'model-picker'), 'toggling a star must not select a model or close the picker')
  const keyboard = render('keyboard-choice', { scene: 'models', events: [key('Down', .1), key('Enter', .3)] })
  assert.ok(!get(keyboard, 'model-picker'))
  assert.ok(text(keyboard, 'gpt-4.1'), 'Down/Enter selects the next model, not its favorite button')
  const shortcut = render('model-shortcut', { scene: 'models', events: [key('2', .1, { cmd: true })] })
  assert.ok(!get(shortcut, 'model-picker'))
  assert.ok(text(shortcut, 'gpt-4.1'))
  const settings = render('draft-settings', { events: [tap('reasoning-trigger', .1), tap('effort-ultra', .3), tap('tier-priority', .5), key('Escape', .7), tap('permissions-trigger', .9), tap('approval-off', 1.1)] })
  assert.ok(text(settings, 'Ultra'))
  assert.ok(text(settings, 'Full access'))
  const voice = render('voice-fan', { events: [{ type: 'move', key: 'voice-fan', t: .1 }] })
  assert.ok(get(voice, 'voice-speak'))
  assert.ok(get(voice, 'voice-wake'))
  const voiceClosed = render('voice-close', { events: [{ type: 'move', key: 'voice-fan', t: .1 }, key('Escape', .4)] })
  assert.ok(!get(voiceClosed, 'voice-wake'))
  const attached = render('native-file-choice', { events: [tap('remove-attachment-0', .1), tap('attach-file', .3), { type: 'file-dialog', t: .5, path: path.join(app, 'README.md') }] })
  assert.ok(text(attached, 'README.md'))
  assert.ok(!get(attached, 'attachment-1'))
  const small = render('short-dark-picker', { scene: 'models', theme: 'dark', size: '1024x520' })
  const [x,y,w,h] = get(small, 'model-picker').rect
  assert.ok(x >= 0 && y >= 0 && x + w <= 1025 && y + h <= 521, 'picker stays inside a short window')
  assert.ok(options(small).length > 0)
  console.log('Passed 12 native composer checks: form, autofocus search, filtered physical Enter, provider filter, favorites, keyboard selection, shortcuts, draft settings, voice fan, Escape, file dialog and short-window dark appearance')
  passed = true
} finally {
  if (passed) rmSync(out, { recursive: true, force: true })
  else console.error(`Synthetic composer evidence: ${out}`)
}
