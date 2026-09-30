#!/bin/sh
set -eu
umask 077
[ "$(id -u)" = 0 ]
. /etc/os-release
case "$ID" in debian|ubuntu) ;; *) exit 21 ;; esac
if [ ! -s /etc/ssl/certs/ca-certificates.crt ]; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq
    apt-get install -y --no-install-recommends ca-certificates
fi
command -v systemctl >/dev/null
command -v useradd >/dev/null
getent passwd vistart-probe >/dev/null || useradd --system --home-dir /var/lib/vistart-probe-agent --shell /usr/sbin/nologin vistart-probe
install -d -o root -g root -m 0755 /opt/vistart-probe-agent
install -d -o root -g vistart-probe -m 0750 /var/lib/vistart-probe-agent
probe_ssh_user=__SSH_USER__
probe_ssh_home=$(getent passwd "$probe_ssh_user" | cut -d: -f6)
probe_ssh_group=$(id -gn "$probe_ssh_user")
[ -d "$probe_ssh_home" ]
[ ! -L "$probe_ssh_home/.ssh" ]
[ ! -L "$probe_ssh_home/.ssh/authorized_keys" ]
install -d -o "$probe_ssh_user" -g "$probe_ssh_group" -m 0700 "$probe_ssh_home/.ssh"
probe_keys="$probe_ssh_home/.ssh/authorized_keys"
touch "$probe_keys"
probe_new_key=$(cat authorized-key)
if ! grep -qxF "$probe_new_key" "$probe_keys"; then
    cp -p "$probe_keys" ./authorized_keys.before
    printf '\n%s\n' "$probe_new_key" >> "$probe_keys"
fi
chown "$probe_ssh_user:$probe_ssh_group" "$probe_keys"
chmod 0600 "$probe_keys"
probe_had_binary=0
probe_had_config=0
probe_had_unit=0
[ ! -f /opt/vistart-probe-agent/agent ] || { cp -p /opt/vistart-probe-agent/agent ./agent.before; probe_had_binary=1; }
[ ! -f /var/lib/vistart-probe-agent/config.json ] || { cp -p /var/lib/vistart-probe-agent/config.json ./config.before; probe_had_config=1; }
[ ! -f /etc/systemd/system/vistart-probe-agent.service ] || { cp -p /etc/systemd/system/vistart-probe-agent.service ./unit.before; probe_had_unit=1; }
rollback() {
    if [ "$probe_had_binary" = 1 ]; then cp -p ./agent.before /opt/vistart-probe-agent/agent.rollback; mv /opt/vistart-probe-agent/agent.rollback /opt/vistart-probe-agent/agent; else rm -f /opt/vistart-probe-agent/agent; fi
    if [ "$probe_had_config" = 1 ]; then cp -p ./config.before /var/lib/vistart-probe-agent/config.json; else rm -f /var/lib/vistart-probe-agent/config.json; fi
    if [ "$probe_had_unit" = 1 ]; then cp -p ./unit.before /etc/systemd/system/vistart-probe-agent.service; else systemctl disable --now vistart-probe-agent.service >/dev/null 2>&1 || true; rm -f /etc/systemd/system/vistart-probe-agent.service; fi
    [ ! -f ./authorized_keys.before ] || cp -p ./authorized_keys.before "$probe_keys"
    systemctl daemon-reload
    [ "$probe_had_unit" != 1 ] || systemctl restart vistart-probe-agent.service || true
}
trap 'probe_exit=$?; if [ "$probe_exit" != 0 ]; then rollback; fi' EXIT
trap 'exit 24' HUP INT TERM
install -m 0755 -o root -g root agent /opt/vistart-probe-agent/agent.next
mv /opt/vistart-probe-agent/agent.next /opt/vistart-probe-agent/agent
install -m 0640 -o root -g vistart-probe config.json /var/lib/vistart-probe-agent/config.json.next
mv /var/lib/vistart-probe-agent/config.json.next /var/lib/vistart-probe-agent/config.json
cat > /etc/systemd/system/vistart-probe-agent.service <<'UNIT'
[Unit]
Description=Vistart Probe monitoring agent
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
Environment=GOMEMLIMIT=64MiB
Environment=GOMAXPROCS=1
[Install]
WantedBy=multi-user.target
UNIT
systemctl daemon-reload
if ! systemctl enable --now vistart-probe-agent.service >/dev/null 2>&1 || ! systemctl restart vistart-probe-agent.service || ! systemctl is-active --quiet vistart-probe-agent.service; then exit 23; fi
trap - EXIT HUP INT TERM
printf 'probe_install_ok\n'
