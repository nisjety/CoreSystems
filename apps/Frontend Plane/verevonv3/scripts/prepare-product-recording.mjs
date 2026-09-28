import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, realpathSync, statSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { productCaptureSettings } from '../tests/e2e/product-capture-policy.ts';

const args = {};
for (let index=2; index<process.argv.length; index+=2) {
  const key=process.argv[index];
  if (!['--capture','--plan','--output'].includes(key) || !process.argv[index+1] || args[key]) throw new Error('Use --capture PRIVATE_CAPTURE_DIRECTORY --plan REVIEWED_EDIT_PLAN.json --output NEW_PRIVATE_DIRECTORY');
  args[key]=process.argv[index+1];
}
if (!args['--capture'] || !args['--plan'] || !args['--output']) throw new Error('Capture, edit plan and private output are required');
const frontend=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const output=productCaptureSettings('rehearsal',{tasks:[]},args['--output'],resolve(frontend,'../../..')).outputDir;
if (existsSync(output)) throw new Error('Keep previous exports; choose a new output directory');
let ancestor=output;
const remainder=[];
while (!existsSync(ancestor)) {
  const parent=dirname(ancestor);
  if (parent===ancestor) throw new Error('Recording output has no accessible filesystem root');
  remainder.unshift(basename(ancestor)); ancestor=parent;
}
productCaptureSettings('rehearsal',{tasks:[]},join(realpathSync(ancestor),...remainder),realpathSync(resolve(frontend,'../../..')));
const captureDir=resolve(args['--capture']);
const run=JSON.parse(readFileSync(captureDir+'.run.json','utf8'));
if (run.exitCode!==0) throw new Error('This capture failed acceptance; preserve it as evidence, not a successful film');
const walk=directory=>readdirSync(directory,{withFileTypes:true}).flatMap(entry=>entry.isDirectory()?walk(join(directory,entry.name)):[join(directory,entry.name)]);
const files=walk(captureDir);
const videos=files.filter(path=>path.endsWith('/video.webm') || path.endsWith('\\video.webm'));
const metadata=files.filter(path=>path.endsWith('/capture.json') || path.endsWith('\\capture.json'));
if (videos.length!==1 || metadata.length!==1) throw new Error('Expected one raw video and one capture metadata file');
const raw=videos[0];
const capture=JSON.parse(readFileSync(metadata[0],'utf8'));
if (!capture.accepted || capture.model!=='gpt-5.6-terra' || capture.provider!=='openai-codex-subscription') throw new Error('An accepted Terra subscription capture is required');
if (!['01-kundesvar','02-salgsrapport','03-kampanje','04-prosjektplan'].includes(capture.scenario)) throw new Error('Unknown product recording scenario');
const hash=path=>createHash('sha256').update(readFileSync(path)).digest('hex');
const plan=JSON.parse(readFileSync(args['--plan'],'utf8'));
if (plan.rawSha256!==hash(raw)) throw new Error('The edit plan does not match this raw recording');
const probe=path=>JSON.parse(execFileSync('ffprobe',['-v','error','-show_streams','-show_format','-of','json',path],{encoding:'utf8'}));
const original=probe(raw);
const duration=Number(original.format.duration);
const stream=original.streams.find(stream=>stream.codec_type==='video');
if (!stream || !Number.isFinite(duration) || duration<=0 || original.streams.some(stream=>stream.codec_type==='audio')) throw new Error('Expected a readable silent browser recording');
if (!Array.isArray(plan.clips) || !plan.clips.length || !Array.isArray(plan.rawCaptions) || !plan.rawCaptions.length) throw new Error('The edit plan needs chronological clips and separate full-recording captions');
let previousEnd=0;
let editDuration=0;
for (const clip of plan.clips) {
  if (![clip.start,clip.end].every(Number.isFinite) || clip.start<previousEnd || clip.end<=clip.start || clip.end>duration
    || typeof clip.caption!=='string' || !clip.caption.trim() || /-->|[\r\n<>]/.test(clip.caption)) throw new Error('Invalid clip boundaries or caption');
  previousEnd=clip.end;
  editDuration+=clip.end-clip.start;
}
const [minimum,maximum]=capture.scenario==='01-kundesvar' ? [45,60] : [25,40];
if (editDuration<minimum || editDuration>maximum) throw new Error(`This scenario needs a ${minimum}–${maximum} second edit; preserve actual waiting in the raw recording`);
if (!Number.isFinite(plan.posterTime) || plan.posterTime<0 || plan.posterTime>=duration) throw new Error('Select a real result frame for the poster');
const stamp=seconds=>new Date(Math.round(seconds*1000)).toISOString().slice(11,23);
function vtt(cues,limit) {
  let lastEnd=0;
  for (const cue of cues) {
    if (![cue.start,cue.end].every(Number.isFinite) || cue.start<lastEnd || cue.end<=cue.start || cue.end>limit
      || typeof cue.text!=='string' || !cue.text.trim() || /-->|[\r\n<>]/.test(cue.text)) throw new Error('Invalid caption timing/text');
    lastEnd=cue.end;
  }
  return 'WEBVTT\n\n'+cues.map(cue=>`${stamp(cue.start)} --> ${stamp(cue.end)}\n${cue.text}\n`).join('\n');
}
let offset=0;
const shortCues=plan.clips.map(clip=>{const cue={start:offset,end:offset+clip.end-clip.start,text:clip.caption};offset=cue.end;return cue;});
// Floating-point summation may put the last cue a few femtoseconds beyond
// the identically derived edit duration. Keep the endpoint exact for VTT validation.
shortCues.at(-1).end=editDuration;
vtt(shortCues,editDuration);
const rawVtt=vtt(plan.rawCaptions,duration);
mkdirSync(output,{recursive:true});
copyFileSync(raw,join(output,'raw.webm'));
writeFileSync(join(output,'raw-captions.vtt'),rawVtt);
const disclosure='PRØVEOPPTAK  ·  Fiktive data  ·  Ventetid klippet  ·  Ikke publiseringsgodkjent';
writeFileSync(join(output,'disclosure.txt'),disclosure);
const font=process.platform==='win32' ? "fontfile='C\\:/Windows/Fonts/segoeui.ttf'" : 'font=Sans';
const graph=plan.clips.map((clip,index)=>`[0:v]trim=start=${clip.start}:end=${clip.end},setpts=PTS-STARTPTS[v${index}]`).join(';')
  +';'+plan.clips.map((_,index)=>`[v${index}]`).join('')+`concat=n=${plan.clips.length}:v=1:a=0,pad=iw:ih+60:0:0:black,drawtext=${font}:textfile=disclosure.txt:fontsize=22:fontcolor=white:x=24:y=h-42[out]`;
execFileSync('ffmpeg',['-hide_banner','-loglevel','error','-i',raw,'-filter_complex',graph,'-map','[out]','-an','-c:v','libx264','-preset','medium','-crf','18','-pix_fmt','yuv420p','-movflags','+faststart',join(output,'short.mp4')],{cwd:output,stdio:'inherit'});
execFileSync('ffmpeg',['-hide_banner','-loglevel','error','-ss',String(plan.posterTime),'-i',raw,'-frames:v','1',join(output,'poster.png')],{stdio:'inherit'});
for (const name of ['raw.webm','short.mp4']) execFileSync('ffmpeg',['-v','error','-i',join(output,name),'-f','null','-'],{stdio:'inherit'});
const edited=probe(join(output,'short.mp4'));
if (Math.abs(Number(edited.format.duration)-editDuration)>0.12) throw new Error('The exported duration differs from the edit plan');
// Frame rounding can make the encoded movie a few milliseconds shorter.
// Never leave the final caption extending past the actual media duration.
shortCues.at(-1).end=Math.min(shortCues.at(-1).end,Number(edited.format.duration));
writeFileSync(join(output,'captions.vtt'),vtt(shortCues,Number(edited.format.duration)));
if (hash(join(output,'raw.webm'))!==plan.rawSha256) throw new Error('The uninterrupted original changed');
const assets=Object.fromEntries(['raw.webm','short.mp4','poster.png','captions.vtt','raw-captions.vtt'].map(name=>[name,{bytes:statSync(join(output,name)).size,sha256:hash(join(output,name))}]));
const record={schemaVersion:1,scenario:capture.scenario,publicationApproved:false,mode:'rehearsal',createdAt:new Date().toISOString(),
  rawDuration:duration,editedDuration:Number(edited.format.duration),sourceCodec:stream.codec_name,sourceFrameRate:stream.avg_frame_rate,
  sourceSize:{width:stream.width,height:stream.height},captionType:'Norwegian descriptions of visible actions; no recorded speech',
  disclosure,editing:'Chronological time cuts; no invented frames, changed output or speed claim',capture,plan,assets};
writeFileSync(join(output,'media-review.json'),JSON.stringify(record,null,2));
console.log(JSON.stringify({output,rawDuration:duration,editedDuration:record.editedDuration,sourceFrameRate:record.sourceFrameRate,publicationApproved:false}));
