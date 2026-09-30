
export function createOperations({el,api,toast,field,button,heading,getRecords,getAuth,refresh}){
 const apiRoot='/api/admin/ops/';
 const bytes=n=>{n=Number(n)||0;for(const u of ['B','KiB','MiB','GiB','TiB','PiB']){if(n<1024||u==='PiB')return n.toFixed(n<10?2:1)+' '+u;n/=1024;}};
 const date=n=>n?new Date(n*1000).toLocaleString('zh-CN',{hour12:false}):'—';
 const section=(panel,title,note='')=>{const box=el('section','glass admin-box');box.append(heading(title,note));panel.append(box);return box;};
 const saveFile=(name,data,type='application/json')=>{const url=URL.createObjectURL(new Blob([typeof data==='string'?data:JSON.stringify(data,null,2)],{type})),a=el('a');a.href=url;a.download=name;a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);};
 const run=(b,fn)=>async()=>{b.disabled=true;try{await fn();}catch(e){toast(e.message,true);}finally{b.disabled=false;}};
 const action=(label,fn,cls='secondary-button')=>{const b=button(label,cls,()=>{});b.onclick=run(b,fn);return b;};
 function select(label,name,values,current){const wrap=el('label','form-field',label),input=el('select');input.name=name;for(const [value,text]of values)input.append(new Option(text,value));input.value=String(current??values[0]?.[0]??"");wrap.append(input);return [wrap,input];}
 let authenticating=null;
 function reauth(){
  if(authenticating)return authenticating;
  authenticating=new Promise((resolve,reject)=>{
   const d=el('dialog','node-dialog ops-dialog'),body=el('div','dialog-inner'),form=el('form','settings-form');
   const [pw,p]=field('当前管理员密码','password','reauthPassword','');p.required=true;p.autocomplete='current-password';
   const [code,c]=field('动态码或未使用的恢复码','text','reauthCode','');c.pattern='[0-9]{6}|[a-fA-F0-9-]{32,35}';c.maxLength=35;c.inputMode='text';c.required=!!getAuth().mfaEnabled;code.hidden=!c.required;
   const err=el('p','form-error'),save=el('button','primary-button','验证身份');save.type='submit';
   let accepted=false;
   form.append(pw,code,err,save,button('取消','secondary-button',()=>d.close()));
   body.append(el('h2','','验证管理身份'),el('p','form-subtitle','验证后 5 分钟内可执行敏感操作。'),form);d.append(body);document.body.append(d);
   d.addEventListener('close',()=>{p.value='';c.value='';d.remove();authenticating=null;if(!accepted)reject(Error('已取消身份验证'));});
   form.addEventListener('submit',async e=>{e.preventDefault();save.disabled=true;try{await api('/api/admin/reauth','POST',{password:p.value,code:c.value});accepted=true;d.close();resolve();}catch(e){err.textContent=e.message;p.value='';}finally{save.disabled=false;}});
   d.showModal();p.focus();
  });return authenticating;
 }
 function showRecovery(codes){
  if(!codes?.length)return;
  const d=el('dialog','node-dialog ops-dialog'),body=el('div','dialog-inner'),list=el('pre','recovery-codes',codes.join('\n'));
  body.append(el('h2','','保存备用恢复码'),el('p','form-subtitle','每条只能使用一次。重新生成后，旧恢复码立即失效。'),list,
   action('下载恢复码',()=>saveFile('yuji-recovery-codes.txt',codes.join('\n')+'\n','text/plain')),
   button('已妥善保存','primary-button',()=>d.close()));
  d.append(body);document.body.append(d);d.addEventListener('close',()=>{list.textContent='';codes.length=0;d.remove();});d.showModal();
 }
 async function sessions(panel){
  const box=section(panel,'登录设备','可撤销设备会话，并关闭其终端与文件连接。');
  const list=el('div','audit-list');box.append(list);
  const draw=async()=>{const data=await api(apiRoot+'sessions');if(!box.isConnected)return;list.replaceChildren();
   for(const s of data.sessions){const row=el('div','ops-session'),info=el('div');info.append(el('strong','',s.current?'当前设备':s.source||'来源未记录'),el('p','',s.source||'—'),el('small','',s.device||'设备未记录'),el('p','form-footnote','登录 '+date(s.created)+' · 最近活动 '+date(s.seen)+' · 终端 '+s.terminals));row.append(info,action('退出此设备',async()=>{await api(apiRoot+'sessions/revoke','POST',{id:s.id});if(s.current)location.reload();else await draw();}));list.append(row);}
  };
  box.append(action('退出其他设备',async()=>{await api(apiRoot+'sessions/revoke','POST',{id:'others'});await draw();}));
  try{await draw();}catch(e){list.textContent=e.message;}
  if(getAuth().mfaEnabled){const recovery=section(panel,'备用恢复码','重新生成需要再次验证身份，新代码仅显示一次。');recovery.append(action('重新生成恢复码',async()=>{await reauth();const r=await api(apiRoot+'recovery','POST',{});showRecovery(r.codes);}));}
 }
 async function backups(panel){
  const box=section(panel,'加密备份','备份包含管理员、节点密钥、设置、流量账期与历史记录。请将备份和口令分开保存。');
  const form=el('form','settings-form'),[pass,p]=field('备份口令','password','backupPassphrase','');p.minLength=12;p.maxLength=128;p.required=true;p.autocomplete='new-password';
  const exportButton=el('button','primary-button','加密并下载');exportButton.type='submit';form.append(pass,exportButton);
  form.addEventListener('submit',async e=>{e.preventDefault();exportButton.disabled=true;try{const r=await api(apiRoot+'backup/create','POST',{passphrase:p.value});saveFile(r.name,r.backup);toast('加密备份已生成');}catch(e){toast(e.message,true);}finally{p.value='';exportButton.disabled=false;}});box.append(form);
  const scheduled=section(panel,'自动备份','保存于站点私有目录，按保留数量自动轮换；建议定期下载到其他设备。'),status=el('p','form-footnote');scheduled.append(status);
  try{
   const saved=await api(apiRoot+'backup');if(!scheduled.isConnected)return;
   const f=el('form','settings-form'),[period,periodInput]=select('备份周期','everyHours',[[0,'关闭'],[6,'每 6 小时'],[12,'每 12 小时'],[24,'每天'],[168,'每周']],saved.everyHours);
   const [retention,keep]=field('保留份数','number','keep',saved.keep||7);keep.min=1;keep.max=30;keep.required=true;
   const [pw,pwi]=field('自动备份口令','password','schedulePassphrase','');pwi.placeholder=saved.hasPassphrase?'留空保留已有口令':'至少 12 字节';pwi.autocomplete='new-password';
   const save=el('button','primary-button','保存自动备份');save.type='submit';f.append(period,retention,pw,save);f.addEventListener('submit',async e=>{e.preventDefault();save.disabled=true;try{await api(apiRoot+'backup/schedule','POST',{everyHours:Number(periodInput.value),keep:Number(keep.value),passphrase:pwi.value});toast('自动备份已保存');}catch(e){toast(e.message,true);}finally{pwi.value='';save.disabled=false;}});scheduled.append(f);
   status.textContent='最近备份：'+date(saved.last)+(saved.error?' · '+saved.error:'');
   for(const file of saved.files.sort((a,b)=>b.name.localeCompare(a.name))){const row=el('div','ops-session');row.append(el('span','',file.name+' · '+bytes(file.size)),action('下载',async()=>{const r=await api(apiRoot+'backup/download','POST',{name:file.name});saveFile(r.name,r.backup);}));scheduled.append(row);}
  }catch(e){status.textContent=e.message;}
  const restore=section(panel,'恢复与迁移','保留当前站点域名。域名变化时，新主控会通过专用 SSH 恢复密钥修正 Agent 地址，并轮换节点凭据。');
  const rf=el('form','settings-form'),[upload,u]=field('选择加密备份','file','restoreFile','');u.accept='.backup,application/json';u.required=true;
  const [pw,pwi]=field('备份口令','password','restorePassphrase','');pwi.required=true;pwi.autocomplete='off';
  const confirm=el('label','check-field'),check=el('input');check.type='checkbox';check.required=true;confirm.append(check,document.createTextNode('确认替换当前管理员、节点与设置，并使用备份账户重新登录'));
  const submit=el('button','primary-button','验证并恢复');submit.type='submit';
  rf.append(upload,pw,confirm,el('p','form-footnote','迁移前应已升级旧节点以安装恢复密钥，并允许新主控访问节点 SSH 端口。'),submit);restore.append(rf);
  rf.addEventListener('submit',async e=>{e.preventDefault();submit.disabled=true;try{if(!u.files[0]||u.files[0].size>24*1024*1024)throw Error('备份文件须小于 24 MiB');const backup=JSON.parse(await u.files[0].text());const r=await api(apiRoot+'restore','POST',{backup,passphrase:pwi.value,confirm:check.checked});toast(r.message);location.reload();}catch(e){toast(e.message,true);}finally{pwi.value='';submit.disabled=false;}});
 }
 function policies(panel){
  const box=section(panel,'节点策略','权限、维护窗口、资源告警与流量账期。'),[nodeSelect,selectNode]=select('服务器','policyNode',getRecords().filter(n=>!n.removing).map(n=>[n.public.id,n.public.name]),getRecords()[0]?.public.id),area=el('div');
  box.append(nodeSelect,area);
  function draw(){
   const n=getRecords().find(n=>n.public.id===selectNode.value);area.replaceChildren();if(!n)return;
   const p=n.policy||{},form=el('form','settings-form ops-form-grid'),entries={};
   const add=(label,type,name,value,min,max)=>{const [w,i]=field(label,type,name,value);if(min!==undefined)i.min=min;if(max!==undefined)i.max=max;entries[name]=i;form.append(w);return i;};
   const [terminal,terminalInput]=select('浏览器终端','terminal',[[1,'允许'],[0,'关闭']],p.terminal===false?0:1);
   const [files,filesInput]=select('文件访问','files',[['write','浏览与编辑'],['read','只读'],['off','关闭']],p.files||'write');form.append(terminal,files);
   form.append(el('p','form-footnote ops-span','需要严格只读时，同时关闭终端。文件只读选项限制内置文件编辑器。'));
   add('维护截止时间','datetime-local','maintenanceUntil',p.maintenanceUntil?new Date(p.maintenanceUntil*1000-new Date().getTimezoneOffset()*60000).toISOString().slice(0,16):'');
   for(const [name,label]of [['cpu','CPU 阈值 %'],['memory','内存阈值 %'],['disk','磁盘阈值 %']])add(label+'（0 关闭）','number',name,p[name]||0,0,100);
   add('持续超限秒数','number','sustainedSeconds',p.sustainedSeconds||300,10,3600);
   add('重复通知间隔秒数','number','repeatSeconds',p.repeatSeconds||3600,300,86400);
   add('每月账期开始日（UTC）','number','billingDay',p.billingDay||1,1,28);
   const quota=add('每账期额度 GiB（0 不限制）','number','quota',Number(p.quotaBytes||0)/1073741824,0);quota.step='0.01';
   add('流量提醒阈值 %','number','trafficPercent',p.trafficPercent||80,1,100);
   const [mode,modeInput]=select('流量统计方向','trafficMode',[['both','上传 + 下载'],['tx','仅上传'],['rx','仅下载']],p.trafficMode||'both');form.append(mode);
   const names=add('统计网卡（逗号分隔，留空为物理网卡）','text','interfaces',(p.interfaces||[]).join(','));
   names.placeholder='eth0, ens3';form.append(el('p','form-footnote ops-span','修改账期日或统计网卡会开始新的统计周期。重启、网卡重建期间未采集的流量无法补算。'));
   const save=el('button','primary-button','保存节点策略');save.type='submit';form.append(save);area.append(form);
   form.addEventListener('submit',async e=>{e.preventDefault();save.disabled=true;try{
    const policy={...p,terminal:terminalInput.value==='1',files:filesInput.value,maintenanceUntil:entries.maintenanceUntil.value?Math.floor(new Date(entries.maintenanceUntil.value).getTime()/1000):0,cpu:Number(entries.cpu.value),memory:Number(entries.memory.value),disk:Number(entries.disk.value),sustainedSeconds:Number(entries.sustainedSeconds.value),repeatSeconds:Number(entries.repeatSeconds.value),billingDay:Number(entries.billingDay.value),quotaBytes:Math.round(Number(quota.value)*1073741824),trafficPercent:Number(entries.trafficPercent.value),trafficMode:modeInput.value,interfaces:names.value.split(',').map(v=>v.trim()).filter(Boolean),trafficRevision:p.trafficRevision||''};
    await api(apiRoot+'policy','POST',{id:n.public.id,policy});await refresh();toast('节点策略已保存，原远程会话已关闭');
   }catch(e){toast(e.message,true);}finally{save.disabled=false;}});
  }selectNode.addEventListener('change',draw);draw();
 }
 async function versions(panel){
  const box=section(panel,'版本与迁移','升级由管理员手动触发。签名与完整性校验通过后才会安装。'),summary=el('p');box.append(summary);
  const list=el('div');box.append(list);
  async function draw(){
   const r=await api(apiRoot+'status');if(!box.isConnected)return;summary.textContent='主控 '+r.version+' · 配套 Agent '+r.agentVersion;list.replaceChildren();
   for(const n of getRecords()){const row=el('div','ops-session'),text=el('div');text.append(el('strong','',n.public.name),el('p','',n.agentVersion||'待接入'));
    const t=r.transfers.find(t=>t.id===n.public.id);if(t)text.append(el('p','form-footnote',t.state+' · '+t.message));
    row.append(text);const actions=el('div','row-actions');
    if(!n.removing){actions.append(action('升级 Agent',async()=>{await api(apiRoot+'agent-update','POST',{id:n.public.id});await draw();}),action('回退 Agent',async()=>{await api(apiRoot+'agent-rollback','POST',{id:n.public.id});await draw();}));}
    if(t?.state==='retry')actions.append(action('重试',async()=>{await api(apiRoot+'migration/retry','POST',{id:n.public.id});await draw();}));
    row.append(actions);list.append(row);
   }
  }
  box.append(action('刷新任务状态',draw));
  try{await draw();}catch(e){summary.textContent=e.message;}
  const panelBox=section(panel,'主控更新','更新前自动保留程序与私有状态快照；失败时恢复原版本。'),status=el('p','form-footnote');panelBox.append(status);
  panelBox.append(action('检查 GitHub 新版',async()=>{const r=await api(apiRoot+'update-check');status.textContent='当前 '+r.current+' · 最新 '+r.latest;
   if(r.available)panelBox.append(action('更新到 '+r.latest,async()=>{await api(apiRoot+'panel-update','POST',{version:r.latest});status.textContent='更新请求已提交。服务重启后请刷新页面。';},'primary-button'));
  }),action('回退上次主控版本',async()=>{await api(apiRoot+'panel-rollback','POST',{});status.textContent='回退请求已提交，请稍后刷新。';}),
  action('查看更新结果',async()=>{const r=await api(apiRoot+'update-status');status.textContent=r.message||r.state;}));
 }
 function history(panel,id){
  const box=section(panel,'历史与流量','24 小时保留分钟采样，7 / 30 天显示 15 分钟采样。'),bar=el('div','history-toolbar');
  const [range,ri]=select('时间范围','historyDays',[[1,'24 小时'],[7,'7 天'],[30,'30 天']],1);
  const [metric,mi]=select('指标','historyMetric',[['cpu','CPU %'],['memory','内存 %'],['disk','磁盘 %'],['rx','下载速率'],['tx','上传速率']],'cpu');
  const graph=el('canvas','history-canvas');graph.width=900;graph.height=220;graph.setAttribute('role','img');graph.setAttribute('aria-label','服务器历史曲线');
  const detail=el('p','history-cursor'),summary=el('p','form-footnote'),events=el('details');events.append(el('summary','','上下线记录'));const eventList=el('div','audit-list');events.append(eventList);
  bar.append(range,metric,action('刷新',load));box.append(bar,summary,graph,detail,events);let points=[];box.dataset.historyNode=id;
  function plot(){const ctx=graph.getContext('2d'),key=mi.value,network=['rx','tx'].includes(key),max=network?Math.max(1,...points.map(p=>Number(p[key])||0)):100;ctx.clearRect(0,0,900,220);ctx.strokeStyle='rgba(93,117,133,.18)';ctx.lineWidth=1;
   for(let j=0;j<5;j++){const y=15+j*45;ctx.beginPath();ctx.moveTo(45,y);ctx.lineTo(890,y);ctx.stroke();ctx.fillStyle='#607785';ctx.font='12px system-ui';ctx.fillText(network?bytes(max*(4-j)/4):String(max*(4-j)/4),0,y+4);}
   ctx.strokeStyle='#327e89';ctx.lineWidth=2;ctx.beginPath();let active=false;points.forEach((p,k)=>{if(!p.online){active=false;return;}const x=45+845*k/Math.max(1,points.length-1),y=195-180*(Number(p[key])||0)/max;if(active)ctx.lineTo(x,y);else ctx.moveTo(x,y);active=true;});ctx.stroke();
   detail.textContent=points.length?date(points[0].at)+' — '+date(points.at(-1).at):'暂无历史数据，接入后自动开始记录';
  }
  async function load(){try{const r=await api(apiRoot+'history','POST',{id,days:Number(ri.value)});if(!box.isConnected)return;points=r.points;plot();summary.textContent='账期 '+(r.period||'尚未开始')+' · 下载 '+bytes(r.rx)+' · 上传 '+bytes(r.tx)+' · 已用 '+bytes(r.used)+(r.quota?' / '+bytes(r.quota):'')+' · 所选范围已观测离线 '+Math.round(r.downtimeSeconds/60)+' 分钟';eventList.replaceChildren(...r.incidents.slice(-100).reverse().map(e=>el('p','',date(e.at)+' · '+(e.online?'上线':'离线'))));}catch(e){summary.textContent=e.message;}}
  graph.addEventListener('pointermove',e=>{if(!points.length)return;const x=(e.clientX-graph.getBoundingClientRect().left)/graph.clientWidth;const p=points[Math.max(0,Math.min(points.length-1,Math.round((x*900-45)/845*(points.length-1))))];detail.textContent=date(p.at)+' · '+(p.online?(['rx','tx'].includes(mi.value)?bytes(p[mi.value])+'/s':Number(p[mi.value]).toFixed(1)+'%'):'离线');});
  ri.addEventListener('change',load);mi.addEventListener('change',plot);load();
 }
 function clear(){for(const d of document.querySelectorAll('.ops-dialog'))d.close();}
 return {reauth,showRecovery,sessions,backups,policies,versions,history,clear};
}
