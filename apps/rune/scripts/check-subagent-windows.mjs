#!/usr/bin/env node
/** Actual Rune child/registry acceptance; the gateway is a private synthetic peer. */
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import {fileURLToPath} from 'node:url'
import {spawn,execFileSync} from 'node:child_process'
import {WebSocketServer} from 'ws'
const rune=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const args=process.argv.slice(2)
const option=name=>{const i=args.indexOf(name);return i>=0?args[i+1]:null}
const binary=option('--binary')??path.join(rune,'target/debug/hermes-rune')
if(process.platform==='linux'&&!process.env.DISPLAY)throw new Error('Run native window acceptance under xvfb-run -a or an isolated display')
const home=fs.mkdtempSync(path.join(os.tmpdir(),'hermes-subagent-native-'))
const out=path.resolve(option('--out')??home);fs.mkdirSync(out,{recursive:true})
const server=new WebSocketServer({host:'127.0.0.1',port:0})
await new Promise(resolve=>server.once('listening',resolve))
const calls=[],sockets=new Set();let attachedResolve
const attached=new Promise(resolve=>attachedResolve=resolve)
let failure,closedSockets=0
server.on('connection',socket=>{
 sockets.add(socket)
 socket.on('close',()=>{sockets.delete(socket);closedSockets++})
 socket.on('message',bytes=>{
  try{
   const frame=JSON.parse(bytes);calls.push(frame)
   assert.equal(frame.method,'session.resume','The spectator sent an agent action')
   assert.equal(frame.params.session_id,'child');assert.equal(frame.params.profile,'work');assert.equal(frame.params.lazy,true);assert.equal(frame.params.defer_history,false);assert.notEqual(frame.params.omit_messages,true)
   socket.send(JSON.stringify({jsonrpc:'2.0',id:frame.id,result:{session_id:'live-child',stored_session_id:'child',revision:1,execution_generation:1,authority_epoch:7,replay_epoch:'native-proof',last_sequence:0,running:true,prompts:[],pending:[],info:{profile_name:'work',desktop_protocol:'hermes-gateway-v1'},messages:[{role:'user',content:'Compare all 80 source images and summarize the differences.'}]}}))
   const event=(seq,session,type,payload)=>socket.send(JSON.stringify({method:'event',params:{session_id:session,type,seq,replay_epoch:'native-proof',execution_generation:1,authority_epoch:7,payload}}))
   event(1,'other-child','message.delta',{text:'WRONG CHILD MUST NEVER APPEAR',turn_id:'wrong',segment:0,offset:0})
   event(1,'live-child','message.delta',{text:'I checked all 80 images across eight scenarios. The source comparisons are complete.\n\nThe strongest proposals match the brief and their reference images.',turn_id:'proof',segment:0,offset:0})
   event(2,'live-child','reasoning.delta',{text:'Verify each reference before comparing the proposals.'})
   event(3,'live-child','tool.complete',{name:'image_analysis',summary:'Analyzed 16 images'})
   event(4,'live-child','tool.complete',{name:'read_file',summary:'Explored 6 files'})
   event(5,'live-child','message.complete',{status:'complete'})
   attachedResolve()
  }catch(error){failure=error;socket.close()}
 })
})
const endpoint=`http://127.0.0.1:${server.address().port}`
const child=spawn(binary,['--check-subagent-windows','--gateway',endpoint],{env:{...process.env,HOME:home,USERPROFILE:home,XDG_DATA_HOME:home,HERMES_HOME:path.join(home,'unused-agent'),HERMES_GATEWAY_TOKEN:''},stdio:['pipe','pipe','pipe']})
let output='',stderr='',phases=[],work=Promise.resolve(),pid
const send=()=>child.stdin.write(JSON.stringify({type:'next'})+'\n')
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms))
function capture(name,windowId){
 const target=path.join(out,`${name}.png`)
 const geometry=Object.fromEntries(execFileSync('xdotool',['getwindowgeometry','--shell',String(windowId)],{encoding:'utf8'}).trim().split('\n').map(line=>line.split('=')))
 const bounds=['X','Y','WIDTH','HEIGHT'].map(key=>Number(geometry[key]))
 assert.ok(bounds.every(Number.isFinite),'The native window geometry is unavailable')
 execFileSync('/usr/bin/python3',['-c',`import gi,os,sys;gi.require_version('Gdk','3.0');from gi.repository import Gdk;display=Gdk.Display.open(os.environ['DISPLAY']);root=display.get_default_screen().get_root_window();x,y,w,h=map(int,sys.argv[2:6]);image=Gdk.pixbuf_get_from_window(root,x,y,w,h);image.savev(sys.argv[1],'png',[],[])`,target,...bounds.map(String)],{env:process.env,stdio:'pipe'})
 return target
}

async function phase(value){
 phases.push(value.phase)
 if(value.phase==='opened'){
  assert.equal(value.windows,1);pid=value.pid
  await attached;await delay(450)
  const windows=execFileSync('xdotool',['search','--onlyvisible','--pid',String(pid)],{encoding:'utf8'}).trim().split('\n').filter(Boolean)
  assert.equal(windows.length,1,'A subagent must have exactly one visible native window')
  capture('subagent-native-light',windows[0]);send()
 }else if(value.phase==='reused'){
  assert.equal(value.pid,pid);assert.equal(value.windows,1);assert.equal(value.focus,true);assert.equal(value.dark,true)
  await delay(350)
  const windows=execFileSync('xdotool',['search','--onlyvisible','--pid',String(pid)],{encoding:'utf8'}).trim().split('\n').filter(Boolean)
  assert.equal(windows.length,1,'Reopening duplicated the native spectator')
  capture('subagent-native-dark',windows[0]);send()
 }else if(value.phase==='closed'){assert.equal(value.windows,0)}else throw new Error('Unknown native acceptance phase')
}
try{
 await new Promise((resolve,reject)=>{
  const timer=setTimeout(()=>{failure=new Error('Native spectator acceptance timed out');child.kill('SIGKILL')},30000)
  child.stdout.on('data',bytes=>{output+=bytes;let n;while((n=output.indexOf('\n'))>=0){const line=output.slice(0,n);output=output.slice(n+1);work=work.then(()=>phase(JSON.parse(line))).catch(error=>{failure=error;child.kill('SIGKILL')})}})
  child.stderr.on('data',bytes=>stderr+=bytes)
  child.once('error',reject)
  child.once('close',async code=>{clearTimeout(timer);await work;if(failure||code!==0)reject(failure??new Error(stderr.slice(-1500)));else resolve()})
 })
 const closedDeadline=Date.now()+1000;while(closedSockets!==1&&Date.now()<closedDeadline)await delay(10)
 assert.deepEqual(phases,['opened','reused','closed']);assert.equal(calls.length,1,'Focus/theme/reuse must keep the spectator attachment');assert.equal(closedSockets,1,'Closing the window must detach its viewer socket')
 console.log(JSON.stringify({native_rune:true,lazy_resume:true,read_only:true,window_reused:true,focus_receipt:true,theme_changed:true,detached:true,screenshots:out}))
}finally{
 if(child.exitCode==null)child.kill('SIGKILL')
 for(const socket of sockets)socket.terminate()
 await new Promise(resolve=>server.close(resolve))
}
