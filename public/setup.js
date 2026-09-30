const form=document.querySelector('#setup-form');
form?.addEventListener('submit',async event=>{
 event.preventDefault();const button=form.querySelector('button');const error=document.querySelector('#setup-error');
 button.disabled=true;button.textContent='正在安装…';error.hidden=true;
 try{
  const response=await fetch('/install',{method:'POST',credentials:'same-origin',headers:{'Content-Type':'application/json','X-CSRF-Token':form.dataset.csrf},body:JSON.stringify(Object.fromEntries(new FormData(form)))});
  const result=await response.json();if(!response.ok)throw Error(result.error||'安装未完成');
  form.querySelector('[name="password"]').value='';location.replace(result.redirect);
 }catch(e){error.textContent=e.message;error.hidden=false;button.disabled=false;button.textContent='重新尝试';}
});
