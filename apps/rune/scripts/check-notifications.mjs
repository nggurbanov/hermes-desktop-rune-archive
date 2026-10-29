import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, writeFileSync, rmSync, mkdirSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
const app = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const binary = process.argv[2] || path.join(app, 'target/debug/hermes-rune')
const home = mkdtempSync(path.join(os.tmpdir(), 'hermes-notification-ui-home-'))
const out = process.env.HERMES_SCREEN_EVIDENCE || mkdtempSync(path.join(os.tmpdir(), 'hermes-notification-ui-'))
mkdirSync(out, { recursive: true })
const env = { ...process.env, HOME: home, USERPROFILE: home, XDG_STATE_HOME: path.join(home, 'state'), HERMES_HOME: path.join(home, 'agent'), HERMES_GATEWAY_TOKEN: '' }
const get = (tree, key) => tree.elements.find(e => e.key === key && !e.exiting)
let count = 0, passed = false
function render(name, theme='light', size='1440x900', events=[]) {
  const script = path.join(out, name+'-script.json'), tree = path.join(out, name+'.json')
  writeFileSync(script, JSON.stringify({ events }))
  const result = spawnSync(binary, ['--scene','settings','--settings-section','notifications','--theme',theme,'--size',size,'--scale','1','--script',script,'--at','2','--tree',tree,'--snap',path.join(out,name+'.png')], { cwd: app, env, encoding:'utf8', timeout:45000, maxBuffer:1024*1024 })
  assert.equal(result.status, 0, `${name}: ${result.error || result.stderr}`)
  count++
  return JSON.parse(readFileSync(tree,'utf8'))
}
try {
  for (const theme of ['light','dark']) {
    const tree = render('notifications-'+theme,theme,'1440x900',[{type:'tap',key:'native-notifications-test',t:.2}])
    assert.ok(get(tree,'native-notifications-settings'))
    for (const kind of ['approval','backgroundDone','credits','input','plugin','turnDone','turnError']) assert.ok(get(tree,'native-notifications-'+kind),kind)
    assert.equal(get(tree,'native-notifications-test').focusable,false,'a synthetic scene cannot deliver an OS notice')
    assert.ok(!get(tree,'native-notifications-result'),'a disabled test button cannot invoke the preview delivery action')
    assert.equal(get(tree,'native-notifications-backgroundDone').focusable,true,'background alerts have their native process producer')
    assert.equal(get(tree,'native-notifications-plugin').focusable,true,'plugin alerts have their owned native producer')
    assert.equal(get(tree,'native-notifications-approval').focusable,true,'implemented category controls remain interactive')
  }
  const off = render('notifications-disabled','light','1440x900',[{type:'tap',key:'native-notifications-enabled',t:.2}])
  for (const kind of ['approval','backgroundDone','credits','input','plugin','turnDone','turnError']) assert.equal(get(off,'native-notifications-'+kind).focusable,false,'master switch disables actual category controls')
  const short = render('notifications-short','dark','1024x520',[{type:'scroll',key:'native-notifications-settings',dy:-700,dur:.2,t:.2}])
  const refresh = get(short,'native-notifications-refresh')
  assert.ok(refresh && refresh.rect[1]>=37 && refresh.rect[1]+refresh.rect[3]<=520,'the service controls are visible after scrolling in the minimum window')
  const back = render('notifications-back','light','1440x900',[{type:'tap',key:'page-back',t:.2}])
  assert.ok(get(back,'composer-input')&&!get(back,'native-notifications-settings'),'the page returns to the retained chat')
  console.log(`Passed ${count} native notification settings checks`)
  passed = true
} finally {
  rmSync(home,{recursive:true,force:true})
  if (passed && !process.env.HERMES_SCREEN_EVIDENCE) rmSync(out,{recursive:true,force:true})
  else console.error(`Notification UI evidence: ${out}`)
}
