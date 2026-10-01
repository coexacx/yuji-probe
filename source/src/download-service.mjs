// This worker only handles one-use attachment URLs under its own /assets/ scope.
// It never reads OPFS or caches responses. Bytes come from the authorized page's dedicated worker.
const tickets=new Map();
self.addEventListener('message',event=>{
 const v=event.data,port=event.ports[0];let source;
 try{source=new URL(event.source.url);}catch{port?.close();return;}
 if(source.origin!==self.location.origin||event.source.type!=='window'||!port||v?.type!=='download'||!/^[a-f0-9]{64}$/.test(v.id)||!Number.isSafeInteger(v.size)||v.size<0||v.size>2*1024**3||typeof v.name!=='string'||v.name.length>1024){port?.close();return;}
 if(tickets.size>=8||tickets.has(v.id)){event.source.postMessage({type:'download-error',id:v.id});port.close();return;}
 const timer=setTimeout(()=>{const t=tickets.get(v.id);if(t){tickets.delete(v.id);t.port.postMessage({type:'cancel'});t.port.close();}},60000);
 tickets.set(v.id,{port,size:v.size,name:v.name,timer});event.source.postMessage({type:'download-prepared',id:v.id});
});
self.addEventListener('fetch',event=>{
 const url=new URL(event.request.url),match=url.pathname.match(/^\/assets\/yuji-download-([a-f0-9]{64})$/);
 if(url.origin!==self.location.origin||!match)return;
 if(event.request.method!=='GET'||url.search){event.respondWith(new Response('',{status:404}));return;}
 const t=tickets.get(match[1]);if(!t){event.respondWith(new Response('',{status:404,headers:{'Cache-Control':'no-store'}}));return;}
 tickets.delete(match[1]);clearTimeout(t.timer);
 let finishLifetime;event.waitUntil(new Promise(resolve=>{finishLifetime=resolve;}));
 let position=0,pending=null,ended=false,timer=null;
 const stop=()=>{if(ended)return;ended=true;finishLifetime();clearTimeout(timer);t.port.postMessage({type:'cancel'});t.port.close();pending?.reject(Error('Download interrupted'));pending=null;};
 t.port.onmessage=({data:v})=>{if(!pending||ended)return;clearTimeout(timer);const p=pending;pending=null;if(v?.type!=='chunk'||!(v.buffer instanceof ArrayBuffer)||v.buffer.byteLength>131072||position+v.buffer.byteLength>t.size){p.reject(Error('Invalid download data'));stop();return;}position+=v.buffer.byteLength;p.resolve(new Uint8Array(v.buffer));};
 const stream=new ReadableStream({async pull(controller){
  try{if(position===t.size){ended=true;finishLifetime();controller.close();t.port.postMessage({type:'done'});t.port.close();return;}
   const bytes=await new Promise((resolve,reject)=>{pending={resolve,reject};timer=setTimeout(()=>{stop();},60000);t.port.postMessage({type:'pull'});});if(!bytes.length)throw Error('Incomplete download');controller.enqueue(bytes);
  }catch(e){controller.error(e);stop();}
 },cancel(){stop();}},{highWaterMark:1});
 const name=encodeURIComponent(t.name.replace(/[\x00-\x1f\x7f"\\]/g,'_').slice(0,240)).replace(/'/g,'%27');
 event.respondWith(new Response(stream,{headers:{'Content-Type':'application/octet-stream','Content-Length':String(t.size),'Content-Disposition':"attachment; filename=\"download.bin\"; filename*=UTF-8''"+name,'Cache-Control':'no-store','X-Content-Type-Options':'nosniff','Content-Security-Policy':"default-src 'none'; sandbox"}}));
});

