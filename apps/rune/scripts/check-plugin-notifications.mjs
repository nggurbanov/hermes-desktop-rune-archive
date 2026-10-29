#!/usr/bin/env node
/** Real original SDK closures in an owned Wry window; no desktop notification is sent. */
import fs from 'node:fs'
import path from 'node:path'
import os from 'node:os'
import zlib from 'node:zlib'
import {spawn} from 'node:child_process'
import {fileURLToPath} from 'node:url'
import assert from 'node:assert/strict'
const rune=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const args=process.argv.slice(2), at=args.indexOf('--binary')
const binary=at>=0?args[at+1]:path.join(rune,'target/debug/hermes-rune')
if(!process.env.DISPLAY)throw new Error('Use an isolated native test display (xvfb-run -a)')
const assets=JSON.parse(zlib.gunzipSync(fs.readFileSync(path.join(rune,'assets/plugin-host/assets.json.gz'))))
const source=`import * as React from 'react';
const counts={false:0,error:0,body:0,action:0,closed:0};
const emit=value=>window.ipc.postMessage(JSON.stringify(value));
export default {id:'notification-proof',name:'Notification proof',version:'1.0.0',register(ctx){
ctx.register({id:'proof',area:'primarySidebar',title:'Proof',render:()=>React.createElement('h1',null,'Real SDK native notifications')});
window.addEventListener('hermes-host-message',event=>{const value=event.detail;
if(value.type==='proof-notify'){const phase=value.phase;ctx.os.notify({title:phase,body:'Synthetic notification proof',activate:'/proof/'+phase,onActivate:()=>{counts[phase]++;emit({type:'proof-callback',phase,counts})},actions:[{id:'do',label:'Do it',activate:'/proof/item/1',onAction:()=>{counts.action++;emit({type:'proof-callback',phase:'action',counts})}}]});}
if(value.type==='proof-empty')void window.hermesDesktop.notify({kind:'plugin',tag:'empty',title:'empty'});
if(value.type==='proof-inspect')emit({type:'proof-counts',counts,hash:window.location.hash});
});
emit({type:'host-window-state',focused:false,visible:false});
}};`
const html=`<!doctype html><meta charset=utf-8><meta http-equiv=Content-Security-Policy content="default-src 'self' data: blob:; script-src 'self' 'unsafe-inline' blob:; style-src 'self' 'unsafe-inline'; connect-src 'self'; object-src 'none'"><link rel=stylesheet href=host.css><div id=root></div><script type=module src=host.js></script>`
const privateState=fs.mkdtempSync(path.join(os.tmpdir(),'hermes-notification-sdk-'))
const child=spawn(binary,['--native-web-host'],{env:{...process.env,HOME:privateState,XDG_CONFIG_HOME:path.join(privateState,'config'),XDG_CACHE_HOME:path.join(privateState,'cache'),XDG_DATA_HOME:path.join(privateState,'data'),WEBKIT_DISABLE_COMPOSITING_MODE:'1'},stdio:['pipe','pipe','pipe']})
const send=value=>child.stdin.write(JSON.stringify(value)+'\n')
let output='',stderr='',failure,closing=false,phase='false',asked=0,focused=false,hidden=false,spoof=false,reportedError=false
const ids=new Map(),timers=[]
const later=(fn,ms)=>timers.push(setTimeout(fn,ms))
function owner(muted=false){return {profile:'research',profiles:[{name:'research'}],connection_id:'native:synthetic',runtime_session:'runtime-proof',session:'stored-proof',sound_muted:muted,sound_preset:muted?14:1}}
function start(next){phase=next;if(next==='action')send({type:'plugin-state',owner:owner(true)});send({type:'visibility',visible:false});later(()=>send({type:'proof-notify',phase:next}),200)}
function activate(name,actionId){send({type:'focus'});send({type:'plugin-notify-activate',notifyId:ids.get(name),actionId,activate:actionId?'/proof/item/1':'/proof/'+name,pluginId:'notification-proof'});later(()=>send({type:'proof-inspect'}),180)}
function onEvent(raw){
 const native=raw.type!=='host-web-message', event=native?raw:raw.message
 if(event.type==='host-window-state'){
  if(!native){spoof=true;return}
  focused||=event.focused===true&&event.visible===true; hidden||=event.visible===false;return
 }
 if(event.type==='plugin-bootstrap-ready')send({type:'plugin-load',source,origin:'/synthetic/notifications.js',owner:owner()})
 if(event.type==='plugin-rendered'){assert.ok(event.text.includes('Real SDK native notifications'));later(()=>start('false'),4500)}
 if(event.type==='plugin-request'){
  const reply=value=>send({type:'plugin-response',id:event.id,value})
  if(event.kind==='plugin-gateway-prepare')reply({url:'ws://hermes-native-gateway.invalid/11111111-2222-4333-8444-555555555555'})
  else if(event.kind==='plugin-socket-open'){reply({accepted:true});send({type:'plugin-socket',receipt:event.payload.receipt,event:'open',data:{protocol:''}})}
  else if(event.kind==='plugin-socket-send'){reply({accepted:true});const frame=JSON.parse(event.payload.frame.text);if(frame.id)send({type:'plugin-socket',receipt:event.payload.receipt,event:'message',data:{text:JSON.stringify({jsonrpc:'2.0',id:frame.id,result:{}})}})}
  else if(event.kind==='native-notify'){
   const name=event.payload.title;assert.equal(event.payload.kind,'plugin');asked++
   if(name==='empty'){reply(false);later(()=>activate('body'),120);return}
   assert.ok(event.payload.notifyId);ids.set(name,event.payload.notifyId);assert.equal(Boolean(event.payload.silent),name==='action'||name==='closed','native device mute must hydrate original SDK notification sound policy')
   if(name==='error')send({type:'plugin-response',id:event.id,error:'Synthetic daemon rejected delivery'})
   else reply(name!=='false')
   if(name==='body'){later(()=>send({type:'proof-empty'}),100);return}
   if(name==='closed'){send({type:'plugin-notify-closed',notifyId:event.payload.notifyId});later(()=>activate(name),120);return}
   later(()=>activate(name,name==='action'?'do':undefined),120)
  } else reply({})
 }
 if(event.type==='plugin-error'||event.type==='error'){assert.equal(event.message,'Synthetic daemon rejected delivery');reportedError=true}
 if(event.type==='plugin-navigate')assert.fail('Notification activation escaped its owned child router')
 if(event.type==='proof-counts'){
  assert.equal(event.counts.false,0);assert.equal(event.counts.error,0);assert.equal(event.counts.closed,0)
  if(phase==='false')later(()=>start('error'),1100)
  else if(phase==='error')later(()=>start('body'),1100)
  else if(phase==='body'){assert.equal(event.counts.body,1,'callback-free false must not clear another accepted closure');assert.equal(event.hash,'#/proof/body');later(()=>start('action'),1100)}
  else if(phase==='action'){assert.equal(event.counts.action,1);assert.equal(event.hash,'#/proof/item/1');later(()=>start('closed'),1100)}
  else{assert.equal(event.counts.body,1);assert.equal(event.counts.action,1);assert.ok(spoof&&focused&&hidden&&reportedError);assert.equal(asked,6);closing=true;send({type:'close'})}
 }
}
await new Promise((resolve,reject)=>{
 const deadline=setTimeout(()=>{failure=new Error('The notification SDK proof timed out');child.kill('SIGKILL')},30000)
 child.stdout.on('data',bytes=>{output+=bytes;let n;while((n=output.indexOf('\n'))>=0){const line=output.slice(0,n);output=output.slice(n+1);try{onEvent(JSON.parse(line))}catch(error){failure=error;child.kill('SIGKILL')}}})
 child.stderr.on('data',bytes=>stderr+=bytes)
 child.on('close',code=>{clearTimeout(deadline);for(const timer of timers)clearTimeout(timer);if(failure||!closing||code!==0)reject(failure??new Error('Notification host closed unexpectedly: '+stderr.slice(-600)));else resolve()})
 send({type:'document',document:{title:'Hermes notification proof',html,assets}})
}).finally(()=>fs.rmSync(privateState,{recursive:true,force:true}))
console.log('Passed native original SDK notify/false/error/body/action/closed cleanup, owned router and native focus/visibility proof')
