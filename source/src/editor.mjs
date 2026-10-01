import {EditorState, Compartment} from '@codemirror/state';
import {EditorView, lineNumbers, highlightActiveLineGutter, keymap, highlightActiveLine, drawSelection} from '@codemirror/view';
import {defaultKeymap, history, historyKeymap, indentWithTab} from '@codemirror/commands';
import {search, searchKeymap, openSearchPanel} from '@codemirror/search';
import {syntaxHighlighting, defaultHighlightStyle, StreamLanguage, bracketMatching} from '@codemirror/language';
import {javascript} from '@codemirror/lang-javascript';
import {json} from '@codemirror/lang-json';
import {css} from '@codemirror/lang-css';
import {html} from '@codemirror/lang-html';
import {python} from '@codemirror/lang-python';
import {yaml} from '@codemirror/lang-yaml';
import {shell} from '@codemirror/legacy-modes/mode/shell';
import {diffLines} from 'diff';
const lang=path=>{const ext=path.split('.').pop().toLowerCase();if(['js','mjs','cjs','ts','tsx','jsx'].includes(ext))return javascript({typescript:ext.startsWith('t'),jsx:ext.endsWith('x')});if(ext==='json')return json();if(ext==='css')return css();if(['html','htm','xml','svg'].includes(ext))return html();if(ext==='py')return python();if(['yaml','yml'].includes(ext))return yaml();if(['sh','bash','zsh'].includes(ext)||/\/\.(bashrc|profile)$/.test(path))return StreamLanguage.define(shell);return [];};
export function createEditor(input,save){
 const mount=document.createElement('div');mount.className='file-code-editor';input.after(mount);input.hidden=true;
 const readonly=new Compartment(),language=new Compartment(),wrap=new Compartment();let updating=false;
 const view=new EditorView({parent:mount,state:EditorState.create({doc:input.value,extensions:[
 EditorState.phrases.of({'Find':'查找','Replace':'替换','next':'下一个','previous':'上一个','all':'全选','match case':'区分大小写','by word':'完整单词','regexp':'正则表达式','replace':'替换','replace all':'全部替换','close':'关闭','current match':'当前匹配','replaced $ matches':'已替换 $ 处'}),
 lineNumbers(),highlightActiveLineGutter(),history(),drawSelection(),highlightActiveLine(),bracketMatching(),
 syntaxHighlighting(defaultHighlightStyle),search({top:true}),
 keymap.of([{key:'Mod-s',run:()=>{save();return true;}},...defaultKeymap,...historyKeymap,...searchKeymap,indentWithTab]),
 readonly.of(EditorState.readOnly.of(false)),language.of([]),wrap.of([]),
 EditorView.contentAttributes.of({'aria-label':'文件内容',spellcheck:'false',autocapitalize:'off',autocorrect:'off'}),
 EditorView.updateListener.of(u=>{if(u.docChanged&&!updating){input.value=u.state.doc.toString();input.dispatchEvent(new Event('input'));}}),
 EditorView.theme({'&':{height:'100%',color:'inherit',backgroundColor:'transparent'},'.cm-scroller':{overflow:'auto',fontFamily:'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',fontSize:'13px',lineHeight:'1.6'},'.cm-content':{padding:'12px 0',caretColor:'currentColor'},'.cm-gutters':{backgroundColor:'transparent',color:'inherit',borderRight:'1px solid #8883'},'.cm-activeLine,.cm-activeLineGutter':{backgroundColor:'#80808015'},'.cm-panels':{backgroundColor:'transparent',color:'inherit'},'.cm-search input':{color:'inherit',backgroundColor:'transparent',maxWidth:'160px'},'.cm-selectionBackground':{backgroundColor:'#659bbc55 !important'},'.cm-cursor':{borderLeftColor:'currentColor'}})
 ]})});
 function sync(path='',reset=false){updating=true;const text=input.value;const changes=view.state.doc.toString()===text?undefined:{from:0,to:view.state.doc.length,insert:text};view.dispatch({changes,effects:[readonly.reconfigure(EditorState.readOnly.of(input.readOnly||input.disabled)),...(path?[language.reconfigure(lang(path))]:[])]});updating=false;if(reset)view.dispatch({selection:{anchor:0}});}
 return {sync,wrap:value=>view.dispatch({effects:wrap.reconfigure(value?EditorView.lineWrapping:[])}),search:()=>openSearchPanel(view),focus:()=>view.focus(),destroy:()=>{view.destroy();mount.remove();}};
}
export function reviewChanges(scope,before,after,path){
 return new Promise(resolve=>{
 const d=document.createElement('dialog');d.className='node-dialog file-diff-dialog';
 const title=document.createElement('h2');title.textContent='保存前核对';
 const name=document.createElement('p');name.textContent=path;
 const body=document.createElement('div');body.className='file-diff-content';
 const parts=diffLines(before,after,{timeout:150,maxEditLength:4000});
 if(!parts){const p=document.createElement('p');p.textContent='修改范围较大，请返回编辑器核对全文后保存。';body.append(p);}
 else {let shown=0;for(const part of parts){if(!part.added&&!part.removed){const lines=part.value.split('\n');const p=document.createElement('pre');p.textContent=lines.length>8?lines.slice(0,3).join('\n')+'\n… '+(lines.length-6)+' 行未修改 …\n'+lines.slice(-3).join('\n'):part.value;body.append(p);continue;}const p=document.createElement('pre');p.className=part.added?'diff-added':'diff-removed';const text=part.value.slice(0,65536-shown);p.textContent=(part.added?'+ ':'− ')+text;shown+=text.length;body.append(p);if(shown>=65536){const note=document.createElement('p');note.textContent='预览已截断，请在编辑器核对完整内容。';body.append(note);break;}}}
 const actions=document.createElement('div');actions.className='dialog-actions';const cancel=document.createElement('button'),save=document.createElement('button');cancel.type=save.type='button';cancel.textContent='继续编辑';save.textContent='确认保存';save.className='primary-button';
 const finish=v=>{d.close();d.remove();resolve(v);};cancel.onclick=()=>finish(false);save.onclick=()=>finish(true);d.oncancel=e=>{e.preventDefault();finish(false);};d.append(title,name,body,actions);actions.append(cancel,save);scope.append(d);d.showModal();cancel.focus();
 });
}

