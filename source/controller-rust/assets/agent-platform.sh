#!/bin/sh
set -eu
[ "$(id -u)" = 0 ] || { echo '需要 root 或 sudo 权限' >&2; exit 20; }
[ -f /etc/os-release ] || exit 21
# shellcheck disable=SC1091
. /etc/os-release
case "$ID:${VERSION_ID:-}" in
    debian:11|debian:12|debian:13|ubuntu:20.04|ubuntu:22.04|ubuntu:24.04|ubuntu:26.04) probe_platform=apt ;;
    rocky:8|rocky:8.*|rocky:9|rocky:9.*|rocky:10|rocky:10.*|almalinux:8|almalinux:8.*|almalinux:9|almalinux:9.*|almalinux:10|almalinux:10.*) probe_platform=rpm ;;
    centos:9|centos:10) case "$NAME" in *Stream*) probe_platform=rpm ;; *) exit 21 ;; esac ;;
    fedora:42|fedora:43|fedora:44|amzn:2023) probe_platform=rpm ;;
    *) echo '不支持此 Linux 发行版本' >&2; exit 21 ;;
esac
[ -d /run/systemd/system ] && command -v systemctl >/dev/null || exit 22
case "$(uname -m)" in x86_64) probe_arch=amd64 ;; aarch64|arm64) probe_arch=arm64 ;; *) exit 23 ;; esac
if [ "${1:-}" = prepare ]; then
    if [ "$probe_platform" = apt ]; then
        probe_missing=''
        for probe_pair in 'python3:python3' 'tmux:tmux' 'tar:tar' 'gzip:gzip' 'useradd:passwd' 'getent:libc-bin'; do
            command -v "${probe_pair%%:*}" >/dev/null || probe_missing="$probe_missing ${probe_pair#*:}"
        done
        [ -s /etc/ssl/certs/ca-certificates.crt ] || probe_missing="$probe_missing ca-certificates"
        if [ -n "$probe_missing" ]; then
            export DEBIAN_FRONTEND=noninteractive
            apt-get update -qq
            # Names above are fixed, never taken from user input.
            # shellcheck disable=SC2086
            apt-get install -y -qq --no-install-recommends $probe_missing
        fi
    else
        probe_missing=''
        for probe_pair in 'python3:python3' 'tmux:tmux' 'tar:tar' 'gzip:gzip' 'useradd:shadow-utils' 'getent:glibc-common'; do
            command -v "${probe_pair%%:*}" >/dev/null || probe_missing="$probe_missing ${probe_pair#*:}"
        done
        [ -s /etc/pki/tls/certs/ca-bundle.crt ] || probe_missing="$probe_missing ca-certificates"
        if [ -n "$probe_missing" ]; then
            # shellcheck disable=SC2086
            dnf install -y -q $probe_missing
        fi
    fi
fi
printf '%s\n' "$probe_arch"
