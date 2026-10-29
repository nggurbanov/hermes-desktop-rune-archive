#!/usr/bin/env node
/** Original SDK cross-source proof: duplicate profile names, retained runtimes and native activation. */
import fs from 'node:fs'
import path from 'node:path'
import zlib from 'node:zlib'
import {spawn} from 'node:child_process'
import {fileURLToPath} from 'node:url'
const rune=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
if(!process.env.DISPLAY&&process.platform==='linux')throw new Error('Run with an isolated native display, e.g. xvfb-run -a')
const assets=JSON.parse(zlib.gunzipSync(fs.readFileSync(path.join(rune,'assets/plugin-host/assets.json.gz'))))
const a='native:synthetic-a',b='saved:synthetic-b:revision-b',profile='research'
const owner={profile,target_profile:profile,profiles:[{name:profile,display_name:'Research A'}],connection_id:a,busy:false}
const source=`import * as React from 'react';import {host} from '@hermes/plugin-sdk';
export default{id:'route-proof',name:'Route proof',version:'1.0.0',register(ctx){ctx.onEvent('proof.event',event=>window.ipc.postMessage(JSON.stringify({type:'proof-event',source:event.connectionId,profile:event.profile})));ctx.register({id:'proof',area:'primarySidebar',title:'Proof',render:()=>React.createElement('p',null,'Cross-source SDK proof')});(async()=>{const routes=await host.profileRoutes();const route=routes.find(route=>route.connectionId===${JSON.stringify(b)});if(!route)throw new Error('The second source route is absent');const release=await host.retainProfile(route);try{const first=await host.requestProfile(route,'native.create',{});const second=await host.requestProfile(route,'native.followup',{session_id:first.session_id});window.ipc.postMessage(JSON.stringify({type:'proof-retained',first,second}));await host.ensureAgent(route.connectionId,route.profile);const active=await host.request('native.active',{});window.ipc.postMessage(JSON.stringify({type:'proof-active',active}));}finally{release();}})().catch(error=>window.ipc.postMessage(JSON.stringify({type:'plugin-error',message:String(error.message)})));ctx.onDispose(()=>window.ipc.postMessage(JSON.stringify({type:'proof-disposed'})));}}`
const child=spawn(path.join(rune,'target/debug/hermes-rune'),['--native-web-host'],{env:{...process.env,WEBKIT_DISABLE_COMPOSITING_MODE:'1'},stdio:['pipe','pipe','pipe']})
const send=value=>child.stdin.write(JSON.stringify(value)+'\n'),answer=(id,value)=>send({type:'plugin-response',id,value})
const sockets=new Map(),events=new Set();let creates=0,sequence=0,output='',stderr='',failure,retained=false,active=false,adopted=false,disposed=false,closing=false
const require=(condition,message)=>{if(!condition)throw new Error(message)}
function finish(){if(retained&&active&&adopted&&events.size===2&&!closing){closing=true;send({type:'close'})}}
function handle(event){
 if(process.argv.includes('--debug'))console.error(JSON.stringify(event))
 if(event.type==='plugin-bootstrap-ready')send({type:'plugin-load',source,origin:'/synthetic/route/plugin.js',owner})
 if(event.type==='plugin-request'){
  const{id,kind,payload}=event
  if(kind==='registry-roster'||kind==='registry-metadata')answer(id,{primary:a,current:{id:a,label:'A',kind:'remote',mode:'remote',profiles:owner.profiles},connections:[{id:a,label:'A',kind:'remote',mode:'remote',profiles:owner.profiles,attached:true},{id:b,label:'B',kind:'remote',mode:'remote',profiles:[{name:profile,display_name:'Research B'}],attached:true}]})
  else if(kind==='api'){require(String(payload.path).split('?')[0]==='/api/profiles','Unexpected API request '+payload.path);answer(id,{profiles:owner.profiles,active:profile})}
  else if(kind==='source-descriptor'){require([a,b].includes(payload.connectionId)&&payload.profile===profile,'Wrong source descriptor');answer(id,{baseUrl:'http://native-host.invalid',token:'',authMode:'native',mode:'remote',profile,connectionId:payload.connectionId})}
  else if(kind==='plugin-gateway-prepare'){require([a,b].includes(payload.connectionId)&&payload.profile===profile,'Wrong gateway route');const receipt='11111111-2222-4333-8444-'+String(++sequence).padStart(12,'0');sockets.set(receipt,{source:payload.connectionId,created:false,calls:0});answer(id,{url:'ws://hermes-native-gateway.invalid/'+receipt})}
  else if(kind==='plugin-socket-open'){answer(id,{accepted:true});send({type:'plugin-socket',receipt:payload.receipt,event:'open',data:{protocol:''}})}
  else if(kind==='plugin-socket-send'){
   const socket=sockets.get(payload.receipt);require(socket,'Unknown gateway receipt');const frame=JSON.parse(payload.frame.text);socket.calls++
   answer(id,{accepted:true});let result={}
   if(frame.method==='profiles.list')result={profiles:owner.profiles,active:profile}
   else if(frame.method==='client.capabilities')result={declines_not_shown:true}
   else if(frame.method==='native.create'){require(++creates===1,'The same plugin registered more than once');require(socket.source===b,'Create went to the ambient source');socket.created=true;result={session_id:'runtime-b',source:b}}
   else if(frame.method==='native.followup'){require(socket.source===b&&socket.created&&frame.params.session_id==='runtime-b','The retained runtime did not survive the second RPC');result={source:b,same_socket:true};for(const[sourceId,receipt]of [[a,[...sockets].find(([,s])=>s.source===a)[0]],[b,payload.receipt]])send({type:'plugin-socket',receipt,event:'message',data:{text:JSON.stringify({jsonrpc:'2.0',method:'event',params:{type:'proof.event',profile,session_id:'same-id',seq:1,payload:{source:sourceId}}})}})}
   else if(frame.method==='native.active'){require(socket.source===b,'ensureAgent left host.request on its old source');result={source:b}}
   else if(frame.method!=='ping')throw new Error('Unexpected gateway method '+frame.method)
   if(frame.id!=null)send({type:'plugin-socket',receipt:payload.receipt,event:'message',data:{text:JSON.stringify({jsonrpc:'2.0',id:frame.id,result})}})
  }
  else if(kind==='ensure-agent'){require(payload.connectionId===b&&payload.profile===profile,'The native activation lost its target');answer(id,{ok:true})}
  else if(kind==='adopt-sdk-route'){require(payload.connectionId===b&&payload.profile===profile,'The post-activation owner was changed');adopted=true;answer(id,{ok:true})}
  else if(kind==='plugin-socket-close'){answer(id,{accepted:true});send({type:'plugin-socket',receipt:payload.receipt,event:'close',data:{code:1000,clean:true,reason:''}})}
  else throw new Error('Unexpected native operation '+kind)
 }
 if(event.type==='proof-retained'){require(event.first.source===b&&event.second.source===b&&event.second.same_socket,'Retained source proof failed');retained=true}
 if(event.type==='proof-active'){require(event.active.source===b,'Active source proof failed');active=true}
 if(event.type==='proof-event'){require(event.profile===profile&&[a,b].includes(event.source),'Event ownership was lost');require(!events.has(event.source),'Duplicate event source');events.add(event.source)}
 if(event.type==='proof-disposed')disposed=true
 if(event.type==='plugin-error'||event.type==='error')throw new Error(event.message)
 finish()
}
const done=new Promise((resolve,reject)=>{const timer=setTimeout(()=>{failure=new Error('The cross-source proof timed out');child.kill('SIGKILL')},30000);child.stdout.on('data',bytes=>{output+=bytes;let n;while((n=output.indexOf('\n'))>=0){const line=output.slice(0,n);output=output.slice(n+1);try{handle(JSON.parse(line))}catch(error){failure=error;child.kill('SIGKILL')}}});child.stderr.on('data',bytes=>stderr+=bytes);child.on('close',code=>{clearTimeout(timer);if(failure||!closing||!disposed||code!==0)reject(failure??new Error('The routing host did not close cleanly: '+stderr.slice(-800)));else resolve()})})
send({type:'document',document:{title:'Synthetic cross-source routing proof',html:'<!doctype html><meta charset=utf-8><link rel=stylesheet href=host.css><div id=root></div><script type=module src=host.js></script>',assets,allow_media:false}})
await done
console.log(JSON.stringify({original_sdk:true,duplicate_profile_sources:2,retained_runtime:true,native_activation:true,exact_event_sources:events.size,disposed:true}))
