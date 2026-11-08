# Scull

能力指向の言語 **Scull** と、その上に作る OS **Coxless** のプロジェクト。

- コンセプト: [docs/00_language_and_os_concept.md](docs/00_language_and_os_concept.md)
- Scull 0 実装指示書: [docs/scull0/01_scull0_implementation_spec.md](docs/scull0/01_scull0_implementation_spec.md)

## 開発環境(Ubuntu 24.04)

必要なもの: Rust(stable、`rust-toolchain.toml` で固定)、C コンパイラ(`cc` / clang)。

```sh
./scripts/setup-dev.sh          # Scull 0 に必要なもの
./scripts/setup-dev.sh --riscv  # 加えて QEMU / RISC-V ツールチェーン(Coxless 用)
```

VS Code / GitHub Codespaces では `.devcontainer/` の Ubuntu 24.04 コンテナをそのまま使える
(RISC-V ツールと QEMU も入っている)。

## 状態

開発環境の準備まで。コンパイラ `scullc` は実装指示書のマイルストーン M1 から作る。
