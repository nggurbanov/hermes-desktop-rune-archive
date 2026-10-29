#!/usr/bin/env node
/** Native original Bot Mode smoke against a synthetic parent/gateway only. */
import fs from 'node:fs'
import path from 'node:path'
import zlib from 'node:zlib'
import { spawn, execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
const rune = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const args=process.argv.slice(2), option=name=>{const n=args.indexOf(name);return n<0?null:args[n+1]}
if (!process.env.DISPLAY && process.platform === 'linux') throw new Error('Use an isolated native test display, such as xvfb-run -a')
const snapshot=option('--snapshot'), mode=args.includes('--petdex')?'petdex':args.includes('--group-dialog')?'group':null
const assets=JSON.parse(zlib.gunzipSync(fs.readFileSync(path.join(rune,'assets/plugin-host/assets.json.gz'))))
const profiles=[{name:'research',title:'Research',display_name:'Research',description:'Synthetic researcher',ui_meta:{}},{name:'muse',title:'Muse',display_name:'Muse',description:'Synthetic creative partner',ui_meta:{}}]
const owner={profile:'research',target_profile:'research',profiles,connection_id:'native:synthetic',session:null,runtime_session:null,busy:false}
const html=`<!doctype html><meta charset=utf-8><meta http-equiv=Content-Security-Policy content="default-src 'self' data: blob:; script-src 'self' 'unsafe-inline' blob:; style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; object-src 'none'"><link rel=stylesheet href=host.css><div id=root></div><script type=module src=host.js></script><script>window.addEventListener('hermes-host-message',e=>{if(e.detail.type==='inspect-bots')window.ipc.postMessage(JSON.stringify({type:'bots-inspected',text:document.getElementById('plugin-content').textContent,buttons:[...document.querySelectorAll('button')].map(b=>b.getAttribute('aria-label')||b.title||b.textContent)}));if(e.detail.type==='open-create'){const b=[...document.querySelectorAll('button')].find(b=>b.getAttribute('aria-label')==='New bot or group chat');b?.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));setTimeout(()=>window.ipc.postMessage(JSON.stringify({type:'create-menu',items:[...document.querySelectorAll('[role=menuitem]')].map(b=>b.textContent)})),150);}if(e.detail.type==='pick-create'){const b=[...document.querySelectorAll('[role=menuitem]')].find(b=>b.textContent===e.detail.label);b?.click();setTimeout(()=>window.ipc.postMessage(JSON.stringify({type:'create-dialog',text:document.querySelector('[role=dialog]')?.textContent,buttons:[...document.querySelectorAll('[role=dialog] button')].map(b=>b.textContent)})),450);}if(e.detail.type==='open-petdex'){[...document.querySelectorAll('[role=dialog] button')].find(b=>b.textContent==='Pet')?.click();setTimeout(()=>window.ipc.postMessage(JSON.stringify({type:'petdex-dialog',text:document.querySelector('[role=dialog]')?.textContent,images:[...document.querySelectorAll('[role=dialog] img')].map(image=>({width:image.naturalWidth,complete:image.complete}))})),550);}})</script>`
const child=spawn(option('--binary')??path.join(rune,'target/debug/hermes-rune'),['--native-web-host'],{env:{...process.env,WEBKIT_DISABLE_COMPOSITING_MODE:'1'},stdio:['pipe','pipe','pipe']})
const send=value=>child.stdin.write(JSON.stringify(value)+'\n')
const gateways=new Map();let gatewayCount=0
let output='',stderr='',failure,rendered=false,readRoster=false,closing=false,petGallery=false,petThumbs=0
const require=(condition,message)=>{if(!condition)throw new Error(message)}
function thumbnail(slug) {
  const rows=Buffer.alloc(40*(40*4+1)), color=slug.endsWith('owl')?[97,193,180]:[225,155,95]
  for(let y=0;y<40;y++)for(let x=0;x<40;x++){const n=y*161+1+x*4,inside=(x-20)**2+(y-20)**2<330,eye=y>=15&&y<20&&(x>=11&&x<16||x>=24&&x<29);rows.set(eye?[25,30,35,255]:[...color,inside?255:0],n)}
  const crc=bytes=>{let crc=0xffffffff;for(const byte of bytes){crc^=byte;for(let n=0;n<8;n++)crc=(crc>>>1)^((crc&1)?0xedb88320:0)}return(crc^0xffffffff)>>>0}
  const chunk=(name,bytes)=>{const tag=Buffer.from(name),size=Buffer.alloc(4),check=Buffer.alloc(4);size.writeUInt32BE(bytes.length);check.writeUInt32BE(crc(Buffer.concat([tag,bytes])));return Buffer.concat([size,tag,bytes,check])}
  const header=Buffer.alloc(13);header.writeUInt32BE(40,0);header.writeUInt32BE(40,4);header[8]=8;header[9]=6
  return 'data:image/png;base64,'+Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',header),chunk('IDAT',zlib.deflateSync(rows)),chunk('IEND',Buffer.alloc(0))]).toString('base64')
}
function answer(id,value){send({type:'plugin-response',id,value})}
function capture(){if(!snapshot)return;execFileSync('/usr/bin/python3',['-c',`import gi,os,sys;gi.require_version('Gdk','3.0');from gi.repository import Gdk;d=Gdk.Display.open(os.environ['DISPLAY']);w=d.get_default_screen().get_root_window();Gdk.pixbuf_get_from_window(w,0,0,w.get_width(),w.get_height()).savev(sys.argv[1],'png',[],[])`,path.resolve(snapshot)],{env:process.env,stdio:'pipe'})}
function handle(event){
  if(args.includes('--debug'))console.error(JSON.stringify(event, (key,value)=> typeof value==='string' && value.length>1000 ? value.slice(0,300)+'… [synthetic payload omitted]' : value))
  if(event.type==='plugin-bootstrap-ready')send({type:'plugin-load',source:'builtin:hermes-bots',origin:'builtin:hermes-bots',owner})
  if(event.type==='plugin-loaded'){require(event.id==='hermes-bots','The bundled original Bots module did not register');setTimeout(()=>send({type:'inspect-bots'}),1800)}
  if(event.type==='plugin-rendered'){rendered=event.areas.includes('panes')}
  if(event.type==='plugin-request'){
    const {id,kind,payload}=event
    if(kind==='plugin-gateway-prepare'){require(payload.connectionId===owner.connection_id,'Gateway owner changed');const receipt='11111111-2222-4333-8444-'+String(++gatewayCount).padStart(12,'0');gateways.set(receipt,payload.profile);answer(id,{url:'ws://hermes-native-gateway.invalid/'+receipt})}
    else if(kind==='plugin-socket-open'){answer(id,{accepted:true});send({type:'plugin-socket',receipt:payload.receipt,event:'open',data:{protocol:''}})}
    else if(kind==='plugin-socket-send'){
      const frame=JSON.parse(payload.frame.text);answer(id,{accepted:true})
      let result={ok:true}
      if(frame.method==='groups.capabilities')result={driver:true,persistent_process:true,seatable_profiles:['research','muse']}
      else if(frame.method==='groups.list')result={rooms:[],next_offset:null}
      else if(frame.method==='pet.gallery'){petGallery=true;result={pets:[{slug:'synthetic-owl',displayName:'Synthetic owl',installed:true},{slug:'synthetic-fox',displayName:'Synthetic fox',curated:true}]}}
      else if(frame.method==='pet.thumb'){petThumbs++;result={ok:true,dataUri:thumbnail(frame.params.slug)}}
      else if(frame.method==='profiles.list'){readRoster=true;result={profiles,active:'research',bot_mode_protocol:true}}
      else if(frame.method==='client.capabilities')result={declines_not_shown:true}
      else if(frame.method.includes('poll')||frame.method.includes('list'))result={items:[],messages:[],deliveries:[],groups:[],sessions:[],agents:[]}
      if(frame.id!=null)send({type:'plugin-socket',receipt:payload.receipt,event:'message',data:{text:JSON.stringify({jsonrpc:'2.0',id:frame.id,result})}})
    }else if(kind==='plugin-socket-close'){answer(id,{accepted:true});send({type:'plugin-socket',receipt:payload.receipt,event:'close',data:{code:1000,clean:true,reason:''}})}
    else if(kind==='registry-metadata'||kind==='registry-roster')answer(id,{primary:owner.connection_id,current:{id:owner.connection_id,label:'Synthetic device',kind:'remote',mode:'remote',profiles},connections:[{id:owner.connection_id,label:'Synthetic device',kind:'remote',mode:'remote',attached:true,profiles}]})
    else if(kind==='source-descriptor')answer(id,{baseUrl:'http://native-host.invalid',token:'',authMode:'native',mode:'remote',profile:payload.profile,connectionId:payload.connectionId})
    else if(kind==='api'){
      const route=String(payload.path).split('?')[0]
      if(route==='/api/profiles')answer(id,{profiles,active:'research'})
      else if(route.includes('sessions'))answer(id,{sessions:[],total:0,has_more:false})
      else if(route.includes('config'))answer(id,{config:{}})
      else if(route.includes('ui-state'))answer(id,{ok:true,state:{},value:{},data:{},revision:1})
      else answer(id,{ok:true,items:[],groups:[],sources:[]})
    }else if(kind==='notify')answer(id,{ok:true})
    else throw new Error('Unsupported native bridge door requested by the original Bots plugin: '+kind)
  }
  if(event.type==='bots-inspected'){
    require(rendered&&readRoster,'The native Bots pane did not render or read its real transport')
    require(!/Spread syntax|Unhandled|TypeError|failed to/i.test(event.text),'The original Bots pane showed an error: '+event.text)
    require(event.text.includes('Research')&&event.text.includes('Muse'),'The original Bots roster did not display the two synthetic identities: '+event.text)
    require(event.buttons.some(text=>/group|room/i.test(text)),'The original group/room action is absent')
    if(mode)send({type:'open-create'});else{capture();closing=true;send({type:'close'})}
  }
  if(event.type==='create-menu'){const label=mode==='group'?'New group chat':'New bot';require(event.items.includes(label),'The original create menu is missing '+label);send({type:'pick-create',label})}
  if(event.type==='create-dialog'){require(typeof event.text==='string'&&!/Spread syntax|TypeError/.test(event.text),'The original create dialog did not mount');if(mode==='petdex'){require(event.buttons.includes('Pet'),'The original Pet tab is absent');send({type:'open-petdex'})}else{require(event.text.includes('New group chat')&&event.text.includes('Research')&&event.text.includes('Muse'),'The group member selection is absent: '+event.text);capture();closing=true;send({type:'close'})}}
  if(event.type==='petdex-dialog'){require(petGallery&&petThumbs===2&&event.images.filter(image=>image.width===40&&image.complete).length===2&&event.text.includes('Synthetic owl')&&event.text.includes('Synthetic fox'),'The original Petdex did not fetch/render its gallery and thumbnails: '+event.text);capture();closing=true;send({type:'close'})}
  if(event.type==='plugin-error'||event.type==='error')throw new Error(event.message)
}
const done=new Promise((resolve,reject)=>{
  const timeout=setTimeout(()=>{failure=new Error('The native bundled Bots smoke timed out');child.kill('SIGKILL')},30000)
  child.stdout.on('data',bytes=>{output+=bytes;let n;while((n=output.indexOf('\n'))>=0){const line=output.slice(0,n);output=output.slice(n+1);try{handle(JSON.parse(line))}catch(error){failure=error;child.kill('SIGKILL')}}})
  child.stderr.on('data',bytes=>stderr+=bytes)
  child.on('error',error=>failure=error)
  child.on('close',code=>{clearTimeout(timeout);if(failure||!closing||code!==0)reject(failure??new Error(stderr.slice(-1000)));else resolve()})
})
send({type:'document',document:{title:'Synthetic original Hermes Bots',html,assets,allow_media:false}})
await done
console.log(JSON.stringify({native_window:true,original_bundled_bots:true,synthetic_profiles:2,group_action:true,group_dialog:mode==='group',petdex:mode==='petdex',snapshot:snapshot?path.resolve(snapshot):null}))
