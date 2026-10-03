import test from 'node:test';
import assert from 'node:assert/strict';
import {TerminalRetry,canRetryRequest,canRetryClose} from '../src/terminal-retry.mjs';
function fixture(){
 let id=0,time=0,attempts=0;const timers=new Map(),waiting=[];
 const retry=new TerminalRetry({retry:()=>attempts++,waiting:(...args)=>waiting.push(args),setTimer:(fn,delay)=>{timers.set(++id,{fn,at:time+delay});return id;},clearTimer:id=>timers.delete(id)});
 function tick(ms){const end=time+ms;while(true){const entry=[...timers].sort((a,b)=>a[1].at-b[1].at)[0];if(!entry||entry[1].at>end)break;timers.delete(entry[0]);time=entry[1].at;entry[1].fn();}time=end;}
 return {retry,tick,waiting,timers,get attempts(){return attempts;}};
}
test('transport reconnect uses increasing delays and stops after six retries',()=>{
 const f=fixture();for(const delay of [2000,5000,10000,20000,30000,30000]){assert(f.retry.schedule());f.tick(delay-1);const previous=f.attempts;f.tick(1);assert.equal(f.attempts,previous+1);}
 assert.equal(f.retry.schedule(),false);f.tick(60000);assert.equal(f.attempts,6);assert.equal(f.timers.size,0);
});
test('duplicate close notifications share one pending retry',()=>{
 const f=fixture();assert(f.retry.schedule());assert(f.retry.schedule());assert.equal(f.waiting.length,1);f.tick(2000);assert.equal(f.attempts,1);
});
test('manual close, logout and disposal cancel scheduled work',()=>{
 const f=fixture();f.retry.schedule();f.retry.reset();f.tick(60000);assert.equal(f.attempts,0);assert.equal(f.timers.size,0);
});
test('brief successful connections do not reset the retry limit',()=>{
 const f=fixture();for(let i=0;i<6;i++){assert(f.retry.schedule());f.tick(30000);f.retry.connected();f.tick(59000);}
 assert.equal(f.retry.schedule(),false);assert.equal(f.attempts,6);
});
test('one minute of healthy connection restores the retry budget',()=>{
 const f=fixture();for(let i=0;i<6;i++){f.retry.schedule();f.tick(30000);}f.retry.connected();f.tick(60000);assert(f.retry.schedule());assert.equal(f.waiting.at(-1)[0],2000);
});
test('fresh authorization rejects security failures instead of retrying',()=>{
 for(const status of [400,401,403,404,428,429])assert.equal(canRetryRequest({status}),false);
 for(const status of [408,409,502,503,504])assert(canRetryRequest({status}));
 assert(canRetryRequest({network:true}));assert(!canRetryRequest({network:true,staleSession:true}));
});
test('explicit server decisions override transport close codes',()=>{
 assert(canRetryClose(1006,null));assert(canRetryClose(1000,true));
 for(const code of [1000,1006,1011])assert(!canRetryClose(code,false));
 assert(!canRetryClose(1000,null));assert(!canRetryClose(1008,null));
});
