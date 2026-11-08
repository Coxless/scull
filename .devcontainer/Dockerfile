# Scull / Coxless 開発用コンテナ(Ubuntu 24.04)
FROM ubuntu:24.04

ARG USERNAME=ubuntu
ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      build-essential clang lld pkg-config git curl ca-certificates sudo \
      qemu-system-misc gcc-riscv64-unknown-elf gdb-multiarch \
 && rm -rf /var/lib/apt/lists/* \
 && echo "$USERNAME ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/$USERNAME

USER $USERNAME
ENV PATH=/home/$USERNAME/.cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain stable -c rustfmt -c clippy \
 && rustup target add riscv64gc-unknown-none-elf
