import {sha256} from '@noble/hashes/sha2.js';
let handle=null,file=null,hash=null,position=0,root=null,storageName='',exportEnd=null;
const hex=b=>Array.from(b,x=>x.toString(16).padStart(2,'0')).join('');
const digest=()=>hex(hash.clone().digest());
async function close(){if(handle){handle.flush();handle.close();handle=null;}file=null;hash=null;position=0;}
async function initialize(v){
 await close();hash=sha256.create();position=0;
 if(v.upload){file=v.file;if(!file||file.size<v.offset)throw Error('请选择原上传文件');}
 else{root=await navigator.storage.getDirectory();storageName='yuji-'+v.id;if(!/^yuji-[a-f0-9]{64}$/.test(storageName))throw Error('无效的下载标识');const entry=await root.getFileHandle(storageName,{create:v.offset===0});if(typeof entry.createSyncAccessHandle!=='function')throw Error('此浏览器不支持低内存下载，请更新浏览器后重试');handle=await entry.createSyncAccessHandle();if(handle.getSize()<v.offset)throw Error('本地下载缓存不完整，请重新下载');}
 while(position<v.offset){const length=Math.min(1048576,v.offset-position);let b;if(file)b=new Uint8Array(await file.slice(position,position+length).arrayBuffer());else{b=new Uint8Array(length);if(handle.read(b,{at:position})!==length)throw Error('读取缓存失败');}hash.update(b);position+=b.length;}
 if(handle){handle.truncate(position);handle.flush();}
 return {sha256:digest(),offset:position};
}
async function processMessage({data:v}){
 try{let data;
 switch(v.action){
 case 'init':data=await initialize(v);break;
 case 'read':{if(!file||v.offset!==position)throw Error('上传位置发生变化');const b=new Uint8Array(await file.slice(position,position+Math.min(v.size,131072)).arrayBuffer());hash.update(b);position+=b.length;let s='';for(let i=0;i<b.length;i+=8192)s+=String.fromCharCode(...b.subarray(i,i+8192));data={content:btoa(s),offset:position,sha256:digest()};break;}
 case 'write':{if(!handle||v.offset!==position)throw Error('下载位置发生变化');const s=atob(v.content),b=new Uint8Array(s.length);for(let i=0;i<s.length;i++)b[i]=s.charCodeAt(i);if(b.length>131072||handle.write(b,{at:position})!==b.length)throw Error('下载缓存写入失败');hash.update(b);position+=b.length;handle.flush();data={offset:position,sha256:digest()};break;}
 case 'finish':{if(!handle)throw Error('下载缓存未打开');handle.flush();data={size:position,sha256:digest()};break;}
 case 'stream':{if(!handle||!v.port)throw Error('下载缓存未打开');const port=v.port;let sent=0;data=await new Promise((resolve,reject)=>{const end=ok=>{port.close();exportEnd=null;ok?resolve({done:true}):reject(Error('保存已中断，可以重试'));};exportEnd=()=>end(false);port.onmessage=({data:m})=>{try{if(m.type==='done'){if(sent!==position)throw Error('下载长度不一致');end(true);}else if(m.type==='cancel'){end(false);}else if(m.type==='pull'){const length=Math.min(131072,position-sent),bytes=new Uint8Array(length);if(handle.read(bytes,{at:sent})!==length)throw Error('下载缓存读取失败');sent+=length;port.postMessage({type:'chunk',buffer:bytes.buffer},[bytes.buffer]);}}catch{end(false);}};port.start();});break;}
 case 'close':await close();data={};break;
 case 'remove':await close();root=await navigator.storage.getDirectory();if(!/^[a-f0-9]{64}$/.test(v.id))throw Error('无效缓存标识');await root.removeEntry('yuji-'+v.id).catch(()=>{});data={};break;
 default:throw Error('无效操作');
 }
 self.postMessage({request:v.request,ok:true,data});
 }catch(e){self.postMessage({request:v.request,ok:false,error:e.message||'文件缓存不可用'});}
}
let chain=Promise.resolve();self.onmessage=event=>{if(event.data?.action==='cancel-export'){exportEnd?.();return;}chain=chain.then(()=>processMessage(event)).catch(()=>{});};
