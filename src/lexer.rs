//! 字句解析。改行を NEWLINE トークンとして出力する(仕様 §3)。

use crate::diag::{Diagnostic, Span};

#[derive(Clone, Debug, PartialEq)]
pub enum TokKind {
    Int(i64),
    Str(Vec<StrPart>),
    Ident(String),
    /// Scull 0 では使えない予約語。
    Reserved(String),
    // キーワード
    Fun,
    Let,
    Var,
    If,
    Else,
    While,
    Return,
    Effect,
    Handler,
    For,
    Handle,
    By,
    Uses,
    True,
    False,
    // 演算子・記号
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Bang,
    Eq,
    PlusEq,
    MinusEq,
    Arrow,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Dot,
    Newline,
    Eof,
}

/// 文字列リテラルの部品。補間部分はあらかじめトークン列にしておく(末尾は Eof)。
#[derive(Clone, Debug, PartialEq)]
pub enum StrPart {
    Lit(String),
    Interp(Vec<Token>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokKind,
    pub span: Span,
    /// 行頭(1 列目)のトークンかどうか(構文エラーからの回復に使う)。
    pub line_start: bool,
}

pub fn describe(kind: &TokKind) -> String {
    use TokKind::*;
    let s = match kind {
        Int(n) => return format!("整数 `{n}`"),
        Str(_) => return "文字列".to_string(),
        Ident(s) => return format!("識別子 `{s}`"),
        Reserved(s) => return format!("予約語 `{s}`"),
        Fun => "fun",
        Let => "let",
        Var => "var",
        If => "if",
        Else => "else",
        While => "while",
        Return => "return",
        Effect => "effect",
        Handler => "handler",
        For => "for",
        Handle => "handle",
        By => "by",
        Uses => "uses",
        True => "true",
        False => "false",
        Plus => "+",
        Minus => "-",
        Star => "*",
        Slash => "/",
        Percent => "%",
        EqEq => "==",
        NotEq => "!=",
        Lt => "<",
        Le => "<=",
        Gt => ">",
        Ge => ">=",
        AndAnd => "&&",
        OrOr => "||",
        Bang => "!",
        Eq => "=",
        PlusEq => "+=",
        MinusEq => "-=",
        Arrow => "->",
        LParen => "(",
        RParen => ")",
        LBrace => "{",
        RBrace => "}",
        Comma => ",",
        Colon => ":",
        Dot => ".",
        Newline => return "改行".to_string(),
        Eof => return "ファイルの終わり".to_string(),
    };
    format!("`{s}`")
}

/// このトークンの直後の改行は NEWLINE にしない。
fn continues_line(kind: &TokKind) -> bool {
    use TokKind::*;
    matches!(
        kind,
        Plus | Minus
            | Star
            | Slash
            | Percent
            | EqEq
            | NotEq
            | Lt
            | Le
            | Gt
            | Ge
            | AndAnd
            | OrOr
            | Comma
            | Eq
            | PlusEq
            | MinusEq
            | Arrow
            | Colon
    )
}

fn keyword(s: &str) -> Option<TokKind> {
    use TokKind::*;
    Some(match s {
        "fun" => Fun,
        "let" => Let,
        "var" => Var,
        "if" => If,
        "else" => Else,
        "while" => While,
        "return" => Return,
        "effect" => Effect,
        "handler" => Handler,
        "for" => For,
        "handle" => Handle,
        "by" => By,
        "uses" => Uses,
        "true" => True,
        "false" => False,
        _ => return None,
    })
}

pub const RESERVED: &[&str] = &[
    "throws", "inout", "sink", "set", "struct", "enum", "match", "requires", "ensures", "ghost",
    "import",
];

struct Lexer<'a, 'd> {
    src: &'a str,
    base: usize,
    pos: usize,
    paren: usize,
    at_line_start: bool,
    tokens: Vec<Token>,
    diags: &'d mut Vec<Diagnostic>,
}

/// `src` を字句解析する。`base` は `src` の先頭がファイル中で何バイト目か(補間用)。
pub fn lex(src: &str, base: usize, diags: &mut Vec<Diagnostic>) -> Vec<Token> {
    let mut lx = Lexer {
        src,
        base,
        pos: 0,
        paren: 0,
        at_line_start: true,
        tokens: Vec::new(),
        diags,
    };
    lx.run();
    lx.tokens
}

impl<'a, 'd> Lexer<'a, 'd> {
    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.src[self.pos..].chars().nth(n)
    }

    fn span(&self, start: usize, end: usize) -> Span {
        Span::new(self.base + start, self.base + end)
    }

    fn error(&mut self, start: usize, end: usize, msg: impl Into<String>) {
        let sp = self.span(start, end);
        self.diags.push(Diagnostic::error("E0001", sp, msg));
    }

    fn push(&mut self, kind: TokKind, start: usize, end: usize) {
        let span = self.span(start, end);
        self.tokens.push(Token {
            kind,
            span,
            line_start: self.at_line_start,
        });
        self.at_line_start = false;
    }

    fn newline(&mut self, at: usize) {
        if self.paren > 0 {
            return;
        }
        match self.tokens.last() {
            None => {}
            Some(t) if t.kind == TokKind::Newline || continues_line(&t.kind) => {}
            Some(_) => {
                let span = self.span(at, at + 1);
                self.tokens.push(Token {
                    kind: TokKind::Newline,
                    span,
                    line_start: false,
                });
            }
        }
    }

    fn run(&mut self) {
        while let Some(c) = self.peek() {
            let start = self.pos;
            match c {
                ' ' | '\t' | '\r' => {
                    self.pos += 1;
                    self.at_line_start = false;
                }
                '\n' => {
                    self.pos += 1;
                    self.newline(start);
                    self.at_line_start = true;
                }
                '/' if self.peek_at(1) == Some('/') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.pos += c.len_utf8();
                    }
                }
                '0'..='9' => self.number(),
                'a'..='z' | 'A'..='Z' | '_' => self.ident(),
                '"' => self.string(),
                _ => self.symbol(c),
            }
        }
        let end = self.src.len();
        self.push(TokKind::Eof, end, end);
    }

    fn number(&mut self) {
        let start = self.pos;
        let mut value: Option<i64> = Some(0);
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                let d = (c as u8 - b'0') as i64;
                value = value
                    .and_then(|v| v.checked_mul(10))
                    .and_then(|v| v.checked_add(d));
            } else if c != '_' {
                break;
            }
            self.pos += 1;
        }
        match value {
            Some(v) => self.push(TokKind::Int(v), start, self.pos),
            None => {
                self.error(start, self.pos, "整数リテラルが大きすぎます(i64 の範囲外)");
                self.push(TokKind::Int(0), start, self.pos);
            }
        }
    }

    fn ident(&mut self) {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let text = &self.src[start..self.pos];
        let kind = if let Some(k) = keyword(text) {
            k
        } else if RESERVED.contains(&text) {
            TokKind::Reserved(text.to_string())
        } else {
            TokKind::Ident(text.to_string())
        };
        self.push(kind, start, self.pos);
    }

    fn string(&mut self) {
        let start = self.pos;
        self.pos += 1; // 開きの "
        let mut parts = Vec::new();
        let mut buf = String::new();
        loop {
            let c = match self.peek() {
                None | Some('\n') => {
                    self.error(start, self.pos, "文字列リテラルが閉じられていません");
                    break;
                }
                Some(c) => c,
            };
            match c {
                '"' => {
                    self.pos += 1;
                    break;
                }
                '\\' => {
                    let esc_start = self.pos;
                    self.pos += 1;
                    let e = match self.peek() {
                        None | Some('\n') => continue, // 次の周回で「閉じられていない」になる
                        Some(e) => e,
                    };
                    self.pos += e.len_utf8();
                    match e {
                        'n' => buf.push('\n'),
                        't' => buf.push('\t'),
                        '"' => buf.push('"'),
                        '\\' => buf.push('\\'),
                        '{' => buf.push('{'),
                        '}' => buf.push('}'),
                        _ => self.error(
                            esc_start,
                            self.pos,
                            format!("不正なエスケープ `\\{e}` です"),
                        ),
                    }
                }
                '{' => {
                    let open = self.pos;
                    let inner_start = self.pos + 1;
                    let mut j = inner_start;
                    let mut ok = false;
                    for ch in self.src[inner_start..].chars() {
                        match ch {
                            '}' => {
                                ok = true;
                                break;
                            }
                            '"' | '{' | '\n' => break,
                            _ => j += ch.len_utf8(),
                        }
                    }
                    if !ok {
                        self.error(
                            open,
                            j,
                            "文字列補間が `}` で閉じられていません(補間の中に文字列リテラルや `{` は書けません)",
                        );
                        // 行末まで読み飛ばして回復する
                        while let Some(ch) = self.peek() {
                            if ch == '\n' {
                                break;
                            }
                            self.pos += ch.len_utf8();
                        }
                        break;
                    }
                    if !buf.is_empty() {
                        parts.push(StrPart::Lit(std::mem::take(&mut buf)));
                    }
                    let inner = &self.src[inner_start..j];
                    let toks = lex(inner, self.base + inner_start, self.diags);
                    parts.push(StrPart::Interp(toks));
                    self.pos = j + 1;
                }
                '}' => {
                    self.error(
                        self.pos,
                        self.pos + 1,
                        "文字列の中の `}` は `\\}` と書いてください",
                    );
                    self.pos += 1;
                }
                _ => {
                    buf.push(c);
                    self.pos += c.len_utf8();
                }
            }
        }
        if !buf.is_empty() {
            parts.push(StrPart::Lit(buf));
        }
        self.push(TokKind::Str(parts), start, self.pos);
    }

    fn symbol(&mut self, c: char) {
        use TokKind::*;
        let start = self.pos;
        let next = self.peek_at(1);
        let (kind, len) = match (c, next) {
            ('+', Some('=')) => (PlusEq, 2),
            ('+', _) => (Plus, 1),
            ('-', Some('>')) => (Arrow, 2),
            ('-', Some('=')) => (MinusEq, 2),
            ('-', _) => (Minus, 1),
            ('*', _) => (Star, 1),
            ('/', _) => (Slash, 1),
            ('%', _) => (Percent, 1),
            ('=', Some('=')) => (EqEq, 2),
            ('=', _) => (Eq, 1),
            ('!', Some('=')) => (NotEq, 2),
            ('!', _) => (Bang, 1),
            ('<', Some('=')) => (Le, 2),
            ('<', _) => (Lt, 1),
            ('>', Some('=')) => (Ge, 2),
            ('>', _) => (Gt, 1),
            ('&', Some('&')) => (AndAnd, 2),
            ('|', Some('|')) => (OrOr, 2),
            ('(', _) => {
                self.paren += 1;
                (LParen, 1)
            }
            (')', _) => {
                self.paren = self.paren.saturating_sub(1);
                (RParen, 1)
            }
            ('{', _) => (LBrace, 1),
            ('}', _) => (RBrace, 1),
            (',', _) => (Comma, 1),
            (':', _) => (Colon, 1),
            ('.', _) => (Dot, 1),
            _ => {
                let len = c.len_utf8();
                self.error(start, start + len, format!("使えない文字 `{c}` です"));
                self.pos += len;
                return;
            }
        };
        self.pos += len;
        self.push(kind, start, self.pos);
    }
}
