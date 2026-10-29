import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
const app=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const binary=process.argv[2]||path.join(app,'target/debug/hermes-rune')
const home=mkdtempSync(path.join(os.tmpdir(),'hermes-local-settings-home-'))
const out=process.env.HERMES_SCREEN_EVIDENCE||mkdtempSync(path.join(os.tmpdir(),'hermes-local-settings-'))
const env={...process.env,HOME:home,USERPROFILE:home,XDG_STATE_HOME:path.join(home,'state'),HERMES_HOME:path.join(home,'agent'),HERMES_GATEWAY_TOKEN:''}
let passed=false,count=0
const get=(t,key)=>t.elements.find(e=>e.key===key&&!e.exiting)
const has=(t,value)=>t.elements.some(e=>e.text?.includes(value)&&!e.exiting)
const tap=(key,t)=>({type:'tap',key,t})
const physical=(chord,t)=>{
 const parts=chord.split('+'),code=parts.at(-1),shift=parts.includes('shift'),ctrl=parts.includes('ctrl')
 const name=code==='escape'?'Escape':shift&&!ctrl?code.toUpperCase():code
 return {type:'key',name,code,t,shift,ctrl,cmd:ctrl&&process.platform!=='darwin'}
}
function render(name,{scene='settings',section='shortcuts',events=[],theme='light',size='1440x900'}={}){
 const script=path.join(out,name+'-script.json'),tree=path.join(out,name+'.json')
 writeFileSync(script,JSON.stringify({events}))
 const r=spawnSync(binary,['--scene',scene,'--settings-section',section,'--theme',theme,'--size',size,'--scale','1','--script',script,'--at','2','--tree',tree,'--snap',path.join(out,name+'.png')],{cwd:app,env,encoding:'utf8',timeout:45000,maxBuffer:1024*1024})
 assert.equal(r.status,0,`${name}: ${r.error||r.stderr}`);count++
 return JSON.parse(readFileSync(tree,'utf8'))
}
const search=(q)=>[tap('keybindings-search',.1),{type:'type',text:q,every:.001,t:.2}]
try{
 const base=render('reader-default',{scene:'reader'})
 const appearance=render('appearance',{section:'appearance',events:[{type:'scroll',key:'client-preferences',dy:-430,dur:.2,t:.1}]})
 assert.ok(get(appearance,'display-font-chat-trigger')&&get(appearance,'display-font-terminal-trigger'),'agent font controls remain native, profile-owned selectors')
 const compact=get(base,'session-default-sample-one')||base.elements.find(e=>/^session-default-/.test(e.key||''))
 const detailed=render('density-detailed',{section:'appearance',events:[{type:'scroll',key:'client-preferences',dy:-220,dur:.2,t:.1},tap('display-density-detailed',.5),tap('page-back',.8)]})
 const detailRow=get(detailed,compact.key)
 assert.ok(detailRow.rect[3]>compact.rect[3],'density changes the actual sidebar row geometry')
 const large=render('chat-text-large',{section:'chat',events:[tap('display-scale-150',.2),tap('page-back',.5)]})
 const paragraph='Давай соберём спокойный план на неделю.'
 const normalText=base.elements.find(e=>e.text?.includes(paragraph))
 const largeText=large.elements.find(e=>e.text?.includes(paragraph))
 assert.ok(normalText&&largeText&&largeText.rect[3]>normalText.rect[3],'text size changes actual transcript layout')
 const transparent=render('bubble-transparent',{section:'chat',events:[tap('display-bubble-100',.2),tap('page-back',.5)]})
 const bubble=base.elements.find(e=>e.key?.startsWith('body-message-')&&e.fill&&e.fill!=='#00000000')
 assert.ok(bubble&&get(transparent,bubble.key).fill.endsWith('00'),'bubble preference changes the actual message fill alpha')
 const collapsed=render('reasoning-collapsed',{scene:'chat-working',section:'chat',events:[tap('settings',.1),tap('display-collapse-reasoning',.4),tap('page-back',.7)]})
 assert.ok(!has(collapsed,'Checking the requested files and comparing the implementation.'),'reasoning preference folds the actual live reasoning body')
 const rebound=render('keybinding-rebound',{events:[...search('sidebar'),tap('keybind-capture-view.toggleSidebar',.4),physical('ctrl+shift+y',.6),physical('ctrl+b',.8),physical('ctrl+shift+y',1)],size:'1024x520',theme:'dark'})
 assert.ok(!get(rebound,'sessions-tab'),'replacement shortcut dispatches and old default stays cleared')
 assert.ok(has(rebound,'Y'),'the captured binding is shown in the editor')
 const cleared=render('keybinding-cleared',{events:[...search('sidebar'),tap('keybind-clear-view.toggleSidebar',.4),physical('ctrl+b',.7)]})
 assert.ok(get(cleared,'sessions-tab'),'clearing a binding prevents fixed-key fallback')
 const reset=render('keybinding-reset',{events:[...search('sidebar'),tap('keybind-clear-view.toggleSidebar',.4),tap('keybind-reset-view.toggleSidebar',.6),physical('ctrl+b',.9)]})
 assert.ok(!get(reset,'sessions-tab'),'reset restores the shipped shortcut behavior')
 const cancelled=render('keybinding-cancelled',{events:[...search('sidebar'),tap('keybind-capture-view.toggleSidebar',.4),physical('escape',.6),physical('ctrl+b',.8)]})
 assert.ok(!get(cancelled,'sessions-tab'),'Escape cancels capture without removing the old binding')
 const captureInput=render('keybinding-capture-input',{events:[...search('sidebar'),tap('keybind-capture-view.toggleSidebar',.4),tap('keybindings-search',.6),physical('z',.8)]})
 assert.ok(!has(captureInput,'Listening for'),'a disabled search cannot swallow the key being recorded')
 assert.equal(get(captureInput,'keybindings-search').text??captureInput.elements.find(e=>e.path.startsWith(get(captureInput,'keybindings-search').path)&&e.text)?.text,'sidebar','capture does not edit the search query')
 const hiddenCapture=render('keybinding-hidden-capture',{events:[...search('sidebar'),tap('keybind-capture-view.toggleSidebar',.4),tap('page-back',.6),tap('settings',.8),physical('ctrl+b',1.1)]})
 assert.ok(!get(hiddenCapture,'sessions-tab'),'leaving the shortcuts page cancels capture before returning')
 const conflicts=render('keybinding-conflict',{events:[...search('Settings'),tap('keybind-capture-nav.settings',.4),physical('ctrl+n',.6)]})
 assert.ok(has(conflicts,'Also assigned to: New conversation'),'duplicate shortcuts identify their competing action')
 const typing=render('typing-shift-x',{scene:'reader',events:[tap('composer-input',.1),physical('shift+x',.3)]})
 assert.equal(get(typing,'hermes-window').fill,get(base,'hermes-window').fill,'capital X in an input does not switch the theme')
 assert.equal(get(typing,'composer-input').text,'X','the shifted character is still inserted into the draft')
 const outside=render('theme-shortcut',{scene:'reader',events:[physical('shift+x',.2)]})
 assert.notEqual(get(outside,'hermes-window').fill,get(base,'hermes-window').fill,'the unmodified theme action works when no input owns the key')
 console.log(`Passed ${count} native client-settings checks: actual typography/density, profile font controls, rebinding/clear/reset/cancel/conflicts and shifted typing safety`)
 passed=true
}finally{
 rmSync(home,{recursive:true,force:true})
 if(passed&&!process.env.HERMES_SCREEN_EVIDENCE)rmSync(out,{recursive:true,force:true})
 else console.error(`Client settings evidence: ${out}`)
}
