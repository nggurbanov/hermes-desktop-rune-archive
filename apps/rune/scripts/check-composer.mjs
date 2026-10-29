import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, writeFileSync, rmSync, mkdirSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const app = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const binary = process.argv[2] || path.join(app, 'target/debug', process.platform === 'win32' ? 'hermes-rune.exe' : 'hermes-rune')
const out = process.env.HERMES_SCREEN_EVIDENCE || mkdtempSync(path.join(os.tmpdir(), 'hermes-composer-'))
mkdirSync(out, { recursive: true })
const env = { ...process.env, HOME: out, USERPROFILE: out, HERMES_HOME: path.join(out, 'agent'), HERMES_GATEWAY_TOKEN: '' }
let passed = false
const tap = (key, t) => ({ type: 'tap', key, t })
const key = (name, t, mods) => ({ type: 'key', name, t, ...mods })
function render(name, { scene = 'composer', events = [], theme = 'light', size = '1440x900' } = {}) {
  const script = path.join(out, `${name}-script.json`)
  const tree = path.join(out, `${name}.json`)
  writeFileSync(script, JSON.stringify({ events }))
  const result = spawnSync(binary, ['--scene', scene, '--theme', theme, '--size', size, '--scale', '1', '--script', script, '--at', '2', '--tree', tree, '--snap', path.join(out, `${name}.png`)], {
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
  assert.ok(get(field, 'attach-file').rect[0] < get(field, 'model-picker-trigger').rect[0], 'attachment action stays at the left')
  assert.ok(get(field, 'model-settings-trigger').rect[0] > get(field, 'model-picker-trigger').rect[0])
  assert.ok(!get(field, 'reasoning-trigger') && !get(field, 'permissions-trigger'), 'one combined settings trigger replaces the old toolbar')
  assert.ok(get(field, 'composer-send') && !get(field, 'voice-chat-start'), 'payload uses Send instead of voice chat')
  const idle = render('idle-voice', { scene: 'reader' })
  assert.ok(get(idle, 'voice-dictation') && get(idle, 'voice-chat-start') && get(idle, 'voice-engine-trigger'))
  assert.ok(!get(idle, 'composer-send'), 'empty idle composer uses the separate voice conversation primary')
  const engines = render('voice-engines', { scene: 'voice-engine', events: [tap('voice-engine-gpt-live', .1)] })
  assert.ok(get(engines, 'voice-engine-menu'), 'read-only engine rows cannot start a call or fake a settings write')
  assert.ok(get(engines, 'voice-engine-chained') && get(engines, 'voice-engine-gpt-live'))
  const unknownVoice = render('unconfirmed-voice', { scene: 'reader', events: [tap('voice-engine-trigger', .1)] })
  assert.ok(get(unknownVoice, 'voice-engine-menu') && !get(unknownVoice, 'voice-engine-chained') && !get(unknownVoice, 'voice-engine-gpt-live'), 'unknown backend status must not fabricate engine choices')
  const closedVoice = render('voice-engine-escape', { scene: 'voice-engine', events: [key('Escape', .1)] })
  assert.ok(!get(closedVoice, 'voice-engine-menu') && get(closedVoice, 'voice-engine-trigger'))
  const shortVoice = render('short-voice-engine', { scene: 'voice-engine', size: '1024x520' })
  const voiceRect = get(shortVoice, 'voice-engine-menu').rect
  assert.ok(voiceRect[0] >= 0 && voiceRect[1] >= 0 && voiceRect[0] + voiceRect[2] <= 1025 && voiceRect[1] + voiceRect[3] <= 521)
  const typed = render('search-ready', { scene: 'models', events: [{ type: 'type', t: .1, text: 'sonnet', every: .03 }] })
  assert.equal(options(typed).length, 1, 'opening the picker is immediately ready to type')
  assert.ok(get(typed, 'model-option-1-0'))
  const filteredChoice = render('filtered-keyboard-choice', { scene: 'models', events: [{ type: 'type', t: .1, text: 'sonnet', every: .03 }, { type: 'key', name: 'Enter', code: 'Enter', t: .5 }] })
  assert.ok(!get(filteredChoice, 'model-picker'))
  assert.ok(text(filteredChoice, 'claude sonnet 4 5 High'), 'physical Enter selects the filtered result')
  const providers = render('provider-filter', { scene: 'models', events: [tap('provider-1', .1)] })
  assert.equal(options(providers).length, 2)
  const favorites = render('favorite-filter', { scene: 'models', events: [tap('models-favorites', .1), tap('favorite-1-0', .3)] })
  assert.equal(options(favorites).length, 1)
  assert.ok(get(favorites, 'model-picker'), 'toggling a star must not select a model or close the picker')
  const keyboard = render('keyboard-choice', { scene: 'models', events: [key('Down', .1), key('Enter', .3)] })
  assert.ok(!get(keyboard, 'model-picker'))
  assert.ok(text(keyboard, '4.1'), 'Down/Enter selects the next model, not its favorite button')
  const shortcut = render('model-shortcut', { scene: 'models', events: [key('2', .1, { cmd: true })] })
  assert.ok(!get(shortcut, 'model-picker'))
  assert.ok(text(shortcut, '4.1'))
  const settings = render('draft-settings', { events: [tap('model-settings-trigger', .1), tap('effort-ultra', .3), tap('tier-priority', .5), { type: 'scroll', key: 'model-settings-menu', dy: -700, dur: .2, t: .7 }, tap('approval-off', 1.1)] })
  assert.ok(text(settings, '5 Ultra'))
  assert.ok(get(settings, 'model-settings-menu') && get(settings, 'permissions-selector'))
  assert.ok(text(settings, 'Off'))
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
  const shortSettings = render('short-dark-settings', { scene: 'reasoning', theme: 'dark', size: '1024x520', events: [{ type: 'scroll', key: 'model-settings-menu', dy: -700, dur: .25, t: .1 }, tap('approval-off', .6)] })
  const settingsRect = get(shortSettings, 'model-settings-menu').rect
  assert.ok(settingsRect[0] >= 0 && settingsRect[1] >= 0 && settingsRect[0] + settingsRect[2] <= 1025 && settingsRect[1] + settingsRect[3] <= 521, 'combined settings stays inside a short window')
  assert.ok(text(shortSettings, 'Off'), 'scrolling reaches permissions in a short window')
  const settingsEscape = render('settings-escape', { scene: 'reasoning', events: [key('Escape', .1)] })
  assert.ok(!get(settingsEscape, 'model-settings-menu') && get(settingsEscape, 'model-settings-trigger'))
  console.log('Passed 19 native composer checks: form, voice primary/engine status/unknown/Escape/short-window, autofocus search, filtered physical Enter, provider filter, favorites, keyboard selection, shortcuts, draft settings, voice fan, Escape, file dialog and short-window dark appearance')
  passed = true
} finally {
  if (passed && !process.env.HERMES_SCREEN_EVIDENCE) rmSync(out, { recursive: true, force: true })
  else console.error(`Synthetic composer evidence: ${out}`)
}
