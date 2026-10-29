"""Offline synthetic codec fixtures. ffmpeg generates assets; production never invokes it."""
from pathlib import Path
import hashlib,json,math,struct,subprocess,tempfile,wave
root=Path(__file__).resolve().parents[1]
out=root/'assets/test-audio';out.mkdir(parents=True,exist_ok=True)
with tempfile.TemporaryDirectory(prefix='hermes-codec-fixtures-') as tmp:
 source=Path(tmp)/'tone.wav'
 with wave.open(str(source),'wb') as writer:
  writer.setparams((2,2,48000,0,'NONE','not compressed'))
  writer.writeframes(b''.join(struct.pack('<hh',int(math.sin(i/48000*math.tau*220)*.2*32767),int(math.sin(i/48000*math.tau*330)*.15*32767)) for i in range(96000)))
 records=[]
 for name,codec in [('tone.mp3','libmp3lame'),('tone.ogg','libvorbis'),('tone.flac','flac'),('tone.m4a','aac')]:
  command=['ffmpeg','-nostdin','-y','-i',str(source),'-map_metadata','-1','-fflags','+bitexact','-flags:a','+bitexact','-c:a',codec,str(out/name)]
  subprocess.run(command,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  data=(out/name).read_bytes();records.append({'file':name,'codec':codec,'sha256':hashlib.sha256(data).hexdigest(),'bytes':len(data),'command':['ffmpeg',*command[1:4],'tone.wav',*command[5:-1],name]})
 version=subprocess.check_output(['ffmpeg','-version'],text=True).splitlines()[0]
 (out/'manifest.json').write_text(json.dumps({'source':'Generated 2-second 48kHz stereo sine: left220Hz amplitude.2;right330Hz amplitude.15; signed16PCM. No recordings.','generator':'apps/rune/scripts/build-audio-test-fixtures.py','generator_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'ffmpeg':version,'fixtures':records},indent=2)+'\n')
