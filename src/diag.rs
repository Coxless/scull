//! 診断(エラー・警告)の表現と表示。

/// ソース中のバイト範囲 `[start, end)`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Span {
        Span { start, end }
    }

    /// 2つの範囲を覆う範囲。
    pub fn to(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub span: Span,
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn error(code: &'static str, span: Span, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            severity: Severity::Error,
            code,
            message: message.into(),
            span,
            help: None,
        }
    }

    pub fn warning(code: &'static str, span: Span, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            severity: Severity::Warning,
            code,
            message: message.into(),
            span,
            help: None,
        }
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Diagnostic {
        self.help = Some(help.into());
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// バイト位置から (行, 列, 行頭のバイト位置) を求める。行と列は 1 始まりで、列は文字単位。
fn locate(src: &str, pos: usize) -> (usize, usize, usize) {
    let pos = pos.min(src.len());
    let mut line = 1;
    let mut line_start = 0;
    for (i, b) in src.bytes().enumerate() {
        if i >= pos {
            break;
        }
        if b == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    let col = src[line_start..pos].chars().count() + 1;
    (line, col, line_start)
}

/// rustc に似た形式で診断を文字列にする。
pub fn render(d: &Diagnostic, path: &str, src: &str) -> String {
    let kind = match d.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };
    let (line, col, line_start) = locate(src, d.span.start);
    let line_end = src[line_start..]
        .find('\n')
        .map(|i| line_start + i)
        .unwrap_or(src.len());
    let text = src[line_start..line_end].trim_end_matches('\r');
    let start = d.span.start.clamp(line_start, line_end);
    let end = d.span.end.clamp(start, line_start + text.len());
    let pad: String = src[line_start..start]
        .chars()
        .map(|c| if c == '\t' { '\t' } else { ' ' })
        .collect();
    let carets = src[start..end].chars().count().max(1);

    let w = line.to_string().len();
    let blank = " ".repeat(w + 1);
    let mut out = String::new();
    out.push_str(&format!("{kind}[{}]: {}\n", d.code, d.message));
    out.push_str(&format!("{}--> {path}:{line}:{col}\n", " ".repeat(w + 1)));
    out.push_str(&format!("{blank} |\n"));
    out.push_str(&format!(" {line} | {text}\n"));
    out.push_str(&format!("{blank} | {pad}{}\n", "^".repeat(carets)));
    if let Some(h) = &d.help {
        out.push_str(&format!("{blank} = 助言: {h}\n"));
    }
    out
}
