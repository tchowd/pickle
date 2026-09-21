const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('browser-extension/extract.js','utf8');
function extract(mode, {videos=[],rows=[],article='Article content',selection='',host='example.com'}={}) {
  const context = {TextEncoder,location:{href:`https://${host}/watch`,protocol:'https:',hostname:host},getSelection:()=>selection,
    Readability: class {parse(){return {textContent:article}}},
    document:{title:'Video title',cloneNode:()=>({querySelectorAll:()=>[],querySelector:()=>null}),querySelector:()=>null,
      querySelectorAll:selector=>selector==='video'?videos:rows}};
  vm.createContext(context);vm.runInContext(source,context);return context.pickleExtract(mode);
}
const video = {paused:false,currentTime:110,clientWidth:640,clientHeight:360,getBoundingClientRect:()=>({width:640}),
  textTracks:[{kind:'captions',mode:'showing',cues:[
    {startTime:5,endTime:10,text:'Too early'}, {startTime:30,endTime:40,text:'Background'},
    {startTime:100,endTime:120,text:'Current moment'}, {startTime:150,endTime:160,text:'Too late'}]}]};
const moment = extract('moment',{videos:[video]});
assert.match(moment.reference.text,/Current moment/);assert.match(moment.reference.text,/Background/);
assert.doesNotMatch(moment.reference.text,/Too early|Too late/);assert.equal(moment.reference.timestamp,110);
assert.match(extract('video',{videos:[video]}).reference.text,/Too early/);
assert.throws(()=>extract('moment'),/own page/);
assert.throws(()=>extract('video',{videos:[{...video,textTracks:[]}]}),/No readable captions/);
assert.throws(()=>extract('article',{article:' '}),/No article/);
const huge = extract('article',{article:'🌍'.repeat(10000),selection:'🌍'.repeat(10000)});
assert.ok(Buffer.byteLength(huge.reference.text)<=12000);assert.ok(Buffer.byteLength(huge.selection)<=12000);
const rows = [{querySelector:s=>({textContent:s==='.segment-timestamp'?'1:40':'YouTube caption'})}];
assert.match(extract('moment',{videos:[{...video,textTracks:[]}],rows,host:'www.youtube.com'}).reference.text,/YouTube caption/);
assert.throws(()=>extract('moment',{videos:[{...video,textTracks:[]}],rows,host:'youtube.com.attacker.com'}),/No readable captions/);
const longVideo = {...video,textTracks:[{kind:'captions',cues:[{startTime:0,endTime:2,text:'🌍'.repeat(8000)}]}]};
assert.match(extract('video',{videos:[longVideo]}).reference.text,/Transcript excerpt only/);
console.log('PASS: video windows, transcript scope, unavailable captions, YouTube adapter, host boundary, Unicode limits');
