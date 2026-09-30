import {readFile,writeFile,mkdir,rename,rm,cp} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'.');
const stage=path.join(root,'state','build-'+Date.now());
await mkdir(path.join(stage,'assets'),{recursive:true});
async function asset(name,body,extension){const hash=createHash('sha256').update(body).digest('hex').slice(0,12);const url=`/assets/${name}-${hash}.${extension}`;await writeFile(stage+url,body);return url;}
const xterm=await asset('xterm',await readFile(root+'/frontend-tools/node_modules/@xterm/xterm/lib/xterm.mjs'),'js');
const fit=await asset('fit',await readFile(root+'/frontend-tools/node_modules/@xterm/addon-fit/lib/addon-fit.mjs'),'js');
const files=await asset('files',await readFile(root+'/src/files.mjs'),'js');
const terminal=await asset('terminal',(await readFile(root+'/src/terminal.mjs','utf8')).replace("'./xterm.mjs'",JSON.stringify(xterm)).replace("'./addon-fit.mjs'",JSON.stringify(fit)).replace("'./files.mjs'",JSON.stringify(files)),'js');
const qr=await asset('qrcode',await readFile(root+'/frontend-tools/node_modules/qrcode-generator/dist/qrcode.mjs'),'js');
const authenticator=await asset('authenticator',(await readFile(root+'/src/authenticator.mjs','utf8')).replace("'./qrcode.mjs'",JSON.stringify(qr)),'js');
let countrySource=await readFile(root+'/src/countries.mjs','utf8');
const dataset=JSON.parse(await readFile(root+'/controller-rust/assets/countries.json','utf8'));
for(const code of Object.keys(dataset)){const low=code.toLowerCase();const file=await asset('flag-'+low,await readFile(root+'/src/flags/'+low+'.svg'),'svg');countrySource=countrySource.replace(JSON.stringify('./flags/'+low+'.svg'),JSON.stringify(file));}
const countries=await asset('countries',countrySource,'js');
const network=await asset('network',await readFile(root+'/src/network.mjs'),'js');
const renewals=await asset('renewals',await readFile(root+'/src/renewals.mjs'),'js');
const admin=await asset('admin',(await readFile(root+'/src/admin.mjs','utf8')).replace("'./terminal.mjs'",JSON.stringify(terminal)).replace("'./authenticator.mjs'",JSON.stringify(authenticator)).replace("'./countries.mjs'",JSON.stringify(countries)).replace("'./renewals.mjs'",JSON.stringify(renewals)),'js');
const app=await asset('app',(await readFile(root+'/src/app.mjs','utf8')).replace("'./admin.mjs'",JSON.stringify(admin)).replace("'./countries.mjs'",JSON.stringify(countries)).replace("'./network.mjs'",JSON.stringify(network)),'js');
const style=await asset('style',(await readFile(root+'/frontend-tools/node_modules/@xterm/xterm/css/xterm.css','utf8'))+'\n'+(await readFile(root+'/src/style.css','utf8')),'css');
const html=(await readFile(root+'/src/index.html','utf8')).replace('__STYLE__',style).replace('__SCRIPT__',app);
if(html.includes('__STYLE__')||html.includes('__SCRIPT__'))throw Error('Unresolved build placeholders');
await writeFile(stage+'/index.html',html);
await cp(root+'/src/favicon.svg',stage+'/favicon.svg');
// Publish immutable assets first; swap HTML only after all its dependencies exist.
await mkdir(root+'/public/assets',{recursive:true});
await cp(stage+'/assets',root+'/public/assets',{recursive:true});
await cp(stage+'/favicon.svg',root+'/public/favicon.svg');
await cp(stage+'/index.html',root+'/public/.index.next.html');
await rename(root+'/public/.index.next.html',root+'/public/index.html');
await rm(stage,{recursive:true});
console.log(JSON.stringify({htmlBytes:Buffer.byteLength(html),assets:[app,admin,style]},null,2));
