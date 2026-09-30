const presets=new Set(['default','clear','sketch','anime','seasons','alpine']);
let epoch=0,saved=null,shown=null,inflight=null,previewing=false,urls={background:null,favicon:null},imageKeys={},brandOriginal=null;
const root=document.documentElement;
let scenery=null,sceneryEpoch=0,sceneryKind='';
function sceneryFor(preset){
 const kind=['seasons','alpine'].includes(preset)?preset:'',epoch=++sceneryEpoch;
 layer.classList.toggle('has-season-scene',!!kind);
 if(kind!==sceneryKind){scenery?.destroy();scenery=null;sceneryKind=kind;}
 if(kind&&!scenery)import("/assets/seasons-3025c22f0af5.js").then(({mountSeasons})=>{
  if(epoch===sceneryEpoch&&root.dataset.appearance===kind&&!scenery)scenery=mountSeasons(layer,{kind});
 }).catch(()=>{});
}
const style=document.createElement('style');style.id='appearance-overrides';document.head.append(style);
const layer=document.createElement('div');layer.className='theme-background';layer.setAttribute('aria-hidden','true');document.body.prepend(layer);
function blobURL(value,key){
 const fingerprint=value?.mime+':'+value?.data;
 if(imageKeys[key]===fingerprint)return urls[key];
 if(urls[key])URL.revokeObjectURL(urls[key]);urls[key]=null;imageKeys[key]=fingerprint;
 if(!value||!['image/png','image/jpeg','image/webp'].includes(value.mime)||typeof value.data!=='string'||value.data.length>2200000)return null;
 try{const bytes=Uint8Array.from(atob(value.data),c=>c.charCodeAt(0));return urls[key]=URL.createObjectURL(new Blob([bytes],{type:value.mime}));}catch{return null;}
}
function luminance(rgb){return rgb.map(v=>{v/=255;return v<=.04045?v/12.92:((v+.055)/1.055)**2.4;}).reduce((s,v,i)=>s+v*[.2126,.7152,.0722][i],0);}
function colors(){
 const t=shown?.theme;for(const v of ['--accent','--accent-wash','--focus','--chart','--theme-button-color','--theme-button-text'])root.style.removeProperty(v);
 if(!t||!/^#[0-9a-f]{6}$/i.test(t.accent))return;
 const rgb=[1,3,5].map(n=>parseInt(t.accent.slice(n,n+2),16)),dark=root.dataset.theme==='dark';let text=rgb.slice();
 const background=dark?.035:.88;
 for(let n=0;n<16;n++){const l=luminance(text),ratio=(Math.max(l,background)+.05)/(Math.min(l,background)+.05);if(ratio>=4.5)break;text=text.map(v=>Math.round(v+(dark?255-v:-v)*.13));}
 const hex='#'+text.map(v=>v.toString(16).padStart(2,'0')).join('');
 root.style.setProperty('--accent',hex);root.style.setProperty('--focus',hex);root.style.setProperty('--chart',hex);
 root.style.setProperty('--accent-wash','rgb('+rgb.join(' ')+' / 12%)');
 root.style.setProperty('--theme-button-color',t.accent);root.style.setProperty('--theme-button-text',luminance(rgb)>.179?'#15202b':'#ffffff');
}
function apply(doc){
 if(!doc?.theme)return;shown=doc;const t=doc.theme;
 root.dataset.appearance=presets.has(t.preset)?t.preset:'default';
 sceneryFor(t.preset);
 const background=blobURL(['seasons','alpine'].includes(t.preset)?null:t.background,'background');root.classList.toggle('has-theme-background',!!background);
 layer.style.setProperty('--theme-image',background?'url("'+background+'")':'none');layer.style.setProperty('--theme-dim',String(Math.min(85,Math.max(0,t.backgroundDim??20))/100));layer.style.setProperty('--theme-blur',Math.min(24,Math.max(0,t.backgroundBlur??0))+'px');
 const favicon=blobURL(t.favicon,'favicon'),link=document.querySelector('link[rel=icon]');link.href=favicon||'/favicon.svg';link.type=favicon?t.favicon.mime:'image/svg+xml';
 const mark=document.querySelector('.brand-mark');if(!brandOriginal)brandOriginal=mark.firstElementChild.cloneNode(true);
 if(favicon&&t.brandIcon){const img=document.createElement('img');img.className='custom-brand-icon';img.alt='';img.src=favicon;mark.replaceChildren(img);mark.classList.add('has-custom-icon');}else{mark.replaceChildren(brandOriginal.cloneNode(true));mark.classList.remove('has-custom-icon');}
 style.textContent=typeof doc.css==='string'?doc.css:'';
 colors();
}
new MutationObserver(colors).observe(root,{attributes:true,attributeFilter:['data-theme']});
export function acceptAppearance(doc){epoch++;saved=doc;if(!previewing)apply(doc);}
export async function refreshAppearance(revision){
 if(saved&&(revision===undefined||saved.theme.revision===revision))return;
 if(inflight)return inflight;
 const generation=epoch;inflight=(async()=>{try{const r=await fetch('/api/public/theme',{credentials:'same-origin'});if(r.ok){const doc=await r.json();if(generation===epoch)acceptAppearance(doc);}}catch{}finally{inflight=null;}})();return inflight;
}
export function endAppearancePreview(){previewing=false;document.querySelector('#theme-preview-banner')?.remove();if(saved)apply(saved);}
export function previewAppearance(doc,save){
 endAppearancePreview();previewing=true;apply(doc);
 const banner=document.createElement('div');banner.id='theme-preview-banner';banner.setAttribute('role','status');const text=document.createElement('span');text.textContent='主题预览';
 const cancel=document.createElement('button');cancel.type='button';cancel.textContent='退出预览';cancel.onclick=endAppearancePreview;
 const commit=document.createElement('button');commit.type='button';commit.textContent='保存主题';commit.onclick=save;banner.append(text,cancel,commit);document.body.append(banner);
}
refreshAppearance();
