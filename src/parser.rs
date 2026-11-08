//! 手書きの再帰下降パーサー(仕様 §4)。

use crate::ast::*;
use crate::diag::{Diagnostic, Span};
use crate::lexer::{describe, StrPart, TokKind, Token};

/// 構文エラー。診断はすでに記録済みで、呼び出し側は回復処理だけを行う。
struct Abort;
type PResult<T> = Result<T, Abort>;

pub struct Parser<'d> {
    toks: Vec<Token>,
    pos: usize,
    diags: &'d mut Vec<Diagnostic>,
}

pub fn parse(toks: Vec<Token>, diags: &mut Vec<Diagnostic>) -> Program {
    let mut p = Parser {
        toks,
        pos: 0,
        diags,
    };
    p.program()
}

impl<'d> Parser<'d> {
    // ---- 基本操作 ----

    fn peek(&self) -> &TokKind {
        &self.toks[self.pos].kind
    }

    fn peek_at(&self, n: usize) -> &TokKind {
        let i = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[i].kind
    }

    fn tok(&self) -> &Token {
        &self.toks[self.pos]
    }

    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn at(&self, k: &TokKind) -> bool {
        self.peek() == k
    }

    fn skip_nl(&mut self) {
        while self.at(&TokKind::Newline) {
            self.bump();
        }
    }

    /// 現在のトークンについて構文エラーを報告する。予約語なら E0003 にする。
    fn unexpected(&mut self, expected: &str) -> Abort {
        let t = self.tok().clone();
        let d = match &t.kind {
            TokKind::Reserved(w) => {
                Diagnostic::error("E0003", t.span, format!("`{w}` は Scull 0 では未対応です"))
            }
            TokKind::Else => Diagnostic::error(
                "E0002",
                t.span,
                "`else` は直前の `}` と同じ行に書いてください",
            )
            .with_help("`} else {` のように書きます"),
            k => Diagnostic::error(
                "E0002",
                t.span,
                format!("{expected}が必要ですが、{}があります", describe(k)),
            ),
        };
        self.diags.push(d);
        Abort
    }

    fn expect(&mut self, k: TokKind) -> PResult<Token> {
        if self.at(&k) {
            Ok(self.bump())
        } else {
            Err(self.unexpected(&describe(&k)))
        }
    }

    fn ident(&mut self, what: &str) -> PResult<Ident> {
        if let TokKind::Ident(name) = self.peek() {
            let name = name.clone();
            let t = self.bump();
            Ok(Ident { name, span: t.span })
        } else {
            Err(self.unexpected(what))
        }
    }

    /// 文の終わり(改行か `}`)を要求する。
    fn end_of_item(&mut self) -> PResult<()> {
        match self.peek() {
            TokKind::Newline => {
                self.skip_nl();
                Ok(())
            }
            TokKind::RBrace | TokKind::Eof => Ok(()),
            _ => Err(self.unexpected("改行")),
        }
    }

    /// エラー後、次のトップレベル宣言(行頭の fun / effect / handler)まで読み飛ばす。
    fn sync_top(&mut self) {
        loop {
            let t = self.tok();
            match t.kind {
                TokKind::Eof => return,
                TokKind::Fun | TokKind::Effect | TokKind::Handler if t.line_start => return,
                _ => {
                    self.bump();
                }
            }
        }
    }

    // ---- 宣言 ----

    fn program(&mut self) -> Program {
        let mut prog = Program::default();
        self.skip_nl();
        while !self.at(&TokKind::Eof) {
            let start = self.pos;
            let r = match self.peek() {
                TokKind::Fun => self.fun_decl(true).map(|f| prog.funs.push(f)),
                TokKind::Effect => self.effect_decl().map(|e| prog.effects.push(e)),
                TokKind::Handler => self.handler_decl().map(|h| prog.handlers.push(h)),
                _ => Err(self.unexpected("トップレベルの宣言(`fun` / `effect` / `handler`)")),
            };
            let r = r.and_then(|_| {
                if self.at(&TokKind::Eof) {
                    Ok(())
                } else {
                    self.expect(TokKind::Newline).map(|_| ())
                }
            });
            if r.is_err() {
                if self.pos == start {
                    self.bump();
                }
                self.sync_top();
            }
            self.skip_nl();
        }
        prog
    }

    fn params(&mut self) -> PResult<Vec<Param>> {
        self.expect(TokKind::LParen)?;
        let mut params = Vec::new();
        while !self.at(&TokKind::RParen) {
            let name = self.ident("引数名")?;
            self.expect(TokKind::Colon)?;
            let ty = self.type_ann()?;
            params.push(Param { name, ty });
            if self.at(&TokKind::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        self.expect(TokKind::RParen)?;
        Ok(params)
    }

    fn type_ann(&mut self) -> PResult<TypeAnn> {
        let id = self.ident("型")?;
        Ok(TypeAnn {
            name: id.name,
            span: id.span,
        })
    }

    fn ret_type(&mut self) -> PResult<Option<TypeAnn>> {
        self.skip_nl();
        if self.at(&TokKind::Arrow) {
            self.bump();
            let t = self.type_ann()?;
            self.skip_nl();
            Ok(Some(t))
        } else {
            Ok(None)
        }
    }

    fn uses_clause(&mut self) -> PResult<Vec<Ident>> {
        let mut uses = Vec::new();
        if self.at(&TokKind::Uses) {
            self.bump();
            loop {
                uses.push(self.ident("能力の名前")?);
                if self.at(&TokKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
            self.skip_nl();
        }
        Ok(uses)
    }

    fn fun_decl(&mut self, allow_uses: bool) -> PResult<FunDecl> {
        self.expect(TokKind::Fun)?;
        let name = self.ident("関数名")?;
        let params = self.params()?;
        let ret = self.ret_type()?;
        let uses = if self.at(&TokKind::Uses) && !allow_uses {
            let sp = self.tok().span;
            self.diags.push(
                Diagnostic::error("E0002", sp, "ハンドラの操作には `uses` を書けません")
                    .with_help("`uses` はハンドラの宣言(`handler H for E uses ...`)に書きます"),
            );
            return Err(Abort);
        } else {
            self.uses_clause()?
        };
        let body = self.block()?;
        Ok(FunDecl {
            name,
            params,
            ret,
            uses,
            body,
        })
    }

    fn effect_decl(&mut self) -> PResult<EffectDecl> {
        self.expect(TokKind::Effect)?;
        let name = self.ident("エフェクト名")?;
        self.skip_nl();
        self.expect(TokKind::LBrace)?;
        self.skip_nl();
        let mut ops = Vec::new();
        while !self.at(&TokKind::RBrace) {
            self.expect(TokKind::Fun)?;
            let opname = self.ident("操作名")?;
            let params = self.params()?;
            let ret = if self.at(&TokKind::Arrow) {
                self.bump();
                Some(self.type_ann()?)
            } else {
                None
            };
            ops.push(OpSigDecl {
                name: opname,
                params,
                ret,
            });
            self.end_of_item()?;
        }
        self.expect(TokKind::RBrace)?;
        Ok(EffectDecl { name, ops })
    }

    fn handler_decl(&mut self) -> PResult<HandlerDecl> {
        self.expect(TokKind::Handler)?;
        let name = self.ident("ハンドラ名")?;
        self.expect(TokKind::For)?;
        let effect = self.ident("エフェクト名")?;
        self.skip_nl();
        let uses = self.uses_clause()?;
        self.expect(TokKind::LBrace)?;
        self.skip_nl();
        let mut ops = Vec::new();
        while !self.at(&TokKind::RBrace) {
            ops.push(self.fun_decl(false)?);
            self.end_of_item()?;
        }
        self.expect(TokKind::RBrace)?;
        Ok(HandlerDecl {
            name,
            effect,
            uses,
            ops,
        })
    }

    // ---- 文 ----

    fn block(&mut self) -> PResult<Block> {
        self.expect(TokKind::LBrace)?;
        self.skip_nl();
        let mut stmts = Vec::new();
        while !self.at(&TokKind::RBrace) {
            if self.at(&TokKind::Eof) {
                return Err(self.unexpected("`}`"));
            }
            stmts.push(self.stmt()?);
            self.end_of_item()?;
        }
        self.expect(TokKind::RBrace)?;
        Ok(Block { stmts })
    }

    fn stmt(&mut self) -> PResult<Stmt> {
        match self.peek() {
            TokKind::Let | TokKind::Var => {
                let mutable = self.at(&TokKind::Var);
                self.bump();
                let name = self.ident("変数名")?;
                let ann = if self.at(&TokKind::Colon) {
                    self.bump();
                    Some(self.type_ann()?)
                } else {
                    None
                };
                self.expect(TokKind::Eq)?;
                let init = self.expr()?;
                Ok(Stmt::Let {
                    mutable,
                    name,
                    ann,
                    init,
                })
            }
            TokKind::If => self.if_stmt(),
            TokKind::While => {
                self.bump();
                let cond = self.expr()?;
                let body = self.block()?;
                Ok(Stmt::While { cond, body })
            }
            TokKind::Return => {
                let t = self.bump();
                let value = match self.peek() {
                    TokKind::Newline | TokKind::RBrace => None,
                    _ => Some(self.expr()?),
                };
                let span = match &value {
                    Some(v) => t.span.to(v.span),
                    None => t.span,
                };
                Ok(Stmt::Return { value, span })
            }
            TokKind::Handle => {
                self.bump();
                let effect = self.ident("エフェクト名")?;
                self.expect(TokKind::By)?;
                let handler = self.ident("ハンドラ名")?;
                let body = self.block()?;
                Ok(Stmt::Handle {
                    effect,
                    handler,
                    body,
                })
            }
            _ => {
                let e = self.expr()?;
                let op = match self.peek() {
                    TokKind::Eq => Some(AssignOp::Set),
                    TokKind::PlusEq => Some(AssignOp::Add),
                    TokKind::MinusEq => Some(AssignOp::Sub),
                    _ => None,
                };
                match op {
                    None => Ok(Stmt::Expr(e)),
                    Some(op) => {
                        let op_span = self.bump().span;
                        let name = match e.kind {
                            ExprKind::Var(id) => id,
                            _ => {
                                self.diags.push(Diagnostic::error(
                                    "E0002",
                                    e.span,
                                    "代入の左辺は変数でなければなりません",
                                ));
                                return Err(Abort);
                            }
                        };
                        let value = self.expr()?;
                        Ok(Stmt::Assign {
                            name,
                            op,
                            value,
                            op_span,
                        })
                    }
                }
            }
        }
    }

    fn if_stmt(&mut self) -> PResult<Stmt> {
        self.expect(TokKind::If)?;
        let cond = self.expr()?;
        let then = self.block()?;
        let els = if self.at(&TokKind::Else) {
            self.bump();
            if self.at(&TokKind::If) {
                Some(Else::If(Box::new(self.if_stmt()?)))
            } else {
                Some(Else::Block(self.block()?))
            }
        } else {
            None
        };
        Ok(Stmt::If { cond, then, els })
    }

    // ---- 式 ----

    fn expr(&mut self) -> PResult<Expr> {
        self.or_expr()
    }

    fn binary(lhs: Expr, op: BinOp, rhs: Expr) -> Expr {
        let span = lhs.span.to(rhs.span);
        Expr::new(
            ExprKind::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
            span,
        )
    }

    fn or_expr(&mut self) -> PResult<Expr> {
        let mut e = self.and_expr()?;
        while self.at(&TokKind::OrOr) {
            self.bump();
            let r = self.and_expr()?;
            e = Self::binary(e, BinOp::Or, r);
        }
        Ok(e)
    }

    fn and_expr(&mut self) -> PResult<Expr> {
        let mut e = self.cmp_expr()?;
        while self.at(&TokKind::AndAnd) {
            self.bump();
            let r = self.cmp_expr()?;
            e = Self::binary(e, BinOp::And, r);
        }
        Ok(e)
    }

    fn cmp_op(&self) -> Option<BinOp> {
        Some(match self.peek() {
            TokKind::EqEq => BinOp::Eq,
            TokKind::NotEq => BinOp::Ne,
            TokKind::Lt => BinOp::Lt,
            TokKind::Le => BinOp::Le,
            TokKind::Gt => BinOp::Gt,
            TokKind::Ge => BinOp::Ge,
            _ => return None,
        })
    }

    fn cmp_expr(&mut self) -> PResult<Expr> {
        let e = self.add_expr()?;
        if let Some(op) = self.cmp_op() {
            self.bump();
            let r = self.add_expr()?;
            if self.cmp_op().is_some() {
                let sp = self.tok().span;
                self.diags.push(
                    Diagnostic::error("E0002", sp, "比較演算子は連鎖できません")
                        .with_help("`a < b && b < c` のように `&&` でつないでください"),
                );
                return Err(Abort);
            }
            return Ok(Self::binary(e, op, r));
        }
        Ok(e)
    }

    fn add_expr(&mut self) -> PResult<Expr> {
        let mut e = self.mul_expr()?;
        loop {
            let op = match self.peek() {
                TokKind::Plus => BinOp::Add,
                TokKind::Minus => BinOp::Sub,
                _ => return Ok(e),
            };
            self.bump();
            let r = self.mul_expr()?;
            e = Self::binary(e, op, r);
        }
    }

    fn mul_expr(&mut self) -> PResult<Expr> {
        let mut e = self.unary()?;
        loop {
            let op = match self.peek() {
                TokKind::Star => BinOp::Mul,
                TokKind::Slash => BinOp::Div,
                TokKind::Percent => BinOp::Rem,
                _ => return Ok(e),
            };
            self.bump();
            let r = self.unary()?;
            e = Self::binary(e, op, r);
        }
    }

    fn unary(&mut self) -> PResult<Expr> {
        let op = match self.peek() {
            TokKind::Minus => UnOp::Neg,
            TokKind::Bang => UnOp::Not,
            _ => return self.primary(),
        };
        let t = self.bump();
        let inner = self.unary()?;
        let span = t.span.to(inner.span);
        Ok(Expr::new(
            ExprKind::Unary {
                op,
                expr: Box::new(inner),
            },
            span,
        ))
    }

    fn args(&mut self) -> PResult<(Vec<Expr>, Span)> {
        self.expect(TokKind::LParen)?;
        let mut args = Vec::new();
        while !self.at(&TokKind::RParen) {
            args.push(self.expr()?);
            if self.at(&TokKind::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        let close = self.expect(TokKind::RParen)?;
        Ok((args, close.span))
    }

    fn primary(&mut self) -> PResult<Expr> {
        let t = self.tok().clone();
        match &t.kind {
            TokKind::Int(n) => {
                self.bump();
                Ok(Expr::new(ExprKind::Int(*n), t.span))
            }
            TokKind::True | TokKind::False => {
                self.bump();
                Ok(Expr::new(ExprKind::Bool(t.kind == TokKind::True), t.span))
            }
            TokKind::Str(parts) => {
                self.bump();
                let mut pieces = Vec::new();
                for part in parts {
                    match part {
                        StrPart::Lit(s) => pieces.push(StrPiece::Lit(s.clone())),
                        StrPart::Interp(toks) => {
                            let mut sub = Parser {
                                toks: toks.clone(),
                                pos: 0,
                                diags: &mut *self.diags,
                            };
                            if sub.at(&TokKind::Eof) {
                                let sp = sub.tok().span;
                                self.diags.push(Diagnostic::error(
                                    "E0002",
                                    sp,
                                    "文字列補間 `{}` の中に式がありません",
                                ));
                                return Err(Abort);
                            }
                            let e = sub.expr()?;
                            if !sub.at(&TokKind::Eof) {
                                return Err(sub.unexpected("補間の終わり `}`"));
                            }
                            pieces.push(StrPiece::Expr(e));
                        }
                    }
                }
                Ok(Expr::new(ExprKind::Str(pieces), t.span))
            }
            TokKind::Ident(name) => {
                let id = Ident {
                    name: name.clone(),
                    span: t.span,
                };
                self.bump();
                match self.peek() {
                    TokKind::LParen => {
                        let (args, close) = self.args()?;
                        Ok(Expr::new(
                            ExprKind::Call { callee: id, args },
                            t.span.to(close),
                        ))
                    }
                    TokKind::Dot if matches!(self.peek_at(1), TokKind::Ident(_)) => {
                        self.bump();
                        let op = self.ident("操作名")?;
                        let (args, close) = self.args()?;
                        Ok(Expr::new(
                            ExprKind::OpCall {
                                effect: id,
                                op,
                                args,
                            },
                            t.span.to(close),
                        ))
                    }
                    _ => Ok(Expr::new(ExprKind::Var(id), t.span)),
                }
            }
            TokKind::LParen => {
                self.bump();
                let e = self.expr()?;
                self.expect(TokKind::RParen)?;
                Ok(e)
            }
            _ => Err(self.unexpected("式")),
        }
    }
}
