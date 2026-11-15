
'use strict';
const $=id=>document.getElementById(id);
let auth='',ws=null,timer=null,busy=false,current='overview',history=[],hist=0,points=[];
const encoder=new TextEncoder();let ansi='',output='';
function preference(key,value){try{if(value!==undefined)localStorage.setItem('chaos-'+key,value);else return localStorage.getItem('chaos-'+key)}catch(_){return null}}
// Uploaded HTML has an opaque origin and no network access, including previews.
const appPolicy="<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; connect-src 'none'; form-action 'none'; base-uri 'none'\">";
function showApp(source){$('appFrame').srcdoc=appPolicy+source;$('appFrame').hidden=false}
function notice(text){$('toastMessage').textContent=text;$('toast').classList.add('show');clearTimeout(notice.timer);notice.timer=setTimeout(()=>$('toast').classList.remove('show'),3500)}
async function api(path,method='GET',body){const controller=new AbortController(),timeout=setTimeout(()=>controller.abort(),15000);try{const r=await fetch('/api/'+path,{method,body,headers:{Authorization:'Basic '+auth,'Content-Type':'application/octet-stream'},signal:controller.signal,cache:'no-store'});const data=await r.json();if(!r.ok)throw Error(data.error||'请求失败 '+r.status);return data}finally{clearTimeout(timeout)}}
function theme(){const t=$('theme').value;document.documentElement.dataset.theme=t==='auto'?(matchMedia('(prefers-color-scheme:dark)').matches?'dark':'light'):t;preference('theme',t);chart()}
for(const [id,key,value] of [['theme','theme','auto'],['interval','interval','5'],['fontSize','font','14']]){const saved=preference(key);$(id).value=[...$(id).options].some(x=>x.value===saved)?saved:value;$(id).onchange=()=>{preference(key,$(id).value);theme();$('terminalOutput').style.fontSize=$('fontSize').value+'px';schedule()}}
theme();$('terminalOutput').style.fontSize=$('fontSize').value+'px';matchMedia('(prefers-color-scheme:dark)').addEventListener('change',theme);
function duration(s){if(s===null||s===undefined)return '暂未提供';const d=Math.floor(s/86400);return(d?d+' 天 ':'')+[Math.floor(s/3600)%24,Math.floor(s/60)%60,s%60].map(x=>String(x).padStart(2,'0')).join(':')}
function bytes(n){return(n/1024).toFixed(1)+' KiB'}
function details(d){$('miniModel').textContent=d.model;$('miniIp').textContent=d.ip;$('firmware').textContent=d.firmware;$('statusFirmware').textContent=d.firmware;$('reboot').disabled=!d.canReboot;$('reboot').title=d.canReboot?'':'该固件未开放重启接口';const list=$('deviceDetails');list.replaceChildren();for(const [key,value] of [['掩码',d.mask],['网关',d.gateway],['DNS（DHCP）',d.dns==='0.0.0.0'?'未提供':d.dns],['MTU',d.mtu],['接收 / 发送帧',d.rxFrames+' / '+d.txFrames]]){const row=document.createElement('div');row.className='setting-row';const k=document.createElement('span'),v=document.createElement('span');k.textContent=key;v.textContent=value;row.append(k,v);list.append(row)}}
async function refresh(){if(busy||!auth)return;busy=true;try{const d=await api('system/status');$('model').textContent=d.model||'未提供';$('ramValue').textContent=d.heapTotal?Math.round(100*(d.heapTotal-d.heapFree)/d.heapTotal):'—';$('ramDetail').textContent=d.heapTotal?bytes(d.heapTotal-d.heapFree)+' / '+bytes(d.heapTotal):'暂未提供';$('ramBar').style.width=d.heapTotal?(100*(d.heapTotal-d.heapFree)/d.heapTotal)+'%':'0%';$('uptime').textContent=duration(d.uptime);$('ip').textContent=d.ip;$('updated').textContent='更新于 '+new Date().toLocaleTimeString();$('connection').textContent='已连接';$('miniConnection').textContent='已连接';$('networkState').textContent='已连接';$('systemState').textContent='运行中';$('eventTime').textContent=new Date().toLocaleTimeString();$('eventNetwork').textContent='已连接 '+d.ip;details(d);if(d.heapTotal){points.push(100*(d.heapTotal-d.heapFree)/d.heapTotal);if(points.length>40)points.shift();chart()}}catch(e){$('connection').textContent='连接中断';$('miniConnection').textContent='连接中断';$('networkState').textContent='连接中断';$('systemState').textContent='数据未更新';$('updated').textContent='数据未更新 · '+e.message}finally{busy=false}}
function schedule(){clearTimeout(timer);if(auth&&!document.hidden&&+$('interval').value)timer=setTimeout(async()=>{await refresh();schedule()},+$('interval').value*1000)}
document.addEventListener('visibilitychange',schedule);
function chart(){const canvas=$('resourceChart');if(!canvas)return;const w=canvas.clientWidth||300,h=140,d=devicePixelRatio||1;canvas.width=w*d;canvas.height=h*d;const c=canvas.getContext('2d');c.scale(d,d);c.clearRect(0,0,w,h);c.strokeStyle=getComputedStyle(document.documentElement).getPropertyValue('--primary');c.lineWidth=2;c.beginPath();points.forEach((p,i)=>{const x=i*w/39,y=h-p*h/100;if(!i)c.moveTo(x,y);else c.lineTo(x,y)});c.stroke()}window.addEventListener('resize',chart);
$('loginForm').onsubmit=async e=>{e.preventDefault();try{auth=btoa($('username').value+':'+$('password').value);await api('system/status');$('password').value='';$('login').close();await refresh();schedule()}catch(e){auth='';notice('登录失败：'+e.message)}};
$('logout').onclick=()=>{closeTerminal();auth='';clearTimeout(timer);$('appFrame').srcdoc='';$('appFrame').hidden=true;$('login').showModal();$('connection').textContent='未连接';$('reboot').disabled=true};
const pageNames={overview:'设备概览',terminal:'NSH 终端',apps:'网页应用',settings:'设备设置'};
function navigate(page){if(current==='terminal'&&page!=='terminal')closeTerminal();current=page;document.querySelectorAll('.page').forEach(p=>p.classList.toggle('active',p.id==='page-'+page));document.querySelectorAll('[data-page]').forEach(p=>p.classList.toggle('active',p.dataset.page===page));$('pageTitle').textContent=pageNames[page];$('sidebar').classList.remove('open');$('scrim').classList.remove('show');if(page==='overview')chart()}
document.querySelectorAll('[data-page]').forEach(b=>b.onclick=()=>navigate(b.dataset.page));$('menuBtn').onclick=()=>{$('sidebar').classList.add('open');$('scrim').classList.add('show')};$('scrim').onclick=()=>{$('sidebar').classList.remove('open');$('scrim').classList.remove('show')};$('themeBtn').onclick=()=>{$('theme').value=document.documentElement.dataset.theme==='dark'?'light':'dark';theme()};
$('refresh').onclick=refresh;
function append(text){ansi+=text;let at=0;while(at<ansi.length){const ch=ansi[at];if(ch==='\x1b'){if(at+1>=ansi.length)break;if(ansi[at+1]==='['){let end=at+2;while(end<ansi.length&&!/[\x40-\x7e]/.test(ansi[end]))end++;if(end===ansi.length)break;at=end+1;continue}at+=2;continue}if(ch==='\b'||ch==='\x7f')output=output.slice(0,-1);else if(ch!=='\r'&&ch!=='\x00')output+=ch;at++}ansi=ansi.slice(at);if(ansi.length>128)ansi='';if(output.length>65536)output=output.slice(-49152);$('terminalOutput').textContent=output;$('terminalOutput').scrollTop=$('terminalOutput').scrollHeight}
function closeTerminal(){if(ws){const socket=ws;ws=null;clearInterval(socket.heartbeat);socket.close()}$('terminalState').textContent='未连接';$('nshState').textContent='未连接'}
function send(text){if(!ws||ws.readyState!==WebSocket.OPEN){notice('请先连接终端');return false}const data=encoder.encode(text);if(data.length>1024){notice('单次输入不能超过 1024 字节');return false}ws.send(data);return true}
let attempt=0,busyState='';
function openTerminal(){
  output='';ansi='';$('terminalOutput').textContent='';busyState='';
  const decoder=new TextDecoder(),socket=new WebSocket('ws://'+location.host+'/ws/nsh');ws=socket;socket.binaryType='arraybuffer';$('terminalState').textContent='正在连接';
  socket.onopen=()=>{if(ws!==socket){socket.close();return}socket.send(encoder.encode('AUTH '+auth));$('terminalState').textContent='正在启动 NSH';socket.heartbeat=setInterval(()=>{if(ws===socket&&socket.readyState===1)socket.send(new Uint8Array())},30000)};
  socket.onmessage=e=>{if(ws!==socket)return;append(typeof e.data==='string'?e.data:decoder.decode(e.data,{stream:true}));$('terminalState').textContent='已连接';$('nshState').textContent='已连接'};
  socket.onclose=e=>{clearInterval(socket.heartbeat);if(ws!==socket)return;ws=null;append(decoder.decode());
    /* 服务端把占用原因写在 close 帧的 reason 里(如"会话占用 5": 1 启动中 / 2 运行中 / 5 回收中) */
    if(e.reason)busyState=e.reason;
    /* 上一次会话还在回收(或刚断开、服务端还没处理完)时自动重试, 别让人手动点 */
    if(e.code===1013&&attempt<3){attempt++;$('terminalState').textContent='正在回收上一次会话，自动重试 '+attempt+'/3';$('nshState').textContent='未连接';setTimeout(()=>{if(!ws&&auth)openTerminal()},800*attempt);return}
    attempt=0;$('terminalState').textContent=e.code===1008?'认证失败':e.code===1013?('会话未退出：'+(busyState||'占用中')):'已断开';$('nshState').textContent='未连接'};
  socket.onerror=()=>{if(ws===socket)notice('终端连接失败，请检查网络或稍后重试')};
}
$('connectTerminal').onclick=()=>{if(ws||!auth)return;attempt=0;openTerminal()};
$('disconnectTerminal').onclick=closeTerminal;$('clearTerminal').onclick=()=>{output='';$('terminalOutput').textContent=''};
$('terminalForm').onsubmit=e=>{e.preventDefault();const text=$('terminalInput').value;if(send(text+'\n')){history.push(text);hist=history.length;$('terminalInput').value=''}};
$('terminalInput').onkeydown=e=>{if(e.key==='ArrowUp'||e.key==='ArrowDown'){e.preventDefault();hist=Math.max(0,Math.min(history.length,hist+(e.key==='ArrowUp'?-1:1)));$('terminalInput').value=history[hist]||''}if(e.ctrlKey&&e.key==='c'){e.preventDefault();send('\x03')}};
document.querySelectorAll('[data-key]').forEach(b=>b.onclick=()=>send(b.dataset.key==='interrupt'?'\x03':'\t'));document.querySelectorAll('[data-cmd]').forEach(b=>b.onclick=()=>send(b.dataset.cmd+'\n'));
$('uploadApp').onclick=async()=>{const f=selectedFile||$('appFile').files[0];if(!f){notice('请选择 HTML 文件');return}if(f.size>16384){notice('文件不能超过 16 KiB');return}try{await api('apps/upload','POST',await f.arrayBuffer());$('appFrame').hidden=true;$('appFrame').srcdoc='';$('appState').textContent='已上传';$('appName').textContent=f.name;$('appVersion').textContent=bytes(f.size)+' · 暂存 RAM';$('runApp').disabled=false;$('stopApp').disabled=false}catch(e){notice(e.message)}};
$('runApp').onclick=async()=>{try{await api('apps/run','POST');const r=await fetch('/app/',{headers:{Authorization:'Basic '+auth},cache:'no-store'});if(!r.ok)throw Error('应用打开失败');showApp(await r.text());$('appState').textContent='正在浏览器中运行'}catch(e){notice(e.message)}};
$('stopApp').onclick=async()=>{try{await api('apps/stop','POST');$('appFrame').hidden=true;$('appFrame').srcdoc='';$('appState').textContent='已停止'}catch(e){notice(e.message)}};
$('reboot').onclick=()=>{$('confirmTitle').textContent='重启手环';$('confirmMessage').textContent='当前终端和网络连接将结束，确定重启吗？';$('confirmDialog').showModal()};$('cancelConfirm').onclick=()=>$('confirmDialog').close();$('acceptConfirm').onclick=async()=>{$('confirmDialog').close();closeTerminal();try{await api('system/reboot','POST');notice('已提交重启请求')}catch(e){notice(e.message)}};
window.addEventListener('pagehide',closeTerminal);

let selectedFile=null;
function selectFile(file){if(!file)return;if(!/\.html?$/i.test(file.name)||file.size>16384){notice('请选择不超过 16 KiB 的 HTML 文件');return}selectedFile=file;$('selectedFile').style.display='flex';$('selectedFile').hidden=false;$('fileName').textContent=file.name;$('fileMeta').textContent=bytes(file.size);$('uploadApp').disabled=false;$('previewBtn').disabled=false}
$('dropZone').onclick=()=>$('appFile').click();$('dropZone').onkeydown=e=>{if(e.key==='Enter'||e.key===' '){e.preventDefault();$('appFile').click()}};$('appFile').onchange=()=>selectFile($('appFile').files[0]);$('dropZone').ondragover=e=>e.preventDefault();$('dropZone').ondrop=e=>{e.preventDefault();selectFile(e.dataTransfer.files[0])};$('removeFile').onclick=()=>{selectedFile=null;$('appFile').value='';$('selectedFile').hidden=true;$('uploadApp').disabled=true;$('previewBtn').disabled=true};$('previewBtn').onclick=async()=>{if(!selectedFile)return;showApp(await selectedFile.text())};
$('login').addEventListener('cancel',e=>e.preventDefault());$('login').showModal();

