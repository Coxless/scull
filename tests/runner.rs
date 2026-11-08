//! tests/run と tests/fail を自動で回す統合テスト。
//!
//! - tests/run/foo.scull: `scullc run` の標準出力が foo.out と一致すること。
//!   先頭のコメントで `// exit: N`(期待する終了コード、既定 0)と
//!   `// expect: CODE`(コンパイル時に出るべき警告)を指定できる。
//! - tests/fail/foo.scull: 1 行目の `// expect: CODE` の診断が出て、終了コード 1 になること。
//! - すべての run テストと examples について、生成した C が `-Werror` で警告なくコンパイルできること。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SCULLC: &str = env!("CARGO_BIN_EXE_scullc");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn sources(dir: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(root().join(dir))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "scull"))
        .collect();
    v.sort();
    assert!(!v.is_empty(), "{dir} にテストがありません");
    v
}

/// 先頭のコメント行から `// key: value` を集める。
fn headers(src: &str, key: &str) -> Vec<String> {
    let prefix = format!("// {key}:");
    src.lines()
        .take_while(|l| l.starts_with("//"))
        .filter_map(|l| l.strip_prefix(&prefix).map(|v| v.trim().to_string()))
        .collect()
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("scull-tests-{}", std::process::id()));
    fs::create_dir_all(&d).unwrap();
    d.join(name)
}

#[test]
fn run_tests() {
    let mut failures = Vec::new();
    for path in sources("tests/run") {
        let src = fs::read_to_string(&path).unwrap();
        let expected = fs::read_to_string(path.with_extension("out"))
            .unwrap_or_else(|_| panic!("{} の .out がありません", path.display()));
        let exit: i32 = headers(&src, "exit")
            .first()
            .map(|s| s.parse().unwrap())
            .unwrap_or(0);
        let out = Command::new(SCULLC).arg("run").arg(&path).output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let name = path.display();
        if stdout != expected {
            failures.push(format!(
                "{name}: 出力が違います\n--- 期待\n{expected}--- 実際\n{stdout}--- stderr\n{stderr}"
            ));
        }
        if out.status.code() != Some(exit) {
            failures.push(format!(
                "{name}: 終了コード {:?}(期待 {exit})\n{stderr}",
                out.status.code()
            ));
        }
        if exit == 101 && !stderr.contains("scull: runtime error:") {
            failures.push(format!(
                "{name}: 実行時エラーのメッセージがありません\n{stderr}"
            ));
        }
        for code in headers(&src, "expect") {
            if !stderr.contains(&format!("[{code}]")) {
                failures.push(format!("{name}: 診断 {code} が出ていません\n{stderr}"));
            }
        }
        if exit == 0 && headers(&src, "expect").is_empty() && !stderr.is_empty() {
            failures.push(format!(
                "{name}: 予期しない出力が stderr にあります\n{stderr}"
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
}

#[test]
fn fail_tests() {
    let mut failures = Vec::new();
    for path in sources("tests/fail") {
        let src = fs::read_to_string(&path).unwrap();
        let codes = headers(&src, "expect");
        assert!(
            !codes.is_empty(),
            "{}: `// expect:` がありません",
            path.display()
        );
        let out = Command::new(SCULLC)
            .arg("check")
            .arg(&path)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        let name = path.display();
        for code in &codes {
            if !stderr.contains(&format!("[{code}]")) {
                failures.push(format!("{name}: 診断 {code} が出ていません\n{stderr}"));
            }
        }
        if out.status.code() != Some(1) {
            failures.push(format!(
                "{name}: 終了コード {:?}(期待 1)",
                out.status.code()
            ));
        }
        if stderr.contains("panicked") {
            failures.push(format!("{name}: コンパイラが panic しました\n{stderr}"));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
}

fn c_compilers() -> Vec<String> {
    let mut v = vec![std::env::var("CC").unwrap_or_else(|_| "cc".to_string())];
    for extra in ["gcc", "clang"] {
        let found = Command::new(extra).arg("--version").output().is_ok();
        if found && !v.iter().any(|c| c == extra) {
            v.push(extra.to_string());
        }
    }
    v
}

fn assert_c_clean(path: &Path, failures: &mut Vec<String>) {
    let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
    let c = scratch(&format!("{stem}.c"));
    let out = Command::new(SCULLC)
        .args(["emit-c"])
        .arg(path)
        .arg("-o")
        .arg(&c)
        .output()
        .unwrap();
    if !out.status.success() {
        failures.push(format!(
            "{}: emit-c に失敗\n{}",
            path.display(),
            String::from_utf8_lossy(&out.stderr)
        ));
        return;
    }
    for cc in c_compilers() {
        let exe = scratch(&format!("{stem}-{cc}"));
        let out = Command::new(&cc)
            .args([
                "-std=c11",
                "-O2",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-pedantic",
                "-o",
            ])
            .arg(&exe)
            .arg(&c)
            .output()
            .unwrap();
        if !out.status.success() || !out.stderr.is_empty() {
            failures.push(format!(
                "{} ({cc}): C の警告またはエラー\n{}",
                path.display(),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }
}

#[test]
fn generated_c_is_warning_free() {
    let mut failures = Vec::new();
    let mut files = sources("tests/run");
    files.extend(sources("examples"));
    for path in files {
        assert_c_clean(&path, &mut failures);
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
}

#[test]
fn examples_match_tests() {
    // examples/ のサンプルは tests/run にも同じものを置き、期待出力で検査している
    for path in sources("examples") {
        let copy = root().join("tests/run").join(path.file_name().unwrap());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            fs::read_to_string(&copy).unwrap_or_default(),
            "{} と {} が一致しません",
            path.display(),
            copy.display()
        );
    }
}
