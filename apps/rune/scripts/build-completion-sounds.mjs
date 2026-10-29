// Build-time audio rendering only. OfflineAudioContext never connects to speakers.
// Sound synthesis is reused from the retained Hermes Desktop MIT source.
import fs from 'node:fs'
import path from 'node:path'
import crypto from 'node:crypto'
import { fileURLToPath } from 'node:url'
import { transform } from 'esbuild'
import { chromium } from 'playwright'

const app = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const repo = path.resolve(app, '../..')
const input = 'apps/desktop/src/lib/completion-sound.ts'
const original = fs.readFileSync(path.join(repo,input),'utf8')
const begin = original.indexOf('type OscType = ')
const end = original.indexOf('\nfunction playVariant(')
if (begin<0||end<=begin) throw new Error('The retained sound bank boundaries changed; review its pure synthesis source')
const pure = original.slice(begin,end)
const seed = 0x4845524d
const source = `${pure}\nexport async function renderPreset(id) {
 const sampleRate = 48000, length = sampleRate * 4
 const ac = new OfflineAudioContext(2, length, sampleRate)
 const master = ac.createGain(), tone = ac.createBiquadFilter()
 tone.type = 'lowpass'; tone.frequency.setValueAtTime(3800, 0); tone.Q.setValueAtTime(.32,0)
 master.gain.setValueAtTime(.48,0); master.connect(tone)
 const dry=ac.createGain();dry.gain.setValueAtTime(.88,0);tone.connect(dry);dry.connect(ac.destination)
 const reverb=makeReverb(ac),wet=ac.createGain();wet.gain.setValueAtTime(.34,0);tone.connect(reverb);reverb.connect(wet);wet.connect(ac.destination)
 const preset=COMPLETION_SOUND_VARIANTS.find(p=>p.id===id)
 if(!preset||!id)throw new Error('Select one of the fourteen original sound presets')
 preset.play(ac,master,.01)
 const buffer=await ac.startRendering(),channels=[buffer.getChannelData(0),buffer.getChannelData(1)]
 let last=0,peak=0
 for(let i=0;i<length;i++)for(let c=0;c<2;c++){const a=Math.abs(channels[c][i]);if(!Number.isFinite(a))throw new Error('Non-finite rendered sound');peak=Math.max(peak,a);if(a>1/65536)last=i}
 if(peak>=1)throw new Error('Clipping in original sound bank')
 const frames=Math.min(length,last+257),bytes=new Uint8Array(44+frames*4),view=new DataView(bytes.buffer)
 const str=(offset,value)=>{for(let i=0;i<value.length;i++)view.setUint8(offset+i,value.charCodeAt(i))}
 str(0,'RIFF');view.setUint32(4,bytes.length-8,true);str(8,'WAVE');str(12,'fmt ');view.setUint32(16,16,true);view.setUint16(20,1,true);view.setUint16(22,2,true);view.setUint32(24,sampleRate,true);view.setUint32(28,sampleRate*4,true);view.setUint16(32,4,true);view.setUint16(34,16,true);str(36,'data');view.setUint32(40,frames*4,true)
 for(let i=0;i<frames;i++)for(let c=0;c<2;c++){const x=channels[c][i];view.setInt16(44+(i*2+c)*2,Math.round(x<0?x*32768:x*32767),true)}
 let binary='';for(let offset=0;offset<bytes.length;offset+=8192)binary+=String.fromCharCode(...bytes.subarray(offset,offset+8192))
 return {id,name:preset.name,frames,sample_rate:sampleRate,channels:2,bits:16,peak,base64:btoa(binary)}
}`
const script = (await transform(source,{loader:'ts',format:'iife',globalName:'HermesOriginalSoundBank',target:'es2022'})).code
const browser = await chromium.launch({executablePath:process.argv[2]||process.env.CHROME_BIN||'/usr/bin/google-chrome',headless:true,args:['--mute-audio','--no-sandbox','--disable-background-networking']})
const output = path.join(app,'assets/completion-sounds')
fs.mkdirSync(output,{recursive:true})
const sha = value => crypto.createHash('sha256').update(value).digest('hex')
try {
 const page=await browser.newPage()
 await page.route('**/*',route=>route.abort())
 await page.evaluate(seed=>{let state=seed>>>0;Math.random=()=>{state^=state<<13;state^=state>>>17;state^=state<<5;return(state>>>0)/4294967296}},seed)
 await page.addScriptTag({content:script})
 const presets=[]
 for(let id=1;id<=14;id++){
  const result=await page.evaluate(id=>HermesOriginalSoundBank.renderPreset(id),id)
  const bytes=Buffer.from(result.base64,'base64'),file=`${String(id).padStart(2,'0')}.wav`
  fs.writeFileSync(path.join(output,file),bytes)
  delete result.base64
  presets.push({...result,file,bytes:bytes.length,sha256:sha(bytes)})
 }
 const baseline=JSON.parse(fs.readFileSync(path.join(repo,'desktop-baseline.json'),'utf8'))
 const manifest={version:1,source:{file:input,sha256:sha(original),pure_synthesis_sha256:sha(pure),repository:baseline.upstream.repository,baseline_commit:baseline.upstream.commit},generator:{file:'apps/rune/scripts/build-completion-sounds.mjs',sha256:sha(fs.readFileSync(fileURLToPath(import.meta.url))),browser:browser.version(),offline:true,random_seed:seed,encoding:'PCM signed 16-bit stereo WAV, silence trimmed after last half-LSB + 256 frames',signal_path:'original playVariant: gain .48, lowpass 3800Hz Q .32, dry .88, stereo 1.6s convolver wet .34, starts at 10ms'},presets}
 fs.writeFileSync(path.join(output,'manifest.json'),JSON.stringify(manifest,null,2)+'\n')
 console.log(`Rendered ${presets.length} original Hermes sound presets without an audio output device (${presets.reduce((n,p)=>n+p.bytes,0)} bytes)`)
} finally { await browser.close() }
