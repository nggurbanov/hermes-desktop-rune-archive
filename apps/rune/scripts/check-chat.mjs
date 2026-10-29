import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
const app=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const binary=process.argv[2]||path.join(app,'target/debug/hermes-rune')
const out=mkdtempSync(path.join(os.tmpdir(),'hermes-chat-acceptance-'))
const env={...process.env,HOME:out,USERPROFILE:out,XDG_STATE_HOME:path.join(out,'state'),HERMES_HOME:path.join(out,'agent'),HERMES_GATEWAY_TOKEN:''}
function scene(name,events=[],theme='light',size='1440x1200'){
 const script=path.join(out,`${name}.script.json`), tree=path.join(out,`${name}.json`)
 writeFileSync(script,JSON.stringify({events}))
 const result=spawnSync(binary,['--scene','rich-chat','--theme',theme,'--size',size,'--script',script,'--at','2','--tree',tree,'--snap',path.join(out,`${name}.png`)],{cwd:app,env,encoding:'utf8',timeout:30000,maxBuffer:1048576})
 assert.equal(result.status,0,`${name}: ${result.error||result.stderr}`)
 return JSON.parse(readFileSync(tree,'utf8'))
}
const get=(tree,key)=>tree.elements.find(e=>e.key===key&&!e.exiting)
const text=(tree,value)=>tree.elements.some(e=>e.text===value&&!e.exiting)
const tap=(key,t)=>({type:'tap',key,t})
const base=scene('rich-light')
assert.ok(get(base,'toggle-row-102-part-0'),'readable thought has a native disclosure')
assert.ok(text(base,'Targeted checks passed. No live agent prompts were sent.'),'tool results are joined to their actual call')
assert.ok(get(base,'edit-row-101')&&get(base,'restore-row-101')&&get(base,'retry-row-101'),'saved user messages offer history actions')
assert.ok(text(base,'↗ report.md'),'files remain explicit native actions')
const quote=scene('quote',[tap('quote-row-101',.1)])
assert.ok(get(quote,'composer-input').text.includes('> Проверь, что перенесено в Rune'),'quote populates this conversation draft')
const edit=scene('edit',[tap('edit-row-101',.1)])
assert.ok(get(edit,'chat-edit-input')&&get(edit,'chat-edit-save'),'edit opens an inline multiline draft')
const confirm=scene('confirm-edit',[tap('edit-row-101',.1),tap('chat-edit-save',.3)])
assert.ok(confirm.elements.some(e=>e.text?.startsWith('Edit and resend? This removes this turn')),'history removal has explicit confirmation before mutation')
const restore=scene('confirm-restore',[tap('restore-row-101',.1)])
assert.ok(restore.elements.some(e=>e.text?.startsWith('Restore to draft?')))
const collapse=scene('collapsed',[tap('toggle-row-102-part-1',.1)])
assert.ok(!text(collapse,'Targeted checks passed. No live agent prompts were sent.'),'collapse hides tool output without dropping the call')
const short=scene('rich-dark-short',[],'dark','1024x520')
assert.ok(get(short,'composer-input').rect[1]>=0,'compact window retains the composer')
console.log(`Passed 8 native rich-chat checks. Evidence: ${out}`)
