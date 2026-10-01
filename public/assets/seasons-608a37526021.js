// Seasonal scenery and metric colors use local assets; no external requests.
export const SEASON_MS=60000;
export const TRANSITION_MS=6000;
export const SEASON_NAMES=['春','夏','秋','冬'];
const smooth=n=>n*n*(3-2*n);
export function seasonFrame(milliseconds){
 const t=Math.max(0,Number.isFinite(milliseconds)?milliseconds:0),slot=Math.floor(t/SEASON_MS),phase=t%SEASON_MS;
 return {index:slot%4,next:(slot+1)%4,phase,blend:smooth(Math.max(0,(phase-(SEASON_MS-TRANSITION_MS))/TRANSITION_MS))};
}

const METRIC_COLORS={light:['#20482c','#184954','#663e12','#304464'],dark:['#b7ddb7','#a5dce4','#f0cf97','#bed3f2']};
export function seasonMetricColor(frame,dark=false){
 const colors=METRIC_COLORS[dark?'dark':'light'],a=colors[frame.index],b=colors[frame.next];
 return '#'+[1,3,5].map(n=>Math.round(parseInt(a.slice(n,n+2),16)*(1-frame.blend)+parseInt(b.slice(n,n+2),16)*frame.blend).toString(16).padStart(2,'0')).join('');
}

const palettes=[
 {sky:['#cbdfe3','#f3ebda'],far:'#bdcfc8',hill:'#a9bba9',shore:'#91a88a',ground:'#b5c19a',water:['#b9d5ce','#8db8b5'],leaf:['#d8a6a6','#e6bbb4','#f0d1c3'],trunk:'#827d6b',pine:'#93ad9b'},
 {sky:['#b9dce2','#f0efda'],far:'#b0cdc9',hill:'#94b6a3',shore:'#829f80',ground:'#a4b98a',water:['#aacfc9','#76aaa9'],leaf:['#81a17c','#9bb684','#b0c590'],trunk:'#727b62',pine:'#83a68f'},
 {sky:['#e4d8c7','#f6ead7'],far:'#c9c9b0',hill:'#b9b691',shore:'#b2a37b',ground:'#c6b080',water:['#c5d2bf','#99b7b0'],leaf:['#c19b62','#d5aa6d','#dbbc82'],trunk:'#857965',pine:'#9eac8e'},
 {sky:['#cbd9e1','#ecedeb'],far:'#d1dcd9',hill:'#bacdca',shore:'#c7d4cf',ground:'#e5e9df',water:['#ccdce0','#acc7ce'],leaf:['#e9efea','#f1f2eb','#e0e7e3'],trunk:'#8e9da0',pine:'#9fb7b2'}
];
function random(seed){return()=>{seed=(Math.imul(seed,1664525)+1013904223)>>>0;return seed/4294967296;};}
function ellipse(c,x,y,rx,ry,color,angle=0){c.fillStyle=color;c.beginPath();c.ellipse(x,y,rx,ry,angle,0,Math.PI*2);c.fill();}
function wash(c,x,y,r,color){const g=c.createRadialGradient(x,y,0,x,y,r);g.addColorStop(0,color);g.addColorStop(1,'#ffffff00');c.fillStyle=g;c.fillRect(x-r,y-r,r*2,r*2);}
function tree(c,x,y,sx,sy,p,season,seed){
 const rand=random(seed),tips=[];c.save();c.translate(x,y);c.scale(sx,sy);c.lineCap='round';c.lineJoin='round';
 function branch(bx,by,len,angle,width,depth){
  const ex=bx+Math.cos(angle)*len,ey=by+Math.sin(angle)*len,bend=(rand()-.5)*len*.25;
  c.strokeStyle=p.trunk;c.lineWidth=width;c.beginPath();c.moveTo(bx,by);c.quadraticCurveTo((bx+ex)/2+bend,(by+ey)/2,ex,ey);c.stroke();
  if(season===3&&depth<4){c.strokeStyle='#f4f5ed';c.lineWidth=Math.max(1,width*.32);c.beginPath();c.moveTo(bx-1,by-2);c.quadraticCurveTo((bx+ex)/2+bend-1,(by+ey)/2-3,ex-1,ey-2);c.stroke();}
  if(depth===0){tips.push([ex,ey]);return;}
  branch(ex,ey,len*(.63+rand()*.12),angle-.3-rand()*.36,width*.62,depth-1);
  branch(ex,ey,len*(.65+rand()*.13),angle+.3+rand()*.37,width*.63,depth-1);
 }
 branch(0,0,167,-Math.PI/2+.04,16,5);
 // Reuse the branch geometry in every season, including winter's bare boughs.
 const foliage=random(seed+23);
 if(season!==3){
  for(const [tx,ty]of tips){
   ellipse(c,tx,ty,27+foliage()*22,17+foliage()*14,p.leaf[0]+'42',foliage());
   for(let i=0;i<18;i++){
    const a=foliage()*Math.PI*2,r=Math.sqrt(foliage())*49,lx=tx+Math.cos(a)*r,ly=ty+Math.sin(a)*r*.63;
    c.globalAlpha=.42+foliage()*.38;
    ellipse(c,lx,ly,4+foliage()*7,2.5+foliage()*4,p.leaf[Math.floor(foliage()*3)],foliage()*3);
    if(season===0&&i%5===0){c.globalAlpha=.9;ellipse(c,lx+2,ly-2,2.4,2,p.leaf[2]);}
   }
  }
 }else{
  for(const [tx,ty]of tips){c.globalAlpha=.75;ellipse(c,tx,ty-3,6+foliage()*9,2+foliage()*2,p.leaf[1],-.08);}
 }
 c.globalAlpha=1;c.restore();
}
function landscape(canvas,index){
 const c=canvas.getContext('2d',{alpha:false}),h=1000,w=canvas.width/canvas.height*h,p=palettes[index],rand=random(24710);
 c.setTransform(canvas.height/h,0,0,canvas.height/h,0,0);
 const sky=c.createLinearGradient(0,0,0,690);sky.addColorStop(0,p.sky[0]);sky.addColorStop(1,p.sky[1]);c.fillStyle=sky;c.fillRect(0,0,w,h);
 wash(c,w*.75,165,170,'#fff6db75');ellipse(c,w*.75,165,38,38,'#fff5db9c');
 for(let i=0;i<8;i++){const x=rand()*w,y=95+rand()*250,rx=50+rand()*90;ellipse(c,x,y,rx,11+rand()*15,'#ffffff19');}
 // Long, uneven ridgelines keep the horizon quiet behind the interface.
 c.fillStyle=p.far;c.beginPath();c.moveTo(0,510);c.bezierCurveTo(w*.11,425,w*.18,493,w*.27,414);c.bezierCurveTo(w*.38,505,w*.40,369,w*.49,432);c.bezierCurveTo(w*.60,482,w*.69,384,w*.77,420);c.bezierCurveTo(w*.87,490,w*.93,417,w,449);c.lineTo(w,710);c.lineTo(0,710);c.fill();
 c.fillStyle=p.hill;c.beginPath();c.moveTo(0,568);c.bezierCurveTo(w*.15,464,w*.24,579,w*.36,513);c.bezierCurveTo(w*.49,466,w*.53,587,w*.66,527);c.bezierCurveTo(w*.82,465,w*.88,536,w,501);c.lineTo(w,752);c.lineTo(0,752);c.fill();
 wash(c,w*.43,535,w*.42,'#f5efd542');
 c.fillStyle=p.shore;c.beginPath();c.moveTo(0,645);c.bezierCurveTo(w*.18,581,w*.27,669,w*.45,627);c.bezierCurveTo(w*.66,587,w*.81,606,w,650);c.lineTo(w,767);c.lineTo(0,767);c.fill();
 for(let i=0;i<Math.max(30,w/23);i++){
  const x=rand()*w,y=620+Math.sin(x/w*11)*12,th=12+rand()*34;
  c.fillStyle=p.pine+'80';c.beginPath();c.moveTo(x,y-th);c.lineTo(x+th*.24,y);c.quadraticCurveTo(x,y-4,x-th*.28,y);c.closePath();c.fill();
 }
 const water=c.createLinearGradient(0,655,0,1000);water.addColorStop(0,p.water[0]);water.addColorStop(1,p.water[1]);c.fillStyle=water;
 c.beginPath();c.moveTo(0,720);c.bezierCurveTo(w*.2,668,w*.37,715,w*.5,680);c.bezierCurveTo(w*.75,651,w*.8,701,w,673);c.lineTo(w,1000);c.lineTo(0,1000);c.fill();
 // A small lakeside dwelling provides a constant landmark through the year.
 const hx=w*.63,hy=660,hs=Math.min(1,w/650);c.save();c.translate(hx,hy);c.scale(hs,hs);
 c.fillStyle=index===3?'#e3e2d6':'#d7c6a3';c.fillRect(-24,-25,48,27);c.fillStyle=index===3?'#f0eee3':'#9a9581';c.beginPath();c.moveTo(-32,-23);c.lineTo(-6,-44);c.lineTo(33,-23);c.closePath();c.fill();
 c.fillStyle='#919c8d';c.fillRect(-9,-16,8,18);c.fillStyle='#eee3bb';c.fillRect(10,-16,9,8);c.restore();
 // The near banks and reeds frame, rather than fill, the centre of the scene.
 c.fillStyle=p.ground;c.beginPath();c.moveTo(0,807);c.bezierCurveTo(w*.12,781,w*.17,855,w*.29,875);c.bezierCurveTo(w*.36,904,w*.49,922,w*.57,1000);c.lineTo(0,1000);c.fill();
 c.beginPath();c.moveTo(w,771);c.bezierCurveTo(w*.93,824,w*.85,848,w*.76,878);c.bezierCurveTo(w*.67,919,w*.64,951,w*.59,1000);c.lineTo(w,1000);c.fill();
 const sx=Math.min(1,w/830);
 tree(c,w*.075,904,sx*.73,.77,p,index,617);
 tree(c,w*.97,878,-sx*.57,.62,p,index,341);
 tree(c,w*.135,970,sx*.30,.36,p,index,113);
 for(let i=0;i<155;i++){
  const side=rand()>.5,x=side?w*(.80+rand()*.2):rand()*w*.24,y=877+rand()*123;
  c.strokeStyle=(index===3?'#abbdbc':'#89967a')+'42';c.lineWidth=.6+rand();c.beginPath();c.moveTo(x,y);c.quadraticCurveTo(x-3,y-5,x-5+rand()*10,y-8-rand()*16);c.stroke();
  if(index===0&&i%8===0)ellipse(c,x,y-12,2.5,2,'#edcfc5');
 }
 // Fine pigment speckles are drawn once and cached, never regenerated per frame.
 for(let i=0;i<950;i++){const x=rand()*w,y=rand()*h;c.globalAlpha=.025+rand()*.018;ellipse(c,x,y,.6+rand()*.8,.5+rand()*.6,i%3?'#756e5b':'#fffdf1');}
 c.globalAlpha=1;
}
function fadeOpacity(element,from,to,phase){
 const a=element.animate([{opacity:from},{opacity:to}],{duration:TRANSITION_MS,delay:SEASON_MS-TRANSITION_MS,easing:'cubic-bezier(.333333,0,.666667,1)',fill:'both'});a.pause();a.currentTime=phase;return a;
}
function weatherLayer(host){
 const layer=document.createElement('div');layer.className='season-weather';layer.setAttribute('aria-hidden','true');const groups=[],waves=[];let running=false,seasonIndex=-1,lastDark=false,fades=[];
 for(let season=0;season<4;season++){
  const group=document.createElement('div');group.className='season-particles season-particles-'+season;group.hidden=true;const rand=random(740+season),animations=[],particles=[];
  for(let i=0;i<(season===3?24:season===1?6:14);i++){
   const p=document.createElement('i'),duration=(34+rand()*32)*1000,drift=22+rand()*48,offset=rand()*duration;
   p.className='season-sprite';p.style.left=(rand()*100).toFixed(2)+'%';p.style.setProperty('--flake',(2+rand()*3).toFixed(1)+'px');group.append(p);particles.push(p);
   const animation=p.animate([
    {transform:'translate3d(0,-35px,0) rotate(0deg)'},
    {transform:'translate3d('+(-drift*.25)+'px,48vh,0) rotate(135deg)',offset:.5},
    {transform:'translate3d('+drift+'px,calc(100vh + 40px),0) rotate(280deg)'}
   ],{duration,iterations:Infinity,easing:'linear'});animation.pause();animation.currentTime=offset;animations.push(animation);
  }
  groups.push({group,animations,particles,opacity:[.55,.3,.54,.6][season],playing:false});layer.append(group);
 }
 for(let i=0;i<9;i++){
  const p=document.createElement('i');p.className='season-ripple';p.style.left=(33+Math.sin(i*7.3)*13)+'%';p.style.top=(72+i*2.2)+'%';p.style.width=(22+i*4)+'px';layer.append(p);
  const a=p.animate([{transform:'translate3d(-6px,0,0) scaleX(.85)',opacity:.12},{transform:'translate3d(6px,0,0) scaleX(1.15)',opacity:.32},{transform:'translate3d(-6px,0,0) scaleX(.85)',opacity:.12}],{duration:16000,iterations:Infinity,easing:'ease-in-out'});a.pause();a.currentTime=i*1000;waves.push(a);
 }
 host.append(layer);
 function playback(){for(const g of groups){const playing=running&&!g.group.hidden;if(playing!==g.playing){g.playing=playing;for(const a of g.animations)playing?a.play():a.pause();}}}
 return {
  update(frame,sync=false){
   const dark=document.documentElement.dataset.theme==='dark';if(seasonIndex!==frame.index||lastDark!==dark){for(const a of fades)a.cancel();seasonIndex=frame.index;lastDark=dark;const current=groups[frame.index],next=groups[frame.next],factor=dark?.52:1;fades=[...current.particles.map(p=>fadeOpacity(p,current.opacity*factor,0,frame.phase)),...next.particles.map(p=>fadeOpacity(p,0,next.opacity*factor,frame.phase))];if(running)for(const a of fades)a.play();}
   else if(sync)for(const a of fades)a.currentTime=frame.phase;
   for(let s=0;s<4;s++){const g=groups[s],weight=s===frame.index?1-frame.blend:s===frame.next?frame.blend:0;if(g.group.hidden!==(weight<.001))g.group.hidden=weight<.001;}playback();
  },
  motion(value){if(running===value)return;running=value;playback();for(const a of [...waves,...fades])value?a.play():a.pause();},
  destroy(){for(const g of groups)for(const a of g.animations)a.cancel();for(const a of [...waves,...fades])a.cancel();layer.remove();}
 };
}

export function mountSeasons(host){
 // Cache each landscape once; blend two bitmaps into one bounded opaque surface during the transition.
 const scene=document.createElement('div');scene.className='season-scene';scene.setAttribute('aria-hidden','true');host.append(scene);
 const display=document.createElement('canvas');display.className='season-landscape';scene.append(display);const output=display.getContext('2d',{alpha:false});const sheets=Array.from({length:2},()=>({canvas:document.createElement('canvas'),index:-1}));
 if(!output||!sheets.every(s=>s.canvas.getContext('2d',{alpha:false}))){scene.remove();return{destroy(){}};}
 const paintBuffer=document.createElement('canvas');let paintKey='';
 const camera=scene.animate([{transform:'translate3d(-.45%,.15%,0) scale(1.035)'},{transform:'translate3d(.45%,-.15%,0) scale(1.045)'},{transform:'translate3d(-.45%,.15%,0) scale(1.035)'}],{duration:120000,iterations:Infinity,easing:'ease-in-out'});camera.pause();
 const metricStyle=document.createElement('style');metricStyle.id='season-metric-colors';metricStyle.textContent='html[data-appearance=seasons]{}';document.head.append(metricStyle);const metricRule=metricStyle.sheet.cssRules[0].style;
 const weather=weatherLayer(host),control=document.createElement('div');control.className='season-controls';const label=document.createElement('span');label.className='season-name';const button=document.createElement('button');button.type='button';control.append(label,button);document.querySelector('.footer')?.append(control);
 const reduce=matchMedia('(prefers-reduced-motion: reduce)'),forced=matchMedia('(forced-colors: active)');
 let destroyed=false,raf=0,resizeTimer=0,idle=0,elapsed=0,last=0,lastRender=-Infinity,lastMetric=-Infinity,lastColor='',paused=false,reduced=reduce.matches||forced.matches,lastSeason=-1,drawnIndex=-1,drawnBlend=-1;
 const cancelIdle=()=>{if(idle){if(window.cancelIdleCallback)cancelIdleCallback(idle);else clearTimeout(idle);idle=0;}};
 function effects(){return{dark:document.documentElement.dataset.theme==='dark',blur:Math.max(0,Math.min(24,parseFloat(host.style.getPropertyValue('--theme-blur'))||0))};}
 function paint(sheet,index){
  const canvas=sheet.canvas,c=canvas.getContext('2d',{alpha:false}),{dark,blur}=effects();
  if(paintBuffer.width!==canvas.width||paintBuffer.height!==canvas.height){paintBuffer.width=canvas.width;paintBuffer.height=canvas.height;}
  landscape(paintBuffer,index);c.setTransform(1,0,0,1,0,0);c.clearRect(0,0,canvas.width,canvas.height);
  const ratio=canvas.width/Math.max(1,innerWidth),effect=(dark?'brightness(.40) saturate(.65) ':'')+'blur('+(blur*ratio)+'px)',pad=Math.ceil(blur*ratio*2);
  // Bake filters into the cached bitmap; a live CSS filter would process the entire viewport every frame.
  if('filter' in c){c.filter=effect;canvas.style.filter='none';}else{canvas.style.filter=(dark?'brightness(.40) saturate(.65) ':'')+'blur('+blur+'px)';}
  c.fillStyle=palettes[index].sky[0];c.fillRect(0,0,canvas.width,canvas.height);c.drawImage(paintBuffer,-pad,-pad,canvas.width+pad*2,canvas.height+pad*2);c.filter='none';
  sheet.index=index;canvas.dataset.season=String(index);
 }
 function refreshEffects(){const key=JSON.stringify(effects());if(key!==paintKey){paintKey=key;cancelIdle();for(const sheet of sheets)sheet.index=-1;}render(true);}

 function prepare(frame){
  let current=sheets.find(s=>s.index===frame.index);if(!current){current=sheets[0];paint(current,frame.index);}
  const next=sheets.find(s=>s!==current);
  if(next.index!==frame.next&&!idle){const work=()=>{idle=0;if(!destroyed)paint(next,frame.next);};idle=window.requestIdleCallback?requestIdleCallback(work,{timeout:1200}):setTimeout(work,120);}
  return{current,next};
 }
 function render(force=false){
  if(destroyed)return;const frame=seasonFrame(elapsed),{current,next}=prepare(frame),blend=next.index===frame.next?frame.blend:0;
  if(force||drawnIndex!==frame.index||drawnBlend!==blend){
   output.globalAlpha=1;output.drawImage(current.canvas,0,0);
   if(blend>0){output.globalAlpha=blend;output.drawImage(next.canvas,0,0);output.globalAlpha=1;}
   if(display.style.filter!==current.canvas.style.filter)display.style.filter=current.canvas.style.filter;
   drawnIndex=frame.index;drawnBlend=blend;
  }
  weather.update(frame,force);
  if(force||elapsed-lastMetric>=100){const color=seasonMetricColor(frame,document.documentElement.dataset.theme==='dark');if(color!==lastColor){metricRule.setProperty('--season-metric',color);lastColor=color;}lastMetric=elapsed;}
  if(frame.index!==lastSeason){lastSeason=frame.index;label.textContent=SEASON_NAMES[frame.index];host.dataset.season=String(frame.index);}
 }
 function stop(){if(last)elapsed+=Math.max(0,performance.now()-last);cancelAnimationFrame(raf);raf=0;last=0;camera.pause();weather.motion(false);host.dataset.motion='paused';}
 function buttons(){button.textContent=paused||reduced?'播放':'暂停';button.setAttribute('aria-label',paused||reduced?'播放四季背景动画':'暂停四季背景动画');button.setAttribute('aria-pressed',String(paused||reduced));}
 function tick(now){
  if(destroyed||paused||reduced||document.hidden){stop();return;}
  if(last)elapsed+=Math.max(0,now-last);last=now;
  // Follow vsync at 60 Hz; on high-refresh displays avoid unnecessary 120/144 Hz JS work.
  if(now-lastRender>=1000/60-.5){render();lastRender=Number.isFinite(lastRender)?now-((now-lastRender)%(1000/60)):now;}
  raf=requestAnimationFrame(tick);
 }
 function run(){stop();buttons();render(true);if(!destroyed&&!paused&&!reduced&&!document.hidden){host.dataset.motion='playing';camera.play();weather.motion(true);lastRender=-Infinity;raf=requestAnimationFrame(tick);}}
 function resize(){
  cancelIdle();const cssW=Math.max(1,innerWidth),cssH=Math.max(1,innerHeight),dpr=Math.min(devicePixelRatio||1,1.35,Math.sqrt(650000/(cssW*cssH))),w=Math.ceil(cssW*dpr),h=Math.ceil(cssH*dpr);
  if(display.width!==w||display.height!==h){display.width=w;display.height=h;drawnIndex=-1;drawnBlend=-1;}
  for(const sheet of sheets)if(sheet.canvas.width!==w||sheet.canvas.height!==h){sheet.canvas.width=w;sheet.canvas.height=h;sheet.index=-1;}
  render(true);
 }
 function onResize(){clearTimeout(resizeTimer);resizeTimer=setTimeout(resize,140);}
 function visibility(){if(document.hidden)stop();else run();}
 function preference(){reduced=reduce.matches||forced.matches;run();}
 button.onclick=()=>{if(forced.matches)return;if(reduced){reduced=false;paused=false;}else paused=!paused;run();};
 document.addEventListener('visibilitychange',visibility);window.addEventListener('resize',onResize);reduce.addEventListener('change',preference);forced.addEventListener('change',preference);
 const observer=new MutationObserver(refreshEffects);observer.observe(document.documentElement,{attributes:true,attributeFilter:['data-theme']});observer.observe(host,{attributes:true,attributeFilter:['style']});
 paintKey=JSON.stringify(effects());resize();run();
 return{destroy(){destroyed=true;stop();cancelIdle();clearTimeout(resizeTimer);observer.disconnect();metricStyle.remove();paintBuffer.width=0;paintBuffer.height=0;display.width=0;display.height=0;camera.cancel();weather.destroy();control.remove();for(const s of sheets){s.canvas.width=0;s.canvas.height=0;}scene.remove();document.removeEventListener('visibilitychange',visibility);window.removeEventListener('resize',onResize);reduce.removeEventListener('change',preference);forced.removeEventListener('change',preference);delete host.dataset.motion;delete host.dataset.season;}};
}
