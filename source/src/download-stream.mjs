let registration;
async function service(){
 if(!navigator.serviceWorker)throw Error('此浏览器不支持流式下载，请更新浏览器后重试');
 registration??=navigator.serviceWorker.register('__DOWNLOAD_SERVICE__',{scope:'/assets/',type:'module'}).catch(e=>{registration=null;throw e;});
 const r=await registration;if(r.active)return r.active;
 const w=r.installing||r.waiting;if(!w)throw Error('下载服务尚未就绪');
 return new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('下载服务启动超时，请重试')),15000);const change=()=>{if(w.state==='activated'){clearTimeout(timer);w.removeEventListener('statechange',change);resolve(w);}else if(w.state==='redundant'){clearTimeout(timer);w.removeEventListener('statechange',change);reject(Error('下载服务不可用'));}};w.addEventListener('statechange',change);change();});
}
export async function exportDownload(engine,name,size){
 const sw=await service(),id=Array.from(crypto.getRandomValues(new Uint8Array(32)),b=>b.toString(16).padStart(2,'0')).join(''),channel=new MessageChannel();
 let listener,timer,frame;
 const prepared=new Promise((resolve,reject)=>{listener=e=>{if(e.source!==sw||e.data?.id!==id)return;clearTimeout(timer);navigator.serviceWorker.removeEventListener('message',listener);e.data.type==='download-prepared'?resolve():reject(Error('下载服务繁忙，请稍后重试'));};navigator.serviceWorker.addEventListener('message',listener);timer=setTimeout(()=>reject(Error('下载服务未响应')),15000);});
 const completed=engine.run('stream',{port:channel.port2},[channel.port2]);completed.catch(()=>{});
 try{
  sw.postMessage({type:'download',id,name,size},[channel.port1]);await prepared;
  frame=document.createElement('iframe');frame.hidden=true;frame.src=new URL('yuji-download-'+id,sw.scriptURL).href;frame.setAttribute('aria-hidden','true');document.body.append(frame);
  await completed;
 }catch(e){engine.cancelExport();throw e;}
 finally{clearTimeout(timer);navigator.serviceWorker.removeEventListener('message',listener);if(frame)setTimeout(()=>frame.remove(),1000);}
}

