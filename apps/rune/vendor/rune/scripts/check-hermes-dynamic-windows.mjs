#!/usr/bin/env node
// Run inside a private Xvfb display. All windows/state are synthetic, without backend or audio.
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import {spawn, spawnSync, execFileSync} from 'node:child_process'
const [binary, output] = process.argv.slice(2)
assert.ok(binary && output && path.isAbsolute(output), 'Pass binary and absolute evidence directory')
fs.mkdirSync(output, {recursive:true})
const child=spawn(binary,[output],{stdio:['ignore','ignore','pipe']})
let stderr='', completed=false, screenshots=false, windowCount=0
child.stderr.on('data', b=>stderr+=b)
const ended = new Promise(resolve=>child.on('exit',(code,signal)=>resolve({code,signal})))
const deadline=Date.now()+90000
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms))
function inspection(){try{return JSON.parse(fs.readFileSync(path.join(output,'inspection.json'),'utf8'))}catch{return {}}}
function windows(){const r=spawnSync('xdotool',['search','--onlyvisible','--pid',String(child.pid)],{encoding:'utf8'});return r.status===0?r.stdout.trim().split('\n').filter(Boolean):[]}
function capture(){execFileSync('/usr/bin/python3',['-c',`import gi,os,sys;gi.require_version('Gdk','3.0');from gi.repository import Gdk;d=Gdk.Display.open(os.environ['DISPLAY']);r=d.get_default_screen().get_root_window();p=Gdk.pixbuf_get_from_window(r,0,0,r.get_width(),r.get_height());p.savev(sys.argv[1],'png',[],[])`,path.join(output,'two-windows.png')])}
try {
 while(Date.now()<deadline){
  const s=inspection()
  if(s.phase==='two-native-windows'&&!screenshots){windowCount=windows().length;assert.equal(windowCount,2,'Live duplicate key must not create a third native window');capture();screenshots=true}
  if(s.phase==='complete'){completed=true;break}
  if(child.exitCode!==null)throw new Error(`Native proof exited early: ${stderr.slice(-6000)}`)
  await delay(25)
 }
 assert.ok(completed,`Native proof did not complete: ${JSON.stringify(inspection())}\n${stderr.slice(-6000)}`)
 const exit=await ended
 assert.deepEqual(exit,{code:0,signal:null},stderr.slice(-6000))
 const s=inspection()
 assert.ok(s.cycles>=80&&s.vetoes===1&&s.root_close_hooks===1&&s.initial_geometry&&s.duplicate_rejected&&s.parent_survived_veto&&s.peer_survived_parent)
 assert.ok(screenshots)
 const result={...s,private_display:true,non_send_state:true,stale_route_isolation:true,native_window_count:windowCount,global_quit:true}
 fs.writeFileSync(path.join(output,'result.json'),JSON.stringify(result,null,2)+'\n')
 fs.writeFileSync(path.join(output,'stderr.log'),stderr)
 console.log(JSON.stringify(result))
}finally{if(child.exitCode===null&&child.signalCode===null){child.kill('SIGTERM');await Promise.race([ended,delay(2000)]);if(child.exitCode===null&&child.signalCode===null){child.kill('SIGKILL');await ended}}}
