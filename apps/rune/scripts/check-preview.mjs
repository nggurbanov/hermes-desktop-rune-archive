#!/usr/bin/env node
/** Real Rune input acceptance against a private, mutation-free file peer. */
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import {createServer} from 'node:http'
import {spawn,execFileSync} from 'node:child_process'
import {deflateSync} from 'node:zlib'
import {fileURLToPath} from 'node:url'
import {WebSocketServer} from 'ws'
const rune=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const args=process.argv.slice(2),option=name=>{const i=args.indexOf(name);return i>=0?args[i+1]:null}
const binary=option('--binary')??path.join(rune,'target/debug/hermes-rune')
const out=path.resolve(option('--out')??'/tmp/hermes-preview-proof')
fs.mkdirSync(out,{recursive:true})
const home=fs.mkdtempSync(path.join(os.tmpdir(),'hermes-preview-home-'))
function crc(bytes){let n=0xffffffff;for(const byte of bytes){n^=byte;for(let i=0;i<8;i++)n=(n>>>1)^((n&1)?0xedb88320:0)}return (n^0xffffffff)>>>0}
function chunk(kind,bytes){const type=Buffer.from(kind),length=Buffer.alloc(4),checksum=Buffer.alloc(4);length.writeUInt32BE(bytes.length);checksum.writeUInt32BE(crc(Buffer.concat([type,bytes])));return Buffer.concat([length,type,bytes,checksum])}
function fixture(){const width=1280,height=720,header=Buffer.alloc(13),data=Buffer.alloc(height*(width*4+1));header.writeUInt32BE(width);header.writeUInt32BE(height,4);header[8]=8;header[9]=6;for(let y=0;y<height;y++){for(let x=0;x<width;x++){const i=y*(width*4+1)+1+x*4,cell=(Math.floor(x/160)+Math.floor(y/120))%2;let color=cell?[99,149,157]:[223,218,196];if(x<12||y<12||x>width-13||y>height-13)color=[50,79,86];if(x>450&&x<830&&y>250&&y<470)color=[210,143,91];data.set([...color,255],i)}}return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',header),chunk('IDAT',deflateSync(data)),chunk('IEND',Buffer.alloc(0))])}
const png=fixture(),reads=[],calls=[],sockets=new Set()
const http=createServer((request,response)=>{try{const url=new URL(request.url,'http://localhost');assert.equal(request.method,'GET');assert.equal(url.pathname,'/api/fs/download');assert.equal(url.searchParams.get('profile'),'research');assert.equal(url.searchParams.get('session_id'),'preview-session');assert.equal(url.searchParams.get('path'),'/fixture/preview.png');assert.equal(url.searchParams.get('token'),'preview-fixture');reads.push(url.pathname);response.writeHead(200,{'content-type':'image/png','content-length':png.length,'etag':'"preview-fixture"','accept-ranges':'bytes'});response.end(png)}catch(error){response.writeHead(400);response.end();failure=error}})
const ws=new WebSocketServer({server:http,path:'/api/ws'})
ws.on('connection',socket=>{sockets.add(socket);socket.on('message',bytes=>{calls.push(JSON.parse(bytes));failure=new Error('Preview created an unexpected agent RPC')});socket.on('close',()=>sockets.delete(socket))})
await new Promise(resolve=>http.listen(0,'127.0.0.1',resolve))
let failure,stderr=''
const child=spawn(binary,['--preview-proof','--gateway',`http://127.0.0.1:${http.address().port}`,'--proof-output',out],{env:{...process.env,HOME:home,USERPROFILE:home,XDG_DATA_HOME:home,HERMES_HOME:path.join(home,'unused-agent'),HERMES_GATEWAY_TOKEN:''},stdio:['ignore','ignore','pipe']})
child.stderr.on('data',bytes=>stderr+=bytes)
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms))
function inspection(){try{return JSON.parse(fs.readFileSync(path.join(out,'inspection.json'),'utf8'))}catch{return {}}}
async function wait(check){const deadline=Date.now()+12000;while(Date.now()<deadline){if(failure)throw failure;if(child.exitCode!=null)throw new Error(stderr.slice(-2000));const value=inspection();if(check(value))return value;await delay(40)}throw new Error(`Native preview did not settle: ${JSON.stringify(inspection())}\n${stderr.slice(-800)}`)}
function input(...args){execFileSync('xdotool',args.map(String),{stdio:'pipe'})}
function capture(name,window){const target=path.join(out,`${name}.png`),geometry=Object.fromEntries(execFileSync('xdotool',['getwindowgeometry','--shell',String(window)],{encoding:'utf8'}).trim().split('\n').map(line=>line.split('='))),bounds=['X','Y','WIDTH','HEIGHT'].map(k=>Number(geometry[k]));execFileSync('/usr/bin/python3',['-c',`import gi,os,sys;gi.require_version('Gdk','3.0');from gi.repository import Gdk;display=Gdk.Display.open(os.environ['DISPLAY']);root=display.get_default_screen().get_root_window();x,y,w,h=map(int,sys.argv[2:6]);image=Gdk.pixbuf_get_from_window(root,x,y,w,h);image.savev(sys.argv[1],'png',[],[])`,target,...bounds.map(String)],{stdio:'pipe'});return target}
try{
 const initial=await wait(s=>s.ready&&s.viewport?.[0]>0)
 assert.equal(initial.error,null);assert.deepEqual(initial.image,[1280,720])
 const windows=execFileSync('xdotool',['search','--onlyvisible','--pid',String(child.pid)],{encoding:'utf8'}).trim().split('\n').filter(Boolean);assert.equal(windows.length,1);const window=windows[0]
 await delay(200);capture('preview-image-fit-light',window)
 input('mousemove','--window',window,480,340,'click','4')
 const zoomed=await wait(s=>s.scale>initial.scale+0.01);assert.ok(zoomed.scale<=8)
 input('keydown','ctrl','click','4','keyup','ctrl')
 const pinched=await wait(s=>s.scale>zoomed.scale+0.01)
 input('mousemove','--window',window,480,340,'mousedown','1','mousemove','--window',window,550,390,'mouseup','1')
 const panned=await wait(s=>Math.abs(s.pan?.[0]-pinched.pan[0])>30)
 await delay(120);capture('preview-image-zoom-pan-light',window)
 input('mousemove','--window',window,35,18,'click','1');await wait(s=>s.dark);await delay(160);capture('preview-image-zoom-pan-dark',window)
 input('mousemove','--window',window,155,18,'click','1');await wait(s=>s.saved)
 assert.deepEqual(fs.readFileSync(path.join(out,'saved-original.png')),png,'Save original changed image bytes')
 input('mousemove','--window',window,285,18,'click','1');await wait(s=>s.closed)
 assert.equal(reads.length,1);assert.deepEqual(calls,[])
 console.log(JSON.stringify({native_rune:true,scope:true,wheel:true,ctrl_wheel_pinch:true,drag_pan:true,themes:true,original_byte_exact:true,owner_retired:true,agent_rpc_calls:calls.length,image_reads:reads.length,screenshots:out}))
}finally{
 if(child.exitCode==null)child.kill('SIGTERM')
 for(const socket of sockets)socket.terminate()
 await new Promise(resolve=>http.close(resolve))
 await new Promise(resolve=>ws.close(resolve))
 fs.rmSync(home,{recursive:true,force:true})
}
