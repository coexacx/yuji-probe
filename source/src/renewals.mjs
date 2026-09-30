import {money,totals,cycleLabel} from './billing.mjs';
export function formatExpiry(value){const date=new Date(value);return Number.isFinite(date.getTime())?date.toLocaleString('zh-CN',{year:'numeric',month:'2-digit',day:'2-digit',hour:'2-digit',minute:'2-digit',hour12:false}):'未设置';}
export function localExpiry(value){if(!value)return '';const date=new Date(value);if(!Number.isFinite(date.getTime()))return '';const pad=n=>String(n).padStart(2,'0');return date.getFullYear()+'-'+pad(date.getMonth()+1)+'-'+pad(date.getDate())+'T'+pad(date.getHours())+':'+pad(date.getMinutes());}
export function createRenewalUI({el,api,isAdmin,onChanged,toast}){
 const dialog=document.querySelector('#renewal-dialog'),list=document.querySelector('#renewal-list'),error=document.querySelector('#renewal-error');
 let records=[],dismissed=new Set(),drafts=new Map(),timer=null,busy=false,epoch=0;
 const key=n=>n.public.id+':'+n.renewalVersion;
 const due=()=>records.filter(n=>n.renewalDue&&!dismissed.has(key(n)));
 function dismiss(){for(const n of due())dismissed.add(key(n));}
 dialog.addEventListener('close',dismiss);dialog.addEventListener('cancel',e=>{if(busy)e.preventDefault();});
 document.querySelector('#renewal-later').addEventListener('click',()=>dialog.close());
 async function renew(record,button,action){
  const draft=drafts.get(key(record));let nextExpiresAt;if(action==='renew'&&draft?.custom){const date=new Date(draft.value);if(!Number.isFinite(date.getTime())||date<=new Date(record.expiresAt)){error.textContent='请填写晚于原到期时间的新到期时间';return;}nextExpiresAt=date.toISOString();}
  if(busy||!isAdmin())return;const run=epoch;busy=true;error.textContent='';button.disabled=true;document.querySelector('#renewal-later').disabled=true;dialog.querySelector('.dialog-dismiss').disabled=true;
  try{const result=await api('/api/admin/renewals/'+encodeURIComponent(record.public.id),'POST',{version:record.renewalVersion,expiresAt:record.expiresAt,action,...(nextExpiresAt?{nextExpiresAt}:{})});if(run!==epoch)return;dismissed.add(key(record));toast(result.message);await onChanged();}
  catch(e){if(run!==epoch)return;error.textContent=e.message;if(e.status===409)await onChanged();}
  finally{if(run===epoch){busy=false;button.disabled=false;document.querySelector('#renewal-later').disabled=false;dialog.querySelector('.dialog-dismiss').disabled=false;render();}}
 }
 function render(){
  if(!isAdmin()){clear();return;}const outstanding=due();if(!outstanding.length){if(dialog.open)dialog.close();return;}
  list.replaceChildren();const groups=new Map();
  for(const n of outstanding){const day=n.renewalDay||'待确认日期';if(!groups.has(day))groups.set(day,[]);groups.get(day).push(n);}
  for(const [day,nodes]of [...groups].sort(([a],[b])=>a.localeCompare(b))){
   const group=el('section','renewal-group');const title=el('h3','renewal-group-title',day+' 到期 · '+nodes.length+' 台');group.append(title,el('p','renewal-sum',totals(nodes)));
   for(const n of nodes){
    const item=el('article','renewal-item');item.dataset.id=n.public.id;const info=el('div','renewal-item-info');info.append(el('strong','',n.public.name),el('p','',formatExpiry(n.expiresAt)+' 到期'));if(n.providerName)info.append(el('small','',n.providerName));
    info.append(el('p','',cycleLabel(n)+' · '+money(n.renewalAmount,n.renewalCurrency)));
    if(n.nextRenewalExpiry)info.append(el('p','','续费后到期：'+formatExpiry(n.nextRenewalExpiry)));
    const draft=drafts.get(key(n))||{custom:false,value:localExpiry(n.nextRenewalExpiry)};drafts.set(key(n),draft);
    const adjust=el('label','check-field'),check=el('input');check.type='checkbox';check.checked=draft.custom;check.disabled=busy;adjust.append(check,document.createTextNode('填写实际到期时间'));
    const field=el('label','form-field','新到期时间'),date=el('input');date.type='datetime-local';date.min=localExpiry(n.expiresAt);date.max='2199-12-31T23:59';date.value=draft.value;date.disabled=busy;date.setAttribute('aria-label',n.public.name+' 新到期时间');field.hidden=!draft.custom;field.append(date);
    check.addEventListener('change',()=>{draft.custom=check.checked;field.hidden=!check.checked;});date.addEventListener('input',()=>{draft.value=date.value;});info.append(adjust,field);
    const actions=el('div','renewal-item-actions');
    for(const [action,label,cls]of [['renew','已续费','primary-button'],['stop','不再续费该服务器','secondary-button']]){const button=el('button',cls,label);button.type='button';button.dataset.action=action;button.disabled=busy;button.addEventListener('click',()=>renew(n,button,action));actions.append(button);}
    item.append(info,actions);group.append(item);
   }
   list.append(group);
  }
  if(!dialog.open&&!document.querySelector('dialog[open]'))dialog.showModal();
 }
 function schedule(){clearTimeout(timer);if(!isAdmin()||!due().length)return;timer=setTimeout(()=>{if(!busy&&!dialog.open)render();if(!dialog.open&&due().length)schedule();},500);}
 function update(next){const before=JSON.stringify(records.map(n=>[key(n),n.public.name,n.renewalDue,n.providerName]));records=next;const valid=new Set(records.map(key));for(const k of drafts.keys())if(!valid.has(k))drafts.delete(k);if(dialog.open&&!busy&&before!==JSON.stringify(records.map(n=>[key(n),n.public.name,n.renewalDue,n.providerName])))render();schedule();}
 function clear(){epoch++;clearTimeout(timer);records=[];dismissed.clear();drafts.clear();busy=false;list.replaceChildren();error.textContent='';if(dialog.open)dialog.close();document.querySelector('#renewal-later').disabled=false;dialog.querySelector('.dialog-dismiss').disabled=false;}
 return {update,clear};
}
