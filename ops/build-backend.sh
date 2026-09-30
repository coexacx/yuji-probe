#!/usr/bin/env bash
set -euo pipefail
if (( $# > 1 )); then echo 'Usage: build-backend.sh [output-directory]' >&2; exit 2; fi
probe_ops_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
probe_root="$(CDPATH= cd -- "$probe_ops_dir/.." && pwd)"
probe_output="${1:-$probe_root/build}"
mkdir -p -- "$probe_output"
probe_output="$(CDPATH= cd -- "$probe_output" && pwd)"
probe_tmp="$(mktemp -d "$probe_output/.compile.XXXXXXXX")"
trap 'rm -rf -- "$probe_tmp"' EXIT
for probe_crate in agent-rust controller-rust; do
 cd "$probe_root/source/$probe_crate"
 cargo fmt --all --check
 cargo test --locked
 cargo clippy --locked --all-targets -- -D warnings
 for probe_arch in amd64 arm64; do
  if [[ "$probe_arch" == amd64 ]]; then probe_target=x86_64-unknown-linux-musl; else
   probe_target=aarch64-unknown-linux-musl
   export CC_aarch64_unknown_linux_musl="${CC_aarch64_unknown_linux_musl:-aarch64-linux-gnu-gcc}"
   export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER:-aarch64-linux-gnu-gcc}"
  fi
  cargo build --locked --release --target "$probe_target"
  probe_target_dir="${CARGO_TARGET_DIR:-$PWD/target}"
  if [[ "$probe_crate" == controller-rust ]]; then
   cp "$probe_target_dir/$probe_target/release/vistart-probe-controller" "$probe_tmp/probe-linux-$probe_arch"
  else
   probe_version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)"
   cp "$probe_target_dir/$probe_target/release/vistart-probe-agent" "$probe_tmp/vistart-probe-agent-$probe_version-linux-$probe_arch"
  fi
 done
done
for probe_file in "$probe_tmp"/*; do mv -- "$probe_file" "$probe_output/"; done
printf 'Rust main controller and Agent binaries written to %s\n' "$probe_output"
