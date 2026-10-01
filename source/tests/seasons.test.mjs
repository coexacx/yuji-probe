import test from 'node:test';
import assert from 'node:assert/strict';
import {seasonFrame,SEASON_MS,TRANSITION_MS,seasonMetricColor} from '../src/seasons.mjs';
import {terminalAppearance} from '../src/terminal-appearance.mjs';
test('four 60-second slots wrap winter into spring',()=>{
 assert.equal(SEASON_MS,60000);assert.equal(TRANSITION_MS,6000);
 for(let cycle=0;cycle<3;cycle++)for(let season=0;season<4;season++){
  const start=(cycle*4+season)*60000;
  assert.equal(seasonFrame(start).index,season);
  assert.equal(seasonFrame(start+59999).index,season);
  assert.equal(seasonFrame(start+60000).index,(season+1)%4);
 }
});
test('soft painting transition holds, eases and reaches the next scene',()=>{
 for(let season=0;season<4;season++){
  const t=season*60000;
  assert.equal(seasonFrame(t+53999).blend,0);
  assert.equal(seasonFrame(t+54000).blend,0);
  assert.equal(seasonFrame(t+57000).blend,.5);
  assert(seasonFrame(t+59999).blend>.999999);
  assert.equal(seasonFrame(t+60000).blend,0);
  assert.equal(seasonFrame(t+59999).next,seasonFrame(t+60000).index);
 }
});
test('invalid clock readings cannot escape the four bounded seasons',()=>{
 for(const t of [-1,NaN,Infinity,-Infinity])assert.deepEqual(seasonFrame(t),seasonFrame(0));
 for(let t=0;t<480000;t+=139){const f=seasonFrame(t);assert(f.index>=0&&f.index<4&&f.next>=0&&f.next<4&&f.blend>=0&&f.blend<=1);}
});
test('default and unknown terminal themes retain the original opaque palette',()=>{
 const old={background:'#121b29',foreground:'#dce6f0',cursor:'#b9dbef',selectionBackground:'#406584'};
 for(const name of ['default','alpine',undefined,'untrusted'])assert.deepEqual(terminalAppearance({dataset:{appearance:name}}),old);
});
test('custom terminal palettes use transparent backgrounds in light and dark modes',()=>{
 const previous=globalThis.getComputedStyle;
 globalThis.getComputedStyle=root=>({getPropertyValue:name=>name==='--text'?(root.dataset.theme==='dark'?'#e7f0e6':'#203b35'):'#376956'});
 try{for(const name of ['clear','sketch','anime','seasons'])for(const mode of ['light','dark']){
  const t=terminalAppearance({dataset:{appearance:name,theme:mode}});
  assert.equal(t.background,'#00000000');assert.equal(t.cursor,'#376956');
  assert.equal(t.foreground,mode==='dark'?'#e7f0e6':'#203b35');assert.match(t.green,/^#[0-9a-f]{6}$/);assert.match(t.selectionBackground,/^#[0-9a-f]{8}$/);
 }}finally{globalThis.getComputedStyle=previous;}
});

test('seasonal metric colors join continuously and keep dark-mode contrast',()=>{
 for(const dark of [false,true])for(let index=0;index<4;index++){
  const next=(index+1)%4,start=seasonMetricColor({index,next,blend:0},dark),end=seasonMetricColor({index,next,blend:1},dark);
  assert.match(start,/^#[0-9a-f]{6}$/);assert.notEqual(start,end);
  assert.equal(end,seasonMetricColor({index:next,next:(next+1)%4,blend:0},dark));
 }
});

test('uploaded themes based on default also style the terminal',()=>{
 const previous=globalThis.getComputedStyle;
 globalThis.getComputedStyle=()=>({getPropertyValue:name=>name==='--text'?'#234567':'#336699'});
 try{const value=terminalAppearance({dataset:{appearance:'default',themePackage:'sample'}});assert.equal(value.background,'#00000000');assert.equal(value.foreground,'#234567');}
 finally{globalThis.getComputedStyle=previous;}
});
