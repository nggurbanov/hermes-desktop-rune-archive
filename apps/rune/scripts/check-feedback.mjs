import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync,readFileSync,writeFileSync,rmSync,mkdirSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
const app=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const binary=process.argv[2]||path.join(app,'target/debug/hermes-rune')
const home=mkdtempSync(path.join(os.tmpdir(),'hermes-feedback-home-'))
const out=process.env.HERMES_SCREEN_EVIDENCE||mkdtempSync(path.join(os.tmpdir(),'hermes-feedback-'))
mkdirSync(out,{recursive:true})
const env={...process.env,HOME:home,USERPROFILE:home,XDG_STATE_HOME:path.join(home,'state'),HERMES_HOME:path.join(home,'unused-agent'),HERMES_GATEWAY_TOKEN:''}
const get=(tree,key)=>tree.elements.find(e=>e.key===key&&!e.exiting)
const has=(tree,value)=>tree.elements.some(e=>e.text?.includes(value)&&!e.exiting)
const tap=(key,t)=>({type:'tap',key,t})
const scroll=(t=.1)=>({type:'scroll',key:'native-notifications-settings',dy:-900,dur:.2,t})
let count=0,passed=false
function render(name,{scene='background-processes',theme='light',size='1440x900',events=[]}={}){
 const script=path.join(out,name+'-script.json'),tree=path.join(out,name+'.json')
 writeFileSync(script,JSON.stringify({events}))
 const args=['--scene',scene,'--theme',theme,'--size',size,'--scale','1','--script',script,'--at','2','--tree',tree,'--snap',path.join(out,name+'.png')]
 if(scene==='settings')args.push('--settings-section','notifications')
 const result=spawnSync(binary,args,{cwd:app,env,encoding:'utf8',timeout:45000,maxBuffer:1024*1024})
 assert.equal(result.status,0,`${name}: ${result.error||result.stderr}`);count++
 return JSON.parse(readFileSync(tree,'utf8'))
}
try{
 for(const theme of ['light','dark']){
  const tree=render('background-'+theme,{theme})
  for(const id of ['build','tests','lint'])assert.ok(get(tree,'background-process-'+id))
  for(const label of ['Running','Finished','Failed'])assert.ok(has(tree,label))
  assert.equal(get(tree,'background-stop-build').focusable,false,'fixture processes cannot send a kill request')
  assert.equal(get(tree,'background-dismiss-tests').focusable,true,'finished rows remain locally dismissible')
  assert.ok(get(tree,'background-output-body-build')&&has(tree,'Linking Hermes'))
 }
 const collapse=render('background-output-collapsed',{events:[tap('background-output-build',.2)]})
 assert.ok(!get(collapse,'background-output-body-build'))
 const reopen=render('background-output-reopened',{events:[tap('background-output-build',.2),tap('background-output-build',.5)]})
 assert.ok(get(reopen,'background-output-body-build'))
 const dismissed=render('background-dismissed',{events:[tap('background-dismiss-tests',.2)]})
 assert.ok(!get(dismissed,'background-process-tests')&&get(dismissed,'background-process-build')&&get(dismissed,'background-process-lint'))
 // Wheel over the group's left padding, outside the nested Output viewport.
 const short=render('background-short-draft',{theme:'dark',size:'1024x520',events:[tap('composer-input',.1),{type:'type',text:Array.from({length:18},(_,i)=>`Line ${i+1}: keep background processes visible while I write.`).join('\n'),every:.0001,t:.2},{type:'scroll',x:246,y:100,dy:-200,dur:.2,t:.8}]})
 const composer=get(short,'composer-input'),stack=get(short,'background-status-stack'),card=get(short,'composer'),dock=get(short,'dock'),workspace=get(short,'workspace')
 const viewport=short.elements.find(e=>e.path===card.path+'/scroll'&&!e.exiting),toolbar=get(short,'composer-toolbar')
 const bottom=workspace.rect[1]+workspace.rect[3]
 assert.ok(composer&&viewport&&stack&&viewport.rect[1]>=workspace.rect[1]&&viewport.rect[1]+viewport.rect[3]<=bottom+.1,'the long input viewport remains in the workspace')
 assert.ok(dock.rect[1]+dock.rect[3]<=bottom+.1,'the complete composer including footer remains in the minimum window')
 assert.ok(toolbar.rect[1]>=workspace.rect[1]&&toolbar.rect[1]+toolbar.rect[3]<=bottom+.1,'all composer actions stay accessible')
 assert.ok(composer.text.includes('Line 18:'),'scrolling preserves the complete long draft')
 assert.ok(stack.rect[3]<=220.1,'many status rows have bounded scrolling height')
 const last=get(short,'background-process-lint')
 assert.ok(last.rect[1]>=stack.rect[1]&&last.rect[1]+last.rect[3]<=stack.rect[1]+stack.rect[3]+.1,'scrolling still reaches the last feedback row while composing')
 const sounds=render('sound-settings',{scene:'settings',events:[scroll()]})
 assert.ok(get(sounds,'completion-sound-preset-trigger')&&get(sounds,'completion-sound-mute'))
 assert.equal(get(sounds,'completion-sound-preview').focusable,false,'fixture settings never open speakers')
 assert.equal(get(sounds,'native-notifications-backgroundDone').focusable,true,'background alerts have a real producer')
 assert.equal(get(sounds,'native-notifications-plugin').focusable,true,'plugin alerts are exposed after their native producer is present')
 const picker=render('sound-presets',{scene:'settings',theme:'dark',events:[scroll(),tap('completion-sound-preset-trigger',.5)]})
 const content=get(picker,'completion-sound-preset-content')
 assert.ok(content)
 const options=picker.elements.filter(e=>e.path.startsWith(content.path+'/')&&e.role==='option'&&!e.exiting)
 assert.equal(options.length,15,'all fourteen original presets plus None are available')
 for(const name of ['None','Two-note comfort','Music box'])assert.ok(has(picker,name),name)
 const muted=render('sound-muted',{scene:'settings',events:[scroll(),tap('completion-sound-mute',.5)]})
 const control=tree=>tree.elements.find(e=>e.path===get(tree,'completion-sound-mute').path+'/control')
 assert.notEqual(control(muted).fill,control(sounds).fill,'mute changes the actual device switch')
 const none=render('sound-none',{scene:'settings',events:[scroll(),tap('completion-sound-preset-trigger',.5),tap('option0',.8)]})
 assert.equal(get(none,'completion-sound-preset-trigger').label,'None','preset choice updates the actual field')
 assert.ok(!get(none,'completion-sound-preset-content'),'preset choice closes the picker')
 assert.equal(get(none,'completion-sound-preview').focusable,false)
 console.log(`Passed ${count} native background and sound feedback checks`)
 passed=true
}finally{
 rmSync(home,{recursive:true,force:true})
 if(passed&&!process.env.HERMES_SCREEN_EVIDENCE)rmSync(out,{recursive:true,force:true})
 else console.error(`Feedback UI evidence: ${out}`)
}
