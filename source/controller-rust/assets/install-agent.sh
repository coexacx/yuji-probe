#!/bin/sh
set -eu
umask 077
[ "$(id -u)" = 0 ]
. /etc/os-release
case "$ID" in debian|ubuntu) ;; *) exit 21 ;; esac
if [ ! -s /etc/ssl/certs/ca-certificates.crt ] || ! command -v python3 >/dev/null; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq
    apt-get install -y --no-install-recommends ca-certificates python3
fi
command -v systemctl >/dev/null
command -v useradd >/dev/null
probe_ssh_user=__SSH_USER__
getent passwd vistart-probe >/dev/null || useradd --system --home-dir /var/lib/vistart-probe-agent --shell /usr/sbin/nologin vistart-probe
python3 - "$probe_ssh_user" <<'PY'
import os,sys,json,pwd,pathlib,tempfile,stat,subprocess,time,hashlib
user=pwd.getpwnam(sys.argv[1]);agent=pwd.getpwnam('vistart-probe')
install=pathlib.Path('/opt/vistart-probe-agent');state=pathlib.Path('/var/lib/vistart-probe-agent')
unit=pathlib.Path('/etc/systemd/system/vistart-probe-agent.service');sudo=pathlib.Path('/etc/sudoers.d/vistart-probe-recovery')
def checkdir(path):
    if path.exists() and (path.is_symlink() or not path.is_dir()):raise RuntimeError('unsafe installation directory')
    if path.resolve()!=path:raise RuntimeError('symlinked installation directory')
checkdir(install);checkdir(state)
install.mkdir(mode=0o755,exist_ok=True);os.chown(install,0,0);install.chmod(0o755)
state.mkdir(mode=0o700,exist_ok=True)
old_dir=(state.stat().st_uid,state.stat().st_gid,stat.S_IMODE(state.stat().st_mode))
files={}
def capture(path):
    if path.is_symlink():raise RuntimeError('symbolic link rejected')
    if path.exists():
        st=path.stat()
        if not stat.S_ISREG(st.st_mode) or st.st_nlink!=1 or st.st_size>32*1024*1024:raise RuntimeError('unsafe managed file')
        files[path]=(path.read_bytes(),st.st_uid,st.st_gid,stat.S_IMODE(st.st_mode))
    else:files[path]=None
def atomic(path,raw,uid=0,gid=0,mode=0o600):
    fd,name=tempfile.mkstemp(prefix='.yuji-',dir=path.parent)
    try:
        os.fchown(fd,uid,gid);os.fchmod(fd,mode)
        with os.fdopen(fd,'wb') as f:f.write(raw);f.flush();os.fsync(f.fileno())
        os.replace(name,path)
    finally:
        if os.path.exists(name):os.unlink(name)
def service(action,check=True):
    return subprocess.run(['systemctl',action,unit.name],check=check,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
def keyfile(home,uid,gid):
    home=pathlib.Path(home);checkdir(home)
    directory=home/'.ssh';checkdir(directory)
    directory.mkdir(mode=0o700,exist_ok=True)
    os.chown(directory,uid,gid);directory.chmod(0o700)
    if directory.stat().st_uid not in (0,uid):raise RuntimeError('unsafe SSH directory owner')
    fd=os.open(directory,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
    try:
        source=os.open('authorized_keys',os.O_RDONLY|os.O_NOFOLLOW,dir_fd=fd)
    except FileNotFoundError:
        raw=b'';existed=False
    else:
        with os.fdopen(source,'rb') as f:
            st=os.fstat(f.fileno())
            if not stat.S_ISREG(st.st_mode) or st.st_nlink!=1 or st.st_size>1024*1024:raise RuntimeError('unsafe SSH keys')
            raw=f.read();existed=True
    return dict(fd=fd,raw=raw,uid=uid,gid=gid,existed=existed)
def putkeys(entry,raw):
    name='.yuji-'+os.urandom(16).hex()
    fd=os.open(name,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600,dir_fd=entry['fd'])
    try:
        os.fchown(fd,entry['uid'],entry['gid'])
        with os.fdopen(fd,'wb') as f:f.write(raw);f.flush();os.fsync(f.fileno())
        os.rename(name,'authorized_keys',src_dir_fd=entry['fd'],dst_dir_fd=entry['fd'])
        os.fsync(entry['fd'])
    finally:
        try:os.unlink(name,dir_fd=entry['fd'])
        except FileNotFoundError:pass
managed=[install/'agent',install/'agent.manifest',install/'management.json',state/'config.json',unit,sudo]
for path in managed:capture(path)
oldmeta=json.loads(files[install/'management.json'][0]) if files[install/'management.json'] else None
keys={}
keys[user.pw_dir]=keyfile(user.pw_dir,user.pw_uid,user.pw_gid)
if oldmeta and oldmeta['ssh_home'] not in keys:
    keys[oldmeta['ssh_home']]=keyfile(oldmeta['ssh_home'],oldmeta['ssh_uid'],oldmeta['ssh_gid'])
was_active=service('is-active',False).returncode==0;was_enabled=service('is-enabled',False).returncode==0
try:
    os.chown(state,agent.pw_uid,agent.pw_gid);state.chmod(0o700)
    oldkeys=[oldmeta[k] for k in ('public_key','recovery_public')] if oldmeta else []
    for home,entry in keys.items():
        lines=entry['raw'].decode().splitlines()
        lines=[line for line in lines if not any(k in line for k in oldkeys) and (oldmeta or not line.endswith(' vistart-probe-managed'))]
        if home==user.pw_dir:
            normal=pathlib.Path('authorized-key').read_text().strip()
            recovery=' '.join(pathlib.Path('recovery-public').read_text().split()[:2])
            command='/opt/vistart-probe-agent/agent --manage -config /var/lib/vistart-probe-agent/config.json'
            if user.pw_uid:
                subprocess.run(['sudo','-V'],check=True,stdout=subprocess.DEVNULL)
                atomic(sudo,(user.pw_name+' ALL=(root) NOPASSWD: '+command+'\n').encode(),mode=0o440)
                subprocess.run(['visudo','-cf',str(sudo)],check=True,stdout=subprocess.DEVNULL)
                command='/usr/bin/sudo -n '+command
            elif sudo.exists():sudo.unlink()
            lines += [normal,'restrict,command="'+command+'" '+recovery+' vistart-probe-recovery']
        putkeys(entry,('\n'.join(lines)+'\n').encode())
    config=json.loads(pathlib.Path('config.json').read_text())
    normal='ssh-ed25519 '+pathlib.Path('authorized-key').read_text().split('ssh-ed25519 ',1)[1].split()[0]
    recovery=' '.join(pathlib.Path('recovery-public').read_text().split()[:2])
    meta={'node_id':config['node_id'],'ssh_home':user.pw_dir,'ssh_uid':user.pw_uid,'ssh_gid':user.pw_gid,'public_key':normal,'recovery_public':recovery}
    atomic(install/'agent',pathlib.Path('agent').read_bytes(),mode=0o755)
    atomic(install/'agent.manifest',pathlib.Path('agent.manifest').read_bytes())
    atomic(install/'management.json',json.dumps(meta).encode())
    atomic(state/'config.json',pathlib.Path('config.json').read_bytes(),agent.pw_uid,agent.pw_gid)
    atomic(unit,b"""[Unit]
Description=Yuji Probe monitoring agent
After=network-online.target
Wants=network-online.target
[Service]
Type=simple
User=vistart-probe
Group=vistart-probe
ExecStart=/opt/vistart-probe-agent/agent -config /var/lib/vistart-probe-agent/config.json
Restart=always
RestartSec=5
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/vistart-probe-agent
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictSUIDSGID=true
LockPersonality=true
CapabilityBoundingSet=
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
UMask=0077
MemoryMax=96M
CPUQuota=10%
TasksMax=32
LimitNOFILE=128
[Install]
WantedBy=multi-user.target
""",mode=0o644)
    subprocess.run(['systemctl','daemon-reload'],check=True)
    before=time.time();service('enable');service('restart')
    expected=hashlib.sha256(config['token'].encode()).hexdigest()
    for _ in range(50):
        try:health=json.loads((state/'agent.health').read_text())
        except (OSError,ValueError):health={}
        if health.get('at',0)>before and health.get('node_id')==config['node_id'] and health.get('controller_url')==config['controller_url'] and health.get('credential_digest')==expected:break
        time.sleep(1)
    else:raise RuntimeError('new Agent did not confirm WSS connection')
except BaseException:
    service('stop',False)
    for path,value in files.items():
        if value is None:
            try:path.unlink()
            except FileNotFoundError:pass
        else:atomic(path,*value)
    for entry in keys.values():
        if entry['existed']:putkeys(entry,entry['raw'])
        else:
            try:os.unlink('authorized_keys',dir_fd=entry['fd'])
            except FileNotFoundError:pass
    os.chown(state,*old_dir[:2]);state.chmod(old_dir[2])
    subprocess.run(['systemctl','daemon-reload'],check=False)
    service('enable' if was_enabled else 'disable',False)
    if was_active:service('start',False)
    raise
finally:
    for entry in keys.values():os.close(entry['fd'])
print('probe_install_ok')
PY
