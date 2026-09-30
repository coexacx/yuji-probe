import {acceptAppearance,previewAppearance,endAppearancePreview} from './appearance.mjs';
const choices=[['default','默认','保留当前液态玻璃外观'],['clear','纯透明液态玻璃','清透边缘与轻盈层次'],['sketch','简笔画','纸面、线条与手绘边框'],['anime','纯二次元','晴空、樱色与插画背景']];
const empty=()=>({revision:'',preset:'default',accent:'',background:null,favicon:null,backgroundDim:20,backgroundBlur:0,brandIcon:true,customCss:''});
const defaultColors={default:'#226c61',clear:'#326a83',sketch:'#5a624d',anime:'#a65077'};
async function convert(file,icon){
 if(!file||!['image/png','image/jpeg','image/webp'].includes(file.type))throw Error('请选择 PNG、JPG 或 WebP 图片');
 if(file.size>8*1024*1024)throw Error('图片最大 8 MiB');
 const bitmap=await createImageBitmap(file);
 try{
  const edge=icon?192:1920,scale=Math.min(1,edge/Math.max(bitmap.width,bitmap.height)),canvas=document.createElement('canvas');canvas.width=Math.max(1,Math.round(bitmap.width*scale));canvas.height=Math.max(1,Math.round(bitmap.height*scale));const ctx=canvas.getContext('2d');if(!icon){ctx.fillStyle='#ffffff';ctx.fillRect(0,0,canvas.width,canvas.height);}ctx.drawImage(bitmap,0,0,canvas.width,canvas.height);
  const blob=await new Promise(resolve=>canvas.toBlob(resolve,icon?'image/png':'image/jpeg',.85));if(!blob)throw Error('图片转换失败');
  if(blob.size>(icon?192:1536)*1024)throw Error('图片压缩后仍过大，请降低尺寸');
  const data=await new Promise((resolve,reject)=>{const reader=new FileReader();reader.onload=()=>resolve(String(reader.result).split(',')[1]);reader.onerror=()=>reject(Error('无法读取图片'));reader.readAsDataURL(blob);});
  return {mime:blob.type,data};
 }finally{bitmap.close();}
}
export async function showThemes({panel,el,api,heading,toast}){
 const box=el('section','glass admin-box theme-settings');box.id='theme-settings';box.append(heading('主题','保存后对全站访客生效'));const body=el('div');body.textContent='正在读取…';box.append(body);panel.append(box);
 let documentValue,draft,busy=false,localURLs=[],conversion=0;
 function cleanup(){conversion++;for(const u of localURLs)URL.revokeObjectURL(u);localURLs=[];endAppearancePreview();}
 const observer=new MutationObserver(()=>{if(!box.isConnected){cleanup();observer.disconnect();}});observer.observe(document.querySelector('#admin-view'),{childList:true,subtree:true});
 const alive=()=>box.isConnected;
 try{documentValue=await api('/api/admin/theme');if(!alive())return;acceptAppearance(documentValue);draft=structuredClone(documentValue.theme);render();}catch(e){if(alive())body.textContent=e.message;}
 function render(){
  for(const u of localURLs)URL.revokeObjectURL(u);localURLs=[];body.replaceChildren();const form=el('form','theme-form');form.id='theme-form';const error=el('p','form-error');error.id='theme-error';error.setAttribute('role','alert');
  const presets=el('fieldset','theme-presets');presets.append(el('legend','sr-only','选择主题'));
  for(const [id,name,description]of choices){
   const label=el('label','theme-choice'),radio=el('input');radio.type='radio';radio.name='preset';radio.value=id;radio.checked=draft.preset===id;
   const sample=el('span','theme-sample theme-sample-'+id);sample.setAttribute('aria-hidden','true');const mock=el('span','sample-window');mock.append(el('span','sample-dot'),el('span','sample-line'),el('span','sample-bars'));sample.append(mock);
   const title=el('span','theme-choice-title',name),note=el('span','theme-choice-note',description);label.append(radio,sample,title,note);presets.append(label);
   radio.onchange=()=>{draft.preset=id;if(!draft.accent){color.value=defaultColors[id];colorText.value=color.value;}};
  }
  form.append(presets);
  const custom=el('div','theme-custom'),left=el('div','theme-fields'),right=el('div','theme-fields');
  const colorField=el('label','form-field','主题色'),colorRow=el('span','theme-color-row'),color=el('input');color.type='color';color.name='accent';color.value=draft.accent||defaultColors[draft.preset];const colorText=el('input');colorText.name='accentHex';colorText.value=draft.accent||defaultColors[draft.preset];colorText.maxLength=7;colorText.pattern='#[a-fA-F0-9]{6}';colorText.setAttribute('aria-label','主题色十六进制值');
  const useDefault=el('label','check-field'),defaultInput=el('input');defaultInput.type='checkbox';defaultInput.name='defaultAccent';defaultInput.checked=!draft.accent;useDefault.append(defaultInput,document.createTextNode('使用主题默认色'));
  const setColor=value=>{draft.accent=value;defaultInput.checked=false;color.value=value;colorText.value=value;};
  color.oninput=()=>setColor(color.value);colorText.onchange=()=>{if(/^#[0-9a-f]{6}$/i.test(colorText.value))setColor(colorText.value);};
  defaultInput.onchange=()=>{draft.accent=defaultInput.checked?'':color.value;if(defaultInput.checked){color.value=defaultColors[draft.preset];colorText.value=color.value;}};
  colorRow.append(color,colorText);colorField.append(colorRow);left.append(colorField,useDefault);
  function mediaField(key,labelText,icon){
   const area=el('div','theme-media'),label=el('label','form-field',labelText),input=el('input');input.type='file';input.name=key;input.accept='image/png,image/jpeg,image/webp';input.className='theme-file-input';label.append(input);const sample=el('div',icon?'theme-icon-preview':'theme-image-preview'),status=el('span','theme-file-status'),remove=el('button','table-button','恢复主题默认');remove.type='button';
   function draw(){sample.replaceChildren();const value=draft[key];status.textContent=value?'已设置':'使用主题默认';remove.disabled=!value;if(!value){sample.append(el('span','',icon?'站点图标':'背景预览'));return;}const bytes=Uint8Array.from(atob(value.data),c=>c.charCodeAt(0)),url=URL.createObjectURL(new Blob([bytes],{type:value.mime}));localURLs.push(url);const img=el('img');img.src=url;img.alt=labelText;sample.append(img);}
   input.onchange=async()=>{const file=input.files[0];if(!file)return;const run=++conversion;error.textContent='';setBusy(true);status.textContent='正在处理…';try{const value=await convert(file,icon);if(!alive()||run!==conversion)return;draft[key]=value;draw();}catch(e){if(alive()){error.textContent=e.message;draw();}}finally{input.value='';if(alive()&&run===conversion)setBusy(false);}};
   remove.onclick=()=>{draft[key]=null;draw();};const actions=el('div','theme-media-actions');actions.append(status,remove);area.append(label,sample,actions);draw();return area;
  }
  left.append(mediaField('background','背景图',false));
  function range(labelText,name,min,max,suffix){const label=el('label','form-field'),header=el('span','theme-range-label'),output=el('output','',draft[name]+suffix),input=el('input');input.type='range';input.name=name;input.min=min;input.max=max;input.value=draft[name];input.oninput=()=>{draft[name]=Number(input.value);output.textContent=input.value+suffix;};header.append(document.createTextNode(labelText),output);label.append(header,input);return label;}
  const ranges=el('div','form-columns');ranges.append(range('背景遮罩','backgroundDim',0,85,'%'),range('背景模糊','backgroundBlur',0,24,' px'));left.append(ranges);
  right.append(mediaField('favicon','站点图标',true));const brand=el('label','check-field'),brandInput=el('input');brandInput.type='checkbox';brandInput.name='brandIcon';brandInput.checked=draft.brandIcon;brandInput.onchange=()=>draft.brandIcon=brandInput.checked;brand.append(brandInput,document.createTextNode('同时用于左上角站点标识'));right.append(brand,el('p','form-footnote','PNG、JPG、WebP 图片，上传时自动压缩。'));
  const cssLabel=el('label','form-field','局部样式覆盖'),css=el('textarea');css.name='customCss';css.rows=7;css.maxLength=4096;css.spellcheck=false;css.placeholder='.server-card { border-radius: 18px; }\n.node-title { font-size: 16px; }';css.value=draft.customCss;css.oninput=()=>draft.customCss=css.value;cssLabel.append(css);right.append(cssLabel);
  const help=el('details','theme-css-help');help.append(el('summary','','可用的局部样式'));help.append(el('p','','选择器：.server-card、.node-title、.card-footer、.toolbar、.brand-mark、.brand-name、.footer、.admin-box、.node-dialog。每组使用一个选择器。'),el('p','','属性：color、background-color、border-color（十六进制或 transparent）；border-radius（0–40px）；border-width（0–3px）；font-size（11–32px）；font-weight（400/500/600/700）；letter-spacing（0–4px）。'));right.append(help);custom.append(left,right);form.append(custom);
  const actions=el('div','theme-actions'),save=el('button','primary-button','保存主题'),preview=el('button','secondary-button','预览整个站点'),reset=el('button','table-button','恢复默认设置');save.type='submit';preview.type='button';reset.type='button';actions.append(save,preview,reset);
  function setBusy(value){busy=value;for(const input of form.elements)input.disabled=value;}
  async function submit(previewOnly){
   if(busy||!form.reportValidity())return;error.textContent='';setBusy(true);
   try{const result=await api('/api/admin/theme'+(previewOnly?'/preview':''),previewOnly?'POST':'PUT',draft);if(!alive())return;
    if(previewOnly)previewAppearance(result,()=>form.requestSubmit());
    else{documentValue=result;draft=structuredClone(result.theme);endAppearancePreview();acceptAppearance(result);render();toast('主题已保存');}
   }catch(e){if(alive())error.textContent=e.message;}finally{if(alive())setBusy(false);}
  }
  form.onsubmit=e=>{e.preventDefault();submit(false);};preview.onclick=()=>submit(true);
  reset.onclick=()=>{endAppearancePreview();draft={...empty(),revision:documentValue.theme.revision};render();};
  form.append(error,actions);body.append(form);
 }
}
