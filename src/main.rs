//! scullc — Scull 0 のコンパイラ。

mod ast;
mod check;
mod codegen_c;
mod diag;
mod lexer;
mod parser;
mod resolve;

use diag::Diagnostic;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

const USAGE: &str = "\
使い方:
  scullc check FILE.scull               検査だけを行う
  scullc emit-c FILE.scull [-o OUT.c]   C コードを出力する(既定は標準出力)
  scullc build FILE.scull [-o OUT]      実行ファイルを作る
  scullc run FILE.scull                 build してから実行する

環境変数 CC で C コンパイラを指定できる(既定は cc)。";

fn usage_error(msg: &str) -> ! {
    eprintln!("error: {msg}\n\n{USAGE}");
    exit(1);
}

/// 診断を表示する。エラーがあれば true。
fn report(diags: &mut [Diagnostic], path: &str, src: &str) -> bool {
    diags.sort_by_key(|d| (d.span.start, d.span.end));
    let mut errors = 0;
    let mut warnings = 0;
    for d in diags.iter() {
        eprintln!("{}", diag::render(d, path, src));
        if d.is_error() {
            errors += 1;
        } else {
            warnings += 1;
        }
    }
    if errors > 0 {
        eprintln!("エラー {errors} 件、警告 {warnings} 件のためコンパイルを中断しました");
    } else if warnings > 0 {
        eprintln!("警告 {warnings} 件");
    }
    errors > 0
}

/// ソースを読み、検査して C コードを返す。エラーなら終了コード 1 で終了する。
fn compile(path: &str) -> String {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: `{path}` を読めません: {e}");
            exit(1);
        }
    };
    let mut diags = Vec::new();
    let toks = lexer::lex(&src, 0, &mut diags);
    if diags.iter().any(|d| d.is_error()) {
        report(&mut diags, path, &src);
        exit(1);
    }
    let prog = parser::parse(toks, &mut diags);
    if diags.iter().any(|d| d.is_error()) {
        report(&mut diags, path, &src);
        exit(1);
    }
    let globals = resolve::resolve(&prog, &mut diags);
    check::check(&prog, &globals, &mut diags);
    if report(&mut diags, path, &src) {
        exit(1);
    }
    codegen_c::generate(&prog, &globals)
}

fn parse_args(args: &[String]) -> (String, Option<String>) {
    let mut file = None;
    let mut out = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                i += 1;
                match args.get(i) {
                    Some(o) => out = Some(o.clone()),
                    None => usage_error("`-o` の後に出力先がありません"),
                }
            }
            a if a.starts_with('-') => usage_error(&format!("不明なオプション `{a}`")),
            a => {
                if file.is_some() {
                    usage_error("入力ファイルは1つだけ指定できます");
                }
                file = Some(a.to_string());
            }
        }
        i += 1;
    }
    match file {
        Some(f) => (f, out),
        None => usage_error("入力ファイルがありません"),
    }
}

/// C コードを C コンパイラで実行ファイルにする。
fn cc(c_code: &str, c_path: &Path, exe: &Path) {
    if let Err(e) = std::fs::write(c_path, c_code) {
        eprintln!("error: `{}` に書き込めません: {e}", c_path.display());
        exit(1);
    }
    let compiler = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let status = Command::new(&compiler)
        .args(["-std=c11", "-O2", "-Wall", "-Wextra", "-o"])
        .arg(exe)
        .arg(c_path)
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(_) => {
            eprintln!("error: C コンパイラ `{compiler}` が失敗しました");
            exit(1);
        }
        Err(e) => {
            eprintln!("error: C コンパイラ `{compiler}` を起動できません: {e}");
            exit(1);
        }
    }
}

fn temp_dir() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("scullc-{}-{nanos}", std::process::id()));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("error: 一時ディレクトリを作れません: {e}");
        exit(1);
    }
    dir
}

fn stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "a".to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        usage_error("コマンドがありません");
    };
    let rest = &args[1..];
    match cmd.as_str() {
        "check" => {
            let (file, out) = parse_args(rest);
            if out.is_some() {
                usage_error("`check` に `-o` は使えません");
            }
            compile(&file);
        }
        "emit-c" => {
            let (file, out) = parse_args(rest);
            let code = compile(&file);
            match out {
                None => print!("{code}"),
                Some(o) => {
                    if let Err(e) = std::fs::write(&o, code) {
                        eprintln!("error: `{o}` に書き込めません: {e}");
                        exit(1);
                    }
                }
            }
        }
        "build" => {
            let (file, out) = parse_args(rest);
            let code = compile(&file);
            let exe = PathBuf::from(out.unwrap_or_else(|| stem(&file)));
            let dir = temp_dir();
            cc(&code, &dir.join(format!("{}.c", stem(&file))), &exe);
            let _ = std::fs::remove_dir_all(&dir);
        }
        "run" => {
            let (file, out) = parse_args(rest);
            if out.is_some() {
                usage_error("`run` に `-o` は使えません");
            }
            let code = compile(&file);
            let dir = temp_dir();
            let exe = dir.join(stem(&file));
            cc(&code, &dir.join(format!("{}.c", stem(&file))), &exe);
            let status = Command::new(&exe).status();
            let _ = std::fs::remove_dir_all(&dir);
            match status {
                Ok(s) => exit(s.code().unwrap_or(1)),
                Err(e) => {
                    eprintln!("error: `{}` を実行できません: {e}", exe.display());
                    exit(1);
                }
            }
        }
        "-h" | "--help" | "help" => println!("{USAGE}"),
        other => usage_error(&format!("不明なコマンド `{other}`")),
    }
}
