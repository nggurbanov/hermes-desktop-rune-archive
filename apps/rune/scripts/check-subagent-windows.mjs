#!/usr/bin/env node
/** Actual Rune child/registry acceptance; the gateway is a private synthetic peer. */
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import {fileURLToPath} from 'node:url'
import {spawn,execFileSync} from 'node:child_process'
import {WebSocketServer} from 'ws'
import {createServer} from 'node:http'
const rune=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const args=process.argv.slice(2)
const option=name=>{const i=args.indexOf(name);return i>=0?args[i+1]:null}
const binary=option('--binary')??path.join(rune,'target/debug/hermes-rune')
if(process.platform==='linux'&&!process.env.DISPLAY)throw new Error('Run native window acceptance under xvfb-run -a or an isolated display')
const home=fs.mkdtempSync(path.join(os.tmpdir(),'hermes-subagent-native-'))
const out=path.resolve(option('--out')??home);fs.mkdirSync(out,{recursive:true})
const rich=args.includes('--rich-media')
const mediaRequests=[]
const sourceSvg=`<svg xmlns="http://www.w3.org/2000/svg" width="480" height="180"><rect width="480" height="180" fill="#d7eee8"/><rect x="24" y="24" width="120" height="132" rx="8" fill="#509985"/><rect x="162" y="24" width="294" height="58" rx="8" fill="#91baae"/><rect x="162" y="100" width="180" height="56" rx="8" fill="#bd9984"/><text x="178" y="61" font-size="20" fill="#223c34">Synthetic source image</text></svg>`
const httpServer=createServer((request,response)=>{
 try{
  const url=new URL(request.url,'http://127.0.0.1')
  assert.equal(request.method,'GET');assert.equal(url.pathname,'/api/fs/read-data-url');assert.equal(url.searchParams.get('profile'),'work');assert.equal(url.searchParams.get('session_id'),'child');assert.equal(url.searchParams.get('path'),'/agent/reference.svg')
  mediaRequests.push({path:url.pathname,profile:url.searchParams.get('profile'),session:url.searchParams.get('session_id')})
  response.writeHead(200,{'Content-Type':'application/json'});response.end(JSON.stringify({data_url:'data:image/svg+xml;base64,'+Buffer.from(sourceSvg).toString('base64')}))
 }catch(error){failure=error;response.writeHead(400);response.end('{}')}
})
const server=new WebSocketServer({server:httpServer})
await new Promise(resolve=>httpServer.listen(0,'127.0.0.1',resolve))
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
   socket.send(JSON.stringify({jsonrpc:'2.0',id:frame.id,result:{session_id:'live-child',stored_session_id:'child',revision:1,execution_generation:1,authority_epoch:7,replay_epoch:'native-proof',last_sequence:0,running:true,prompts:[],pending:[],info:{profile_name:'work',desktop_protocol:'hermes-gateway-v1'},messages:[{row_id:20,role:'user',content:rich?[{type:'text',text:'Compare the source image and report.'},{type:'image',path:'/agent/reference.svg',name:'Reference image'},{type:'file',path:'/agent/report.txt',name:'Source report.txt'}]:'Compare all 80 source images and summarize the differences.'}]}}))
   const event=(seq,session,type,payload)=>socket.send(JSON.stringify({method:'event',params:{session_id:session,type,seq,replay_epoch:'native-proof',execution_generation:1,authority_epoch:7,payload}}))
   event(1,'other-child','message.delta',{text:'WRONG CHILD MUST NEVER APPEAR',turn_id:'wrong',segment:0,offset:0})
   event(1,'live-child','message.delta',{text:rich?'Checked all 80 candidates. The source comparisons are complete.':'I checked all 80 images across eight scenarios. The source comparisons are complete.\n\nThe strongest proposals match the brief and their reference images.',turn_id:'proof',segment:0,offset:0})
   event(2,'live-child','reasoning.delta',{text:'Verify each reference before comparing the proposals.'})
   event(3,'live-child','tool.complete',{name:'image_analysis',summary:'Analyzed 16 images'})
   event(4,'live-child','tool.complete',{name:'read_file',arguments:{path:'/agent/report.txt'},result:'All source files match the submitted references.',summary:'Explored 6 files'})
   event(5,'live-child','message.complete',{status:'complete'})
   attachedResolve()
  }catch(error){failure=error;socket.close()}
 })
})
const endpoint=`http://127.0.0.1:${server.address().port}`
const child=spawn(binary,['--check-subagent-windows','--gateway',endpoint,...(rich?['--rich-media']:[])],{env:{...process.env,HOME:home,USERPROFILE:home,XDG_DATA_HOME:home,HERMES_HOME:path.join(home,'unused-agent'),HERMES_GATEWAY_TOKEN:''},stdio:['pipe','pipe','pipe']})
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
  capture('subagent-native-light',windows[0])
  if(rich){
   execFileSync('xdotool',['mousemove','--window',windows[0],'30','553','click','1'])
   await delay(150)
   const clipboard=execFileSync('/usr/bin/python3',['-c',`import gi;gi.require_version('Gtk','3.0');from gi.repository import Gtk,Gdk;Gtk.init([]);print(Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD).wait_for_text() or '')`],{encoding:'utf8',timeout:5000})
   assert.ok(clipboard.includes('Checked all 80 candidates'),'The native Copy action did not copy the projected assistant text')
   execFileSync('xdotool',['mousemove','--window',windows[0],'100','685','click','1'])
   await delay(150);capture('subagent-native-tool-output-light',windows[0])
  }
  send()
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
 const expectedSockets=1+mediaRequests.length
 const closedDeadline=Date.now()+1000;while(closedSockets!==expectedSockets&&Date.now()<closedDeadline)await delay(10)
 assert.deepEqual(phases,['opened','reused','closed']);assert.equal(calls.length,1,'Focus/theme/reuse must keep the spectator attachment');assert.equal(closedSockets,expectedSockets,'Closing the window must detach its viewer sockets')
 if(rich){assert.equal(mediaRequests.length,1);assert.equal(mediaRequests[0].session,'child')}
 console.log(JSON.stringify({rich_media:rich,native_copy:rich,tool_output_disclosure:rich,scoped_image_reads:mediaRequests.length,native_rune:true,lazy_resume:true,read_only:true,window_reused:true,focus_receipt:true,theme_changed:true,detached:true,screenshots:out}))
}finally{
 if(child.exitCode==null)child.kill('SIGKILL')
 for(const socket of sockets)socket.terminate()
 await new Promise(resolve=>server.close(resolve))
 await new Promise(resolve=>httpServer.close(resolve))
}
