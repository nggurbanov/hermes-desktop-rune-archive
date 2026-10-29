import assert from 'node:assert/strict'
import {spawnSync} from 'node:child_process'
import {mkdtempSync,writeFileSync,readFileSync} from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import {fileURLToPath} from 'node:url'
const app=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const binary=process.argv[2]||path.join(app,'target/debug/hermes-rune')
const out=mkdtempSync(path.join(os.tmpdir(),'hermes-decks-native-'))
const env={...process.env,HOME:out,USERPROFILE:out,XDG_STATE_HOME:path.join(out,'state'),HERMES_HOME:path.join(out,'agent'),HERMES_GATEWAY_TOKEN:''}
const tap=(key,t)=>({type:'tap',key,t}),key=(name,t,mods={})=>({type:'key',name,code:name.length===1?'Key'+name.toUpperCase():name,t,ctrl:!!mods.cmd&&process.platform!=='darwin',...mods})
const get=(t,key)=>t.elements.find(e=>e.key===key&&!e.exiting)
function render(name,events,size='1440x900',theme='light'){
 const script=path.join(out,name+'.script.json'), tree=path.join(out,name+'.json')
 writeFileSync(script,JSON.stringify({events}))
 const r=spawnSync(binary,['--scene','reader','--theme',theme,'--size',size,'--scale','1','--script',script,'--at','2','--tree',tree,'--snap',path.join(out,name+'.png')],{cwd:app,env,encoding:'utf8',timeout:45000,maxBuffer:1048576})
 assert.equal(r.status,0,`${name}: ${r.error||r.stderr}`)
 return JSON.parse(readFileSync(tree,'utf8'))
}
const choose=id=>[tap('arrange-layout',.1),tap('layout-preset-'+id,.3),tap('layout-done',.5)]
let cases=0
for(const id of ['default','basic','focus','terminal-deck','quad']){
 const t=render(id,choose(id)); const w=get(t,'workspace'), field=get(t,'composer-input')
 assert.ok(w&&w.rect[2]>250&&w.rect[3]>100,id+' keeps a readable conversation')
 assert.ok(field&&field.rect[1]>=0&&field.rect[1]+field.rect[3]<=901,id+' keeps the composer onscreen')
 assert.ok(!t.elements.some(e=>e.role==='alert'),id+' has no runtime error')
 if(id==='basic'||id==='focus'||id==='terminal-deck')assert.ok(get(t,'dock-tab-terminal')&&!get(t,'terminal-panel'),'resting terminal keeps a reopenable rail')
 if(id==='focus')assert.ok(get(t,'dock-tab-files')&&get(t,'dock-tab-review'),'Focus stacks tools behind the conversation')
 cases++
}
const right=render('simple-right',[tap('interface-mode',.1),tap('arrange-layout',.3),tap('layout-preset-sidebar-right',.5),tap('layout-done',.7)],'1024x520','dark')
assert.ok(get(right,'sessions-zone').rect[0]>=get(right,'workspace').rect[0]+get(right,'workspace').rect[2]-1,'Simple can place its sidebar at the right')
assert.ok(!get(right,'terminal-panel')&&!get(right,'review-pane'));cases++
const memory=render('mode-memory',[tap('arrange-layout',.1),tap('layout-preset-quad',.3),tap('layout-done',.5),tap('interface-mode',.7),tap('interface-mode',.9)])
assert.ok(get(memory,'review-pane')&&get(memory,'terminal-panel'),'Advanced restores its layout after Simple')
assert.ok(get(memory,'review-pane').rect[1]>get(memory,'workspace').rect[1]+100);cases++
const saved=render('saved',[tap('arrange-layout',.1),tap('layout-preset-focus',.45),tap('layout-preset-name',.75),{type:'type',text:'Quiet desk',t:.85,every:.01},tap('layout-save-preset',1.05),tap('layout-reset',1.3),tap('layout-preset-user-quiet-desk',1.55),tap('layout-done',1.8)])
assert.ok(get(saved,'dock-tab-files')&&get(saved,'dock-tab-review')&&!get(saved,'terminal-panel'),'Saved layout restores geometry and resting tools');cases++
const terminal=render('positional-fallback',[...choose('focus'),key('j',.7,{cmd:true})])
assert.ok(get(terminal,'terminal-panel'),'CmdJ falls back to the terminal when the root has no right column');cases++
const collapsed=render('terminal-rail',[...choose('focus'),key('j',.7,{cmd:true}),key('j',.9,{cmd:true})])
assert.ok(get(collapsed,'dock-tab-terminal')&&!get(collapsed,'terminal-panel'),'A collapsed terminal retains its native reopen control');cases++
console.log(`Passed ${cases} native layout checks. Evidence: ${out}`)
