# Scull 0 実装指示書(Claude Code 向け)

版: 2026-11-08 / 対象: 言語 Scull の最初のサブセット「Scull 0」とコンパイラ `scullc`

(旧名 Sai。2026-11-08 に言語名を Scull、OS 名を Coxless に変更した。拡張子は `.scull`(`.scl` は Siemens SCL などと重なるため避けた)、生成する C の接頭辞は `scl_`)

---

## 0. この指示書の使い方(Claude Code へ)

- この文書が仕様のすべてである。ここに書かれていない機能は**実装しない**。
- マイルストーン(§9)を **M1 → M4 の順に**進める。各マイルストーンの受け入れ条件をすべて満たし、`cargo test` が通ったらコミットしてから次へ進む。
- 仕様に曖昧な点があれば、**最も単純な解釈**を選び、`DECISIONS.md` に「日付・項目・選んだ解釈・理由」を1行ずつ追記する。
- 生成する C コードは `-std=c11 -Wall -Wextra` で**警告ゼロ**にする。
- 依存クレートは原則として使わない(標準ライブラリのみ)。パーサージェネレータも使わず、手書きの再帰下降パーサーにする。

---

## 1. 背景と目的

Scull は「能力指向」の言語である。

- 関数が外の世界(出力、時刻、ファイルなど)に触れられるのは、**シグネチャの `uses` 節で宣言した能力**を通してだけである。グローバルな権限(アンビエント権限)はない。
- 能力は `effect` として宣言し、`handler` で実装を差し替えられる(テストでのモック、サンドボックス)。
- 将来は OS カーネル(Coxless)と通常のアプリ開発の両方に使う。

Scull 0 の目的は、この核心である**「能力が型で検査され、ハンドラで差し替えられる」ことを、動く最小の言語で示す**ことである。

### 設計原則(構文の判断に迷ったらここに戻る)

1. コードは AI が書き、**人間が読んで確認する**。書く短さより、読む速さと差分の見やすさを優先する。
2. 記号よりキーワードを使う(`uses`、`handle ... by ...`)。
3. 同じことを書く方法は1つにする。
4. 能力の増減が差分で必ず目立つようにする(`uses` は独立した行に置く)。

---

## 2. 成果物

```
scull/
├── Cargo.toml              # パッケージ名 scullc、バイナリ名 scullc
├── README.md               # ビルド方法、使い方、言語の簡単な説明
├── DECISIONS.md            # 実装中に決めたことの記録
├── src/
│   ├── main.rs             # CLI
│   ├── diag.rs             # 診断(エラー・警告)の表示
│   ├── lexer.rs
│   ├── ast.rs
│   ├── parser.rs
│   ├── resolve.rs          # 名前解決
│   ├── check.rs            # 型検査・能力検査
│   ├── codegen_c.rs        # C コード生成
│   └── runtime/scl_rt.h    # C ランタイム(include_str! でバイナリに埋め込む)
├── examples/               # §8 のサンプル
└── tests/
    ├── run/                # 実行テスト: foo.scull + foo.out(期待する標準出力)
    ├── fail/               # エラーテスト: foo.scull(先頭に期待するエラーコードを書く)
    └── runner.rs           # 上記を自動で回す統合テスト
```

### CLI

| コマンド | 動作 |
|---|---|
| `scullc check FILE.scull` | 検査だけを行い、診断を出す |
| `scullc emit-c FILE.scull [-o OUT.c]` | C コードを出力する(既定は標準出力) |
| `scullc build FILE.scull [-o OUT]` | C を生成し、C コンパイラで実行ファイルを作る |
| `scullc run FILE.scull` | build してから実行する。終了コードは実行ファイルのものを返す |

- C コンパイラは環境変数 `CC`(既定は `cc`)を使い、`-std=c11 -O2 -Wall -Wextra` を付ける。
- 終了コードは、成功が 0、コンパイルエラーが 1。警告だけなら 0 を返す。

---

## 3. 字句

- ソースは UTF-8。識別子は ASCII のみとする。
- コメントは `//` から行末まで。ブロックコメントはない。
- **整数リテラル**:10進数のみ(`0`、`42`、`1_000` のように `_` 区切りを許す)。範囲は i64。
- **文字列リテラル**:`"..."`。
  - エスケープは `\n`、`\t`、`\"`、`\\`、`\{`、`\}`。
  - `{式}` で値を埋め込める(文字列補間)。式の型は Int・Bool・String のいずれか。
  - 補間の中には文字列リテラルと `{` `}` を書けない(Scull 0 の制限)。
- **識別子**:
  - 大文字で始まるもの(`Console`、`FixedClock`)は**型・エフェクト・ハンドラ名**。
  - 小文字または `_` で始まるもの(`fib`、`n`)は**関数・変数名**。
  - この区別は字句解析器ではなく、名前解決で検査する(E0105)。
- **キーワード**:`fun let var if else while return effect handler for handle by uses true false`
- **予約語**(Scull 0 では使えない。使ったら E0003「Scull 0 では未対応」):`throws inout sink set struct enum match requires ensures ghost import`
- **演算子・記号**:`+ - * / % == != < <= > >= && || ! = += -= -> ( ) { } , : .`

### 改行の扱い

改行は文の区切りになる。字句解析器は改行を `NEWLINE` トークンとして出力するが、次の場合は出力しない。

- 丸括弧 `( )` の内側にいるとき
- 直前のトークンが二項演算子、`,`、`=`、`+=`、`-=`、`->`、`:` のとき
- 連続する改行は1つにまとめる

パーサーは、宣言ヘッダの中(`)` と `uses` の間、`uses` 節と `{` の間など)と、ブロックの先頭・末尾の `NEWLINE` を読み飛ばす。

`else` は、直前の `}` と同じ行に書かなければならない(`} else {`)。

---

## 4. 構文(EBNF)

```
program      = { NL } { top_decl { NL } } EOF ;
top_decl     = fun_decl | effect_decl | handler_decl ;

fun_decl     = "fun" LIDENT "(" [ params ] ")" [ "->" type ] [ uses ] block ;
params       = param { "," param } [ "," ] ;
param        = LIDENT ":" type ;
uses         = "uses" UIDENT { "," UIDENT } ;
type         = "Int" | "Bool" | "String" ;

effect_decl  = "effect" UIDENT "{" { NL } { op_sig NL { NL } } "}" ;
op_sig       = "fun" LIDENT "(" [ params ] ")" [ "->" type ] ;

handler_decl = "handler" UIDENT "for" UIDENT [ uses ] "{" { NL } { fun_decl_nouses { NL } } "}" ;
fun_decl_nouses = "fun" LIDENT "(" [ params ] ")" [ "->" type ] block ;

block        = "{" { NL } { stmt NL { NL } } [ stmt { NL } ] "}" ;
stmt         = let_stmt | var_stmt | assign_stmt | if_stmt | while_stmt
             | return_stmt | handle_stmt | expr_stmt ;
let_stmt     = "let" LIDENT [ ":" type ] "=" expr ;
var_stmt     = "var" LIDENT [ ":" type ] "=" expr ;
assign_stmt  = LIDENT ( "=" | "+=" | "-=" ) expr ;
if_stmt      = "if" expr block [ "else" ( if_stmt | block ) ] ;
while_stmt   = "while" expr block ;
return_stmt  = "return" [ expr ] ;
handle_stmt  = "handle" UIDENT "by" UIDENT block ;
expr_stmt    = call_expr ;                      (* 呼び出しだけを文として書ける *)

expr         = or_expr ;
or_expr      = and_expr { "||" and_expr } ;
and_expr     = cmp_expr { "&&" cmp_expr } ;
cmp_expr     = add_expr [ ( "==" | "!=" | "<" | "<=" | ">" | ">=" ) add_expr ] ;  (* 連鎖不可 *)
add_expr     = mul_expr { ( "+" | "-" ) mul_expr } ;
mul_expr     = unary { ( "*" | "/" | "%" ) unary } ;
unary        = ( "-" | "!" ) unary | primary ;
primary      = INT | STRING | "true" | "false" | LIDENT | call_expr | "(" expr ")" ;
call_expr    = LIDENT "(" [ args ] ")"               (* 関数呼び出し *)
             | UIDENT "." LIDENT "(" [ args ] ")" ;  (* 能力の操作呼び出し *)
args         = expr { "," expr } [ "," ] ;
```

- `NL` は `NEWLINE` トークンを表す。
- 戻り値の型を省略した関数は Unit(値を返さない)とする。Unit は型として書けない。
- `uses` 節の位置は、戻り値の型の後、本体の `{` の前とする。フォーマッタはないが、サンプルとテストは常に `uses` を独立した行に書く(§8 参照)。
- `if` は文であり、式ではない。Scull 0 には式としての `if` はない。

---

## 5. 意味(静的検査)

### 5.1 名前

- トップレベルの関数・エフェクト・ハンドラは1つの名前空間を共有し、宣言の順序は問わない(相互再帰も可)。重複は E0102。
- ローカル変数のスコープはブロック単位。**シャドーイングは禁止**する。外側のスコープや引数と同じ名前の変数を宣言すると E0103。
- 定義されていない名前は E0101。大文字・小文字の規則に違反した名前は E0105。
- 組み込みエフェクト `Console` は暗黙に宣言されている。`Int` `Bool` `String` `Console` はユーザーが再定義できない。

### 5.2 型

- 型は `Int`(64 ビット符号付き)、`Bool`、`String`(不変)、Unit の4つ。
- `let` / `var` の型は初期化式から推論する。型注釈がある場合は一致を検査する。
- 演算子の型:
  - `+ - * / %` と単項 `-`:Int × Int → Int
  - `< <= > >=`:Int × Int → Bool
  - `== !=`:同じ型どうし(Int、Bool、String)→ Bool
  - `&& ||`(短絡評価)と `!`:Bool → Bool
  - **String の連結には `+` を使わず、文字列補間を使う**(書き方を1つにするため)
- `if` と `while` の条件は Bool でなければならない。
- 代入できるのは `var` だけ。`let` への代入は E0203。`+=` と `-=` は Int にだけ使える。
- 型の不一致は E0201、引数の個数の違いは E0202。
- 式文として書けるのは呼び出しだけ(E0205)。戻り値が Unit 以外の呼び出しを式文にしてもよい(値は捨てる)。
- 戻り値が Unit 以外の関数は、すべての経路で `return 式` に到達しなければならない(E0204)。判定は単純にする。ブロックの最後の文が `return` であるか、`else` を持つ `if` で両方の枝がこの条件を満たせば OK とする。`while` は考慮しない。
- Unit の関数では、`return`(値なし)を書いても書かなくてもよい。

### 5.3 能力(この言語の核心)

各地点には「**利用可能な能力の集合**」がある。

| 場所 | 利用可能な能力 |
|---|---|
| 関数本体 | その関数の `uses` 節に書かれたもの |
| ハンドラの操作本体 | そのハンドラの `uses` 節に書かれたもの |
| `handle E by H { 本体 }` の本体 | 外側の集合 ∪ {E} |

規則は次のとおり。

1. **操作呼び出し** `E.op(...)`:E が利用可能でなければ E0301。E が存在しなければ E0302。op が E の操作でなければ E0101。
2. **関数呼び出し** `f(...)`:f の `uses` がすべて利用可能でなければ E0301。エラーメッセージには、足りない能力の名前を挙げる。
3. **`handle E by H { ... }`**:
   - H は `handler H for E` として宣言されていなければならない(E0303)。
   - H の `uses` は、`handle` を書いた地点で利用可能でなければならない(E0301)。
   - 同じ E を入れ子にしてもよい。内側のハンドラが優先される。`Console` も差し替えられる。
4. **ハンドラ宣言** `handler H for E`:E のすべての操作を、同じ名前・同じ引数の型・同じ戻り値の型で、過不足なく実装しなければならない(E0303)。
5. **`main`**:`fun main()` が必ず存在し、引数なし・戻り値 Unit で、`uses` は `Console` だけか、`uses` なしでなければならない(E0304)。
6. **未使用の能力の警告**(W0301):関数またはハンドラの `uses` に書かれているのに使われていない能力を警告する。「使われている」とは、次のいずれかに当てはまることをいう。
   - 本体に、その能力の操作呼び出しがある
   - その能力を `uses` に含む関数を呼んでいる
   - その能力を `uses` に含むハンドラを `handle` している

   ただし、`handle E by ...` の内側で E を使っていても、それは外側の E の使用としては数えない(ハンドラが供給しているため)。

### 5.4 ハンドラの意味

- Scull 0 のハンドラは **tail-resumptive に限る**。操作本体の戻り値が、そのまま操作呼び出しの結果になる。`resume` のような構文はなく、継続を捕まえることもない。
- 操作本体は普通の関数と同じように書く。本体の中では、ハンドラの `uses` に書いた能力を使える。それらの能力は、`handle` を書いた地点のものが使われる(レキシカル)。

---

## 6. 実行時の意味

- Int の演算でオーバーフローが起きたとき(`+ - *`、単項 `-`)と、ゼロ除算・ゼロ剰余のときは、標準エラーに `scull: runtime error: <理由>` を出力して終了コード 101 で終了する。
- `Console.print(s: String)` は、s と改行を標準出力に書き出す。`Console` の操作はこれだけとする。
- 文字列補間では、Int を10進数、Bool を `true` / `false` に変換する。
- 文字列のメモリは Scull 0 では解放しない(リークを許容する)。このことは README に明記する。

---

## 7. C コード生成

### 7.1 基本方針:能力を引数で渡す

能力を「関数ポインタと環境ポインタの組」として、関数の引数で明示的に渡す。グローバル変数は使わない。言語の「アンビエント権限なし」が、生成される C にもそのまま現れるようにする。

```c
/* effect Clock { fun now() -> Int } */
typedef struct scl_eff_Clock {
    void *env;
    int64_t (*now)(void *env);
} scl_eff_Clock;
```

- 型の対応:Int → `int64_t`、Bool → `bool`、String → `scl_str`(`{ const char *ptr; int64_t len; }`)、Unit → `void`。
- 名前の変換:関数は `scl_f_<名前>`、エフェクトは `scl_eff_<名前>`、ハンドラの操作は `scl_h_<ハンドラ>_<操作>`、ローカル変数は `v_<名前>`。C の予約語や標準ライブラリの名前と衝突しないようにする。
- 関数の能力引数は、通常の引数の後ろに、**能力名の辞書順**で `scl_eff_X *cap_X` として並べる。

```c
/* fun report() uses Clock, Console */
void scl_f_report(scl_eff_Clock *cap_Clock, scl_eff_Console *cap_Console);
```

- 操作呼び出し `Clock.now()` は `cap_Clock->now(cap_Clock->env)` になる。
- 関数呼び出しでは、呼ばれる側の `uses` に含まれる能力について、**呼び出し地点で現在有効なもの**を渡す。

### 7.2 handle の変換

`handle Clock by FixedClock { body }` は、C のブロックスコープに変換する。

```c
{
    struct scl_henv_FixedClock henv_1 = { /* FixedClock の uses の能力ポインタ */ };
    scl_eff_Clock cap_Clock_1 = { &henv_1, scl_h_FixedClock_now };
    /* body: この中では Clock として &cap_Clock_1 を使う */
}
```

- コード生成器は「能力名 → 現在有効な C の式」の対応をスコープつきで管理する。
- ハンドラの環境構造体 `struct scl_henv_H` は、H の `uses` に書かれた能力のポインタを持つ。`uses` がないハンドラでは、空の構造体を避けるためにダミーのメンバを1つ置く。
- 本体の中の `return` は、外側の関数からの return としてそのまま C に出力してよい(すべてスタック上にあるため問題ない)。

### 7.3 ランタイムとエントリポイント

`src/runtime/scl_rt.h` には次のものを用意し、生成する C の先頭に埋め込む。

- `scl_str` 型、文字列リテラルの生成、連結、Int・Bool から文字列への変換、等価比較
- オーバーフロー検査つきの算術(`__builtin_add_overflow` などを使う)と、ゼロ除算の検査
- `scl_panic(const char *reason)`:メッセージを出力し、終了コード 101 で終了する
- `scl_eff_Console` の標準実装(stdout に書き出す)

エントリポイントは次のようにする。`main` の `uses` に `Console` がなければ、能力は渡さない。

```c
int main(void) {
    scl_eff_Console console = { NULL, scl_rt_console_print };
    scl_f_main(&console);
    return 0;
}
```

---

## 8. サンプル(examples/ と tests/ に置く)

### examples/hello.scull

```
fun main()
  uses Console
{
  Console.print("hello, Scull")
}
```

### examples/fib.scull

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

### examples/clock.scull(能力の差し替え)

```
effect Clock {
  fun now() -> Int
}

fun greet(name: String)
  uses Console
{
  Console.print("hello, {name}")
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

// ハンドラ自身も能力を使える
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
  greet("Scull")

  handle Clock by FixedClock {
    report()
  }

  handle Clock by LoggedClock {
    report()
  }
}
```

期待する出力:

```
hello, Scull
now = 20261108
[log] Clock.now called
now = 42
```

### examples/silence.scull(組み込みの Console も差し替えられる)

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

注:`Silent` の `print` は引数 `s` を使わない。生成される C で未使用引数の警告が出ないよう、`(void)s;` を出力するなどの対処をする。

### エラーのサンプル(tests/fail/)

ファイルの1行目に `// expect: E0301` のように期待するコードを書く。テストランナーは、診断にそのコードが含まれることを確かめる。

```
// expect: E0301
fun sneaky(x: Int) -> Int {
  Console.print("hi")
  return x
}

fun main() {
  sneaky(1)
}
```

ほかに最低限、次のケースを用意する。

| ファイル | 期待 | 内容 |
|---|---|---|
| `call_needs_cap.scull` | E0301 | `uses Console` の関数を、能力を持たない関数から呼ぶ |
| `handler_missing_op.scull` | E0303 | 操作の実装が足りないハンドラ |
| `handler_cap_leak.scull` | E0301 | `uses Console` のハンドラを、Console のない場所で handle する |
| `assign_let.scull` | E0203 | `let` への代入 |
| `type_mismatch.scull` | E0201 | `if 1 { }` など |
| `missing_return.scull` | E0204 | Int を返す関数で return が足りない |
| `shadowing.scull` | E0103 | 内側のブロックで同じ名前を宣言する |
| `reserved.scull` | E0003 | `struct` を使う |
| `bad_main.scull` | E0304 | `fun main() uses Clock` |
| `unused_cap.scull` | W0301 | 使わない `Console` を `uses` に書く(エラーではないので実行テストにもなる) |

実行時エラーのテストとして、オーバーフローとゼロ除算のケースも用意し、終了コード 101 を確かめる。

---

## 9. マイルストーン

各マイルストーンで、テストを追加し、README を更新し、コミットする。

### M1:式と関数が動く

- 字句解析、パーサー、AST、最小限の名前解決と型検査、C 生成、`scullc build` / `run` / `emit-c`
- 能力は `Console.print` の呼び出しを C に変換できればよい(能力の検査は M2 で行う)
- 文字列補間を含む `hello.scull` と `fib.scull` が期待どおり出力する
- 受け入れ条件:`scullc run examples/fib.scull` の出力が正しい。生成した C で警告が出ない

### M2:型と能力の検査

- §5.1〜5.3 の規則(ハンドラ関連を除く)と診断の表示
- 診断の形式:

```
error[E0301]: 能力 `Console` が必要ですが、この関数では使えません
  --> tests/fail/sneaky.scull:3:3
   |
 3 |   Console.print("hi")
   |   ^^^^^^^
   = 助言: 関数 `sneaky` に `uses Console` を追加してください
```

- メッセージは日本語、コードは英字と数字。可能な限り、1回の実行ですべてのエラーを報告する
- 受け入れ条件:tests/fail のハンドラ関連以外がすべて期待どおりに失敗する

### M3:エフェクトとハンドラ

- `effect` 宣言、`handler` 宣言、`handle ... by ...`、§7.2 の変換、W0301
- 受け入れ条件:`clock.scull` と `silence.scull` が期待どおり出力し、ハンドラ関連の fail テストが通る

### M4:仕上げ

- 実行時エラー(オーバーフロー、ゼロ除算)
- README に、言語の短いツアー(このサンプル群を使う)と既知の制限を書く
- 次の段階で入れたい機能の候補を `NEXT.md` に箇条書きで残す(例:構造体、`inout`、`throws`、式としての `if`、参照カウント、RISC-V ターゲット)。**実装はしない**

---

## 10. やらないこと(Scull 0 の範囲外)

構造体・列挙型、ジェネリクス、配列、クロージャ、モジュールと import、`inout` / `sink`、`throws`(例外)、マルチショットのハンドラや継続、参照カウント、メモリ解放、`Raw` 能力、標準ライブラリ、フォーマッタ、RISC-V や freestanding のビルド、最適化。

これらが必要に見えても実装せず、`NEXT.md` に記録する。
