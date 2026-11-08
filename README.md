# Scull

能力指向の言語 **Scull** と、その上に作る OS **Coxless** のプロジェクト。
このリポジトリには、最初のサブセット **Scull 0** のコンパイラ `scullc`(Rust 製、C を生成)がある。

- コンセプト: [docs/00_language_and_os_concept.md](docs/00_language_and_os_concept.md)
- Scull 0 実装指示書: [docs/scull0/01_scull0_implementation_spec.md](docs/scull0/01_scull0_implementation_spec.md)
- 実装中に決めたこと: [DECISIONS.md](DECISIONS.md) / 次の候補: [NEXT.md](NEXT.md)

## ビルドと使い方

必要なもの: Rust(stable)と C コンパイラ(`cc`、gcc か clang)。依存クレートはない。

```sh
cargo build --release
./target/release/scullc run examples/hello.scull
```

| コマンド | 動作 |
|---|---|
| `scullc check FILE.scull` | 検査だけを行い、診断を出す |
| `scullc emit-c FILE.scull [-o OUT.c]` | C コードを出力する(既定は標準出力) |
| `scullc build FILE.scull [-o OUT]` | 実行ファイルを作る(既定の名前は入力ファイル名から拡張子を除いたもの) |
| `scullc run FILE.scull` | build してから実行し、その終了コードを返す |

- C コンパイラは環境変数 `CC`(既定は `cc`)で選び、`-std=c11 -O2 -Wall -Wextra` を付ける。
- 終了コード: 成功 0、コンパイルエラー 1(警告だけなら 0)。実行時エラーは 101。

テスト: `cargo test`(`tests/run` の実行テスト、`tests/fail` のエラーテスト、生成した C が gcc と clang の `-Werror -pedantic` で警告なしに通ることを確認する)。

開発環境の準備は `./scripts/setup-dev.sh`(Ubuntu 24.04)。`.devcontainer/` もある。

## 言語ツアー

### 1. 能力がなければ外の世界に触れない

関数が出力・時刻などに触れられるのは、シグネチャの `uses` 節に書いた**能力**を通してだけ。
グローバルな権限はない。`uses` は独立した行に書き、能力の増減が差分で目立つようにする。

```
fun main()
  uses Console
{
  Console.print("hello, Scull")
}
```

`uses Console` のない関数で `Console.print` を呼ぶとコンパイルエラーになる。

```
error[E0301]: 能力 `Console` が必要ですが、この関数では使えません
  --> tests/fail/sneaky.scull:3:3
   |
 3 |   Console.print("hi")
   |   ^^^^^^^
   = 助言: 関数 `sneaky` に `uses Console` を追加してください
```

`uses` に書いたのに使っていない能力は警告(W0301)になる。

### 2. 値と制御構造

型は `Int`(64 ビット)、`Bool`、`String`(不変)の3つ。戻り値の型を省くと値を返さない。
文字列の連結には `+` を使わず、`{式}` で埋め込む。

```
fun fib(n: Int) -> Int {
  if n < 2 {
    return n
  }
  return fib(n - 1) + fib(n - 2)
}

fun main()
  uses Console
{
  var i = 0
  while i <= 10 {
    Console.print("fib({i}) = {fib(i)}")
    i += 1
  }
}
```

- `let` は再代入できず、`var` はできる。シャドーイングは禁止。
- `if` は文で、`} else {` は同じ行に書く。比較演算子は連鎖できない。
- `&&` と `||` は短絡評価。式は左から右に評価する。
- 整数のオーバーフローとゼロ除算は、`scull: runtime error: ...` を出して終了コード 101 で止まる。

### 3. エフェクトとハンドラ:能力の実装を差し替える

能力は `effect` で宣言し、`handler` で実装する。`handle E by H { ... }` の中では、E が H で供給される。

```
effect Clock {
  fun now() -> Int
}

fun report()
  uses Console, Clock
{
  Console.print("now = {Clock.now()}")
}

handler FixedClock for Clock {
  fun now() -> Int {
    return 20261108
  }
}

// ハンドラ自身も能力を使える。使える能力は handle を書いた地点のもの(レキシカル)
handler LoggedClock for Clock
  uses Console
{
  fun now() -> Int {
    Console.print("[log] Clock.now called")
    return 42
  }
}

fun main()
  uses Console
{
  handle Clock by FixedClock {
    report()          // now = 20261108
  }
  handle Clock by LoggedClock {
    report()          // [log] Clock.now called / now = 42
  }
}
```

`main` の `uses` には `Console` しか書けないので、それ以外の能力は必ず `handle` で用意する。
テストでのモックやサンドボックスはこの仕組みで書く。

### 4. 組み込みの Console も差し替えられる

```
handler Silent for Console {
  fun print(s: String) {
  }
}

fun noisy()
  uses Console
{
  Console.print("this is hidden")
}

fun main()
  uses Console
{
  handle Console by Silent {
    noisy()
  }
  Console.print("only this is shown")
}
```

同じエフェクトの `handle` は入れ子にでき、内側が優先される。

### 生成される C

能力は「関数ポインタと環境ポインタの組」として関数の引数で渡す。生成される C にもグローバル変数はない。

```c
void scl_f_report(scl_eff_Clock *cap_Clock, scl_eff_Console *cap_Console);
/* Clock.now() は cap_Clock->op_now(cap_Clock->env) になる */
```

`scullc emit-c examples/clock.scull` で全体を確認できる。

## 既知の制限

- **文字列のメモリは解放しない**(リークを許容する)。補間や連結のたびに確保したまま残る。
- 型は `Int` / `Bool` / `String` だけ。構造体・配列・ジェネリクス・クロージャはない。
- ハンドラは tail-resumptive に限る(操作本体の戻り値がそのまま操作の結果。`resume` も継続もない)。
- 文字列補間 `{...}` の中には文字列リテラルと `{` `}` を書けない。
- `-9223372036854775808` はリテラルとして直接書けない。
- モジュール、import、標準ライブラリ、フォーマッタはない。ホストの C コンパイラ向けで、RISC-V / freestanding のビルドはまだない。
- 予約語(`throws inout sink set struct enum match requires ensures ghost import`)は使うと E0003 になる。

## 診断コード

| コード | 意味 |
|---|---|
| E0001 | 字句エラー(使えない文字、閉じていない文字列、不正なエスケープなど) |
| E0002 | 構文エラー |
| E0003 | Scull 0 では未対応(予約語) |
| E0101 | 定義されていない名前 |
| E0102 | 名前の重複・組み込み名の再定義 |
| E0103 | シャドーイング |
| E0105 | 大文字・小文字の規則違反 |
| E0201 | 型の不一致 |
| E0202 | 引数の個数の違い |
| E0203 | `let` への代入 |
| E0204 | return に到達しない経路がある |
| E0205 | 呼び出し以外の式文 |
| E0301 | 能力が使えない |
| E0302 | エフェクトが存在しない |
| E0303 | ハンドラの誤り(操作の過不足・不一致、対象の違い) |
| E0304 | `main` の誤り |
| W0301 | 使われていない能力(警告) |
