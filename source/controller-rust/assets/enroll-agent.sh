#!/bin/sh
set -eu
umask 077
[ "$(id -u)" = 0 ] || { echo '请使用 root 或 sudo 执行接入命令' >&2; exit 1; }
[ "$#" = 1 ] || exit 2
case "$1" in *[!a-f0-9]*|'') exit 2 ;; esac
[ "$(printf %s "$1" | wc -c)" = 64 ] || exit 2
[ -r /etc/os-release ] && [ -d /run/systemd/system ] || { echo '需要支持 systemd 的 Linux 系统' >&2; exit 3; }
if ! command -v python3 >/dev/null || [ ! -s /etc/ssl/certs/ca-certificates.crt ] && [ ! -s /etc/pki/tls/certs/ca-bundle.crt ]; then
 if command -v apt-get >/dev/null; then
  export DEBIAN_FRONTEND=noninteractive
  apt-get update -qq
  apt-get install -y -qq --no-install-recommends python3 ca-certificates
 elif command -v dnf >/dev/null; then dnf install -y -q python3 ca-certificates
 else echo '请先安装 python3 和 ca-certificates' >&2; exit 3; fi
fi
probe_join_dir=$(mktemp -d /var/tmp/yuji-enroll.XXXXXXXX)
trap 'rm -rf -- "$probe_join_dir"' EXIT HUP INT TERM
printf '%s' "$1" > "$probe_join_dir/token"
set --
python3 - "$probe_join_dir" <<'PY'
import os,sys,json,base64,hashlib,urllib.request,platform,tarfile,io,pathlib,subprocess
origin=__ORIGIN_JSON__
directory=pathlib.Path(sys.argv[1])
machine=platform.machine()
arch={'x86_64':'amd64','aarch64':'arm64','arm64':'arm64'}.get(machine)
if not arch:raise SystemExit('不支持此 CPU 架构')
host=None
for name in ['ssh_host_ed25519_key.pub','ssh_host_ecdsa_key.pub','ssh_host_rsa_key.pub']:
 p=pathlib.Path('/etc/ssh')/name
 if p.is_file():host=p.read_text().strip();break
if not host:raise SystemExit('找不到 SSH 主机公钥，请先安装并启用 OpenSSH 服务')
class NoRedirect(urllib.request.HTTPRedirectHandler):
 def redirect_request(self,*args,**kwargs):raise RuntimeError('禁止跳转接入地址')
token=(directory/'token').read_text()
payload=json.dumps({'token':token,'arch':arch,'host_key':host}).encode()
opener=urllib.request.build_opener(urllib.request.ProxyHandler({}),NoRedirect())
req=urllib.request.Request(origin+'/api/enroll/claim',data=payload,headers={'Content-Type':'application/json','User-Agent':'Yuji-Probe-Enroll'})
try:
 with opener.open(req,timeout=170) as response:
  raw=response.read(32*1024*1024+1)
 if len(raw)>32*1024*1024:raise RuntimeError('安装包响应过大')
 data=json.loads(raw)
 archive=base64.b64decode(data['archive'],validate=True)
 if hashlib.sha256(archive).hexdigest()!=data['sha256']:raise RuntimeError('安装包校验失败')
 allowed={'agent','config.json','install.sh','agent-platform.sh','authorized-key','recovery-public','agent.manifest'}
 with tarfile.open(fileobj=io.BytesIO(archive),mode='r:gz') as t:
  members=t.getmembers()
  if len(members)!=len(allowed) or {m.name for m in members}!=allowed or any(not m.isfile() or m.size>32*1024*1024 for m in members):raise RuntimeError('安装包内容不正确')
  for m in members:
   with open(directory/m.name,'xb') as out:out.write(t.extractfile(m).read())
   os.chmod(directory/m.name,0o700 if m.name=='agent' else 0o600)
 (directory/'token').unlink()
 subprocess.run(['sh','./install.sh'],cwd=directory,check=True)
 print('Agent 已接入，返回面板查看监控。')
except Exception:
 print('接入未完成；请在面板重新生成命令。',file=sys.stderr)
 raise
PY

