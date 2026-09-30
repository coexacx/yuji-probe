export async function renderOffsite(panel,{el,api,toast,field,section,select,action,saveFile,bytes,date}){
 const box=section(panel,'异地备份','自动备份完成后，加密上传至 S3 兼容存储或 WebDAV。备份口令丢失后无法恢复。'),status=el('p','form-footnote','正在读取配置…');box.append(status);
 try{
  const saved=await api('/api/admin/ops/offsite');if(!box.isConnected)return;
  const form=el('form','settings-form');form.autocomplete='off';
  const enabledWrap=el('label','check-field'),enabled=el('input');enabled.type='checkbox';enabled.checked=saved.enabled;enabledWrap.append(enabled,document.createTextNode('启用异地备份'));
  const [kindWrap,kind]=select('存储类型','offsiteKind',[['s3','S3 兼容存储'],['webdav','WebDAV']],saved.kind||'s3');
  const [endpointWrap,endpoint]=field('HTTPS 存储地址','url','offsiteEndpoint',saved.endpoint);endpoint.placeholder='https://s3.example.com';endpoint.maxLength=1024;
  const [bucketWrap,bucket]=field('Bucket','text','offsiteBucket',saved.bucket);
  const [regionWrap,region]=field('Region','text','offsiteRegion',saved.region||'us-east-1');
  const [prefixWrap,prefix]=field('目录前缀','text','offsitePrefix',saved.prefix||'yuji-probe');prefix.maxLength=240;
  const [identityWrap,identity]=field('Access Key ID','text','offsiteIdentity',saved.identity);identity.maxLength=200;
  const [secretWrap,secret]=field('Secret Access Key','password','offsiteSecret','');secret.autocomplete='new-password';secret.maxLength=4096;secret.placeholder=saved.hasSecret?'留空保留已保存密钥':'填写存储密钥';
  const [keepWrap,keep]=field('远端保留份数','number','offsiteKeep',saved.keep||7);keep.min=1;keep.max=30;
  const save=el('button','primary-button','保存异地备份');save.type='submit';
  const changeKind=()=>{const s3=kind.value==='s3';bucketWrap.hidden=regionWrap.hidden=!s3;identityWrap.firstChild.textContent=s3?'Access Key ID':'WebDAV 用户名';secretWrap.firstChild.textContent=s3?'Secret Access Key':'WebDAV 密码 / 应用密码';endpoint.placeholder=s3?'https://s3.example.com':'https://dav.example.com/remote.php/dav/files/user';};kind.onchange=changeKind;changeKind();
  form.append(enabledWrap,kindWrap,endpointWrap,bucketWrap,regionWrap,prefixWrap,identityWrap,secretWrap,keepWrap,el('p','form-footnote','使用公网 HTTPS 地址。S3 采用路径式访问；WebDAV 地址填写已有的存储目录。密钥需有读取、写入和删除备份的权限。'),save);box.append(form);
  const controls=el('div','offsite-actions'),list=el('div','offsite-list');box.append(controls,list);
  function show(s){status.textContent=(s.enabled?'已启用':'未启用')+' · 最近成功：'+date(s.last)+(s.busy?' · 任务进行中':'')+(s.error?' · '+s.error:'')+(s.pending?' · 待上传 '+s.pending+(s.attempts>=5?'，请检查配置后重试':''):'');list.replaceChildren();for(const file of [...s.objects].reverse()){const row=el('div','ops-session'),info=el('div');info.append(el('strong','',file.name),el('p','form-footnote',bytes(file.size)+' · '+date(file.at)));row.append(info,action('下载备份',async()=>{const r=await api('/api/admin/ops/offsite/download','POST',{name:file.name});saveFile(r.name,r.backup);}));list.append(row);}}
  async function refresh(){if(!box.isConnected)return;try{show(await api('/api/admin/ops/offsite'));}catch(e){status.textContent=e.message;}}
  show(saved);
  form.onsubmit=async e=>{e.preventDefault();save.disabled=true;try{const s=await api('/api/admin/ops/offsite','POST',{enabled:enabled.checked,kind:kind.value,endpoint:endpoint.value.trim(),bucket:bucket.value.trim(),region:region.value.trim(),prefix:prefix.value.trim(),identity:identity.value.trim(),secret:secret.value,keep:Number(keep.value)});show(s);secret.value='';secret.placeholder=s.hasSecret?'留空保留已保存密钥':'填写存储密钥';toast('异地备份配置已保存');}catch(e){toast(e.message,true);}finally{save.disabled=false;}};
  controls.append(action('测试已保存配置',async()=>{const r=await api('/api/admin/ops/offsite/test','POST',{});toast(r.message);}),action('立即备份并上传',async()=>{await api('/api/admin/ops/offsite/run','POST',{});toast('备份任务已开始');await refresh();}),action('重试上传',async()=>{await api('/api/admin/ops/offsite/retry','POST',{});toast('已安排重试');await refresh();}),action('刷新状态',refresh));
  box.append(el('p','form-footnote','先保存上方的自动备份口令。下载的异地备份可在下方恢复，迁移到新域名时会沿用现有的 Agent 连接修正流程。'));
  const poll=setInterval(()=>{if(!box.isConnected){clearInterval(poll);return;}if(!document.hidden)refresh();},15000);
 }catch(e){status.textContent=e.message;}
}
