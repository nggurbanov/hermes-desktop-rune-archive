#!/usr/bin/env node
// Real production Artifact gallery/detail through the asynchronous untrusted-image decoder.
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import {spawnSync} from 'node:child_process'
import {fileURLToPath} from 'node:url'
const app=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const args=process.argv.slice(2),option=name=>{const i=args.indexOf(name);return i>=0?args[i+1]:null}
const binary=option('--binary')??path.join(app,'target/debug/hermes-rune')
const out=path.resolve(option('--out')??'/tmp/hermes-safe-images-proof')
fs.mkdirSync(out,{recursive:true})
const home=fs.mkdtempSync(path.join(os.tmpdir(),'hermes-safe-images-home-'))
const env={...process.env,HOME:home,USERPROFILE:home,XDG_DATA_HOME:home,HERMES_HOME:path.join(home,'unused-agent'),HERMES_GATEWAY_TOKEN:''}
const has=(tree,text)=>tree.elements.some(e=>!e.exiting&&e.text===text)
function render(name,theme,events=[]){
 const treePath=path.join(out,`${name}.json`),script=path.join(out,`${name}.script.json`)
 fs.writeFileSync(script,JSON.stringify({events}))
 const result=spawnSync(binary,['--scene','safe-images','--theme',theme,'--size','1440x1100','--scale','1','--script',script,'--at','5','--tree',treePath,'--snap',path.join(out,`${name}.png`)],{env,cwd:app,encoding:'utf8',timeout:45000,maxBuffer:1024*1024})
 assert.equal(result.status,0,`${name}: ${result.error??result.stderr}`)
 const tree=JSON.parse(fs.readFileSync(treePath,'utf8'))
 assert.ok(has(tree,'Fixture · 24 images ready'),`${name}: asynchronous 24-image gallery did not finish`)
 for(let i=0;i<24;i++)assert.ok(tree.elements.some(e=>!e.exiting&&e.key===`artifact-select-${i}`),`${name}: missing artifact ${i}`)
 assert.ok(!has(tree,'Loading image…')&&!has(tree,'Retry image')&&!has(tree,'Retry preview'),`${name}: bounded image decoder failed`)
 assert.ok(!tree.elements.some(e=>!e.exiting&&e.role==='alert'),`${name}: native runtime alert`)
 return tree
}
try{
 render('safe-image-gallery-light','light')
 render('safe-image-gallery-dark','dark')
 const detail=render('safe-image-detail-light','light',[{type:'tap',key:'artifact-select-0',t:2}])
 assert.ok(has(detail,'Copy location')&&has(detail,'Save file…')&&has(detail,'100%'),'Selection did not open the real image detail controls')
 assert.ok(!has(detail,'Reading this artifact through its owning agent…'),'Image detail is still pending')
 assert.ok(detail.elements.some(e=>!e.exiting&&e.key==='artifact-native-preview'),'Shared native image viewer action disappeared')
 console.log(JSON.stringify({production_artifact_view:true,gallery_images:24,async_ready:true,mislabeled_svg:true,themes:true,detail:true,original_download_action:true,screenshots:out}))
}finally{fs.rmSync(home,{recursive:true,force:true})}
