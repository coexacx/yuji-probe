export function createInspectorUI(api){
 const el=(tag,cls,text)=>{const n=document.createElement(tag);n.className=cls||'';if(text!==undefined)n.textContent=text;return n;};
 let current=null;
 function close(){current?.close();current=null;}
 async function open(record){
  close();const d=el('dialog','node-dialog inspector-dialog'),inner=el('div','dialog-inner'),top=el('div','dialog-top'),body=el('div','inspector-body'),note=el('p','form-footnote','正在连接…'),bar=el('div','inspector-tabs');
  const title=el('h2','',record.public.name+' · 运行状态'),dismiss=el('button','secondary-button','关闭');dismiss.type='button';dismiss.onclick=()=>d.close();top.append(title,dismiss);
  const tabs=['进程','系统服务'],buttons=tabs.map((v,i)=>{const b=el('button','secondary-button',v);b.type='button';b.onclick=()=>{mode=i;unit='';load();};bar.append(b);return b;});
  const sort=el('select');sort.setAttribute('aria-label','进程排序');sort.append(new Option('按 CPU','cpu'),new Option('按内存','memory'));sort.onchange=()=>load();
  const refresh=el('button','secondary-button','刷新');refresh.type='button';refresh.onclick=()=>load();bar.append(sort,refresh);inner.append(top,bar,note,body);d.append(inner);document.body.append(d);d.showModal();
  let ws=null,alive=true,ready=false,busy=false,mode=0,unit='',seq=0,pending=null,timer=null;
  function stop(){if(!alive)return;alive=false;clearInterval(timer);ws?.close();if(pending){clearTimeout(pending.timer);pending.reject(Error('连接已关闭'));pending=null;}d.remove();}
  d.onclose=stop;current={close:()=>{d.close();stop();}};
  function request(action,target=''){return new Promise((resolve,reject)=>{if(!ready||ws?.readyState!==1)return reject(Error('连接尚未就绪'));if(pending)return reject(Error('请等待当前读取完成'));const id='inspect_'+(++seq);pending={id,resolve,reject,timer:setTimeout(()=>{pending=null;reject(Error('读取超时'));},24000)};ws.send(JSON.stringify({type:'file',id,action,path:'/',target}));});}
  function table(headers,rows){const wrap=el('div','table-scroll'),t=el('table','node-table inspector-table'),head=el('tr'),thead=el('thead'),tbody=el('tbody');for(const h of headers)head.append(el('th','',h));thead.append(head);for(const cells of rows){const tr=el('tr');for(const value of cells){const td=el('td');if(value instanceof Node)td.append(value);else td.textContent=value;tr.append(td);}tbody.append(tr);}t.append(thead,tbody);wrap.append(t);body.replaceChildren(wrap);}
  async function load(){
   if(!alive||!ready||busy)return;busy=true;refresh.disabled=true;const view=mode,selected=unit;
   buttons.forEach((b,i)=>{b.classList.toggle('active',i===mode);b.disabled=true;});sort.hidden=mode!==0;
   try{
    if(mode===0){const r=await request('processes',sort.value);if(!alive||view!==mode)return;table(['PID','进程','CPU','内存'],r.processes.map(p=>[p.pid,p.name,p.cpu===null?'采样中':p.cpu.toFixed(1)+'%',(p.memory/1048576).toFixed(1)+' MiB · '+p.memoryPercent.toFixed(1)+'%']));note.textContent=(r.sampled?'实时采样':'下一次采样后显示 CPU')+' · 共 '+r.total+' 个进程，显示前 100 项 · CPU 按单核 100% 计';}
    else if(unit){const r=await request('service_logs',unit);if(!alive||selected!==unit)return;body.replaceChildren(el('h3','',r.unit),el('pre','service-log',r.text||'暂无日志'));note.textContent='最近 100 行日志 · '+new Date(r.at*1000).toLocaleTimeString();}
    else{const r=await request('services');if(!alive||view!==mode)return;table(['服务','状态','说明','日志'],r.services.map(s=>{const b=el('button','table-button','查看日志');b.type='button';b.onclick=()=>{unit=s.unit;load();};return [s.unit,s.active+' / '+s.state,s.description,b];}));note.textContent='共 '+r.services.length+' 个 systemd 服务 · 只读查看';}
   }catch(e){if(alive)note.textContent=e.message;}finally{busy=false;refresh.disabled=false;buttons.forEach(b=>b.disabled=false);}
  }
  try{const ticket=await api('/api/admin/terminal-ticket','POST',{id:record.public.id});if(!alive)return;ws=new WebSocket(new URL('/api/terminal',location.href).href.replace(/^https:/,'wss:'));ws.onopen=()=>ws.send(JSON.stringify({type:'authorize',ticket:ticket.ticket,mode:'inspect'}));ws.onmessage=e=>{if(typeof e.data!=='string')return;let m;try{m=JSON.parse(e.data);}catch{return;}if(m.type==='ready'){ready=true;load();timer=setInterval(()=>{if(mode===0&&!document.hidden)load();},5000);}else if(m.type==='file_result'&&pending?.id===m.id){const p=pending;pending=null;clearTimeout(p.timer);m.ok?p.resolve(m.data):p.reject(Error(m.error||'读取失败'));}else if(m.type==='error')note.textContent=m.error||m.message||'连接失败';};ws.onclose=()=>{ready=false;clearInterval(timer);if(alive)note.textContent='Agent 通道已断开，请关闭后重新打开';if(pending){clearTimeout(pending.timer);pending.reject(Error('连接已断开'));pending=null;}};ws.onerror=()=>{note.textContent='连接失败';};}catch(e){if(alive)note.textContent=e.message;}
 }
 return {open,close};
}
