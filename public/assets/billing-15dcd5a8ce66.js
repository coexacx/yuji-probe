export const currencies='CNY USD EUR GBP HKD TWD SGD JPY KRW AUD CAD NZD CHF SEK NOK DKK PLN CZK HUF RON RUB UAH TRY INR IDR MYR THB PHP VND BRL MXN ZAR AED SAR ILS PKR BDT ARS CLP ISK BHD KWD OMR JOD TND'.split(' ');
export function precision(code){return ['JPY','KRW','VND','CLP','ISK'].includes(code)?0:['BHD','KWD','OMR','JOD','TND'].includes(code)?3:2;}
export function money(amount,code){if(amount===''||amount==null||!currencies.includes(code))return '金额未设置';return code+' '+Number(amount).toLocaleString('zh-CN',{minimumFractionDigits:precision(code),maximumFractionDigits:precision(code)});}
export function totals(nodes){const sums=new Map();let unknown=0;for(const n of nodes){if(!n.renewalAmount||!currencies.includes(n.renewalCurrency)){unknown++;continue;}const p=precision(n.renewalCurrency),[w,f='']=n.renewalAmount.split('.');const minor=Number(w)*10**p+Number(f.padEnd(p,'0'));sums.set(n.renewalCurrency,(sums.get(n.renewalCurrency)||0)+minor);}return [...sums].sort(([a],[b])=>a.localeCompare(b)).map(([c,n])=>money((n/10**precision(c)).toFixed(precision(c)),c)).concat(unknown?[unknown+' 台金额未设置']:[]).join(' · ');}
export function cycleLabel(n){return n.renewalCycleLabel||'每 30 天';}
export function setupLeaseFields(form){
 const preset=form.elements.renewalPreset,days=form.elements.renewalDays,currency=form.elements.renewalCurrency,amount=form.elements.renewalAmount;
 currency.replaceChildren(...currencies.map(c=>new Option(c,c)));
 const update=()=>{days.closest('label').hidden=preset.value!=='days';days.required=preset.value==='days';const p=precision(currency.value);amount.pattern=p?'[0-9]{1,9}(\\.[0-9]{1,'+p+'})?':'[0-9]{1,9}';amount.placeholder=p?'例如 29.90':'例如 1000';};
 preset.addEventListener('change',update);currency.addEventListener('change',update);
 return record=>{
  const c=record?.renewalCycle||{unit:'months',count:1};
  for(const option of [...preset.options])if(option.dataset.extra)option.remove();
  const value=c.unit==='days'?'days':'months:'+c.count;
  if(![...preset.options].some(o=>o.value===value)){const option=new Option('每 '+c.count+' 个月',value);option.dataset.extra='1';preset.append(option);}
  preset.value=value;days.value=c.unit==='days'?c.count:30;
  amount.value=record?.renewalAmount||'';currency.value=record?.renewalCurrency||'CNY';update();
 };
}
export function leaseInput(form){const p=form.elements.renewalPreset.value;return {renewalCycle:p==='days'?{unit:'days',count:Number(form.elements.renewalDays.value)}:{unit:'months',count:Number(p.split(':')[1])},renewalAmount:form.elements.renewalAmount.value.trim(),renewalCurrency:form.elements.renewalCurrency.value};}
export async function showBilling({panel,el,api,records,edit,heading}){
 const box=el('section','glass admin-box billing-box');box.append(heading('支出汇总','按币种分别统计，金额仅用于预算参考'));const body=el('div','billing-content');body.textContent='正在读取…';box.append(body);panel.append(box);
 try{
  const data=await api('/api/admin/billing');if(!box.isConnected)return;body.replaceChildren();
  const cards=el('div','billing-cards');
  for(const row of data.currencies){const card=el('article','billing-card');card.append(el('h3','',row.currency+' · '+row.nodes+' 台'));const dl=el('div','billing-metrics');for(const [label,key]of [['折合月支出','monthly'],['折合年支出','yearly'],['未来 30 天到期','next30Days'],['已到期未确认','overdue']]){const item=el('dl');item.append(el('dt','',label),el('dd','',money(row[key],row.currency)));dl.append(item);}card.append(dl);cards.append(card);}body.append(cards);
  if(!data.currencies.length)body.append(el('p','admin-empty','在服务器编辑页填写续费周期、金额并开启续费提醒后，即可查看汇总。'));
  body.append(el('p','form-footnote','仅统计已开启续费提醒且有到期时间的服务器。月、年支出按周期折算；按天计费按一年 365 天估算。未来 30 天仅统计每台服务器下一次到期金额，不含已到期项。'));
  if(data.unpriced)body.append(el('p','form-footnote',data.unpriced+' 台尚未填写金额，未计入合计。'));
  const scroll=el('div','table-scroll'),table=el('table','node-table'),head=el('tr');for(const h of ['服务器','续费周期','每期金额','到期时间','提醒','操作'])head.append(el('th','',h));const thead=el('thead');thead.append(head);table.append(thead);const tbody=el('tbody');
  for(const r of records.filter(n=>!n.demo&&!n.removing)){const tr=el('tr');tr.append(el('td','',r.public.name),el('td','',cycleLabel(r)),el('td','',money(r.renewalAmount,r.renewalCurrency)),el('td','',r.expiresAt?new Date(r.expiresAt).toLocaleString('zh-CN',{hour12:false}):'未设置'),el('td','',r.notifyRenewal?'已开启':'已关闭'));const cell=el('td'),b=el('button','table-button','编辑');b.type='button';b.onclick=()=>edit(r.public.id);cell.append(b);tr.append(cell);tbody.append(tr);}table.append(tbody);scroll.append(table);body.append(scroll);
 }catch(e){if(box.isConnected)body.textContent=e.message;}
}
