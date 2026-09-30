export function formatExpiry(value){const date=new Date(value);return Number.isFinite(date.getTime())?date.toLocaleString('zh-CN',{year:'numeric',month:'2-digit',day:'2-digit',hour:'2-digit',minute:'2-digit',hour12:false}):'未设置';}
export function localExpiry(value){if(!value)return '';const date=new Date(value);if(!Number.isFinite(date.getTime()))return '';const pad=n=>String(n).padStart(2,'0');return date.getFullYear()+'-'+pad(date.getMonth()+1)+'-'+pad(date.getDate())+'T'+pad(date.getHours())+':'+pad(date.getMinutes());}
export function createRenewalUI({el,api,isAdmin,onChanged,toast}){
 const dialog=document.querySelector('#renewal-dialog'),list=document.querySelector('#renewal-list'),error=document.querySelector('#renewal-error');
 let records=[],dismissed=new Set(),timer=null,busy=false,epoch=0;
 const key=n=>n.public.id+':'+n.renewalVersion;
 const due=()=>records.filter(n=>n.renewalDue&&!dismissed.has(key(n)));
 function dismiss(){for(const n of due())dismissed.add(key(n));}
 dialog.addEventListener('close',dismiss);dialog.addEventListener('cancel',e=>{if(busy)e.preventDefault();});
 document.querySelector('#renewal-later').addEventListener('click',()=>dialog.close());
 async function renew(record,button,action){
  if(busy||!isAdmin())return;const run=epoch;busy=true;error.textContent='';button.disabled=true;document.querySelector('#renewal-later').disabled=true;dialog.querySelector('.dialog-dismiss').disabled=true;
  try{const result=await api('/api/admin/renewals/'+encodeURIComponent(record.public.id),'POST',{version:record.renewalVersion,expiresAt:record.expiresAt,action});if(run!==epoch)return;dismissed.add(key(record));toast(result.message);await onChanged();}
  catch(e){if(run!==epoch)return;error.textContent=e.message;if(e.status===409)await onChanged();}
  finally{if(run===epoch){busy=false;button.disabled=false;document.querySelector('#renewal-later').disabled=false;dialog.querySelector('.dialog-dismiss').disabled=false;render();}}
 }
 function render(){
  if(!isAdmin()){clear();return;}const outstanding=due();if(!outstanding.length){if(dialog.open)dialog.close();return;}
  list.replaceChildren();const groups=new Map();
  for(const n of outstanding){const day=n.renewalDay||'待确认日期';if(!groups.has(day))groups.set(day,[]);groups.get(day).push(n);}
  for(const [day,nodes]of [...groups].sort(([a],[b])=>a.localeCompare(b))){
   const group=el('section','renewal-group');const title=el('h3','renewal-group-title',day+' 到期 · '+nodes.length+' 台');group.append(title);
   for(const n of nodes){
    const item=el('article','renewal-item');item.dataset.id=n.public.id;const info=el('div');info.append(el('strong','',n.public.name),el('p','',formatExpiry(n.expiresAt)+' 到期'));if(n.providerName)info.append(el('small','',n.providerName));
    const actions=el('div','renewal-item-actions');
    for(const [action,label,cls]of [['renew','已续费，顺延 30 天','primary-button'],['stop','不再续费该服务器','secondary-button']]){const button=el('button',cls,label);button.type='button';button.dataset.action=action;button.disabled=busy;button.addEventListener('click',()=>renew(n,button,action));actions.append(button);}
    item.append(info,actions);group.append(item);
   }
   list.append(group);
  }
  if(!dialog.open&&!document.querySelector('dialog[open]'))dialog.showModal();
 }
 function schedule(){clearTimeout(timer);if(!isAdmin()||!due().length)return;timer=setTimeout(()=>{if(!busy)render();if(!dialog.open&&due().length)schedule();},500);}
 function update(next){records=next;if(dialog.open&&!busy)render();schedule();}
 function clear(){epoch++;clearTimeout(timer);records=[];dismissed.clear();busy=false;list.replaceChildren();error.textContent='';if(dialog.open)dialog.close();document.querySelector('#renewal-later').disabled=false;dialog.querySelector('.dialog-dismiss').disabled=false;}
 return {update,clear};
}
