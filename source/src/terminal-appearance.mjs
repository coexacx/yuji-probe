const original={background:'#121b29',foreground:'#dce6f0',cursor:'#b9dbef',selectionBackground:'#406584'};
const custom=new Set(['clear','sketch','anime','seasons']);
export function terminalAppearance(root=document.documentElement){
 if(!custom.has(root.dataset.appearance)&&!root.dataset.themePackage)return {...original};
 const css=getComputedStyle(root),dark=root.dataset.theme==='dark';
 const lightColors={black:'#24383e',red:'#a43b3d',green:'#286549',yellow:'#806016',blue:'#2c588e',magenta:'#85517c',cyan:'#266d75',white:'#52656b',brightBlack:'#66797c',brightRed:'#b23e44',brightGreen:'#39774e',brightYellow:'#916b1d',brightBlue:'#3669a6',brightMagenta:'#966184',brightCyan:'#347e86',brightWhite:'#24383e'};
 const darkColors={black:'#243641',red:'#eba29c',green:'#a5c6a1',yellow:'#dcca95',blue:'#9cbfdf',magenta:'#d1acd3',cyan:'#96c6c8',white:'#d6e3df',brightBlack:'#90a6ac',brightRed:'#f0b5af',brightGreen:'#b9d6b1',brightYellow:'#e9daad',brightBlue:'#b4d1ef',brightMagenta:'#e1c0e3',brightCyan:'#abd8d6',brightWhite:'#f1f5ed'};
 return {...(dark?darkColors:lightColors),background:'#00000000',foreground:css.getPropertyValue('--text').trim(),cursor:css.getPropertyValue('--accent').trim(),cursorAccent:dark?'#20313b':'#f8faf5',selectionBackground:dark?'#b4c6db4d':'#2b686b35',selectionInactiveBackground:dark?'#b4c6db2b':'#2b686b20'};
}
