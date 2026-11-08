//! 型検査と能力検査(仕様 §5.2〜5.4)。

use crate::ast::*;
use crate::diag::{Diagnostic, Span};
use crate::resolve::{check_lower, check_upper, is_lower, Globals, OpSig};
use std::collections::{BTreeSet, HashMap};

pub fn check(prog: &Program, g: &Globals, diags: &mut Vec<Diagnostic>) {
    let mut c = Checker { g, prog, diags };
    for (i, h) in prog.handlers.iter().enumerate() {
        c.handler_decl(h, i);
    }
    for (i, f) in prog.funs.iter().enumerate() {
        c.fun_decl(f, i);
    }
    c.main();
}

struct Checker<'a> {
    g: &'a Globals,
    prog: &'a Program,
    diags: &'a mut Vec<Diagnostic>,
}

/// 能力の出どころ。
#[derive(Clone, Copy, PartialEq, Eq)]
enum CapSource {
    /// 関数・ハンドラの `uses` 節
    Uses,
    /// 本体の中の `handle`
    Handle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OwnerKind {
    Fun,
    Handler,
}

struct Local {
    ty: Ty,
    mutable: bool,
}

/// 1つの関数(またはハンドラの操作)本体を検査する間の状態。
struct Body {
    owner_kind: OwnerKind,
    owner: String,
    ret: Ty,
    scopes: Vec<HashMap<String, Local>>,
    caps: Vec<(String, CapSource)>,
    /// `uses` に由来する能力のうち、使われたもの。
    used: BTreeSet<String>,
}

impl Body {
    fn lookup(&self, name: &str) -> Option<&Local> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    fn cap(&self, name: &str) -> Option<CapSource> {
        self.caps
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, s)| *s)
    }
}

/// 最後の文が必ず return するか(仕様 §5.2 の単純な判定 + handle)。
fn always_returns(b: &Block) -> bool {
    match b.stmts.last() {
        Some(s) => stmt_returns(s),
        None => false,
    }
}

fn stmt_returns(s: &Stmt) -> bool {
    match s {
        Stmt::Return { .. } => true,
        Stmt::If {
            then,
            els: Some(els),
            ..
        } => {
            always_returns(then)
                && match els {
                    Else::Block(b) => always_returns(b),
                    Else::If(s) => stmt_returns(s),
                }
        }
        Stmt::Handle { body, .. } => always_returns(body),
        _ => false,
    }
}

impl<'a> Checker<'a> {
    fn err(&mut self, code: &'static str, span: Span, msg: impl Into<String>) {
        self.diags.push(Diagnostic::error(code, span, msg));
    }

    fn mismatch(&mut self, span: Span, want: Ty, got: Ty) {
        self.err(
            "E0201",
            span,
            format!(
                "型が一致しません。`{}` が必要ですが `{}` です",
                want.name(),
                got.name()
            ),
        );
    }

    fn expect_ty(&mut self, span: Span, want: Ty, got: Ty) {
        if !want.compatible(got) {
            self.mismatch(span, want, got);
        }
    }

    // ---- 宣言 ----

    fn new_body(
        &mut self,
        kind: OwnerKind,
        owner: &str,
        params: &[Param],
        tys: &[Ty],
        ret: Ty,
        uses: &[String],
    ) -> Body {
        let mut scope = HashMap::new();
        for (p, t) in params.iter().zip(tys) {
            scope.entry(p.name.name.clone()).or_insert(Local {
                ty: *t,
                mutable: false,
            });
        }
        Body {
            owner_kind: kind,
            owner: owner.to_string(),
            ret,
            scopes: vec![scope],
            caps: uses.iter().map(|u| (u.clone(), CapSource::Uses)).collect(),
            used: BTreeSet::new(),
        }
    }

    fn finish_body(&mut self, b: &Body, body: &Block, fname: &Ident) {
        if b.ret != Ty::Unit && b.ret != Ty::Error && !always_returns(body) {
            self.diags.push(
                Diagnostic::error(
                    "E0204",
                    fname.span,
                    format!(
                        "関数 `{}` は `{}` を返しますが、return に到達しない経路があります",
                        fname.name,
                        b.ret.name()
                    ),
                )
                .with_help("ブロックの最後に `return` を書くか、`else` を持つ `if` の両方の枝で return してください"),
            );
        }
    }

    /// 未使用の能力の警告(W0301)。
    fn unused_caps(&mut self, b: &Body, uses: &[Ident]) {
        for u in uses {
            if !self.g.effects.contains_key(&u.name) || b.used.contains(&u.name) {
                continue;
            }
            // 重複して書かれたものは 1 回だけ
            if uses.iter().position(|x| x.name == u.name)
                != uses.iter().position(|x| std::ptr::eq(x, u))
            {
                continue;
            }
            let what = match b.owner_kind {
                OwnerKind::Fun => "関数",
                OwnerKind::Handler => "ハンドラ",
            };
            self.diags.push(
                Diagnostic::warning(
                    "W0301",
                    u.span,
                    format!(
                        "能力 `{}` は{what} `{}` の `uses` に書かれていますが使われていません",
                        u.name, b.owner
                    ),
                )
                .with_help(format!("`uses` から `{}` を削除してください", u.name)),
            );
        }
    }

    fn fun_decl(&mut self, f: &FunDecl, idx: usize) {
        let sig = self.g.funs[idx].clone();
        let mut b = self.new_body(
            OwnerKind::Fun,
            &f.name.name,
            &f.params,
            &sig.params,
            sig.ret,
            &sig.uses,
        );
        self.block(&mut b, &f.body);
        self.finish_body(&b, &f.body, &f.name);
        self.unused_caps(&b, &f.uses);
    }

    fn handler_decl(&mut self, h: &HandlerDecl, idx: usize) {
        let info = self.g.handlers[idx].clone();

        // エフェクトの操作を過不足なく実装しているか(E0303)
        if let Some(eff) = &info.effect {
            let ops: Vec<OpSig> = self.g.effects[eff].ops.clone();
            let mut seen: Vec<&str> = Vec::new();
            for (f, sig) in h.ops.iter().zip(&info.ops) {
                if seen.contains(&sig.name.as_str()) {
                    self.err(
                        "E0303",
                        f.name.span,
                        format!(
                            "ハンドラ `{}` に操作 `{}` が重複しています",
                            h.name.name, sig.name
                        ),
                    );
                    continue;
                }
                seen.push(&sig.name);
                match ops.iter().find(|o| o.name == sig.name) {
                    None => self.err(
                        "E0303",
                        f.name.span,
                        format!("エフェクト `{eff}` に操作 `{}` はありません", sig.name),
                    ),
                    Some(o) => {
                        let same = o.params.len() == sig.params.len()
                            && o.params
                                .iter()
                                .zip(&sig.params)
                                .all(|(a, b)| a.compatible(*b))
                            && o.ret.compatible(sig.ret);
                        if !same {
                            self.diags.push(
                                Diagnostic::error(
                                    "E0303",
                                    f.name.span,
                                    format!(
                                        "操作 `{}` のシグネチャがエフェクト `{eff}` の宣言と一致しません",
                                        sig.name
                                    ),
                                )
                                .with_help(format!(
                                    "宣言は `fun {}({}){}` です",
                                    o.name,
                                    o.params.iter().map(|t| t.name()).collect::<Vec<_>>().join(", "),
                                    if o.ret == Ty::Unit {
                                        String::new()
                                    } else {
                                        format!(" -> {}", o.ret.name())
                                    }
                                )),
                            );
                        }
                    }
                }
            }
            let missing: Vec<&str> = ops
                .iter()
                .filter(|o| !seen.contains(&o.name.as_str()))
                .map(|o| o.name.as_str())
                .collect();
            if !missing.is_empty() {
                self.err(
                    "E0303",
                    h.name.span,
                    format!(
                        "ハンドラ `{}` はエフェクト `{eff}` の操作 {} を実装していません",
                        h.name.name,
                        missing
                            .iter()
                            .map(|m| format!("`{m}`"))
                            .collect::<Vec<_>>()
                            .join("、")
                    ),
                );
            }
        }

        // 各操作の本体。能力はハンドラの uses。使用状況はハンドラ全体で集計する。
        let mut used = BTreeSet::new();
        for (f, sig) in h.ops.iter().zip(&info.ops) {
            let mut b = self.new_body(
                OwnerKind::Handler,
                &h.name.name,
                &f.params,
                &sig.params,
                sig.ret,
                &info.uses,
            );
            self.block(&mut b, &f.body);
            self.finish_body(&b, &f.body, &f.name);
            used.extend(b.used);
        }
        let b = Body {
            owner_kind: OwnerKind::Handler,
            owner: h.name.name.clone(),
            ret: Ty::Unit,
            scopes: vec![],
            caps: vec![],
            used,
        };
        self.unused_caps(&b, &h.uses);
    }

    fn main(&mut self) {
        let Some(&idx) = self.g.fun_index.get("main") else {
            self.err("E0304", Span::new(0, 0), "関数 `main` がありません");
            return;
        };
        let f = &self.prog.funs[idx];
        let sig = &self.g.funs[idx];
        if !f.params.is_empty() || sig.ret != Ty::Unit {
            self.diags.push(
                Diagnostic::error(
                    "E0304",
                    f.name.span,
                    "`main` は引数なし・戻り値なしでなければなりません",
                )
                .with_help("`fun main()` と書いてください"),
            );
        }
        for u in &f.uses {
            if u.name != "Console" {
                self.diags.push(
                    Diagnostic::error(
                        "E0304",
                        u.span,
                        format!("`main` の `uses` には `Console` だけを書けます(`{}` は書けません)", u.name),
                    )
                    .with_help(format!(
                        "`main` の中で `handle {} by ハンドラ {{ ... }}` を使って能力を用意してください",
                        u.name
                    )),
                );
            }
        }
    }

    // ---- 能力 ----

    /// 能力 `name` を要求する。使えなければ false。
    fn use_cap(&mut self, b: &mut Body, name: &str) -> bool {
        match b.cap(name) {
            Some(CapSource::Uses) => {
                b.used.insert(name.to_string());
                true
            }
            Some(CapSource::Handle) => true,
            None => false,
        }
    }

    fn missing_cap_help(&self, b: &Body, missing: &[String]) -> String {
        let list = missing.join(", ");
        match b.owner_kind {
            OwnerKind::Fun if b.owner == "main" && missing.iter().any(|m| m != "Console") => format!(
                "`main` の `uses` には `Console` しか書けません。`handle {} by ハンドラ {{ ... }}` で囲んでください",
                missing.iter().find(|m| *m != "Console").unwrap()
            ),
            OwnerKind::Fun => format!("関数 `{}` に `uses {list}` を追加してください", b.owner),
            OwnerKind::Handler => format!("ハンドラ `{}` に `uses {list}` を追加してください", b.owner),
        }
    }

    fn where_str(b: &Body) -> &'static str {
        match b.owner_kind {
            OwnerKind::Fun => "この関数",
            OwnerKind::Handler => "このハンドラ",
        }
    }

    // ---- 文 ----

    fn block(&mut self, b: &mut Body, block: &Block) {
        b.scopes.push(HashMap::new());
        for s in &block.stmts {
            self.stmt(b, s);
        }
        b.scopes.pop();
    }

    fn declare_local(&mut self, b: &mut Body, name: &Ident, ty: Ty, mutable: bool) {
        if !check_lower(name, "変数名", self.diags) {
            // 名前は登録しておき、後の未定義エラーを避ける
        }
        if b.lookup(&name.name).is_some() {
            self.diags.push(
                Diagnostic::error(
                    "E0103",
                    name.span,
                    format!(
                        "変数 `{}` はすでに宣言されています(シャドーイングは禁止)",
                        name.name
                    ),
                )
                .with_help("別の名前を使ってください"),
            );
            return;
        }
        b.scopes
            .last_mut()
            .unwrap()
            .insert(name.name.clone(), Local { ty, mutable });
    }

    fn stmt(&mut self, b: &mut Body, s: &Stmt) {
        match s {
            Stmt::Let {
                mutable,
                name,
                ann,
                init,
            } => {
                let t = self.value_expr(b, init);
                let ty = match ann {
                    Some(a) => {
                        let want = crate::resolve::resolve_type(a, self.diags);
                        self.expect_ty(init.span, want, t);
                        want
                    }
                    None => t,
                };
                self.declare_local(b, name, ty, *mutable);
            }
            Stmt::Assign {
                name,
                op,
                value,
                op_span,
            } => {
                let vt = self.value_expr(b, value);
                let (lt, mutable) = match b.lookup(&name.name) {
                    Some(l) => (l.ty, l.mutable),
                    None => {
                        self.undefined_var(name);
                        return;
                    }
                };
                if !mutable {
                    self.diags.push(
                        Diagnostic::error(
                            "E0203",
                            name.span,
                            format!("`{}` は `var` ではないので代入できません", name.name),
                        )
                        .with_help(format!("`var {}` で宣言してください", name.name)),
                    );
                }
                match op {
                    AssignOp::Set => self.expect_ty(value.span, lt, vt),
                    AssignOp::Add | AssignOp::Sub => {
                        if !lt.compatible(Ty::Int) {
                            self.err(
                                "E0201",
                                *op_span,
                                format!(
                                    "`+=` と `-=` は Int にだけ使えます(`{}` は `{}`)",
                                    name.name,
                                    lt.name()
                                ),
                            );
                        }
                        self.expect_ty(value.span, Ty::Int, vt);
                    }
                }
            }
            Stmt::If { cond, then, els } => {
                let t = self.value_expr(b, cond);
                self.expect_ty(cond.span, Ty::Bool, t);
                self.block(b, then);
                match els {
                    None => {}
                    Some(Else::Block(e)) => self.block(b, e),
                    Some(Else::If(s)) => self.stmt(b, s),
                }
            }
            Stmt::While { cond, body } => {
                let t = self.value_expr(b, cond);
                self.expect_ty(cond.span, Ty::Bool, t);
                self.block(b, body);
            }
            Stmt::Return { value, span } => match value {
                None => {
                    if b.ret != Ty::Unit && b.ret != Ty::Error {
                        self.err(
                            "E0201",
                            *span,
                            format!("`{}` の値を返す必要があります", b.ret.name()),
                        );
                    }
                }
                Some(v) => {
                    let t = self.value_expr(b, v);
                    if b.ret == Ty::Unit {
                        self.err("E0201", v.span, "戻り値の型がない関数では値を返せません");
                    } else {
                        self.expect_ty(v.span, b.ret, t);
                    }
                }
            },
            Stmt::Handle {
                effect,
                handler,
                body,
            } => self.handle(b, effect, handler, body),
            Stmt::Expr(e) => match &e.kind {
                ExprKind::Call { .. } | ExprKind::OpCall { .. } => {
                    self.expr(b, e);
                }
                _ => {
                    self.expr(b, e);
                    self.err("E0205", e.span, "式文として書けるのは呼び出しだけです");
                }
            },
        }
    }

    fn handle(&mut self, b: &mut Body, effect: &Ident, handler: &Ident, body: &Block) {
        let mut ok = check_upper(effect, "エフェクト名", self.diags)
            & check_upper(handler, "ハンドラ名", self.diags);
        if ok && !self.g.effects.contains_key(&effect.name) {
            self.err(
                "E0302",
                effect.span,
                format!("エフェクト `{}` は存在しません", effect.name),
            );
            ok = false;
        }
        if ok {
            match self.g.handler(&handler.name) {
                None => {
                    self.err(
                        "E0303",
                        handler.span,
                        format!("ハンドラ `{}` は宣言されていません", handler.name),
                    );
                }
                Some(h) if h.effect.as_deref() != Some(effect.name.as_str()) => {
                    let msg = match &h.effect {
                        Some(e) => format!(
                            "ハンドラ `{}` は `{e}` のハンドラであり、`{}` には使えません",
                            handler.name, effect.name
                        ),
                        None => format!(
                            "ハンドラ `{}` は `{}` のハンドラではありません",
                            handler.name, effect.name
                        ),
                    };
                    self.err("E0303", handler.span, msg);
                }
                Some(h) => {
                    // ハンドラの uses は handle を書いた地点で使えなければならない
                    let mut missing = Vec::new();
                    for u in h.uses.clone() {
                        if !self.use_cap(b, &u) {
                            missing.push(u);
                        }
                    }
                    if !missing.is_empty() {
                        let help = self.missing_cap_help(b, &missing);
                        self.diags.push(
                            Diagnostic::error(
                                "E0301",
                                handler.span,
                                format!(
                                    "ハンドラ `{}` には能力 {} が必要ですが、{}では使えません",
                                    handler.name,
                                    quote_list(&missing),
                                    Self::where_str(b)
                                ),
                            )
                            .with_help(help),
                        );
                    }
                }
            }
        }
        b.caps.push((effect.name.clone(), CapSource::Handle));
        self.block(b, body);
        b.caps.pop();
    }

    // ---- 式 ----

    fn undefined_var(&mut self, id: &Ident) {
        if !is_lower(&id.name) {
            self.err(
                "E0105",
                id.span,
                format!(
                    "変数名は小文字または `_` で始めます(`{}` は値として使えません)",
                    id.name
                ),
            );
        } else {
            self.err(
                "E0101",
                id.span,
                format!("変数 `{}` は定義されていません", id.name),
            );
        }
    }

    /// 値として使う式。Unit なら E0201。
    fn value_expr(&mut self, b: &mut Body, e: &Expr) -> Ty {
        let t = self.expr(b, e);
        if t == Ty::Unit {
            self.err("E0201", e.span, "値を返さない呼び出しは値として使えません");
            return Ty::Error;
        }
        t
    }

    fn args(&mut self, b: &mut Body, args: &[Expr], params: &[Ty], what: &str, span: Span) {
        let tys: Vec<Ty> = args.iter().map(|a| self.value_expr(b, a)).collect();
        if args.len() != params.len() {
            self.err(
                "E0202",
                span,
                format!(
                    "{what}の引数は {} 個ですが、{} 個渡されています",
                    params.len(),
                    args.len()
                ),
            );
            return;
        }
        for ((a, t), p) in args.iter().zip(tys).zip(params) {
            self.expect_ty(a.span, *p, t);
        }
    }

    fn expr(&mut self, b: &mut Body, e: &Expr) -> Ty {
        let t = self.expr_inner(b, e);
        e.ty.set(t);
        t
    }

    fn expr_inner(&mut self, b: &mut Body, e: &Expr) -> Ty {
        match &e.kind {
            ExprKind::Int(_) => Ty::Int,
            ExprKind::Bool(_) => Ty::Bool,
            ExprKind::Str(pieces) => {
                for p in pieces {
                    if let StrPiece::Expr(x) = p {
                        let t = self.value_expr(b, x);
                        if !matches!(t, Ty::Int | Ty::Bool | Ty::Str | Ty::Error) {
                            self.mismatch(x.span, Ty::Str, t);
                        }
                    }
                }
                Ty::Str
            }
            ExprKind::Var(id) => match b.lookup(&id.name) {
                Some(l) => l.ty,
                None => {
                    self.undefined_var(id);
                    Ty::Error
                }
            },
            ExprKind::Call { callee, args } => {
                if !check_lower(callee, "関数名", self.diags) {
                    for a in args {
                        self.expr(b, a);
                    }
                    return Ty::Error;
                }
                let Some(sig) = self.g.fun(&callee.name).cloned() else {
                    for a in args {
                        self.expr(b, a);
                    }
                    self.err(
                        "E0101",
                        callee.span,
                        format!("関数 `{}` は定義されていません", callee.name),
                    );
                    return Ty::Error;
                };
                self.args(
                    b,
                    args,
                    &sig.params,
                    &format!("関数 `{}` ", callee.name),
                    e.span,
                );
                let mut missing = Vec::new();
                for u in &sig.uses {
                    if !self.use_cap(b, u) {
                        missing.push(u.clone());
                    }
                }
                if !missing.is_empty() {
                    let help = self.missing_cap_help(b, &missing);
                    self.diags.push(
                        Diagnostic::error(
                            "E0301",
                            callee.span,
                            format!(
                                "関数 `{}` を呼ぶには能力 {} が必要ですが、{}では使えません",
                                callee.name,
                                quote_list(&missing),
                                Self::where_str(b)
                            ),
                        )
                        .with_help(help),
                    );
                }
                sig.ret
            }
            ExprKind::OpCall { effect, op, args } => {
                if !check_upper(effect, "能力(エフェクト)の名前", self.diags) {
                    for a in args {
                        self.expr(b, a);
                    }
                    return Ty::Error;
                }
                let Some(info) = self.g.effects.get(&effect.name) else {
                    for a in args {
                        self.expr(b, a);
                    }
                    self.err(
                        "E0302",
                        effect.span,
                        format!("エフェクト `{}` は存在しません", effect.name),
                    );
                    return Ty::Error;
                };
                let Some(sig) = info.ops.iter().find(|o| o.name == op.name).cloned() else {
                    for a in args {
                        self.expr(b, a);
                    }
                    self.err(
                        "E0101",
                        op.span,
                        format!(
                            "エフェクト `{}` に操作 `{}` はありません",
                            effect.name, op.name
                        ),
                    );
                    return Ty::Error;
                };
                if !self.use_cap(b, &effect.name) {
                    let help = self.missing_cap_help(b, std::slice::from_ref(&effect.name));
                    self.diags.push(
                        Diagnostic::error(
                            "E0301",
                            effect.span,
                            format!(
                                "能力 `{}` が必要ですが、{}では使えません",
                                effect.name,
                                Self::where_str(b)
                            ),
                        )
                        .with_help(help),
                    );
                }
                self.args(
                    b,
                    args,
                    &sig.params,
                    &format!("操作 `{}.{}` ", effect.name, op.name),
                    e.span,
                );
                sig.ret
            }
            ExprKind::Unary { op, expr } => {
                let t = self.value_expr(b, expr);
                let want = match op {
                    UnOp::Neg => Ty::Int,
                    UnOp::Not => Ty::Bool,
                };
                self.expect_ty(expr.span, want, t);
                want
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let lt = self.value_expr(b, lhs);
                let rt = self.value_expr(b, rhs);
                match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                        if *op == BinOp::Add && (lt == Ty::Str || rt == Ty::Str) {
                            self.diags.push(
                                Diagnostic::error(
                                    "E0201",
                                    e.span,
                                    "String の連結に `+` は使えません",
                                )
                                .with_help("文字列補間を使ってください(例: \"{a}{b}\")"),
                            );
                            return Ty::Str;
                        }
                        self.expect_ty(lhs.span, Ty::Int, lt);
                        self.expect_ty(rhs.span, Ty::Int, rt);
                        Ty::Int
                    }
                    BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        self.expect_ty(lhs.span, Ty::Int, lt);
                        self.expect_ty(rhs.span, Ty::Int, rt);
                        Ty::Bool
                    }
                    BinOp::Eq | BinOp::Ne => {
                        if !lt.compatible(rt) {
                            self.err(
                                "E0201",
                                e.span,
                                format!(
                                    "`{}` の両辺の型が違います(`{}` と `{}`)",
                                    op.symbol(),
                                    lt.name(),
                                    rt.name()
                                ),
                            );
                        }
                        Ty::Bool
                    }
                    BinOp::And | BinOp::Or => {
                        self.expect_ty(lhs.span, Ty::Bool, lt);
                        self.expect_ty(rhs.span, Ty::Bool, rt);
                        Ty::Bool
                    }
                }
            }
        }
    }
}

fn quote_list(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("`{n}`"))
        .collect::<Vec<_>>()
        .join("、")
}
