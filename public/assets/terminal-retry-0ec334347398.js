const DELAYS=[2000,5000,10000];
export class TerminalRetry {
 constructor({retry,waiting,setTimer=(callback,delay)=>setTimeout(callback,delay),clearTimer=timer=>clearTimeout(timer)}){this.retry=retry;this.waiting=waiting;this.setTimer=setTimer;this.clearTimer=clearTimer;this.timer=null;this.stable=null;this.attempt=0;}
 cancel(){if(this.timer!==null)this.clearTimer(this.timer);if(this.stable!==null)this.clearTimer(this.stable);this.timer=null;this.stable=null;}
 reset(){this.cancel();this.attempt=0;}
 connected(){this.cancel();this.stable=this.setTimer(()=>{this.stable=null;this.attempt=0;},60000);}
 schedule(){if(this.timer!==null)return true;this.cancel();if(this.attempt>=DELAYS.length)return false;const delay=DELAYS[this.attempt++];this.waiting(delay,this.attempt,DELAYS.length);this.timer=this.setTimer(()=>{this.timer=null;this.retry();},delay);return true;}
}
export function canRetryRequest(error){return !error.staleSession&&(error.network===true||[408,409,502,503,504].includes(error.status));}
export function canRetryClose(code,decision){return decision===true||(decision===null&&[1001,1006,1011,1012,1013].includes(code));}
