export function createTerminalKeys({root,getTerm,getSearch,send,paste,fit}){
 const pane=root.querySelector('#terminal-panel'),screen=pane.querySelector('.terminal-screen');
 const bar=document.createElement('div');bar.className='terminal-keybar';bar.setAttribute('role','group');bar.setAttribute('aria-label','终端快捷键');
 const search=document.createElement('form');search.className='terminal-search';search.hidden=true;
 const input=document.createElement('input');input.type='search';input.maxLength=256;input.placeholder='搜索终端内容';input.setAttribute('aria-label','搜索终端内容');
 const status=document.createElement('span');status.setAttribute('role','status');
 let enabled=false,ctrl=false,timer=null,interval=null,searchVisible=false,repeatUsed=false;
 function button(label,action){const b=document.createElement('button');b.type='button';b.textContent=label;b.disabled=true;b.addEventListener('pointerdown',e=>e.preventDefault());b.addEventListener('click',action);bar.append(b);return b;}
 const ctrlButton=button('Ctrl',()=>{if(enabled){ctrl=!ctrl;ctrlButton.setAttribute('aria-pressed',String(ctrl));getTerm()?.focus();}});ctrlButton.setAttribute('aria-pressed','false');ctrlButton.title='作用于下一个字母';
 function reset(){ctrl=false;ctrlButton.setAttribute('aria-pressed','false');}
 function key(value){if(!enabled)return;reset();send(value);getTerm()?.focus();}
 button('Tab',()=>key('\t'));button('Esc',()=>key('\x1b'));button('Ctrl+C',()=>key('\x03'));
 for(const [label,code]of [['←','D'],['↑','A'],['↓','B'],['→','C']]){
  const b=button(label,()=>{if(!repeatUsed)key('\x1b'+(getTerm()?.modes.applicationCursorKeysMode?'O':'[')+code);repeatUsed=false;});
  b.setAttribute('aria-label',{D:'左方向键',A:'上方向键',B:'下方向键',C:'右方向键'}[code]);
  b.addEventListener('pointerdown',e=>{if(!enabled||e.button!==0)return;stopRepeat();repeatUsed=false;b.setPointerCapture(e.pointerId);timer=setTimeout(()=>{repeatUsed=true;key('\x1b'+(getTerm()?.modes.applicationCursorKeysMode?'O':'[')+code);interval=setInterval(()=>key('\x1b'+(getTerm()?.modes.applicationCursorKeysMode?'O':'[')+code),95);},400);});
  for(const type of ['pointerup','pointercancel','lostpointercapture','pointerleave'])b.addEventListener(type,stopRepeat);
 }
 button('粘贴',()=>{reset();paste();});
 const toggle=button('查找',()=>{searchVisible=!searchVisible;search.hidden=!searchVisible;toggle.setAttribute('aria-expanded',String(searchVisible));reset();if(searchVisible)input.focus();else getTerm()?.focus();fit();});
 toggle.setAttribute('aria-expanded','false');
 function find(back=false){const addon=getSearch(),value=input.value;if(!value){getTerm()?.clearSelection();status.textContent='';return;}const found=back?addon?.findPrevious(value,{caseSensitive:false,regex:false}):addon?.findNext(value,{caseSensitive:false,regex:false});status.textContent=found?'':'未找到';}
 const prev=document.createElement('button');prev.type='button';prev.textContent='上一个';prev.onclick=()=>find(true);
 const next=document.createElement('button');next.type='submit';next.textContent='下一个';
 const close=document.createElement('button');close.type='button';close.textContent='关闭';close.onclick=()=>{search.hidden=true;searchVisible=false;toggle.setAttribute('aria-expanded','false');getTerm()?.focus();fit();};
 search.append(input,prev,next,close,status);search.addEventListener('submit',e=>{e.preventDefault();find();});
 search.addEventListener('keydown',e=>{if(e.key==='Escape'){e.preventDefault();e.stopPropagation();close.click();}});
 pane.insertBefore(search,screen);pane.append(bar);
 function stopRepeat(){clearTimeout(timer);clearInterval(interval);timer=null;interval=null;}
 const visibility=()=>{if(document.hidden){stopRepeat();reset();}};document.addEventListener('visibilitychange',visibility);
 return {
  transform(value){if(!ctrl)return value;reset();if(value.length===1&&/^[a-zA-Z@\[\\\]\^_]$/.test(value))return String.fromCharCode(value.toUpperCase().charCodeAt(0)&31);return value;},
  ready(value){enabled=value;stopRepeat();reset();for(const b of bar.querySelectorAll('button'))b.disabled=!value;},
  reset(){stopRepeat();reset();},
  destroy(){stopRepeat();reset();document.removeEventListener('visibilitychange',visibility);bar.remove();search.remove();}
 };
}
