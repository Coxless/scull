//! C コード生成(仕様 §7)。能力は「関数ポインタと環境ポインタの組」として引数で渡す。
//!
//! 式は左から右に評価する。副作用(呼び出し、実行時エラーになりうる算術)を持つ部分式は
//! 一時変数 `t_N` に入れ、C の引数評価順が未規定であることの影響を受けないようにする。

use crate::ast::*;
use crate::resolve::{Globals, HandlerInfo};

pub const RUNTIME: &str = include_str!("runtime/scl_rt.h");

pub fn generate(prog: &Program, g: &Globals) -> String {
    let mut gen = Gen {
        g,
        prog,
        lines: Vec::new(),
        indent: 0,
        tmp: 0,
        handle_id: 0,
        caps: Vec::new(),
    };
    gen.program();
    let mut out = String::new();
    out.push_str("/* scullc が生成したコード。編集しないこと。 */\n");
    out.push_str(RUNTIME);
    out.push('\n');
    for l in gen.lines {
        out.push_str(&l);
        out.push('\n');
    }
    out
}

fn cty(t: Ty) -> &'static str {
    match t {
        Ty::Int => "int64_t",
        Ty::Bool => "bool",
        Ty::Str => "scl_str",
        Ty::Unit | Ty::Error => "void",
    }
}

/// C の文字列リテラルにする。ASCII の表示可能文字以外は 8 進エスケープにする。
fn c_string(s: &str) -> String {
    let mut out = String::from("\"");
    for b in s.bytes() {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\t' => out.push_str("\\t"),
            // `??` が三文字表記と解釈されないよう `?` もエスケープする
            b'?' => out.push_str("\\?"),
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\{:03o}", b)),
        }
    }
    out.push('"');
    out
}

/// `(...)` 全体を囲む外側の括弧を1組外す(`if ((a == b))` の警告を避ける)。
fn strip_parens(c: &str) -> &str {
    if !(c.starts_with('(') && c.ends_with(')')) {
        return c;
    }
    let mut depth = 0;
    let mut in_str = false;
    let mut prev = ' ';
    for (i, ch) in c.char_indices() {
        if in_str {
            if ch == '"' && prev != '\\' {
                in_str = false;
            }
        } else if ch == '"' {
            in_str = true;
        } else if ch == '(' {
            depth += 1;
        } else if ch == ')' {
            depth -= 1;
            if depth == 0 && i != c.len() - 1 {
                return c; // 外側の括弧は全体を囲んでいない
            }
        }
        prev = if prev == '\\' && ch == '\\' { ' ' } else { ch };
    }
    &c[1..c.len() - 1]
}

fn fun_name(name: &str) -> String {
    format!("scl_f_{name}")
}

fn op_fn_name(handler: &str, op: &str) -> String {
    format!("scl_h_{handler}_{op}")
}

struct Gen<'a> {
    g: &'a Globals,
    prog: &'a Program,
    lines: Vec<String>,
    indent: usize,
    tmp: usize,
    handle_id: usize,
    /// 能力名 → 現在有効な C の式(`scl_eff_X *` 型)。後ろほど内側。
    caps: Vec<(String, String)>,
}

impl<'a> Gen<'a> {
    fn emit(&mut self, s: impl AsRef<str>) {
        let line = format!("{}{}", "    ".repeat(self.indent), s.as_ref());
        self.lines.push(line);
    }

    fn blank(&mut self) {
        self.lines.push(String::new());
    }

    fn fresh(&mut self) -> String {
        self.tmp += 1;
        format!("t_{}", self.tmp)
    }

    fn cap(&self, name: &str) -> String {
        self.caps
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, c)| c.clone())
            .expect("能力は検査済み")
    }

    // ---- 宣言 ----

    fn param_list(params: &[Param], tys: &[Ty], caps: &[String], env: bool) -> String {
        let mut ps: Vec<String> = Vec::new();
        if env {
            ps.push("void *env".to_string());
        }
        for (p, t) in params.iter().zip(tys) {
            ps.push(format!("{} v_{}", cty(*t), p.name.name));
        }
        for c in caps {
            ps.push(format!("scl_eff_{c} *cap_{c}"));
        }
        if ps.is_empty() {
            "void".to_string()
        } else {
            ps.join(", ")
        }
    }

    fn program(&mut self) {
        let prog = self.prog;
        let g = self.g;

        // エフェクトの型(Console はランタイムで定義済み)
        for e in &prog.effects {
            let info = &g.effects[&e.name.name];
            self.emit(format!("/* effect {} */", e.name.name));
            self.emit(format!("typedef struct scl_eff_{} {{", e.name.name));
            self.emit("    void *env;");
            for op in &info.ops {
                let mut ps = vec!["void *env".to_string()];
                ps.extend(op.params.iter().map(|t| cty(*t).to_string()));
                self.emit(format!(
                    "    {} (*op_{})({});",
                    cty(op.ret),
                    op.name,
                    ps.join(", ")
                ));
            }
            self.emit(format!("}} scl_eff_{};", e.name.name));
            self.blank();
        }

        // ハンドラの環境
        for (h, info) in prog.handlers.iter().zip(&g.handlers) {
            self.emit(format!("struct scl_henv_{} {{", h.name.name));
            if info.uses.is_empty() {
                self.emit("    char scl_dummy;");
            }
            for u in &info.uses {
                self.emit(format!("    scl_eff_{u} *cap_{u};"));
            }
            self.emit("};");
            self.blank();
        }

        // プロトタイプ
        for (h, info) in prog.handlers.iter().zip(&g.handlers) {
            for (f, sig) in h.ops.iter().zip(&info.ops) {
                self.emit(format!(
                    "SCL_FN {} {}({});",
                    cty(sig.ret),
                    op_fn_name(&h.name.name, &sig.name),
                    Self::param_list(&f.params, &sig.params, &[], true)
                ));
            }
        }
        for (f, sig) in prog.funs.iter().zip(&g.funs) {
            self.emit(format!(
                "SCL_FN {} {}({});",
                cty(sig.ret),
                fun_name(&f.name.name),
                Self::param_list(&f.params, &sig.params, &sig.uses, false)
            ));
        }
        self.blank();

        // ハンドラの操作
        for (h, info) in prog.handlers.iter().zip(&g.handlers) {
            for (f, sig) in h.ops.iter().zip(&info.ops) {
                self.handler_op(&h.name.name, info, f, &sig.params, sig.ret);
            }
        }

        // 関数
        for (f, sig) in prog.funs.iter().zip(&g.funs) {
            self.tmp = 0;
            self.emit(format!(
                "SCL_FN {} {}({}) {{",
                cty(sig.ret),
                fun_name(&f.name.name),
                Self::param_list(&f.params, &sig.params, &sig.uses, false)
            ));
            self.indent += 1;
            for p in &f.params {
                self.emit(format!("(void)v_{};", p.name.name));
            }
            self.caps.clear();
            for u in &sig.uses {
                self.emit(format!("(void)cap_{u};"));
                self.caps.push((u.clone(), format!("cap_{u}")));
            }
            self.block_body(&f.body);
            self.indent -= 1;
            self.emit("}");
            self.blank();
        }

        // エントリポイント
        let main_uses = &g.fun("main").expect("main は検査済み").uses;
        self.emit("int main(void) {");
        if main_uses.iter().any(|u| u == "Console") {
            self.emit("    scl_eff_Console console = { NULL, scl_rt_console_print };");
            self.emit("    scl_f_main(&console);");
        } else {
            self.emit("    scl_f_main();");
        }
        self.emit("    return 0;");
        self.emit("}");
    }

    fn handler_op(&mut self, hname: &str, info: &HandlerInfo, f: &FunDecl, tys: &[Ty], ret: Ty) {
        self.tmp = 0;
        self.emit(format!(
            "SCL_FN {} {}({}) {{",
            cty(ret),
            op_fn_name(hname, &f.name.name),
            Self::param_list(&f.params, tys, &[], true)
        ));
        self.indent += 1;
        self.emit(format!(
            "struct scl_henv_{hname} *henv = (struct scl_henv_{hname} *)env;"
        ));
        self.emit("(void)henv;");
        for p in &f.params {
            self.emit(format!("(void)v_{};", p.name.name));
        }
        self.caps.clear();
        for u in &info.uses {
            self.caps.push((u.clone(), format!("henv->cap_{u}")));
        }
        self.block_body(&f.body);
        self.indent -= 1;
        self.emit("}");
        self.blank();
    }

    // ---- 文 ----

    /// ブロックの中身を、現在のインデントで出力する(波括弧は呼び出し側)。
    fn block_body(&mut self, b: &Block) {
        for s in &b.stmts {
            self.stmt(s);
        }
    }

    fn block(&mut self, b: &Block) {
        self.indent += 1;
        self.block_body(b);
        self.indent -= 1;
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let { name, init, .. } => {
                let v = self.expr(init);
                self.emit(format!("{} v_{} = {};", cty(init.ty.get()), name.name, v));
                self.emit(format!("(void)v_{};", name.name));
            }
            Stmt::Assign {
                name, op, value, ..
            } => {
                let v = self.expr(value);
                let n = format!("v_{}", name.name);
                match op {
                    AssignOp::Set => self.emit(format!("{n} = {v};")),
                    AssignOp::Add => self.emit(format!("{n} = scl_add({n}, {v});")),
                    AssignOp::Sub => self.emit(format!("{n} = scl_sub({n}, {v});")),
                }
            }
            Stmt::If { cond, then, els } => {
                let c = self.expr(cond);
                self.emit(format!("if ({}) {{", strip_parens(&c)));
                self.block(then);
                match els {
                    None => self.emit("}"),
                    Some(Else::Block(b)) => {
                        self.emit("} else {");
                        self.block(b);
                        self.emit("}");
                    }
                    Some(Else::If(s)) => {
                        self.emit("} else {");
                        self.indent += 1;
                        self.stmt(s);
                        self.indent -= 1;
                        self.emit("}");
                    }
                }
            }
            Stmt::While { cond, body } => {
                let (pre, c) = self.capture(|g| g.expr(cond));
                if pre.is_empty() {
                    self.emit(format!("while ({}) {{", strip_parens(&c)));
                } else {
                    self.emit("for (;;) {");
                    self.lines.extend(pre);
                    self.emit(format!("    if (!({c})) break;"));
                }
                self.block(body);
                self.emit("}");
            }
            Stmt::Return { value, .. } => match value {
                None => self.emit("return;"),
                Some(v) => {
                    let v = self.expr(v);
                    self.emit(format!("return {v};"));
                }
            },
            Stmt::Handle {
                effect,
                handler,
                body,
            } => self.handle(&effect.name, &handler.name, body),
            Stmt::Expr(e) => {
                let call = self.call(e);
                if e.ty.get() == Ty::Unit {
                    self.emit(format!("{call};"));
                } else {
                    self.emit(format!("(void){call};"));
                }
            }
        }
    }

    fn handle(&mut self, effect: &str, handler: &str, body: &Block) {
        let g = self.g;
        self.handle_id += 1;
        let id = self.handle_id;
        let info = g.handler(handler).expect("ハンドラは検査済み");
        let henv = format!("henv_{id}");
        let capv = format!("cap_{effect}_{id}");

        self.emit("{");
        self.indent += 1;
        let init = if info.uses.is_empty() {
            "{ 0 }".to_string()
        } else {
            let fields: Vec<String> = info
                .uses
                .iter()
                .map(|u| format!(".cap_{u} = {}", self.cap(u)))
                .collect();
            format!("{{ {} }}", fields.join(", "))
        };
        self.emit(format!("struct scl_henv_{handler} {henv} = {init};"));
        let mut fields = vec![format!(".env = &{henv}")];
        for op in &g.effects[effect].ops {
            fields.push(format!(
                ".op_{} = {}",
                op.name,
                op_fn_name(handler, &op.name)
            ));
        }
        self.emit(format!(
            "scl_eff_{effect} {capv} = {{ {} }};",
            fields.join(", ")
        ));
        self.emit(format!("(void){capv};"));
        self.caps.push((effect.to_string(), format!("(&{capv})")));
        self.block_body(body);
        self.caps.pop();
        self.indent -= 1;
        self.emit("}");
    }

    // ---- 式 ----

    /// `f` の実行中に出力された行を取り出す。
    fn capture<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> (Vec<String>, T) {
        let saved = std::mem::take(&mut self.lines);
        self.indent += 1;
        let r = f(self);
        self.indent -= 1;
        let pre = std::mem::replace(&mut self.lines, saved);
        (pre, r)
    }

    /// 副作用を持つ式を一時変数に入れ、その名前を返す。
    fn bind(&mut self, ty: Ty, value: String) -> String {
        let t = self.fresh();
        self.emit(format!("{} {t} = {value};", cty(ty)));
        t
    }

    /// 呼び出し式の C の式(一時変数には入れない)。引数は先に評価する。
    fn call(&mut self, e: &Expr) -> String {
        match &e.kind {
            ExprKind::Call { callee, args } => {
                let sig = self.g.fun(&callee.name).expect("関数は検査済み").clone();
                let mut a: Vec<String> = args.iter().map(|x| self.expr(x)).collect();
                for u in &sig.uses {
                    a.push(self.cap(u));
                }
                format!("{}({})", fun_name(&callee.name), a.join(", "))
            }
            ExprKind::OpCall { effect, op, args } => {
                let c = self.cap(&effect.name);
                let mut a = vec![format!("{c}->env")];
                a.extend(args.iter().map(|x| self.expr(x)));
                format!("{c}->op_{}({})", op.name, a.join(", "))
            }
            _ => unreachable!("呼び出しではない式文は検査済み"),
        }
    }

    /// 式を出力し、副作用のない C の式を返す。
    fn expr(&mut self, e: &Expr) -> String {
        match &e.kind {
            ExprKind::Int(n) => format!("INT64_C({n})"),
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::Str(pieces) => {
                let mut acc: Option<String> = None;
                for p in pieces {
                    let piece = match p {
                        StrPiece::Lit(s) if s.is_empty() => continue,
                        StrPiece::Lit(s) => format!("scl_str_make({}, {})", c_string(s), s.len()),
                        StrPiece::Expr(x) => {
                            let v = self.expr(x);
                            match x.ty.get() {
                                Ty::Int => format!("scl_str_from_int({v})"),
                                Ty::Bool => format!("scl_str_from_bool({v})"),
                                _ => v,
                            }
                        }
                    };
                    acc = Some(match acc {
                        None => piece,
                        Some(a) => format!("scl_str_concat({a}, {piece})"),
                    });
                }
                acc.unwrap_or_else(|| "scl_str_make(\"\", 0)".to_string())
            }
            ExprKind::Var(id) => format!("v_{}", id.name),
            ExprKind::Call { .. } | ExprKind::OpCall { .. } => {
                let c = self.call(e);
                self.bind(e.ty.get(), c)
            }
            ExprKind::Unary { op, expr } => match op {
                UnOp::Neg => {
                    if let ExprKind::Int(n) = expr.kind {
                        return format!("INT64_C(-{n})");
                    }
                    let v = self.expr(expr);
                    self.bind(Ty::Int, format!("scl_neg({v})"))
                }
                UnOp::Not => {
                    let v = self.expr(expr);
                    format!("(!{v})")
                }
            },
            ExprKind::Binary { op, lhs, rhs } => {
                if matches!(op, BinOp::And | BinOp::Or) {
                    let l = self.expr(lhs);
                    let (pre, r) = self.capture(|g| g.expr(rhs));
                    let sym = op.symbol();
                    if pre.is_empty() {
                        return format!("({l} {sym} {r})");
                    }
                    let t = self.bind(Ty::Bool, l);
                    let guard = if *op == BinOp::And {
                        t.clone()
                    } else {
                        format!("!{t}")
                    };
                    self.emit(format!("if ({guard}) {{"));
                    self.lines.extend(pre);
                    self.emit(format!("    {t} = {r};"));
                    self.emit("}");
                    return t;
                }
                let l = self.expr(lhs);
                let r = self.expr(rhs);
                let arith = |name: &str| format!("scl_{name}({l}, {r})");
                match op {
                    BinOp::Add => self.bind(Ty::Int, arith("add")),
                    BinOp::Sub => self.bind(Ty::Int, arith("sub")),
                    BinOp::Mul => self.bind(Ty::Int, arith("mul")),
                    BinOp::Div => self.bind(Ty::Int, arith("div")),
                    BinOp::Rem => self.bind(Ty::Int, arith("rem")),
                    BinOp::Eq | BinOp::Ne if lhs.ty.get() == Ty::Str => {
                        let neg = if *op == BinOp::Ne { "!" } else { "" };
                        format!("{neg}scl_str_eq({l}, {r})")
                    }
                    _ => format!("({l} {} {r})", op.symbol()),
                }
            }
        }
    }
}
