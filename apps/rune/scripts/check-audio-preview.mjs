#!/usr/bin/env node
/** Real native metadata/controls/save acceptance; Play is proven separately on private audio. */
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import {createServer} from 'node:http'
import {spawn,execFileSync} from 'node:child_process'
import {fileURLToPath} from 'node:url'
import {WebSocketServer} from 'ws'
const rune=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const args=process.argv.slice(2),option=name=>{const i=args.indexOf(name);return i>=0?args[i+1]:null}
const binary=option('--binary')??path.join(rune,'target/debug/hermes-rune')
const out=path.resolve(option('--out')??'/tmp/hermes-audio-preview-proof')
fs.mkdirSync(out,{recursive:true})
const home=fs.mkdtempSync(path.join(os.tmpdir(),'hermes-audio-preview-home-'))
const wav=Buffer.alloc(44+8*48000*2)
wav.write('RIFF');wav.writeUInt32LE(wav.length-8,4);wav.write('WAVEfmt ',8);wav.writeUInt32LE(16,16);wav.writeUInt16LE(1,20);wav.writeUInt16LE(1,22);wav.writeUInt32LE(48000,24);wav.writeUInt32LE(96000,28);wav.writeUInt16LE(2,32);wav.writeUInt16LE(16,34);wav.write('data',36);wav.writeUInt32LE(wav.length-44,40)
for(let i=0;i<8*48000;i++)wav.writeInt16LE(Math.round(Math.sin(i/48000*Math.PI*440)*4000),44+i*2)
const reads=[],calls=[],sockets=new Set();let failure,stderr=''
const http=createServer((request,response)=>{try{
 const url=new URL(request.url,'http://localhost');assert.ok(['GET','HEAD'].includes(request.method));assert.ok(['/api/fs/download','/api/files/stream'].includes(url.pathname));assert.equal(url.searchParams.get('profile'),'research');assert.equal(url.searchParams.get('session_id'),'audio-session');assert.equal(url.searchParams.get('path'),'/fixture/voice.wav');assert.equal(url.searchParams.get('token'),'audio-fixture')
 let start=0,end=wav.length-1,status=200;const range=request.headers.range
 if(range){const match=/^bytes=(\d+)-(\d+)$/.exec(range);assert.ok(match);start=Number(match[1]);end=Number(match[2]);assert.ok(start<=end&&end<wav.length);assert.equal(request.headers['if-range'],'"audio-v1"');status=206}
 reads.push({route:url.pathname,method:request.method,start,end})
 response.writeHead(status,{'content-type':'audio/wav','content-length':end-start+1,etag:'"audio-v1"','accept-ranges':'bytes',...(range?{'content-range':`bytes ${start}-${end}/${wav.length}`}:{})});response.end(request.method==='HEAD'?undefined:wav.subarray(start,end+1))
}catch(error){response.writeHead(400);response.end();failure=error}})
const ws=new WebSocketServer({server:http,path:'/api/ws'});ws.on('connection',socket=>{sockets.add(socket);socket.on('message',bytes=>{calls.push(JSON.parse(bytes));failure=new Error('Audio preview issued an unexpected agent RPC')});socket.on('close',()=>sockets.delete(socket))})
await new Promise(resolve=>http.listen(0,'127.0.0.1',resolve))
const child=spawn(binary,['--audio-preview-proof','--gateway',`http://127.0.0.1:${http.address().port}`,'--proof-output',out],{env:{...process.env,HOME:home,USERPROFILE:home,XDG_STATE_HOME:home,HERMES_HOME:path.join(home,'unused-agent'),HERMES_GATEWAY_TOKEN:''},stdio:['ignore','ignore','pipe']});child.stderr.on('data',bytes=>stderr+=bytes)
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms))
function json(name){try{return JSON.parse(fs.readFileSync(path.join(out,name+'.json'),'utf8'))}catch{return {}}}
async function wait(check){const deadline=Date.now()+20000;while(Date.now()<deadline){if(failure)throw failure;if(child.exitCode!=null)throw new Error(stderr.slice(-3000));const value=json('inspection');if(check(value))return value;await delay(40)}throw new Error(`Native audio preview did not settle: ${JSON.stringify(json('inspection'))}\n${stderr.slice(-1000)}`)}
function input(...args){execFileSync('xdotool',args.map(String),{stdio:'pipe'})}
function click(window,key,fraction=.5){const rect=json('geometry')[key];assert.ok(rect,`Missing native geometry for ${key}`);input('mousemove','--window',window,rect[0]+rect[2]*fraction,rect[1]+rect[3]/2,'click','1')}
function capture(name,window){const target=path.join(out,`${name}.png`),geometry=Object.fromEntries(execFileSync('xdotool',['getwindowgeometry','--shell',String(window)],{encoding:'utf8'}).trim().split('\n').map(line=>line.split('='))),bounds=['X','Y','WIDTH','HEIGHT'].map(k=>Number(geometry[k]));execFileSync('/usr/bin/python3',['-c',`import gi,os,sys;gi.require_version('Gdk','3.0');from gi.repository import Gdk;display=Gdk.Display.open(os.environ['DISPLAY']);root=display.get_default_screen().get_root_window();x,y,w,h=map(int,sys.argv[2:6]);image=Gdk.pixbuf_get_from_window(root,x,y,w,h);image.savev(sys.argv[1],'png',[],[])`,target,...bounds.map(String)],{stdio:'pipe'})}
try{
 const initial=await wait(s=>s.ready);assert.equal(initial.state,'Paused');assert.equal(initial.error,null);assert.equal(initial.seekable,true);assert.ok(Math.abs(initial.duration-8)<.001)
 const windows=execFileSync('xdotool',['search','--onlyvisible','--pid',String(child.pid)],{encoding:'utf8'}).trim().split('\n').filter(Boolean);assert.equal(windows.length,1);const window=windows[0]
 await delay(250);capture('audio-paused-light',window)
 click(window,'audio-seek',.75);await wait(s=>s.state==='Paused'&&Math.abs(s.position-6)<.15)
 input('windowfocus',window);input('key','Right');await wait(s=>s.state==='Paused'&&Math.abs(s.position-8)<.15)
 input('key','Left');await wait(s=>s.state==='Paused'&&Math.abs(s.position-3)<.15)
 click(window,'audio-seek',.75);await wait(s=>s.state==='Paused'&&Math.abs(s.position-6)<.15)
 click(window,'audio-rate-2');await wait(s=>s.rate===2&&s.state==='Paused'&&Math.abs(s.position-6)<.15)
 click(window,'audio-mute');await wait(s=>s.muted)
 click(window,'audio-volume-down');await wait(s=>s.volume<.95)
 await delay(150);capture('audio-seek-controls-light',window)
 click(window,'proof-theme');await wait(s=>s.dark);await delay(150);capture('audio-seek-controls-dark',window)
 input('windowsize',window,1024,520);await delay(200)
 for(const key of ['audio-seek','audio-rate-2','audio-mute']){const [x,y,w,h]=json('geometry')[key];assert.ok(x>=0&&y>=0&&x+w<=1025&&y+h<=521,`${key} clipped in short window`)}
 capture('audio-short-dark',window)
 click(window,'proof-save');await wait(s=>s.saved);assert.deepEqual(fs.readFileSync(path.join(out,'saved-original.wav')),wav)
 click(window,'proof-retire');await wait(s=>s.closed);assert.deepEqual(calls,[]);assert.ok(reads.some(r=>r.route==='/api/fs/download'))
 fs.writeFileSync(path.join(out,'requests.json'),JSON.stringify(reads,null,2)+'\n')
 console.log(JSON.stringify({native_rune:true,scope:true,metadata:true,paused_seek:true,keyboard_seek:true,short_window:true,pitch_rate_control:true,mute:true,volume:true,themes:true,original_byte_exact:true,owner_retired:true,physical_audio_used:false,agent_rpc_calls:calls.length,reads:reads.length,screenshots:out}))
}finally{if(child.exitCode==null)child.kill('SIGTERM');for(const socket of sockets)socket.terminate();await new Promise(resolve=>http.close(resolve));await new Promise(resolve=>ws.close(resolve));fs.rmSync(home,{recursive:true,force:true})}
