// Exercise native feature navigation and short-window geometry with isolated demo data.
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const app = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const binary = process.argv[2] || path.join(app, 'target/debug', process.platform === 'win32' ? 'hermes-rune.exe' : 'hermes-rune')
const out = process.env.HERMES_SCREEN_EVIDENCE || mkdtempSync(path.join(os.tmpdir(), 'hermes-feature-pages-'))
const home = mkdtempSync(path.join(os.tmpdir(), 'hermes-feature-home-'))
const env = { ...process.env, HOME: home, USERPROFILE: home, HERMES_HOME: path.join(home, 'agent'), HERMES_GATEWAY_TOKEN: '' }
let passed = false, count = 0
const get = (tree, key) => tree.elements.find(e => e.key === key && !e.exiting)
const has = (tree, value) => tree.elements.some(e => e.text?.includes(value) && !e.exiting)
const tap = key => ({ type: 'tap', key, t: .2 })
function render(name, scene, { theme = 'light', size = '1440x900', args = [], events = [] } = {}) {
  const tree = path.join(out, `${name}.json`)
  const script = path.join(out, `${name}-script.json`)
  writeFileSync(script, JSON.stringify({ events }))
  const run = spawnSync(binary, ['--scene', scene, '--theme', theme, '--size', size, '--scale', '1', ...args,
    '--script', script, '--at', '2', '--tree', tree, '--snap', path.join(out, `${name}.png`)], {
    cwd: app, env, encoding: 'utf8', timeout: 45000, maxBuffer: 1024 * 1024,
  })
  assert.equal(run.status, 0, `${name}: ${run.error || run.stderr}`)
  count++
  return JSON.parse(readFileSync(tree, 'utf8'))
}
function inside(child, parent, message) {
  assert.ok(child && parent, message)
  const [x, y, w, h] = child.rect
  const [px, py, pw, ph] = parent.rect
  assert.ok(w > 0 && h > 0 && x >= px - 1 && y >= py - 1 && x + w <= px + pw + 1 && y + h <= py + ph + 1,
    `${message}: ${JSON.stringify(child.rect)} within ${JSON.stringify(parent.rect)}`)
}
try {
  for (const [scene, key, content, args] of [
    ['settings', 'settings-page', 'Memory layers', ['--settings-section', 'memory']],
    ['messaging', 'messaging-page', 'Talk to Hermes from Telegram', []],
    ['artifacts', 'artifacts-page', 'Architecture diagram.png', []],
    ['bot-manager', 'bots-manager', 'Find and organize research', []],
    ['command-center', 'command-center', 'Gateway health', ['--ops-section', 'system']],
    ['review', 'review-pane', 'feature/chat', []],
  ]) {
    for (const [theme, size] of [['light', '1440x900'], ['dark', '1024x520']]) {
      const tree = render(`${scene}-${theme}`, scene, { theme, size, args })
      inside(get(tree, key), get(tree, 'hermes-window'), `${scene} retains a bounded native viewport`)
      assert.ok(has(tree, content), `${scene} keeps its feature data after native initialization`)
      assert.ok(!tree.elements.some(e => e.role === 'alert'), `${scene} renders without a runtime error`)
      if (scene === 'bot-manager') {
        for (const id of ['identity', 'model', 'soul', 'capabilities', 'appearance', 'manage'])
          inside(get(tree, `bot-tab-${id}`), get(tree, 'bots-section-tabs'), `Bot ${id} remains reachable at ${size}`)
      }
      if (scene === 'review') {
        assert.ok(get(tree, 'review-files') && get(tree, 'file:src/main.rs'), 'Review retains real virtualized file rows')
      }
    }
  }
  for (const [tab, expected] of [['identity', 'Profile ID:'], ['model', 'Profile model'], ['soul', 'SOUL.md'],
    ['capabilities', 'Toolsets'], ['appearance', 'Avatar'], ['manage', 'Duplicate bot']]) {
    const tree = render(`bot-tab-${tab}`, 'bot-manager', { theme: 'dark', size: '1024x520', events: [tap(`bot-tab-${tab}`)] })
    assert.ok(has(tree, expected), `Bot ${tab} opens through its native control`)
    inside(get(tree, `bot-tab-${tab}`), get(tree, 'hermes-window'), `Bot ${tab} control stays on screen`)
    if (tab === 'manage') {
      const remove = tree.elements.find(e => e.role === 'button' && e.label === 'Delete profile')
      inside(remove, get(tree, 'hermes-window'), 'Manage profile keeps Delete reachable on short windows')
    }
  }
  for (const [tab, expected] of [[1, 'Pending requests'], [2, 'New subscription'], [3, 'Create a Telegram bot']]) {
    const tree = render(`messaging-tab-${tab}`, 'messaging', { size: '1024x520', events: [tap(`messaging-tab-${tab}`)] })
    assert.ok(has(tree, expected), `Messaging tab ${tab} opens its native view`)
  }
  const files = render('artifact-files', 'artifacts', { size: '1024x520', events: [tap('artifact-filter-file')] })
  assert.ok(has(files, 'report.pdf') && !has(files, 'Architecture diagram.png'), 'Artifact filtering changes the gallery and file table together')
  for (const theme of ['light', 'dark']) {
    const tree = render(`review-scrolled-${theme}`, 'review', { theme, size: '1024x520', events: [{ type: 'scroll', x: 727, y: 55, dy: -700, dur: .3, t: .2 }] })
    assert.ok(get(tree, 'review-diff'), 'The selected Review diff survives scrolling the narrow inspector')
    assert.ok(has(tree, 'println!'), 'The actual diff remains available after scrolling')
    const pane = get(tree, 'review-pane')
    for (const label of ['Copy relative path', 'Branches & worktrees', 'Reveal in file manager']) {
      const control = tree.elements.find(e => e.text === label && e.path.startsWith(pane.path))
      assert.ok(control && control.rect[0] >= pane.rect[0] - 1 && control.rect[0] + control.rect[2] <= pane.rect[0] + pane.rect[2] + 1, `${label} stays inside the narrow Review pane`)
    }
    const [,, , height] = get(tree, 'review-pane').rect
    const diffTop = get(tree, 'review-diff').rect[1]
    assert.ok(diffTop > 37 - get(tree, 'review-diff').rect[3] && diffTop < 37 + height, 'Review diff can be brought into the actual narrow viewport')
  }
  console.log(`Passed ${count} native feature-page checks: initialization, bounded viewports, Bot tabs, Messaging navigation, Artifact filtering and virtualized Review scrolling`)
  passed = true
} finally {
  rmSync(home, { recursive: true, force: true })
  if (passed && !process.env.HERMES_SCREEN_EVIDENCE) rmSync(out, { recursive: true, force: true })
  else console.error(`Native feature-page evidence: ${out}`)
}
