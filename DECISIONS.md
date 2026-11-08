# DECISIONS

実装中に決めたことを「日付・項目・選んだ解釈・理由」で1行ずつ記録する。

| 日付 | 項目 | 選んだ解釈 | 理由 |
|---|---|---|---|
| 2026-11-08 | 文書の置き場所 | `docs/` に設計文書、`docs/scull0/` に Scull 0 指示書 | コンセプト文書内の相対参照 `scull0/01_scull0_implementation_spec.md` をそのまま有効にするため |
| 2026-11-08 | 開発環境 | Ubuntu 24.04 + rustup stable(`rust-toolchain.toml`)+ clang/gcc。devcontainer と `scripts/setup-dev.sh` を用意 | どこでも同じ環境を再現するため |
