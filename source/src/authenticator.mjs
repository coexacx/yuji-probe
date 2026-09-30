import qrcode from './qrcode.mjs';
export function authenticatorCanvas(secret,issuer,account){
 if(!/^[A-Z2-7]{32}$/.test(secret))throw Error('验证器密钥格式不正确');
 const name=(issuer||location.hostname).replaceAll(':',' ').trim();
 const params=new URLSearchParams({secret,issuer:name,algorithm:'SHA1',digits:'6',period:'30'});
 const uri='otpauth://totp/'+encodeURIComponent(name)+':'+encodeURIComponent(account)+'?'+params;
 const qr=qrcode(0,'M');qr.addData(uri,'Byte');qr.make();
 const count=qr.getModuleCount(),cell=5,quiet=4;
 const canvas=document.createElement('canvas');canvas.className='mfa-qr';
 canvas.width=canvas.height=(count+quiet*2)*cell;
 canvas.setAttribute('role','img');canvas.setAttribute('aria-label','二步验证绑定二维码');
 const ctx=canvas.getContext('2d');ctx.fillStyle='#ffffff';ctx.fillRect(0,0,canvas.width,canvas.height);ctx.fillStyle='#101a22';
 for(let row=0;row<count;row++)for(let col=0;col<count;col++)if(qr.isDark(row,col))ctx.fillRect((col+quiet)*cell,(row+quiet)*cell,cell,cell);
 return canvas;
}
