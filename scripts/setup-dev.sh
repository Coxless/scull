#!/usr/bin/env bash
# Scull / Coxless 開発環境セットアップ(Ubuntu 24.04 向け)
#
# 使い方:
#   ./scripts/setup-dev.sh            # Scull 0 に必要なもの(Rust + C コンパイラ)
#   ./scripts/setup-dev.sh --riscv    # 加えて RISC-V / QEMU(Coxless 用、フェーズ3以降)
#
# 冪等なので何度実行してもよい。
set -euo pipefail

WITH_RISCV=0
for arg in "$@"; do
  case "$arg" in
    --riscv) WITH_RISCV=1 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

SUDO=""
if [ "$(id -u)" -ne 0 ]; then SUDO="sudo"; fi

APT_PKGS=(build-essential clang lld pkg-config git curl ca-certificates)
if [ "$WITH_RISCV" -eq 1 ]; then
  APT_PKGS+=(qemu-system-misc gcc-riscv64-unknown-elf gdb-multiarch)
fi

missing=()
for p in "${APT_PKGS[@]}"; do
  dpkg -s "$p" >/dev/null 2>&1 || missing+=("$p")
done
if [ "${#missing[@]}" -gt 0 ]; then
  echo "==> apt install: ${missing[*]}"
  $SUDO apt-get update
  DEBIAN_FRONTEND=noninteractive $SUDO apt-get install -y --no-install-recommends "${missing[@]}"
fi

# Rust(rustup)。バージョンとコンポーネントは rust-toolchain.toml で固定する。
if ! command -v rustup >/dev/null 2>&1; then
  echo "==> install rustup"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi

cd "$(dirname "$0")/.."
rustup show active-toolchain >/dev/null 2>&1 || rustup toolchain install
if [ "$WITH_RISCV" -eq 1 ]; then
  rustup target add riscv64gc-unknown-none-elf
fi

echo "==> versions"
rustc --version
cargo --version
cc --version | head -n1
clang --version | head -n1
if [ "$WITH_RISCV" -eq 1 ]; then qemu-system-riscv64 --version | head -n1; fi
echo "==> done"
