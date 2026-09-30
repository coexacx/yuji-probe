import {authenticatorCanvas} from "/assets/authenticator-19965621ba87.js";
import {createTerminalUI} from "/assets/terminal-1b3b4109b1f1.js";
import {countries as allCountries} from "/assets/countries-3dabb5bbec57.js";
import {createRenewalUI,formatExpiry,localExpiry} from "/assets/renewals-db1c1f262c23.js";
export function initAdmin(hooks){
 const {el,icon,flag}=hooks,$=s=>document.querySelector(s);
 let auth={authenticated:false},csrf='',records=[],site={name:'Vistart Probe',public:true,refreshSeconds:5},activeTab='overview',adminOpen=false,removeId=null,terminalId=null,commands=[],toastTimer,loading=false;
 const countries=Object.fromEntries([['OTHER','待识别'],...allCountries.map(c=>[c.code,c.name])]);
 const labels={node_renewal_stopped:'停止服务器续费提醒',node_renewed:'确认服务器续费',file_read:'查看远程文件',file_saved:'保存远程文件',telegram_updated:'修改 Telegram 设置',telegram_test:'测试 Telegram 通知',telegram_sent:'Telegram 通知已发送',telegram_failed:'Telegram 通知发送失败',login:'管理员登录',login_failed:'登录验证失败',logout:'管理员退出',node_created:'添加服务器',node_updated:'更新服务器',node_deleted:'移除服务器',deploy_started:'开始部署',deploy_done:'部署完成',deploy_failed:'部署失败',ssh_host_trusted:'信任主机',terminal_opened:'连接 SSH',terminal_closed:'关闭 SSH',real_mode_enabled:'启用真实监控',command_created:'添加常用命令',command_updated:'修改常用命令',command_deleted:'移除常用命令',command_executed:'执行常用命令',site_updated:'修改面板设置',password_changed:'修改管理员密码',mfa_enable:'启用二步验证',mfa_disable:'关闭二步验证',agent_connected:'Agent 上线',agent_disconnected:'Agent 离线',deploy_started:'开始部署 Agent',deploy_completed:'Agent 部署完成'};
 function toast(text,error=false){clearTimeout(toastTimer);$('#toast').textContent=text;$('#toast').classList.toggle('is-error',error);$('#toast').hidden=false;toastTimer=setTimeout(()=>$('#toast').hidden=true,6000);}
 let sessionEpoch=0,sessionGate=null,siteEpoch=0;
 const sessionEndpoints=new Set(['/api/session','/api/login','/api/logout','/api/admin/password','/api/admin/mfa/enable','/api/admin/mfa/disable']);
 async function api(path,method='GET',body){
  // Serialize cookie rotation, and discard responses issued under an older session.
  // A delayed monitoring poll must not sign out a freshly authenticated administrator.
  while(sessionGate)await sessionGate;
  const changesSession=sessionEndpoints.has(path);let release;
  if(changesSession){sessionEpoch++;sessionGate=new Promise(resolve=>{release=resolve;});}
  const epoch=sessionEpoch;
  try{
   const options={method,credentials:'same-origin',headers:{'Content-Type':'application/json'}};
   if(method!=='GET'){options.headers['X-CSRF-Token']=csrf;if(body!==undefined)options.body=JSON.stringify(body);}
   let response;
   try{response=await fetch(path,options);}catch{throw Error('连接主控失败，请稍后重试');}
   const data=await response.json().catch(()=>({error:'主控响应异常'}));
   if(epoch!==sessionEpoch){const error=Error('会话已更新，请重试');error.staleSession=true;throw error;}
   if(!response.ok){
    if(response.status===401&&path!=='/api/login')loseAuth();
    const error=Error(data.error||'请求失败');error.status=response.status;throw error;
   }
   if(changesSession&&typeof data.csrf==='string'){auth=data;csrf=data.csrf;authUI();}
   return data;
  }finally{if(release){sessionGate=null;release();}}
 }

 function closeDialog(id){const d=$(id);if(d.open)d.close();}
 function wireDialog(dialog){for(const button of dialog.querySelectorAll('.dialog-dismiss')){button.replaceChildren(icon('close'));button.addEventListener('click',()=>dialog.close());}dialog.addEventListener('keydown',e=>{if(e.key!=='Tab')return;const controls=Array.from(dialog.querySelectorAll('button:not([disabled]),a[href],input:not([disabled]),select:not([disabled]),textarea:not([disabled]),[tabindex]:not([tabindex="-1"])')).filter(e=>e.getClientRects().length>0);const first=controls[0],last=controls.at(-1);if(!first)return;if(controls.length===1||(e.shiftKey&&document.activeElement===first)||(!e.shiftKey&&document.activeElement===last)){e.preventDefault();(e.shiftKey?last:first).focus();}});}
 for(const id of ['#login-dialog','#node-editor','#delete-dialog','#terminal-dialog','#command-editor','#trust-dialog','#terminal-paste-dialog','#renewal-dialog'])wireDialog($(id));
 function authUI(){$('#login-mfa-field').hidden=!auth.mfaRequired;$('#login-form').elements.code.required=!!auth.mfaRequired;const admin=auth.authenticated;$('#admin-badge').hidden=!admin;$('#admin-entry').setAttribute('aria-label',admin?'打开管理后台':'管理员登录');$('#admin-entry').replaceChildren(icon('settings'));}
 function loseAuth(){renewalUI.clear();sessionEpoch++;auth={authenticated:false};csrf='';records=[];commands=[];$('#command-form').reset();adminOpen=false;$('#admin-view').hidden=true;$('#admin-view').replaceChildren();$('#main-content').hidden=false;for(const p of document.querySelectorAll('.admin-ip-row'))p.remove();for(const id of ['#node-editor','#delete-dialog','#terminal-dialog','#command-editor','#trust-dialog','#terminal-paste-dialog'])closeDialog(id);$('#node-form').reset();$('#terminal-target').textContent='';authUI();hooks.onLogout?.();}
 async function refresh(background=false){if(loading)return;loading=true;try{while(sessionGate)await sessionGate;const asAdmin=auth.authenticated,epoch=sessionEpoch,siteVersion=siteEpoch;const result=await api(asAdmin?'/api/admin/nodes':'/api/public/nodes');if(epoch!==sessionEpoch)return;if(siteVersion===siteEpoch)site=result.site;records=asAdmin?result.nodes:[];const publicNodes=asAdmin?records.map(n=>({...n.public,demo:n.demo})):result.nodes.map(n=>({...n,demo:result.preview&&n.online}));hooks.setData(publicNodes,site,result.preview);renewalUI.update(records);if(adminOpen&&(!background||['overview','servers'].includes(activeTab)))renderAdmin();}catch(error){if(error.staleSession)return;hooks.onLoadError(error.message);if(error.status!==403&&error.status!==401)toast(error.message,true);}finally{loading=false;}}
 function openLogin(){if(auth.authenticated){openAdmin();return;}$('#login-error').textContent='';$('#login-dialog').showModal();$('#login-form [name="username"]').focus();api('/api/session').then(v=>{auth=v;csrf=v.csrf;authUI();}).catch(e=>$('#login-error').textContent=e.message);}
 $('#login-dialog').addEventListener('close',()=>{$('#login-form [name="password"]').value='';$('#login-form').elements.code.value='';});
 $('#login-form').addEventListener('submit',async event=>{event.preventDefault();const f=event.currentTarget,b=f.querySelector('button[type="submit"]');b.disabled=true;$('#login-error').textContent='';try{auth=await api('/api/login','POST',{username:f.elements.username.value.trim(),password:f.elements.password.value,code:f.elements.code.value});csrf=auth.csrf;authUI();closeDialog('#login-dialog');await refresh();openAdmin();toast('管理员登录成功');}catch(error){$('#login-error').textContent=error.message;f.elements.password.value='';}finally{b.disabled=false;}});
 $('#admin-entry').addEventListener('click',()=>auth.authenticated?openAdmin():openLogin());$('#admin-badge').addEventListener('click',()=>openAdmin());$('.login-symbol').replaceChildren(icon('shield'));$('.terminal-mark').replaceChildren(icon('terminal'));$('.terminal-placeholder-icon').replaceChildren(icon('terminal'));
 function button(label,cls,action,glyph){const b=el('button',cls);b.type='button';if(glyph)b.append(icon(glyph));b.append(document.createTextNode(label));b.addEventListener('click',action);return b;}
 function openAdmin(tab='overview'){if(!auth.authenticated){openLogin();return;}activeTab=tab;adminOpen=true;$('#main-content').hidden=true;$('#admin-view').hidden=false;renderAdmin();window.scrollTo({top:0});}
 function closeAdmin(){adminOpen=false;$('#admin-view').hidden=true;$('#main-content').hidden=false;window.scrollTo({top:0});}
 $('.brand').addEventListener('click',event=>{if(adminOpen){event.preventDefault();closeAdmin();}});
 function status(record){const n=record.public;const s=el('span','status-badge'+(n.online?'':' is-offline'));s.append(el('i','status-dot '+(n.online?'online':'offline')),document.createTextNode(n.pending?'待接入':n.online?'在线':'离线'));return s;}
 function nodeName(record){const wrap=el('div','admin-node-name');const f=el('span','flag-wrap');f.append(flag(record.public.code));const name=el('div');name.append(el('strong','',record.public.name),el('small','',record.public.country));wrap.append(f,name);return wrap;}
 function nodeTable(compact=false){const wrap=el('div','table-scroll');const table=el('table','node-table');const thead=el('thead'),head=el('tr');for(const label of compact?['服务器','SSH 地址','状态','操作']:['服务器','SSH 地址','状态','公开展示','操作'])head.append(el('th','',label));thead.append(head);const body=el('tbody');for(const r of records){const tr=el('tr');const name=el('td');name.append(nodeName(r));const addr=el('td','address-cell');addr.append(el('code','',r.ip),el('small','',r.username+' · '+r.port));const state=el('td');state.append(status(r));if(r.expiresAt)state.append(el('small','agent-version'+(r.renewalDue?' expiry-due':''),'到期 '+formatExpiry(r.expiresAt)));if(r.agentVersion)state.append(el('small','agent-version','Agent '+r.agentVersion));if(r.deployMessage)state.append(el('small','deploy-status '+r.deployState,r.deployMessage));tr.append(name,addr,state);if(!compact)tr.append(el('td','',r.visible?'公开':'仅管理员'));const ops=el('td');const buttons=el('div','row-actions');buttons.append(button('SSH','table-button',()=>openTerminal(r.public.id),'terminal'));if(!compact){buttons.append(button('编辑','table-button',()=>openEditor(r.public.id),'edit'),button('移除','table-button danger-text',()=>confirmDelete(r.public.id),'trash'));}ops.append(buttons);tr.append(ops);body.append(tr);}table.append(thead,body);wrap.append(table);if(!records.length)wrap.append(el('p','admin-empty','还没有服务器，添加第一台开始监控。'));return wrap;}
 function sectionHeading(title,description){const h=el('div','admin-section-heading');const text=el('div');text.append(el('h2','',title));if(description)text.append(el('p','',description));h.append(text);return h;}
 function overview(panel){const stats=el('div','admin-stats');for(const [label,value,glyph]of [['服务器',records.length,'server'],['在线',records.filter(n=>n.public.online).length,'activity'],['待接入',records.filter(n=>n.public.pending).length,'plus'],['离线',records.filter(n=>!n.public.online&&!n.public.pending).length,'clock']]){const item=el('div','glass stat-tile');const top=el('span');top.append(icon(glyph),document.createTextNode(label));item.append(top,el('strong','',value));stats.append(item);}panel.append(stats);const box=el('section','glass admin-box');const heading=sectionHeading('节点概况','管理员可见 IP 与 SSH 入口');heading.append(button('添加服务器','primary-button',()=>openEditor(),'plus'));box.append(heading,nodeTable(true));panel.append(box);const note=el('section','glass connection-note');note.append(icon('shield'),el('div','','Agent 通过独立 WSS 通道上报。浏览器终端须经过管理员验证与节点授权。'));panel.append(note);}
 function servers(panel){const box=el('section','glass admin-box');const heading=sectionHeading('服务器管理',records.length+' 台服务器');heading.append(button('添加服务器','primary-button',()=>openEditor(),'plus'));box.append(heading,nodeTable());panel.append(box);}
 function field(label,type,name,value){const wrap=el('label','form-field',label);const input=el('input');input.type=type;input.name=name;input.value=String(value??'');wrap.append(input);return [wrap,input];}
 async function commandSettings(panel){const box=el('section','glass admin-box');const heading=sectionHeading('常用命令','在 SSH 终端中快速调用');heading.append(button('添加命令','primary-button',()=>editCommand(),'plus'));box.append(heading);const list=el('div','command-list');box.append(list);panel.append(box);try{commands=(await api('/api/admin/commands')).commands;if(!box.isConnected)return;for(const command of commands){const row=el('article','command-item');const info=el('div','command-info');info.append(el('strong','',command.name),el('pre','',command.script));const actions=el('div','row-actions');actions.append(button('编辑','table-button',()=>editCommand(command.id),'edit'),button('删除','table-button danger-text',async()=>{await removeCommand(command.id);},'trash'));row.append(info,actions);list.append(row);}if(!commands.length)list.append(el('p','admin-empty','还没有常用命令，添加后即可在终端中使用。'));}catch(e){list.textContent=e.message;}}
 function editCommand(id){const c=commands.find(c=>c.id===id),form=$('#command-form');form.reset();form.elements.id.value=id||'';form.elements.name.value=c?.name||'';form.elements.script.value=c?.script||'';$('#command-error').textContent='';$('#command-editor-title').textContent=c?'编辑常用命令':'添加常用命令';$('#command-editor').showModal();form.elements.name.focus();}
 async function removeCommand(id){try{await api('/api/admin/commands/'+encodeURIComponent(id),'DELETE');renderAdmin();toast('命令已删除');}catch(e){toast(e.message,true);}}
 $('#command-form').addEventListener('submit',async event=>{event.preventDefault();const f=event.currentTarget,b=f.querySelector('button[type="submit"]');b.disabled=true;$('#command-error').textContent='';try{const id=f.elements.id.value;await api('/api/admin/commands'+(id?'/'+encodeURIComponent(id):''),id?'PATCH':'POST',{name:f.elements.name.value,script:f.elements.script.value});closeDialog('#command-editor');renderAdmin();toast('常用命令已保存');}catch(e){$('#command-error').textContent=e.message;}finally{b.disabled=false;}});
 function siteSettings(panel){const box=el('section','glass admin-box settings-box');box.append(sectionHeading('面板设置','管理公开看板与显示选项'));const form=el('form','settings-form');const [name,nameInput]=field('站点名称','text','name',site.name);nameInput.required=true;nameInput.maxLength=60;const [refreshField,refreshInput]=field('刷新间隔（秒）','number','refreshSeconds',site.refreshSeconds);refreshInput.min='3';refreshInput.max='30';refreshInput.required=true;const check=el('label','check-field');const input=el('input');input.type='checkbox';input.name='public';input.checked=site.public;check.append(input,document.createTextNode('允许访客查看服务器看板'));form.append(name,refreshField,check,el('p','form-footnote','IP、SSH 信息与管理操作始终仅管理员可见。'));const save=el('button','primary-button','保存设置');save.type='submit';form.append(save);form.addEventListener('submit',async e=>{e.preventDefault();save.disabled=true;try{const result=await api('/api/admin/site','PUT',{name:nameInput.value.trim(),refreshSeconds:Number(refreshInput.value),public:input.checked});siteEpoch++;site=result.site;hooks.setSite?.(site);await refresh();toast('面板设置已保存');}catch(e){toast(e.message,true);}finally{save.disabled=false;}});box.append(form);panel.append(box);mfaSettings(panel);}
 async function notificationSettings(panel){
  const box=el('section','glass admin-box settings-box');
  box.append(sectionHeading('Telegram 通知','服务器状态与续费提醒'));
  const loading=el('p','form-footnote','正在读取设置…');box.append(loading);panel.append(box);
  try{
   let saved=await api('/api/admin/telegram');
   if(!box.isConnected)return;
   loading.remove();
   const form=el('form','settings-form');form.autocomplete='off';
   const check=(label,name,checked)=>{const wrap=el('label','check-field'),input=el('input');input.type='checkbox';input.name=name;input.checked=checked;wrap.append(input,document.createTextNode(label));return [wrap,input];};
   const [enabledField,enabled]=check('启用 Telegram 通知','enabled',saved.enabled);
   const [tokenField,tokenInput]=field('Bot Token','password','telegramToken','');
   tokenInput.autocomplete='new-password';tokenInput.spellcheck=false;tokenInput.maxLength=160;
   const [chatField,chat]=field('接收人 ID','text','chatId',saved.chatId);
   chat.placeholder='个人或群组的 Chat ID';chat.maxLength=17;chat.autocomplete='off';chat.spellcheck=false;
   const events=el('fieldset','notification-events');events.append(el('legend','','通知事件'));
   const [onlineField,online]=check('服务器上线','notifyOnline',saved.notifyOnline);
   const [offlineField,offline]=check('服务器离线','notifyOffline',saved.notifyOffline);
   const [renewalField,renewal]=check('服务器续费提醒','notifyRenewal',saved.notifyRenewal);events.append(onlineField,offlineField,renewalField);
   const [clearField,clear]=check('清除已保存的 Token','clearToken',false);
   const note=el('p','form-footnote','先向 Bot 发送 /start。离线持续 20 秒后通知，短暂重连不提醒。续费按服务器设置，按 UTC+8 日期提前 3 天触发，同日到期的服务器合并通知，每个到期周期通知一次。');
   const error=el('p','form-error');error.setAttribute('role','alert');
   const statusBox=el('div','notification-status');statusBox.setAttribute('role','status');statusBox.setAttribute('aria-live','polite');
   const actions=el('div','notification-actions');
   const save=el('button','primary-button','保存设置');save.type='submit';
   const test=el('button','secondary-button','发送测试通知');test.type='button';
   const refreshStatus=button('刷新状态','table-button',async()=>{try{updateStatus(await api('/api/admin/telegram'));}catch(e){error.textContent=e.message;}});
   actions.append(save,test);
   let dirty=false,busy=false;
   function updateStatus(v){
    saved=v;
    const lines=[];
    if(v.pending)lines.push('待发送：'+v.pending+' 条');
    if(v.test?.status==='pending')lines.push('测试通知正在发送…');
    if(v.test?.status==='sent')lines.push('测试通知已发送');
    if(v.test?.status==='failed')lines.push('测试失败：'+v.test.error);
    if(v.lastSuccess)lines.push('最近发送：'+new Date(v.lastSuccess*1000).toLocaleString('zh-CN',{hour12:false}));
    if(v.lastError)lines.push(v.lastError);
    if(v.dropped)lines.push('队列满时未发送的事件：'+v.dropped+' 条');
    if(!lines.length)lines.push(v.hasToken?'尚未发送通知':'尚未配置');
    statusBox.replaceChildren(...lines.map(text=>el('p','',text)),refreshStatus);
    test.disabled=busy||dirty||!v.hasToken||!v.chatId||v.test?.status==='pending';
   }
   function reflectSaved(v){
    tokenInput.value='';tokenInput.placeholder=v.hasToken?'已保存，留空则保留':'填写 BotFather 提供的 Token';
    clear.checked=false;clearField.hidden=!v.hasToken;dirty=false;updateStatus(v);
   }
   form.addEventListener('input',()=>{dirty=true;test.disabled=true;error.textContent='';});
   clear.addEventListener('change',()=>{if(clear.checked){tokenInput.value='';enabled.checked=false;}});
   form.addEventListener('submit',async e=>{
    e.preventDefault();busy=true;save.disabled=true;test.disabled=true;error.textContent='';
    try{
     const v=await api('/api/admin/telegram','PUT',{enabled:enabled.checked,token:tokenInput.value.trim(),chatId:chat.value.trim(),notifyOnline:online.checked,notifyOffline:offline.checked,notifyRenewal:renewal.checked,clearToken:clear.checked});
     if(!box.isConnected)return;
     reflectSaved(v);toast('Telegram 通知设置已保存');
    }catch(e){error.textContent=e.message;}
    finally{busy=false;save.disabled=false;updateStatus(saved);}
   });
   test.addEventListener('click',async()=>{
    busy=true;test.disabled=true;error.textContent='';
    try{await api('/api/admin/telegram/test','POST',{});updateStatus(await api('/api/admin/telegram'));toast('测试通知已提交');}
    catch(e){error.textContent=e.message;}
    finally{busy=false;updateStatus(saved);}
   });
   form.append(enabledField,tokenField,chatField,events,clearField,note,error,actions,statusBox);box.append(form);reflectSaved(saved);
   async function poll(){
    if(!box.isConnected||!adminOpen||activeTab!=='notifications'||!auth.authenticated)return;
    try{const v=await api('/api/admin/telegram');if(box.isConnected)updateStatus(v);}catch(e){if(!e.staleSession&&box.isConnected)error.textContent=e.message;}
    if(box.isConnected&&adminOpen&&activeTab==='notifications'&&auth.authenticated)setTimeout(poll,5000);
   }
   setTimeout(poll,5000);
  }catch(e){if(box.isConnected)loading.textContent=e.message;}
 }
 function security(panel){const box=el('section','glass admin-box settings-box');box.append(sectionHeading('账户安全','当前管理员：'+auth.username));const status=el('div','security-summary');for(const label of ['服务端密码验证','HttpOnly 安全会话','CSRF 请求校验']){const item=el('span');item.append(icon('check'),document.createTextNode(label));status.append(item);}box.append(status);const form=el('form','settings-form');const [current,currentInput]=field('当前密码','password','current','');currentInput.autocomplete='current-password';currentInput.required=true;const [next,nextInput]=field('新密码','password','new','');nextInput.autocomplete='new-password';nextInput.minLength=12;nextInput.maxLength=72;nextInput.required=true;const [confirm,confirmInput]=field('确认新密码','password','confirm','');confirmInput.autocomplete='new-password';confirmInput.required=true;const error=el('p','form-error');error.setAttribute('role','alert');const save=el('button','primary-button','更新密码');save.type='submit';form.append(current,next,confirm,el('p','form-footnote','更新密码后，其他管理会话会退出登录。'),error,save);form.addEventListener('submit',async event=>{event.preventDefault();error.textContent='';if(nextInput.value!==confirmInput.value){error.textContent='两次填写的新密码不一致';return;}save.disabled=true;try{auth=await api('/api/admin/password','POST',{current:currentInput.value,new:nextInput.value});csrf=auth.csrf;form.reset();toast('密码已更新，其他会话已退出');}catch(e){error.textContent=e.message;}finally{save.disabled=false;}});box.append(form);panel.append(box);}
 function mfaSettings(panel){const box=el('section','glass admin-box settings-box');box.append(sectionHeading('二步验证',auth.mfaEnabled?'已启用 · 登录需要验证器动态验证码':'绑定身份验证器，为管理入口增加一层保护'));const form=el('form','settings-form');const [password,passwordInput]=field('当前密码','password','password','');passwordInput.required=true;passwordInput.autocomplete='current-password';const [code,codeInput]=field('验证器验证码','text','code','');codeInput.inputMode='numeric';codeInput.pattern='[0-9]{6}';codeInput.maxLength=6;codeInput.autocomplete='one-time-code';code.hidden=!auth.mfaEnabled;codeInput.required=!!auth.mfaEnabled;const secret=el('div','mfa-secret');secret.hidden=true;const error=el('p','form-error');error.setAttribute('role','alert');const save=el('button','primary-button',auth.mfaEnabled?'关闭二步验证':'开始绑定');save.type='submit';let preparing=false;form.append(password,secret,code,error,save,el('p','form-footnote','请妥善备份验证器密钥。丢失验证器时需通过服务器管理员恢复。'));form.addEventListener('submit',async e=>{e.preventDefault();save.disabled=true;error.textContent='';try{if(!auth.mfaEnabled&&!preparing){const result=await api('/api/admin/mfa/setup','POST',{password:passwordInput.value});secret.replaceChildren(el('p','','使用身份验证器扫码，或手动填写密钥：'),authenticatorCanvas(result.secret,site.name,auth.username),el('code','',result.secret),el('small','','账户：'+auth.username+' · 6 位数字 · 30 秒'));secret.hidden=false;code.hidden=false;codeInput.required=true;password.hidden=true;passwordInput.required=false;passwordInput.value='';preparing=true;save.textContent='验证并启用';codeInput.focus();}else{auth=await api('/api/admin/mfa/'+(auth.mfaEnabled?'disable':'enable'),'POST',{password:passwordInput.value,code:codeInput.value});csrf=auth.csrf;authUI();renderAdmin();toast(auth.mfaEnabled?'二步验证已启用':'二步验证已关闭');}}catch(e){error.textContent=e.message;}finally{save.disabled=false;passwordInput.value='';}});box.append(form);panel.append(box);}
 async function audit(panel){const box=el('section','glass admin-box');box.append(sectionHeading('操作记录','记录登录、节点管理与设置变更'));const list=el('div','audit-list');box.append(list);panel.append(box);try{const result=await api('/api/admin/audit');if(!box.isConnected)return;for(const event of [...result.events].reverse()){const row=el('div','audit-row');row.append(el('time','',new Date(event.at).toLocaleString('zh-CN',{hour12:false})),el('strong','',labels[event.action]||event.action),el('span','',event.subject));list.append(row);}if(!result.events.length)list.append(el('p','admin-empty','暂无操作记录'));}catch(e){list.textContent=e.message;}}
 function renderAdmin(){if(!auth.authenticated)return;const root=$('#admin-view');const top=el('div','admin-page-heading');const titles=el('div');titles.append(el('div','eyebrow','ADMINISTRATION'),el('h1','','管理后台'));const actions=el('div','admin-heading-actions');actions.append(button('返回看板','secondary-button',closeAdmin,'arrow'),button('退出登录','secondary-button',logout,'logout'));top.append(titles,actions);const layout=el('div','admin-layout');const sidebar=el('nav','glass admin-nav');sidebar.setAttribute('aria-label','管理菜单');for(const [id,label,glyph]of [['overview','概览','grid'],['servers','服务器','server'],['commands','常用命令','terminal'],['settings','面板设置','settings'],['notifications','Telegram 通知','activity'],['security','账户安全','shield'],['audit','操作记录','clock']]){const b=button(label,'admin-nav-item'+(activeTab===id?' active':''),()=>{activeTab=id;renderAdmin();},glyph);b.dataset.tab=id;if(activeTab===id)b.setAttribute('aria-current','page');sidebar.append(b);}const content=el('div','admin-content');layout.append(sidebar,content);root.replaceChildren(top,layout);({overview,servers,commands:commandSettings,settings:siteSettings,notifications:notificationSettings,security,audit}[activeTab]||overview)(content);}
 function openEditor(id){if(!auth.authenticated)return;const r=records.find(n=>n.public.id===id);const form=$('#node-form');form.reset();form.elements.id.value=id||'';form.elements.name.value=r?.public.name||'';form.elements.ip.value=r?.ip||'';form.elements.port.value=r?.port||22;form.elements.username.value=r?.username||'root';form.elements.password.value='';form.elements.code.value=r?.countryAuto?'OTHER':r?.public.code||'OTHER';form.elements.password.required=!r;form.elements.password.placeholder=r?'留空保留，填写重新部署':'仅用于安装 Agent';$('#node-save').textContent=r?'保存服务器':'安装并接入';form.elements.city.value=r?.public.city==='待识别'?'':r?.public.city||'';form.elements.visible.checked=r?r.visible:true;form.elements.providerName.value=r?.providerName||'';form.elements.providerURL.value=r?.providerURL||'';form.elements.expiresAt.value=localExpiry(r?.expiresAt);form.elements.notifyRenewal.checked=!!r?.notifyRenewal;form.elements.renewalVersion.value=r?.renewalVersion||'';$('#editor-title').textContent=r?'编辑服务器':'添加服务器';$('#node-error').textContent='';$('#node-editor').showModal();form.elements.name.focus();}
 $('#node-editor').addEventListener('close',()=>$('#node-form').elements.password.value='');
 function trustHost(fingerprint){return new Promise(resolve=>{const d=$('#trust-dialog');$('#ssh-fingerprint').textContent=fingerprint;const finish=value=>{d.close();resolve(value);};$('#trust-cancel').onclick=()=>finish(false);$('#trust-confirm').onclick=()=>finish(true);d.oncancel=()=>resolve(false);d.showModal();});}
 async function watchDeployment(id){try{for(let i=0;i<70;i++){await new Promise(r=>setTimeout(r,4000));if(!auth.authenticated)return;const {job}=await api('/api/admin/deploy/'+encodeURIComponent(id));await refresh();if(job.state!=='running'){toast(job.message,job.state!=='done');return;}}}catch(e){toast(e.message,true);}}
 $('#node-form').addEventListener('submit',async event=>{event.preventDefault();const f=event.currentTarget,id=f.elements.id.value,save=$('#node-save');save.disabled=true;$('#node-error').textContent='';let password=f.elements.password.value;try{const code=f.elements.code.value;if(password){$('#deployment-note').textContent='正在检查 SSH 主机…';const inspection=await api('/api/admin/inspect-ssh','POST',{ip:f.elements.ip.value.trim(),port:Number(f.elements.port.value)});if(!inspection.trusted){if(!await trustHost(inspection.fingerprint)){throw Error('已取消安装。');}await api('/api/admin/trust-ssh','POST',{inspection:inspection.inspection});}}
 const result=await api('/api/admin/nodes'+(id?'/'+encodeURIComponent(id):''),id?'PATCH':'POST',{name:f.elements.name.value.trim(),ip:f.elements.ip.value.trim(),port:Number(f.elements.port.value),username:f.elements.username.value.trim(),password:'',country:countries[code]||code,city:f.elements.city.value.trim(),code,visible:f.elements.visible.checked,providerName:f.elements.providerName.value.trim(),providerURL:f.elements.providerURL.value.trim(),expiresAt:f.elements.expiresAt.value?(f.elements.expiresAt.value===localExpiry(records.find(r=>r.public.id===id)?.expiresAt)?records.find(r=>r.public.id===id).expiresAt:new Date(f.elements.expiresAt.value).toISOString()):'',notifyRenewal:f.elements.notifyRenewal.checked,renewalVersion:f.elements.renewalVersion.value});f.elements.id.value=result.node.public.id;
 if(password){const deployment=await api('/api/admin/deploy','POST',{id:result.node.public.id,password});watchDeployment(deployment.job.id);toast('部署已开始，可在服务器列表查看进度。');}else toast('服务器已保存');closeDialog('#node-editor');await refresh();}catch(e){$('#node-error').textContent=e.message;}finally{password='';f.elements.password.value='';save.disabled=false;$('#deployment-note').textContent='填写 SSH 信息后自动安装探针。密码仅用于本次安装。';}});
 function confirmDelete(id){const record=records.find(r=>r.public.id===id);if(!record)return;removeId=id;$('#delete-description').textContent='确认移除 '+record.public.name+'？';$('#delete-dialog').showModal();}
 $('#delete-cancel').addEventListener('click',()=>closeDialog('#delete-dialog'));$('#delete-confirm').addEventListener('click',async()=>{const b=$('#delete-confirm');b.disabled=true;try{await api('/api/admin/nodes/'+encodeURIComponent(removeId),'DELETE');closeDialog('#delete-dialog');await refresh();toast('服务器已移除');}catch(e){toast(e.message,true);}finally{b.disabled=false;}});
 async function logout(){try{const result=await api('/api/logout','POST',{});terminalUI.close();loseAuth();auth=result;csrf=result.csrf||'';await refresh();toast('已退出管理员登录');}catch(e){toast(e.message,true);}}
 const terminalUI=createTerminalUI(api);
 function openTerminal(id){const record=records.find(n=>n.public.id===id);if(auth.authenticated&&record)terminalUI.open(record);}
 function decorateCard(card,node){if(!auth.authenticated)return;const record=records.find(n=>n.public.id===node.id);if(!record)return;const line=el('div','admin-ip-row');const ip=el('span');ip.append(icon('shield'),el('code','',record.ip));line.append(ip,button('SSH','ssh-button',()=>openTerminal(node.id),'terminal'));card.insertBefore(line,card.querySelector('.card-footer'));}
 const codeSelect=$('#node-form').elements.code;codeSelect.replaceChildren();const automatic=el('option','','自动识别');automatic.value='OTHER';codeSelect.append(automatic);for(const country of [...allCountries].sort((a,b)=>a.name.localeCompare(b.name,'zh-CN'))){const option=el('option','',country.name+' · '+country.code);option.value=country.code;codeSelect.append(option);}
 const renewalUI=createRenewalUI({el,api,isAdmin:()=>auth.authenticated,onChanged:()=>refresh(),toast});
 function decorateDetail(root,node){
  if(!auth.authenticated)return;const record=records.find(r=>r.public.id===node.id);if(!record)return;
  const section=el('section','admin-node-details');const heading=el('div','section-heading');heading.append(el('h3','','管理信息'),el('span','','仅管理员可见'));section.append(heading);const fields=el('div','detail-hardware');
  for(const [label,value]of [['供应商',record.providerName||'未设置'],['供应商站点',record.providerURL||'未设置'],['到期时间',formatExpiry(record.expiresAt)],['续费提醒',record.notifyRenewal?'已开启 · 提前 3 天':'已关闭'],['Agent 版本',record.agentVersion||'待上报']]){
   const item=el('dl'),content=el('dd');item.append(el('dt','',label),content);
   if(label==='供应商站点'&&record.providerURL){let url;try{url=new URL(record.providerURL);}catch{}if(url&&['https:','http:'].includes(url.protocol)&&!url.username&&!url.password){const a=el('a','provider-link',value);a.href=url.href;a.target='_blank';a.rel='noopener noreferrer';content.append(a);}else content.textContent='未设置';}else content.textContent=value;
   fields.append(item);
  }
  section.append(fields);root.append(section);
 }
 authUI();api('/api/session').then(v=>{auth=v;csrf=v.csrf;authUI();if(new URLSearchParams(location.search).get('login')==='1'){history.replaceState(null,'',location.pathname);openLogin();}return refresh();}).catch(e=>hooks.onLoadError(e.message));
 return {decorateCard,decorateDetail,isAdmin:()=>auth.authenticated,refresh,api,toast,getRecords:()=>records,openTerminal};
}
