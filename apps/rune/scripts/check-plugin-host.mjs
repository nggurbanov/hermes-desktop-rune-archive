#!/usr/bin/env node
/** Real Wry/React smoke; all backend replies and plugin inputs are synthetic. */
import fs from 'node:fs'
import path from 'node:path'
import zlib from 'node:zlib'
import { fileURLToPath } from 'node:url'
import { spawn, execFileSync } from 'node:child_process'
const rune = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const args = process.argv.slice(2)
const option = name => { const index = args.indexOf(name); return index >= 0 ? args[index + 1] : null }
const binary = option('--binary') ?? path.join(rune, 'target/debug/hermes-rune')
const snapshot = option('--snapshot')
const canonical = args.includes('--canonical')
if (!process.env.DISPLAY && process.platform === 'linux') throw new Error('Run this native smoke under xvfb-run -a or an isolated test display')
const assets = JSON.parse(zlib.gunzipSync(fs.readFileSync(path.join(rune, 'assets/plugin-host/assets.json.gz'))))
const source = `import * as React from 'react'; import {Button,host} from '@hermes/plugin-sdk';
let storage;let upload;async function protocolProof(){const created=await host.request('session.create',{profile:'research',title:'Synthetic protocol proof'});const result=await host.request('session.title',{session_id:created.session_id,title:'Synthetic rename'});window.ipc.postMessage(JSON.stringify({type:'proof-canonical',created,result}));}
function Proof(){const[n,setN]=React.useState(0);return React.createElement('section',null,React.createElement('h1',null,'Real Hermes SDK renderer'),React.createElement('p',null,'Synthetic plugin · research profile'),React.createElement(Button,{id:'proof-button',onClick:()=>{setN(v=>v+1);storage.set('clicks',n+1);${canonical ? 'protocolProof();' : ''}host.request('native.proof',{profile:'research'}).then(value=>window.ipc.postMessage(JSON.stringify({type:'proof-rpc',value})));upload().then(value=>window.ipc.postMessage(JSON.stringify({type:'proof-upload',value})));}},'Clicks '+n));}
export default{id:'native-sdk-proof',name:'Native SDK proof',version:'1.0.0',register(ctx){storage=ctx.storage;upload=()=>ctx.rest('/upload',{method:'POST',upload:{filename:'proof.bin',bytes:new Uint8Array([0,255,128]).buffer}});ctx.socket('/events',value=>window.ipc.postMessage(JSON.stringify({type:'proof-socket',value})));ctx.register({id:'proof',area:'primarySidebar',title:'Proof',render:()=>React.createElement(Proof)});ctx.onEvent('proof.event',event=>window.ipc.postMessage(JSON.stringify({type:'proof-event',event})));ctx.onDispose(()=>window.ipc.postMessage(JSON.stringify({type:'proof-disposed'})));}};`
const html = `<!doctype html><meta charset=utf-8><meta http-equiv=Content-Security-Policy content="default-src 'self' blob: data:; script-src 'self' blob: 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; object-src 'none'"><link rel=stylesheet href=host.css><div id=root></div><script type=module src=host.js></script><script>window.addEventListener('hermes-host-message',e=>{if(e.detail.type==='proof-click'){document.getElementById('proof-button')?.click();requestAnimationFrame(()=>window.ipc.postMessage(JSON.stringify({type:'proof-after-click',text:document.getElementById('root').textContent})));}})</script>`
const child = spawn(binary, ['--native-web-host'], { env: { ...process.env, WEBKIT_DISABLE_COMPOSITING_MODE: '1' }, stdio: ['pipe', 'pipe', 'pipe'] })
const send = value => child.stdin.write(JSON.stringify(value) + '\n')
let output = '', stderr = '', failure, rendered = false, clicked = false, rpc = false, uploaded = false, stream = false, stored = false, disposed = false, events = 0, closing = false, canonicalDone = !canonical
let transfer = null, chunked = false
const socketReceipt = 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee'
const gatewayReceipt = '11111111-2222-4333-8444-555555555555'
const require = (condition, message) => { if (!condition) throw new Error(message) }
function capture() {
  execFileSync('/usr/bin/python3', ['-c', `import gi,os,sys;gi.require_version('Gdk','3.0');from gi.repository import Gdk;display=Gdk.Display.open(os.environ['DISPLAY']);window=display.get_default_screen().get_root_window();image=Gdk.pixbuf_get_from_window(window,0,0,window.get_width(),window.get_height());image.savev(sys.argv[1],'png',[],[])`, path.resolve(snapshot)], { env: process.env, stdio: 'pipe' })
}
function finishIfReady() {
  if (!rendered || !clicked || !rpc || !uploaded || !stream || !stored || !canonicalDone || events !== 2 || closing) return
  closing = true
  if (snapshot) capture()
  send({ type: 'close' })
}
function onEvent(event) {
  if (args.includes('--debug')) console.error(JSON.stringify(event))
  switch (event.type) {
    case 'plugin-bootstrap-ready':
      send({ type: 'plugin-load', source, origin: '/synthetic/proof/plugin.js', owner: { profile: 'research', target_profile: 'research', profiles: [{ name: 'research' }], connection_id: 'native:synthetic', busy: false } })
      break
    case 'plugin-rendered':
      require(event.text.includes('Real Hermes SDK renderer') && event.text.includes('Clicks 0'), 'The original SDK components did not paint')
      rendered = true
      for (const seq of [7, 7, 6, 8]) send({ type: 'gateway-event', event: { type: 'proof.event', profile: 'research', connectionId: 'native:synthetic', session_id: 'synthetic', seq, payload: { text: 'Synthetic event' } } })
      send({ type: 'proof-click' })
      break
    case 'plugin-request':
      if (event.kind === 'rpc') {
        require(event.payload.method === 'native.proof' && event.payload.params.profile === 'research', 'The RPC request changed its owner')
        send({ type: 'plugin-response', id: event.id, value: { method: event.payload.method, profile: event.payload.params.profile } })
      } else if (event.kind === 'upload-start') {
        require(event.payload.total_bytes === 3 && event.payload.filename === 'proof.bin' && event.payload.request.profile === 'research' && event.payload.request.path === '/api/plugins/native-sdk-proof/upload', 'The upload start lost its bytes, namespace or owner')
        transfer = event.payload.transfer_id
        send({ type: 'plugin-response', id: event.id, value: { accepted: true } })
      } else if (event.kind === 'upload-chunk') {
        require(event.payload.transfer_id === transfer && event.payload.index === 0 && Buffer.from(event.payload.bytes_base64, 'base64').equals(Buffer.from([0, 255, 128])), 'The chunked upload changed binary bytes')
        chunked = true
        send({ type: 'plugin-response', id: event.id, value: { received_bytes: 3 } })
      } else if (event.kind === 'api') {
        require(chunked && event.payload.upload.transfer_id === transfer && event.payload.profile === 'research' && event.payload.path === '/api/plugins/native-sdk-proof/upload', 'The completed upload changed its owner or path')
        send({ type: 'plugin-response', id: event.id, value: { ok: true, received_bytes: 3, profile: 'research' } })
      } else if (event.kind === 'source-descriptor') {
        require(event.payload.connectionId === 'native:synthetic' && event.payload.profile === 'research', 'The source descriptor changed its owner')
        send({ type: 'plugin-response', id:event.id,value:{baseUrl:'http://native-host.invalid',token:'',authMode:'native',mode:'remote',profile:'research',connectionId:'native:synthetic'}})
      } else if (event.kind === 'plugin-gateway-prepare') {
        require(event.payload.profile === 'research' && event.payload.connectionId === 'native:synthetic', 'The retained gateway changed its owner')
        send({ type: 'plugin-response', id: event.id, value: { url: 'ws://hermes-native-gateway.invalid/' + gatewayReceipt + (canonical ? '?native_dial=opaque&ticket=parent-owned' : '') } })
      } else if (event.kind === 'plugin-socket-prepare') {
        require(event.payload.profile === 'research' && event.payload.pluginId === 'native-sdk-proof' && event.payload.path === '/events', 'The plugin stream escaped its namespace or owner')
        send({ type: 'plugin-response', id: event.id, value: { url: 'ws://hermes-native-plugin.invalid/' + socketReceipt } })
      } else if (event.kind === 'plugin-socket-open') {
        require([socketReceipt, gatewayReceipt].includes(event.payload.receipt), 'The plugin stream receipt changed')
        send({ type: 'plugin-response', id: event.id, value: { accepted: true } })
        send({ type: 'plugin-socket', receipt: event.payload.receipt, event: 'open', data: { protocol: event.payload.receipt === gatewayReceipt && canonical ? 'hermes-gateway-v1' : '' } })
        if (event.payload.receipt === socketReceipt) send({ type: 'plugin-socket', receipt: socketReceipt, event: 'message', data: { text: JSON.stringify({ item: 'synthetic namespace stream' }) } })
      } else if (event.kind === 'plugin-socket-send') {
        require(event.payload.receipt === gatewayReceipt, 'Unexpected stream send')
        const frame = JSON.parse(event.payload.frame.text)
        require(['native.proof', 'client.capabilities', 'ping', ...(canonical ? ['session.create', 'session.mutate'] : [])].includes(frame.method), 'Unexpected gateway wire method: ' + frame.method)
        if (frame.method === 'native.proof') require(frame.params.profile === 'research', 'The original client lost its RPC owner')
        send({ type: 'plugin-response', id: event.id, value: { accepted: true } })
        let result = frame.method === 'native.proof' ? { method: frame.method, profile: frame.params.profile } : { declines_not_shown: true }
        if (frame.method === 'session.create') {
          require(frame.params.profile === 'research' && typeof frame.params.request_id === 'string' && frame.params.source === 'gui', 'The original canonical create identity was not prepared')
          result = { session_id: 'synthetic-session', stored_session_id: 'synthetic-stored', revision: 4, execution_generation: 2, info: { profile_name: 'research' }, prompts: [], messages: [], running: false }
        }
        if (frame.method === 'session.mutate') {
          require(frame.params.profile === 'research' && frame.params.operation === 'rename' && frame.params.expected_revision === 4 && frame.params.payload.title === 'Synthetic rename' && typeof frame.params.request_id === 'string', 'The original canonical CAS mutation was not prepared')
          result = { session_id: 'synthetic-session', revision: 5, status: 'applied' }
        }
        if (frame.id != null) send({ type: 'plugin-socket', receipt: gatewayReceipt, event: 'message', data: { text: JSON.stringify({ jsonrpc: '2.0', id: frame.id, result }) } })
      } else if (event.kind === 'plugin-socket-close') {
        send({ type: 'plugin-response', id: event.id, value: { accepted: true } })
        send({ type: 'plugin-socket', receipt: event.payload.receipt, event: 'close', data: { code: 1000, reason: '', clean: true } })
      } else throw new Error('Unexpected backend operation in a synthetic probe: ' + event.kind)
      break
    case 'proof-canonical': require(event.created.info.stored_session_id === 'synthetic-stored' && event.result.ok === true && event.result.revision === 5, 'The original canonical snapshot/mutation projection failed'); canonicalDone = true; break
    case 'proof-upload': require(event.value.profile === 'research' && event.value.received_bytes === 3, 'The multipart result changed'); uploaded = true; break
    case 'proof-socket': require(event.value.item === 'synthetic namespace stream', 'The namespace stream did not deliver its JSON'); stream = true; break
    case 'proof-after-click':
      require(event.text.includes('Clicks 1'), 'The SDK Button did not update its React hook')
      clicked = true
      break
    case 'proof-rpc':
      require(event.value.profile === 'research' && event.value.method === 'native.proof', 'The RPC bridge changed its owner or response')
      rpc = true
      break
    case 'plugin-storage':
      if (event.values['hermes.plugin.native-sdk-proof.clicks'] === '1') stored = true
      break
    case 'proof-event':
      require(event.event.profile === 'research', 'Event ownership was lost')
      events++
      require(events <= 2, 'A duplicate or older event reached the SDK listener')
      break
    case 'proof-disposed': disposed = true; break
    case 'plugin-error': case 'error': throw new Error(event.message)
  }
  finishIfReady()
}
const completed = new Promise((resolve, reject) => {
  const timeout = setTimeout(() => { failure = new Error('The native plugin did not render and complete its bridge checks'); child.kill('SIGKILL') }, 30000)
  child.stdout.on('data', bytes => {
    output += bytes
    let newline
    while ((newline = output.indexOf('\n')) >= 0) {
      const line = output.slice(0, newline); output = output.slice(newline + 1)
      try { onEvent(JSON.parse(line)) } catch (error) { failure = error; child.kill('SIGKILL') }
    }
  })
  child.stderr.on('data', bytes => { stderr += bytes })
  child.on('error', error => { failure = error })
  child.on('close', code => {
    clearTimeout(timeout)
    if (failure || !closing || !disposed || code !== 0) reject(failure ?? new Error(`The native plugin host did not close cleanly: ${stderr.slice(-1000)}`))
    else resolve()
  })
})
send({ type: 'document', document: { title: 'Synthetic Hermes SDK proof', html, assets, allow_media: false } })
await completed
console.log(JSON.stringify({ native_window: true, original_sdk: true, canonical_adapter: canonical, react_hook: true, rpc_owner: 'research', deduplicated_events: events, storage: true, chunked_upload: true, namespace_stream: true, disposed: true, snapshot: snapshot ? path.resolve(snapshot) : null }))
