import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
const app=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const binary=process.argv[2]||path.join(app,'target/debug/hermes-rune')
const out=process.env.HERMES_SCREEN_EVIDENCE||mkdtempSync(path.join(os.tmpdir(),'hermes-editors-native-'))
mkdirSync(out,{recursive:true})
const env={...process.env,HOME:out,USERPROFILE:out,HERMES_HOME:path.join(out,'agent'),HERMES_GATEWAY_TOKEN:''}
const tap=(key,t,extra={})=>({type:'tap',key,t,...extra})
const first=[tap('composer-input',.1),{type:'key',name:'a',cmd:true,t:.2},{type:'type',text:'first editor unsent',every:.005,t:.3}]
const second=[tap('bots-tab',.5),tap('bot-research',.6,{button:'secondary'}),tap('conversation-actions-item1',.8),tap('composer-input',1),{type:'type',text:'second editor unsent',every:.005,t:1.1}]
function render(name,events,theme='light') {
 const script=path.join(out,name+'-script.json');writeFileSync(script,JSON.stringify({events}))
 const tree=path.join(out,name+'.json')
 const r=spawnSync(binary,['--scene','composer','--theme',theme,'--size','1440x1000','--script',script,'--at','3','--tree',tree,'--snap',path.join(out,name+'.png')],{cwd:app,env,encoding:'utf8',timeout:45000,maxBuffer:1024*1024})
 assert.equal(r.status,0,`${name}: ${r.error||r.stderr}`)
 const data=JSON.parse(readFileSync(tree,'utf8'));assert.deepEqual(data.warnings||[],[])
 return data.elements.filter(e=>!e.exiting)
}
const get=(tree,key)=>tree.find(e=>e.key===key)
const field=tree=>get(tree,'composer-input').text
const secondDraft=render('second-editor',[...first,...second])
assert.equal(field(secondDraft),'second editor unsent')
assert.ok(!get(secondDraft,'attachment-0'),'a second input never borrows another input’s files')
assert.equal(secondDraft.filter(e=>/^conversation-tab-\d+$/.test(e.key||'')).length,2)
const firstAgain=render('first-editor-restored',[...first,...second,tap('activate-tab-0',1.5)])
assert.equal(field(firstAgain),'first editor unsent')
assert.ok(get(firstAgain,'attachment-0'),'the original input retains its attachment')
const secondAgain=render('second-editor-restored',[...first,...second,tap('activate-tab-0',1.5),tap('activate-tab-1',1.7)],'dark')
assert.equal(field(secondAgain),'second editor unsent')
assert.ok(!get(secondAgain,'attachment-0'))
const pending=[...first,tap('attach-file',.45,{hold:.02}),...second,{type:'file-dialog',path:path.join(app,'README.md'),t:1.5}]
const callbackOther=render('file-return-while-other-editor-active',pending)
assert.equal(field(callbackOther),'second editor unsent')
assert.ok(!get(callbackOther,'attachment-0'),'the file chooser cannot attach into the newly selected input')
const callbackOwner=render('file-return-original-editor',[...pending,tap('activate-tab-0',1.8)],'dark')
assert.equal(field(callbackOwner),'first editor unsent')
assert.ok(get(callbackOwner,'attachment-0')&&get(callbackOwner,'attachment-1'))
assert.ok(callbackOwner.some(e=>e.text==='README.md'))
console.log(`Passed 5 native independent-editor checks. Evidence: ${out}`)
